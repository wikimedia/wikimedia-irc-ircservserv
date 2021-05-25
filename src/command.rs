use anyhow::Result;
use irc::client::prelude::*;
use std::sync::Arc;

use crate::{git, LockedState};

/// Handler for !isstrust
pub(crate) async fn iss_trust(
    client: &Arc<Client>,
    target: &str,
    state: &LockedState,
) -> Result<()> {
    let trusted = state.read().await.botconfig.trusted.join(", ");
    client.send_privmsg(target, format!("I trust: {}", trusted))?;
    Ok(())
}

pub(crate) async fn iss_pull(client: &Arc<Client>, target: &str) {
    match git::pull().await {
        Ok(changed) => {
            if changed.is_empty() {
                client
                    .send_privmsg(target, "There are no pending changes.")
                    .unwrap();
            } else {
                client
                    .send_privmsg(
                        target,
                        format!("Pulled changes for: {}", changed.join(", ")),
                    )
                    .unwrap();
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
