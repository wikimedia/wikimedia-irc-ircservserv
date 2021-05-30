use anyhow::Result;
use irc::client::prelude::*;
use irc::proto::mode::ChannelMode::Ban;
use regex::Regex;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

const FLAGS_FOUNDER: &str = "AFRefiorstv";
const FLAGS_CRAT: &str = "Afiortv";
const FLAGS_AUTOVOICE_OP: &str = "AViotv";
const FLAGS_OP: &str = "Aiotv";
const FLAGS_PLUS_O: &str = "o";

// TODO: set forward to -overflow
const GLOBAL_BANS: &str = "$j:#wikimedia-bans";

#[derive(Debug, Default, Deserialize)]
pub struct ManagedChannel {
    #[serde(default)]
    pub founders: HashSet<String>,
    #[serde(default)]
    pub crats: HashSet<String>,
    #[serde(default)]
    pub autovoice_op: HashSet<String>,
    #[serde(default)]
    pub ops: HashSet<String>,
    #[serde(default)]
    pub plus_o: HashSet<String>,
    #[serde(default)]
    pub global_bans: bool,
    #[serde(default)]
    pub bans: HashSet<String>,
    #[serde(default)]
    pub invexes: HashSet<String>,
    // unknown modes
    #[serde(default)]
    pub unknown: HashMap<String, String>,
    // state stuff
    #[serde(default)]
    pub flags_done: bool,
    #[serde(default)]
    pub bans_done: bool,
    #[serde(default)]
    pub invexes_done: bool,
}

impl ManagedChannel {
    pub fn is_done(&self) -> bool {
        self.flags_done && self.bans_done && self.invexes_done
    }

    pub fn fix_flags(&self, cfg: &ManagedChannel) -> Vec<(String, String)> {
        let mut changes: HashMap<String, Vec<String>> = HashMap::new();
        for (name, mode) in self.unknown.iter() {
            changes
                .entry(name.to_string())
                .or_default()
                .push(format!("-{}", mode));
        }

        // FIXME: macro all of this
        for remove in self.founders.difference(&cfg.founders) {
            changes
                .entry(remove.to_string())
                .or_default()
                .push(format!("-{}", FLAGS_FOUNDER));
            //            cmds.push((remove.to_string(), format!("-{}", FLAGS_FOUNDER)))
        }
        for add in cfg.founders.difference(&self.founders) {
            changes
                .entry(add.to_string())
                .or_default()
                .push(format!("+{}", FLAGS_FOUNDER));
        }
        for remove in self.crats.difference(&cfg.crats) {
            changes
                .entry(remove.to_string())
                .or_default()
                .push(format!("-{}", FLAGS_CRAT));
        }
        for add in cfg.crats.difference(&self.crats) {
            changes
                .entry(add.to_string())
                .or_default()
                .push(format!("+{}", FLAGS_CRAT));
        }
        for remove in self.autovoice_op.difference(&cfg.autovoice_op) {
            changes
                .entry(remove.to_string())
                .or_default()
                .push(format!("-{}", FLAGS_AUTOVOICE_OP));
        }
        for add in cfg.autovoice_op.difference(&self.autovoice_op) {
            changes
                .entry(add.to_string())
                .or_default()
                .push(format!("+{}", FLAGS_AUTOVOICE_OP));
        }
        for remove in self.ops.difference(&cfg.ops) {
            changes
                .entry(remove.to_string())
                .or_default()
                .push(format!("-{}", FLAGS_OP));
        }
        for add in cfg.ops.difference(&self.ops) {
            changes
                .entry(add.to_string())
                .or_default()
                .push(format!("+{}", FLAGS_OP));
        }
        for remove in self.plus_o.difference(&cfg.plus_o) {
            changes
                .entry(remove.to_string())
                .or_default()
                .push(format!("-{}", FLAGS_PLUS_O));
        }
        for add in cfg.plus_o.difference(&self.plus_o) {
            changes
                .entry(add.to_string())
                .or_default()
                .push(format!("+{}", FLAGS_PLUS_O));
        }

        changes
            .iter()
            .map(|(name, modes)| (name.to_string(), modes.join("")))
            .collect()
    }

    pub fn fix_modes(&self, cfg: &ManagedChannel) -> Vec<Mode<ChannelMode>> {
        let mut cmds = vec![];
        if cfg.global_bans && !self.bans.contains(GLOBAL_BANS) {
            cmds.push(Mode::Plus(Ban, Some(GLOBAL_BANS.to_string())));
        } else if !cfg.global_bans && self.bans.contains(GLOBAL_BANS) {
            cmds.push(Mode::Minus(Ban, Some(GLOBAL_BANS.to_string())));
        }

        cmds
    }

    pub fn add_flags_from_chanserv(&mut self, line: &str) -> Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn set(arg: &str) -> HashSet<String> {
        let mut set = HashSet::new();
        set.insert(arg.to_string());
        set
    }

    #[test]
    fn test_fix_flags() {
        let managed = ManagedChannel {
            founders: set("foo"),
            ..Default::default()
        };
        let cfg = ManagedChannel {
            founders: set("bar"),
            ops: set("foo"),
            ..Default::default()
        };
        let res = managed.fix_flags(&cfg);
        let expected = vec![
            ("foo".to_string(), "-AFRefiorstv+Aiotv".to_string()),
            ("bar".to_string(), "+AFRefiorstv".to_string()),
        ];
        assert_eq!(expected, res);
    }
}
