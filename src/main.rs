use anyhow::{anyhow, Result};
use futures_util::StreamExt;
use irc::client::data::AccessLevel;
use irc::client::prelude::*;
use irc::proto::caps::Capability;
use irc::proto::mode::ChannelMode::Ban;
use irc::proto::response::Response::{
    RPL_BANLIST, RPL_ENDOFBANLIST, RPL_ENDOFINVITELIST, RPL_INVITELIST,
};
use regex::Regex;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::fs;
use tokio::sync::{mpsc, RwLock};
use tokio::time::{interval, sleep, timeout, Duration};

mod config;

use config::BotConfig;

const FLAGS_FOUNDER: &str = "AFRefiorstv";
const FLAGS_CRAT: &str = "Afiortv";
const FLAGS_AUTOVOICE_OP: &str = "AViotv";
const FLAGS_OP: &str = "Aiotv";
const FLAGS_PLUS_O: &str = "o";

// TODO: set forward to -overflow
const GLOBAL_BANS: &str = "$j:#wikimedia-bans";

#[derive(Debug, Default, Deserialize)]
struct ManagedChannel {
    #[serde(default)]
    founders: HashSet<String>,
    #[serde(default)]
    crats: HashSet<String>,
    #[serde(default)]
    autovoice_op: HashSet<String>,
    #[serde(default)]
    ops: HashSet<String>,
    #[serde(default)]
    plus_o: HashSet<String>,
    #[serde(default)]
    global_bans: bool,
    #[serde(default)]
    bans: HashSet<String>,
    #[serde(default)]
    invexes: HashSet<String>,
    // unknown modes
    #[serde(default)]
    unknown: HashMap<String, String>,
    // state stuff
    #[serde(default)]
    flags_done: bool,
    #[serde(default)]
    bans_done: bool,
    #[serde(default)]
    invexes_done: bool,
}

impl ManagedChannel {
    fn is_done(&self) -> bool {
        self.flags_done && self.bans_done && self.invexes_done
    }

    fn fix_flags(&self, cfg: &ManagedChannel) -> Vec<(String, String)> {
        let mut cmds = vec![];
        for (name, mode) in self.unknown.iter() {
            cmds.push((name.to_string(), format!("-{}", mode)))
        }

        // FIXME: macro all of this
        for remove in self.founders.difference(&cfg.founders) {
            cmds.push((remove.to_string(), format!("-{}", FLAGS_FOUNDER)))
        }
        for add in cfg.founders.difference(&self.founders) {
            cmds.push((add.to_string(), format!("+{}", FLAGS_FOUNDER)))
        }
        for remove in self.crats.difference(&cfg.crats) {
            cmds.push((remove.to_string(), format!("-{}", FLAGS_CRAT)))
        }
        for add in cfg.crats.difference(&self.crats) {
            cmds.push((add.to_string(), format!("+{}", FLAGS_CRAT)))
        }
        for remove in self.autovoice_op.difference(&cfg.autovoice_op) {
            cmds.push((remove.to_string(), format!("-{}", FLAGS_AUTOVOICE_OP)))
        }
        for add in cfg.autovoice_op.difference(&self.autovoice_op) {
            cmds.push((add.to_string(), format!("+{}", FLAGS_AUTOVOICE_OP)))
        }
        for remove in self.ops.difference(&cfg.ops) {
            cmds.push((remove.to_string(), format!("-{}", FLAGS_OP)))
        }
        for add in cfg.ops.difference(&self.ops) {
            cmds.push((add.to_string(), format!("+{}", FLAGS_OP)))
        }
        for remove in self.plus_o.difference(&cfg.plus_o) {
            cmds.push((remove.to_string(), format!("-{}", FLAGS_PLUS_O)))
        }
        for add in cfg.plus_o.difference(&self.plus_o) {
            cmds.push((add.to_string(), format!("+{}", FLAGS_PLUS_O)))
        }

        cmds
    }

    fn fix_modes(&self, cfg: &ManagedChannel) -> Vec<Mode<ChannelMode>> {
        let mut cmds = vec![];
        if cfg.global_bans && !self.bans.contains(GLOBAL_BANS) {
            cmds.push(Mode::Plus(Ban, Some(GLOBAL_BANS.to_string())));
        } else if !cfg.global_bans && self.bans.contains(GLOBAL_BANS) {
            cmds.push(Mode::Minus(Ban, Some(GLOBAL_BANS.to_string())));
        }

        cmds
    }

    fn add_chanserv(&mut self, line: &str) -> Result<()> {
        // 2        legoktm                +AFRefiorstv         (FOUNDER) [modified...
        // FIXME use Skizzerz's regex instead
        // TODO: lazy_static this
        let re =
            Regex::new(r"^\d{1,3}\s+([A-z0-9\*\-!@/]+)\s+\+([A-z]+) ").unwrap();
        if let Some(caps) = re.captures(&line) {
            if caps.len() < 3 {
                return Err(anyhow::anyhow!("Couldn't parse: {}", line));
            }
            let account = caps[1].to_string();
            match &caps[2] {
                FLAGS_FOUNDER => {
                    self.founders.insert(account);
                }
                FLAGS_CRAT => {
                    self.crats.insert(account);
                }
                FLAGS_AUTOVOICE_OP => {
                    self.autovoice_op.insert(account);
                }
                FLAGS_OP => {
                    self.ops.insert(account);
                }
                FLAGS_PLUS_O => {
                    self.plus_o.insert(account);
                }
                mode => {
                    self.unknown.insert(account, mode.to_string());
                }
            };
            Ok(())
        } else {
            Err(anyhow::anyhow!("Couldn't parse: {}", line))
        }
    }
}

fn is_from(message: &Message, name: &str) -> bool {
    if let Some(Prefix::Nickname(_, account, _)) = &message.prefix {
        account == name
    } else {
        false
    }
}

async fn is_trusted(state: &Arc<RwLock<BotState>>, message: &Message) -> bool {
    if let Some(Prefix::Nickname(_, _, cloak)) = &message.prefix {
        let trusted = &state.read().await.botconfig.trusted;
        trusted.contains(&cloak.to_string())
    } else {
        false
    }
}

/// Ask ChanServ for ops in a channel and wait till its set
async fn wait_for_op(client: &Client, channel: &str) -> bool {
    let tmt =
        timeout(Duration::from_secs(5), _wait_for_op(client, channel)).await;
    if tmt.is_err() {
        client
            .send_privmsg(
                channel,
                format!("Error: Unable to get opped in {}", channel),
            )
            .unwrap();
        false
    } else {
        true
    }
}

async fn _wait_for_op(client: &Client, channel: &str) {
    if !is_opped_in(client, channel) {
        println!("Getting ops in {}", channel);
        client
            .send_privmsg("ChanServ", format!("op {}", channel))
            .unwrap();
    } else {
        // Already opped!
        return;
    }
    // Wait until we are
    let mut interval = interval(Duration::from_millis(200));
    loop {
        if is_opped_in(client, channel) {
            break;
        }
        println!("Not opped yet.");
        interval.tick().await;
    }
}

/// Read channel config from the directory
async fn read_channel_config(
    dir: &str,
    channel: &str,
) -> Result<ManagedChannel> {
    Ok(toml::from_str(
        &fs::read_to_string(format!(
            "{}/channels/{}.toml",
            dir,
            channel.trim_start_matches('#')
        ))
        .await?,
    )?)
}

async fn sync_channel(
    client: &Client,
    state: Arc<RwLock<BotState>>,
    channel: &str,
    managed_channel: &ManagedChannel,
) -> Result<()> {
    let cfg = match read_channel_config(
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
    dbg!(&managed_channel, &cfg);
    let flag_cmds = managed_channel.fix_flags(&cfg);
    let mode_cmds = managed_channel.fix_modes(&cfg);
    if flag_cmds.is_empty() && mode_cmds.is_empty() {
        client.send_privmsg(channel, format!("No updates for {}", channel))?;
        return Ok(());
    }
    // If we have to change modes, make sure we're opped (already should've happened)
    if !mode_cmds.is_empty() && !wait_for_op(client, channel).await {
        // Getting op failed
        return Err(anyhow!("Unable to get op"));
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

#[derive(Default)]
struct BotState {
    // channel currently querying flags for
    flags_query: Option<String>,
    channels: HashMap<String, ManagedChannel>,
    botconfig: BotConfig,
}

fn is_opped_in(client: &Client, channel: &str) -> bool {
    if let Some(users) = client.list_users(channel) {
        for user in users {
            if user.get_nickname() == client.current_nickname() {
                return user.access_levels().contains(&AccessLevel::Oper);
            }
        }
    }

    // Not found in channel
    false
}

async fn handle_response(
    resp: &Response,
    data: &[String],
    state: Arc<RwLock<BotState>>,
) {
    if resp == &RPL_BANLIST {
        let mut w = state.write().await;
        let managed = w.channels.entry(data[1].to_string()).or_default();
        managed.bans.insert(data[2].to_string());
    } else if resp == &RPL_ENDOFBANLIST {
        let mut w = state.write().await;
        w.channels.entry(data[1].to_string()).or_default().bans_done = true;
    } else if resp == &RPL_INVITELIST {
        let mut w = state.write().await;
        let managed = w.channels.entry(data[1].to_string()).or_default();
        managed.invexes.insert(data[2].to_string());
    } else if resp == &RPL_ENDOFINVITELIST {
        let mut w = state.write().await;
        w.channels
            .entry(data[1].to_string())
            .or_default()
            .invexes_done = true;
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let botconfig = BotConfig::load("config.toml").await?;
    let mut orig_client = Client::from_config(botconfig.irc.clone()).await?;
    let mut stream = orig_client.stream()?;
    // Now that we've got a mutable stream, wrap it in Arc<> for thread-safe read access
    let client = Arc::new(orig_client);
    // state
    let bot_state = Arc::new(RwLock::new(BotState {
        botconfig,
        ..Default::default()
    }));
    // channel for all messages
    let (tx, mut rx) = mpsc::channel::<Message>(128);
    // channel for ChanServ notices
    let (chanserv_tx, mut chanserv_rx) = mpsc::channel::<String>(128);

    client.send_cap_req(&[Capability::MultiPrefix])?;
    client.identify()?;

    let state = bot_state.clone();
    let client_cs = client.clone();
    let chanserv_processor = tokio::spawn(async move {
        while let Some(notice) = chanserv_rx.recv().await {
            // FIXME: figure out a better internal message passing strategy
            if notice.starts_with("\r\n") {
                if state.read().await.flags_query.is_some() {
                    // Someone else is reading flags, please wait
                    let mut interval = interval(Duration::from_millis(200));
                    loop {
                        if state.read().await.flags_query.is_none() {
                            break;
                        }
                        interval.tick().await;
                    }
                }
                // Internal message with channel name
                let channel = notice.trim_start_matches("\r\n").to_string();
                {
                    let mut w = state.write().await;
                    w.flags_query = Some(channel.to_string());
                }
                client_cs
                    .send_privmsg("ChanServ", format!("flags {}", &channel))
                    .unwrap();
                continue;
            }
            // Clone instead of locking since we need to get the
            // write lock inside to clear it
            let looking = state.read().await.flags_query.clone();
            if notice.starts_with("--------------") {
                continue;
            }
            if let Some(looking) = &looking {
                if notice.starts_with("End of") {
                    let mut w = state.write().await;
                    w.channels.get_mut(looking).unwrap().flags_done = true;
                    w.flags_query = None;
                } else {
                    let mut w = state.write().await;
                    let managed =
                        w.channels.entry(looking.to_string()).or_default();
                    match managed.add_chanserv(&notice) {
                        Ok(_) => {}
                        Err(e) => {
                            dbg!(e);
                        }
                    }
                }
            }
        }
    });

    let state = bot_state.clone();
    let client = client.clone();
    let processor = tokio::spawn(async move {
        while let Some(message) = rx.recv().await {
            if let Command::NOTICE(_, notice) = &message.command {
                if is_from(&message, "ChanServ") {
                    dbg!(&message);
                    chanserv_tx.send(notice.to_string()).await.unwrap();
                    continue;
                }
            }
            if let Command::PRIVMSG(_, privmsg) = &message.command {
                if privmsg == "!isstrust" {
                    let trusted =
                        state.read().await.botconfig.trusted.join(", ");
                    client
                        .send_privmsg(
                            message.response_target().unwrap(),
                            format!("I trust: {}", trusted),
                        )
                        .unwrap();
                }
                if is_trusted(&state, &message).await && privmsg == "!issync" {
                    let channel = match message.response_target() {
                        Some(target) => {
                            if !target.starts_with('#') {
                                // Not a channel
                                client
                                    .send_privmsg(
                                        target,
                                        "This command must be used in-channel.",
                                    )
                                    .unwrap();
                                continue;
                            }
                            target.to_string()
                        }
                        None => {
                            // Not a PM, not in channel? wtf.
                            continue;
                        }
                    };
                    client
                        .send_privmsg(
                            message.response_target().unwrap(),
                            format!("Syncing {}", &channel),
                        )
                        .unwrap();
                    // Start doing flags
                    chanserv_tx
                        .send(format!("\r\n{}", &channel))
                        .await
                        .unwrap();
                    // Make sure we're op before checking +b and +I
                    if !wait_for_op(&client, &channel).await {
                        // Failed at getting op
                        continue;
                    }
                    // TODO: combine these?
                    client
                        .send_mode(
                            &channel,
                            &[Mode::Plus(ChannelMode::Ban, None)],
                        )
                        .unwrap();
                    client
                        .send_mode(
                            &channel,
                            &[Mode::Plus(ChannelMode::InviteException, None)],
                        )
                        .unwrap();
                    let state = state.clone();
                    let channel = channel.to_string();
                    let client = client.clone();
                    tokio::spawn(async move {
                        // Check every 200ms if we're ready to go
                        let mut interval = interval(Duration::from_millis(200));
                        loop {
                            if let Some(managed_channel) =
                                state.read().await.channels.get(&channel)
                            {
                                //dbg!(&managed_channel);
                                if managed_channel.is_done() {
                                    break;
                                }
                            }
                            // Wait a bit (but make sure we're not holding the read lock here)
                            interval.tick().await;
                        }
                        let managed_channel = {
                            let mut w = state.write().await;
                            w.channels.remove(&channel).unwrap()
                        };
                        dbg!(&managed_channel);
                        sync_channel(
                            &client,
                            state.clone(),
                            &channel,
                            &managed_channel,
                        )
                        .await
                        .unwrap();
                        // de-op
                        client
                            .send_mode(
                                &channel,
                                &[Mode::Minus(
                                    UserMode::Oper,
                                    Some(client.current_nickname().to_string()),
                                )],
                            )
                            .unwrap();
                    });
                }
            }
            if let Command::Response(resp, data) = &message.command {
                handle_response(resp, data, state.clone()).await;
            }
        }
    });

    while let Some(message) = stream.next().await.transpose()? {
        tx.send(message).await?;
    }

    processor.await?;
    chanserv_processor.await?;

    Ok(())
}
