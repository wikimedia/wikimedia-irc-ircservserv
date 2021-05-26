use anyhow::Result;
use irc::client::prelude::*;
use std::sync::Arc;

use crate::{git, LockedState};

/// Handler for !isstrust
pub async fn iss_trust(
    client: &Arc<Client>,
    target: &str,
    state: &LockedState,
) -> Result<()> {
    let trusted = state.read().await.botconfig.trusted.join(", ");
    client.send_privmsg(target, format!("I trust: {}", trusted))?;
    Ok(())
}

pub async fn iss_pull(client: &Arc<Client>, target: &str) {
    match git::pull().await {
        Ok(changed) => {
            if changed.is_empty() {
                client
                    .send_privmsg(target, "There are no pending changes.")
                    .unwrap();
                return;
            }

            client
                .send_privmsg(
                    target,
                    format!("Pulled changes for: {}", changed.join(", ")),
                )
                .unwrap();
            // Join any new channels that we just learned about
            let currently_in = client.list_channels().unwrap_or_else(Vec::new);
            for channel in changed {
                if !currently_in.contains(&channel) {
                    client.send_join(&channel).unwrap();
                }
            }
        }
        Err(e) => {
            client
                .send_privmsg(
                    target,
                    format!("Error pulling changes: {}", e.to_string()),
                )
                .unwrap();
        }
    }
}
