use anyhow::{anyhow, Result};
use tokio::process;

/// Execute a git command
async fn git(args: &[&str]) -> Result<String> {
    println!("Running $ git {}", args.join(" "));
    let output = process::Command::new("git").args(args).output().await?;
    if output.status.success() {
        Ok(String::from_utf8(output.stdout)?)
    } else {
        Err(anyhow!(
            "Running git {} failed: {}",
            args.join(" "),
            output.status.code().unwrap_or_default()
        ))
    }
}

/// Get the sha1 of the specified ref
async fn sha1(ref_: &str) -> Result<String> {
    git(&["rev-parse", ref_])
        .await
        .map(|s| s.trim().to_string())
}

/// What are the changed files between two commits?
async fn changes(first: &str, second: &str) -> Result<Vec<String>> {
    let res = tokio::try_join!(sha1(first), sha1(second));
    let (first_sha1, second_sha1) = match res {
        Ok((first_sha1, second_sha1)) => (first_sha1, second_sha1),
        Err(e) => return Err(e),
    };
    Ok(git(&["diff", "--name-only", &first_sha1, &second_sha1])
        .await?
        .trim()
        .split('\n')
        .map(|s| s.to_string())
        .collect())
}

/// Fetch updates to the config repo, identify which are changes
/// to channels and then actually pull it.
pub async fn pull() -> Result<Vec<String>> {
    // Fetch remote updates
    git(&["fetch"]).await?;
    // Identify changes to channel configs
    let changed = changes("HEAD", "origin/master")
        .await?
        .iter()
        // Turn "channels/foo.toml" -> "#foo"
        .filter_map(|file| {
            if file.starts_with("channels/") && file.ends_with(".toml") {
                Some(format!(
                    "#{}",
                    file.trim_start_matches("channels/")
                        .trim_end_matches(".toml")
                ))
            } else {
                None
            }
        })
        .collect();
    // Now actually pull the repo!
    // TODO: race condition if a commit is merged between fetch and pull?
    git(&["pull"]).await?;
    Ok(changed)
}
