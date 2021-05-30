use anyhow::Result;
use futures_util::StreamExt;
use irc::client::prelude::*;
use irc::proto::caps::Capability;
use irc::proto::response::Response::{
    RPL_BANLIST, RPL_ENDOFBANLIST, RPL_ENDOFINVITELIST, RPL_INVITELIST,
};
use log::debug;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

use ircservserv::{
    chanserv, command,
    config::{BotConfig, TrustLevel},
    extract_account, is_trusted, BotState, LockedState,
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
    env_logger::init();
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
    // channel for ChanServ interactions
    let (chanserv_tx, mut chanserv_rx) =
        mpsc::channel::<chanserv::Message>(128);

    client.send_cap_req(&[Capability::MultiPrefix, Capability::AccountTag])?;
    client.identify()?;

    let state = bot_state.clone();
    let client_cs = client.clone();
    let chanserv_processor = tokio::spawn(async move {
        chanserv::listen(&mut chanserv_rx, state, client_cs).await;
    });

    let state = bot_state.clone();
    let client = client.clone();
    let processor = tokio::spawn(async move {
        while let Some(message) = rx.recv().await {
            //dbg!(&message);
            match &message.command {
                Command::NOTICE(_, notice) => {
                    if is_from(&message, "ChanServ") {
                        debug!("From ChanServ: {}", notice);
                        chanserv_tx
                            .send(chanserv::Message::Notice(notice.to_string()))
                            .await
                            .unwrap();
                    }
                }
                Command::PRIVMSG(_, privmsg) => {
                    if privmsg == "!isspull" {
                        if !is_trusted(&state, &message, TrustLevel::Trusted)
                            .await
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
                    } else if privmsg == "!issync" {
                        debug!(
                            "Received !issync for {} from {}",
                            message.response_target().unwrap_or("unknown"),
                            extract_account(&message)
                                .unwrap_or_else(|| "unknown".to_string())
                        );
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
                    }
                }
                Command::Response(resp, data) => {
                    handle_response(resp, data, state.clone()).await;
                }
                _ => {}
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
