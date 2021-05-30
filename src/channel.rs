use anyhow::Result;
use irc::client::prelude::*;
use irc::proto::mode::ChannelMode::Ban;
use regex::Regex;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

const FOUNDER: &[char; 11] =
    &['A', 'F', 'R', 'e', 'f', 'i', 'o', 'r', 's', 't', 'v'];
const CRAT: &[char; 7] = &['A', 'f', 'i', 'o', 'r', 't', 'v'];
const OP: &[char; 5] = &['A', 'i', 'o', 't', 'v'];
const PLUS_O: &[char; 1] = &['o'];
const AUTOVOICE: &[char; 2] = &['V', 'v'];

// TODO: set forward to -overflow
const GLOBAL_BANS: &str = "$j:#wikimedia-bans";

fn parse_flags(input: &str) -> HashSet<char> {
    let mut set = HashSet::new();
    for char in input.chars() {
        if char == '+' || char == '-' {
            continue;
        }
        set.insert(char);
    }
    set
}

#[derive(Debug, Default, Deserialize)]
pub struct ManagedChannel {
    #[serde(default)]
    pub founders: HashSet<String>,
    #[serde(default)]
    pub crats: HashSet<String>,
    #[serde(default)]
    pub ops: HashSet<String>,
    #[serde(default)]
    pub plus_o: HashSet<String>,
    #[serde(default)]
    pub autovoice: HashSet<String>,
    #[serde(default)]
    pub global_bans: bool,
    #[serde(default)]
    pub bans: HashSet<String>,
    #[serde(default)]
    pub invexes: HashSet<String>,
    // unknown modes
    #[serde(default)]
    pub current: HashMap<String, HashSet<char>>,
    // state stuff
    #[serde(default)]
    pub flags_done: bool,
    #[serde(default)]
    pub bans_done: bool,
    #[serde(default)]
    pub invexes_done: bool,
}

#[derive(Default, Debug)]
struct FlagChange {
    current: HashSet<char>,
    should: HashSet<char>,
}

impl FlagChange {
    fn to_mode(&self) -> Option<String> {
        if self.current == self.should {
            // No changes needed
            return None;
        }
        dbg!(self);
        // Flags currently held but shouldn't hold
        let mut remove: Vec<char> =
            self.current.difference(&self.should).cloned().collect();
        remove.sort_unstable();
        // Flags that should be held but currently aren't
        let mut add: Vec<char> =
            self.should.difference(&self.current).cloned().collect();
        add.sort_unstable();
        let mut mode = "".to_string();
        if !remove.is_empty() {
            mode.push('-');
            mode.extend(remove);
        }
        if !add.is_empty() {
            mode.push('+');
            mode.extend(add);
        }
        Some(mode)
    }
}

impl ManagedChannel {
    pub fn is_done(&self) -> bool {
        self.flags_done && self.bans_done && self.invexes_done
    }

    pub fn fix_flags(&self, cfg: &ManagedChannel) -> Vec<(String, String)> {
        let mut changes: HashMap<String, FlagChange> = HashMap::new();
        for (name, flags) in self.current.iter() {
            changes
                .entry(name.to_string())
                .or_default()
                .current
                .extend(flags);
        }

        for name in &cfg.founders {
            changes
                .entry(name.to_string())
                .or_default()
                .should
                .extend(FOUNDER.iter());
        }
        for name in &cfg.crats {
            changes
                .entry(name.to_string())
                .or_default()
                .should
                .extend(CRAT.iter());
        }
        for name in &cfg.ops {
            changes
                .entry(name.to_string())
                .or_default()
                .should
                .extend(OP.iter());
        }
        for name in &cfg.plus_o {
            changes
                .entry(name.to_string())
                .or_default()
                .should
                .extend(PLUS_O.iter());
        }
        for name in &cfg.autovoice {
            changes
                .entry(name.to_string())
                .or_default()
                .should
                .extend(AUTOVOICE.iter());
        }

        changes
            .iter()
            .filter_map(|(username, change)| {
                change.to_mode().map(|mode| (username.to_string(), mode))
            })
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
            self.current.insert(account, parse_flags(&caps[2]));
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
            current: [("foo".to_string(), FOUNDER.iter().cloned().collect())]
                .iter()
                .cloned()
                .collect(),
            ..Default::default()
        };
        let cfg = ManagedChannel {
            founders: set("bar"),
            ops: set("foo"),
            ..Default::default()
        };
        let mut res = managed.fix_flags(&cfg);
        res.sort();
        let expected = vec![
            ("bar".to_string(), "+AFRefiorstv".to_string()),
            ("foo".to_string(), "-FRefrs".to_string()),
        ];
        assert_eq!(expected, res);
    }

    #[test]
    fn test_flag_change() {
        let mut change = FlagChange::default();
        change.current.extend(['A', 'B', 'C'].iter());
        assert_eq!(&change.to_mode().unwrap(), "-ABC");
        change.should.extend(['A', 'B', 'D'].iter());
        assert_eq!(&change.to_mode().unwrap(), "-C+D");
        change.current.clear();
        assert_eq!(&change.to_mode().unwrap(), "+ABD");
    }

    #[test]
    fn test_parse_flags() {
        assert_eq!(parse_flags("+Vv"), vec!['v', 'V'].into_iter().collect(),);
    }
}
