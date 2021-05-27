use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

pub mod channel;
pub mod command;
pub mod config;
pub mod git;

pub type LockedState = Arc<RwLock<BotState>>;

#[derive(Default)]
pub struct BotState {
    // channel currently querying flags for
    pub flags_query: Option<String>,
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
}
