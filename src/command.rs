use anyhow::Result;
use irc::client::prelude::*;
use std::sync::Arc;

use crate::LockedState;

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
