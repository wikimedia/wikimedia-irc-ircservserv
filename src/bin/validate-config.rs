use anyhow::{anyhow, Result};
use std::path::Path;
use std::{env, fs, process};

// TODO: don't hardcode this
const ACCOUNT: &str = "ircservserv-wm";

use ircservserv::channel::ConfiguredChannel;

fn validate(path: &Path) -> Result<()> {
    let cfg: ConfiguredChannel = toml::from_str(&fs::read_to_string(path)?)?;
    if cfg.founders.len() > 4 {
        return Err(anyhow!("Can only have 4 founders"));
    }
    if !cfg.founders.contains(ACCOUNT) {
        return Err(anyhow!("{} must be listed as a founder", ACCOUNT));
    }
    Ok(())
}

fn main() -> Result<()> {
    let mut files = vec![];
    for (index, path) in env::args().enumerate() {
        if index == 0 {
            continue;
        }
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let path = entry.path();
            if let Some(ext) = path.extension() {
                if ext == "toml" {
                    files.push(path);
                }
            }
        }
    }
    if files.is_empty() {
        eprintln!("Error: no TOML files found");
        process::exit(1);
    }
    let mut error = false;
    for path in files {
        match validate(&path) {
            Ok(_) => println!("{}: passed validation", path.to_str().unwrap()),
            Err(e) => {
                println!("{}: failed validation", path.to_str().unwrap());
                println!("{}: {}", path.to_str().unwrap(), e.to_string());
                error = true;
            }
        }
    }
    if error {
        process::exit(1);
    }
    Ok(())
}
