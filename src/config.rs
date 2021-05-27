use anyhow::Result;
use irc::client::prelude::*;
use serde::Deserialize;
use tokio::fs;

#[derive(Deserialize, Default)]
pub struct BotConfig {
    #[serde(default)]
    password_file: Option<String>,
    pub channel_config: String,
    #[serde(default)]
    pub owners: Vec<String>,
    #[serde(default)]
    pub trusted: Vec<String>,
    pub irc: Config,
}

pub enum TrustLevel {
    Owner,
    Trusted,
}

impl BotConfig {
    pub async fn load(path: &str) -> Result<Self> {
        let mut botconfig: BotConfig =
            toml::from_str(&fs::read_to_string(path).await?)?;
        if let Some(password_file) = &botconfig.password_file {
            // If the password_file option is set, read it and set it as the password
            botconfig.irc.password = Some(
                fs::read_to_string(password_file).await?.trim().to_string(),
            );
        }

        Ok(botconfig)
    }
}
