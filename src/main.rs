use anyhow::Result;
use futures_util::StreamExt;
use irc::client::prelude::*;
use irc::proto::caps::Capability;
use irc::proto::response::Response::{
    RPL_BANLIST, RPL_ENDOFBANLIST, RPL_ENDOFINVITELIST, RPL_INVITELIST,
};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio::time::{interval, Duration};

use ircservserv::{
    command,
    config::{BotConfig, TrustLevel},
    is_trusted, BotState, LockedState,
};

fn is_from(message: &Message, name: &str) -> bool {
    if let Some(Prefix::Nickname(_, account, _)) = &message.prefix {
        account == name
    } else {
        false
    }
}

async fn handle_response(resp: &Response, data: &[String], state: LockedState) {
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

    client.send_cap_req(&[Capability::MultiPrefix, Capability::AccountTag])?;
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
            if notice.starts_with("--------------")
                || notice.starts_with("Entry    Nickname/Host")
            {
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
            dbg!(&message);
            if let Command::NOTICE(_, notice) = &message.command {
                if is_from(&message, "ChanServ") {
                    dbg!(&message);
                    chanserv_tx.send(notice.to_string()).await.unwrap();
                    continue;
                }
            }
            if let Command::PRIVMSG(_, privmsg) = &message.command {
                if privmsg == "!isspull" {
                    if !is_trusted(&state, &message, TrustLevel::Trusted).await
                    {
                        // Silently ignore
                        continue;
                    }
                    // FIXME: this should only be done in-channel, maybe only -ops?
                    if let Some(target) = message.response_target() {
                        let target = target.to_string();
                        let client = client.clone();
                        tokio::spawn(async move {
                            command::iss_pull(&client, &target).await;
                        });
                    }
                    continue;
                } else if privmsg == "!issync" {
                    let client = client.clone();
                    let message = message.clone();
                    let state = state.clone();
                    let chanserv_tx = chanserv_tx.clone();
                    tokio::spawn(async move {
                        command::iss_sync(
                            &message,
                            &client,
                            &state,
                            chanserv_tx,
                        )
                        .await;
                    });
                    continue;
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
