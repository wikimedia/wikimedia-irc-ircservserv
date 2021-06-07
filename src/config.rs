use anyhow::Result;
use irc::client::prelude::*;
use serde::Deserialize;
use tokio::fs;

const URL: &str = "https://meta.wikimedia.org/wiki/IRC/Bots/ircservserv";
const GIT_VERSION: &str = git_version::git_version!();

/// IRC Bot configuration: `config.toml`
#[derive(Deserialize, Default)]
pub struct BotConfig {
    /// File to read to get the password
    #[serde(default)]
    password_file: Option<String>,
    /// Path to repository with channel configuration
    pub channel_config: String,
    /// List of accounts who are owners
    #[serde(default)]
    pub owners: Vec<String>,
    /// List of accounts who are trusted
    #[serde(default)]
    pub trusted: Vec<String>,
    /// Configuration for the `irc` crate
    pub irc: Config,
}

/// Differentation between owners and trusted users
pub enum TrustLevel {
    Owner,
    Trusted,
}

impl BotConfig {
    /// Load config from a toml file on disk
    pub async fn load(path: &str) -> Result<Self> {
        let mut botconfig: BotConfig =
            toml::from_str(&fs::read_to_string(path).await?)?;
        if let Some(password_file) = &botconfig.password_file {
            // If the password_file option is set, read it and set it as the password
            botconfig.irc.password = Some(
                fs::read_to_string(password_file).await?.trim().to_string(),
            );
        }
        botconfig.irc.version = Some(format!("{}, git: {}", URL, GIT_VERSION));

        Ok(botconfig)
    }
}
