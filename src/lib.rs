use anyhow::Result;
use irc::client::data::AccessLevel;
use irc::client::prelude::*;
use log::debug;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::fs;
use tokio::sync::RwLock;
use tokio::time::{interval, timeout, Duration};

pub mod channel;
pub mod chanserv;
pub mod command;
pub mod config;
pub mod git;

pub type LockedState = Arc<RwLock<BotState>>;

use channel::ManagedChannel;
use config::TrustLevel;

#[derive(Default)]
pub struct BotState {
    /// What we're currently PMing ChanServ for
    pub chanserv: Option<chanserv::Message>,
    /// State of channels we're currently looking up
    pub channels: HashMap<String, channel::ManagedChannel>,
    pub botconfig: config::BotConfig,
}

impl BotState {
    pub fn is_channel_done(&self, channel: &str) -> bool {
        if let Some(managed_channel) = self.channels.get(channel) {
            managed_channel.is_done()
        } else {
            false
        }
    }

    pub fn is_flags_done(&self, channel: &str) -> bool {
        if let Some(managed_channel) = self.channels.get(channel) {
            managed_channel.flags_done
        } else {
            false
        }
    }

    /// Whether the given username is a founder.
    /// NOTE: you need to check that flags_done is true first
    pub fn is_founder_on(&self, channel: &str, username: &str) -> bool {
        if let Some(managed_channel) = self.channels.get(channel) {
            managed_channel.founders.contains(username)
        } else {
            false
        }
    }
}

/// Ask ChanServ for ops in a channel and wait till its set
async fn wait_for_op(client: &Client, channel: &str) -> bool {
    let tmt =
        timeout(Duration::from_secs(5), _wait_for_op(client, channel)).await;
    if tmt.is_err() {
        debug!("Timeout getting ops for {}", channel);
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
        debug!("Getting ops in {}", channel);
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
        debug!("Not opped in {} yet.", channel);
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

/// Given a message, extract the account of the sender
/// using the "account-tags" IRCv3 capability
pub fn extract_account(message: &Message) -> Option<String> {
    if let Some(tags) = &message.tags {
        for tag in tags {
            if tag.0 == "account" {
                if let Some(name) = &tag.1 {
                    return Some(name.to_string());
                }
            }
        }
    }

    None
}

/// Whether the given message was sent from someone who
/// is in our configured owners or trusted lists
pub async fn is_trusted(
    state: &LockedState,
    message: &Message,
    level: TrustLevel,
) -> bool {
    if let Some(account) = extract_account(message) {
        let list = match level {
            TrustLevel::Owner => state.read().await.botconfig.owners.clone(),
            TrustLevel::Trusted => state.read().await.botconfig.trusted.clone(),
        };
        list.contains(&account)
    } else {
        false
    }
}
