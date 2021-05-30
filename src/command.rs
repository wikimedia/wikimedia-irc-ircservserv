use anyhow::{anyhow, Result};
use irc::client::prelude::*;
use std::sync::Arc;
use tokio::sync::mpsc::Sender as MpscSender;
use tokio::time::{interval, sleep, Duration};

use crate::chanserv;
use crate::config::TrustLevel;
use crate::{channel::ManagedChannel, git, is_trusted, LockedState};

// FIXME: don't hardcode
const PULL_CHANNEL: &str = "#wikimedia-ops";

/// Respond to `!isspull`, which pulls the config repo
///
/// This command must be used in the pull channel. Once
/// it's finished, it will respond with the list of channels
/// that have config updates.
pub async fn iss_pull(client: &Arc<Client>, target: &str) -> Result<()> {
    if target != PULL_CHANNEL {
        return Err(anyhow!(
            "This command can only be used in {}",
            PULL_CHANNEL
        ));
    }
    let changed = git::pull().await?;
    if changed.is_empty() {
        client.send_privmsg(target, "There are no pending changes.")?;
        return Ok(());
    }

    client.send_privmsg(
        target,
        format!("Pulled changes for: {}", changed.join(", ")),
    )?;
    // Join any new channels that we just learned about
    let currently_in = client.list_channels().unwrap_or_else(Vec::new);
    for channel in changed {
        if !currently_in.contains(&channel) {
            client.send_join(&channel)?;
        }
    }
    Ok(())
}

/// Require a command was sent in a channel, not PM
fn must_be_in_channel(message: &Message) -> Result<String> {
    if let Some(target) = message.response_target() {
        if target.starts_with('#') {
            return Ok(target.to_string());
        }
    }
    Err(anyhow!("This command must be used in-channel."))
}

/// Responds to `!issync`, the whole magic of the bot.
/// Basically this command will:
/// * Verify the requestor is logged in
/// * Ask ChanServ for flags/access list
/// * Verify requestor is +F in the channel (or bot owner)
/// * Tell the channel it's syncing
/// * op up to look at the ban and invex lists
/// * Wait for all the lists to come in
/// * Identify any mismatches and execute them
/// * De-op
pub async fn iss_sync(
    message: &Message,
    client: &Arc<Client>,
    state: &LockedState,
    chanserv_tx: MpscSender<chanserv::Message>,
) -> Result<()> {
    let channel = must_be_in_channel(message)?;
    let account = crate::extract_account(&message).ok_or_else(|| {
        anyhow!("You don't have permission to update channel settings")
    })?;
    // First we need to verify the person making the request is a founder
    chanserv_tx
        .send(chanserv::Message::Flags(channel.to_string()))
        .await
        .unwrap();
    let mut flag_interval = interval(Duration::from_millis(200));
    loop {
        if state.read().await.is_flags_done(&channel) {
            break;
        }
        // Wait a bit (but make sure we're not holding the read lock here)
        flag_interval.tick().await;
    }
    // Must be a bot owner or a channel founder
    if !is_trusted(&state, &message, TrustLevel::Owner).await
        && !state.read().await.is_founder_on(&channel, &account)
    {
        return Err(anyhow!(
            "You don't have permission to update channel settings"
        ));
    }
    // At this point the person is authorized to sync
    client.send_privmsg(
        message.response_target().unwrap(),
        format!("Syncing {} (requested by {})", &channel, &account),
    )?;
    // Make sure we're op before checking +b and +I
    crate::wait_for_op(&client, &channel).await?;
    // TODO: combine these?
    client.send_mode(&channel, &[Mode::Plus(ChannelMode::Ban, None)])?;
    client.send_mode(
        &channel,
        &[Mode::Plus(ChannelMode::InviteException, None)],
    )?;
    // Check every 200ms if we're ready to go
    let mut done_interval = interval(Duration::from_millis(200));
    loop {
        if state.read().await.is_channel_done(&channel) {
            break;
        }
        // Wait a bit (but make sure we're not holding the read lock here)
        done_interval.tick().await;
    }
    let managed_channel = {
        let mut w = state.write().await;
        // FIXME not fully safe, if another thread gets the write lock
        // first it could have already removed the channel.
        w.channels.remove(&channel).unwrap()
    };
    //dbg!(&managed_channel);
    sync_channel(&client, state.clone(), &channel, &managed_channel).await?;
    // de-op, TODO: possible race here if our mode changes haven't taken effect yet
    client.send_mode(
        &channel,
        &[Mode::Minus(
            UserMode::Oper,
            Some(client.current_nickname().to_string()),
        )],
    )?;
    Ok(())
}

/// Do the actual sync step, comparing the live channel
/// state to what our configuration says it should be
async fn sync_channel(
    client: &Client,
    state: LockedState,
    channel: &str,
    managed_channel: &ManagedChannel,
) -> Result<()> {
    let cfg = match crate::read_channel_config(
        state.read().await.botconfig.channel_config.clone().as_str(),
        channel,
    )
    .await
    {
        Ok(cfg) => cfg,
        Err(e) => {
            client.send_privmsg(
                channel,
                format!(
                    "Error reading channel configuration: {}",
                    e.to_string()
                ),
            )?;
            return Err(e);
        }
    };
    //dbg!(&managed_channel, &cfg);
    let flag_cmds = managed_channel.fix_flags(&cfg);
    let mode_cmds = managed_channel.fix_modes(&cfg);
    if flag_cmds.is_empty() && mode_cmds.is_empty() {
        client.send_privmsg(channel, format!("No updates for {}", channel))?;
        return Ok(());
    }
    // If we have to change modes, make sure we're opped (already should've happened)
    if !mode_cmds.is_empty() {
        crate::wait_for_op(client, channel).await?;
    }
    // FIXME: Implement proper ratelimiting, see https://github.com/aatxe/irc/issues/190
    for (account, flags) in flag_cmds {
        client.send_privmsg(
            "ChanServ",
            format!("flags {} {} {}", channel, account, flags),
        )?;
        sleep(Duration::from_secs(1)).await;
        client.send_privmsg(
            channel,
            format!("Set /cs flags {} {} {}", channel, account, flags),
        )?;
        sleep(Duration::from_secs(1)).await;
    }
    for mode in mode_cmds {
        client.send_mode(channel, &[mode.clone()])?;
        sleep(Duration::from_secs(1)).await;
        client.send_privmsg(
            channel,
            format!("Set /mode {} {}", channel, &mode),
        )?;
        sleep(Duration::from_secs(1)).await;
    }

    Ok(())
}
