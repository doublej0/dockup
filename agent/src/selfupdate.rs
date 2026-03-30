use anyhow::Result;
use sha2::{Digest, Sha256};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use tracing::{error, info};

pub async fn check_for_update(current_version: &str) -> Option<String> {
    let client = reqwest::Client::builder()
        .user_agent("dockup-agent/0.1.0")
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .ok()?;

    let resp = client
        .get("https://api.github.com/repos/doublej0/dockup/releases/latest")
        .send()
        .await
        .ok()?;

    let release: serde_json::Value = resp.json().await.ok()?;

    let tag_name = release.get("tag_name")?.as_str()?;

    // Strip leading "v" or "agent-" prefix for comparison
    let latest = tag_name
        .trim_start_matches("agent-")
        .trim_start_matches('v');

    if latest != current_version {
        info!(
            "New agent version available: {} (current: {})",
            latest, current_version
        );
        Some(latest.to_string())
    } else {
        info!("Agent is up to date ({})", current_version);
        None
    }
}

pub async fn perform_update(version: &str, arch: &str) -> Result<()> {
    info!("Performing agent self-update to version {} ({})", version, arch);

    let client = reqwest::Client::builder()
        .user_agent("dockup-agent/0.1.0")
        .timeout(std::time::Duration::from_secs(120))
        .build()?;

    let binary_url = format!(
        "https://github.com/doublej0/dockup/releases/download/agent-{}/dockup-agent-{}",
        version, arch
    );
    let checksum_url = format!(
        "https://github.com/doublej0/dockup/releases/download/agent-{}/dockup-agent-{}.sha256",
        version, arch
    );

    info!("Downloading binary from {}", binary_url);
    let binary_bytes = client.get(&binary_url).send().await?.bytes().await?;

    info!("Downloading checksum from {}", checksum_url);
    let checksum_text = client
        .get(&checksum_url)
        .send()
        .await?
        .text()
        .await?;

    let expected_hash = checksum_text
        .split_whitespace()
        .next()
        .ok_or_else(|| anyhow::anyhow!("Invalid checksum file format"))?;

    // Verify checksum
    let mut hasher = Sha256::new();
    hasher.update(&binary_bytes);
    let actual_hash = hex::encode(hasher.finalize());

    if actual_hash != expected_hash {
        anyhow::bail!(
            "Checksum mismatch: expected {}, got {}",
            expected_hash,
            actual_hash
        );
    }

    info!("Checksum verified");

    let tmp_path = "/tmp/dockup-agent-new";
    let mut file = std::fs::File::create(tmp_path)?;
    file.write_all(&binary_bytes)?;

    // chmod +x
    let mut perms = std::fs::metadata(tmp_path)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(tmp_path, perms)?;

    info!("New binary written to {}, executing...", tmp_path);

    // Replace current binary and exec
    std::fs::rename(tmp_path, "/usr/local/bin/dockup-agent")?;

    // exec new binary (replaces current process)
    use std::os::unix::process::CommandExt;
    let err = std::process::Command::new("/usr/local/bin/dockup-agent")
        .exec();

    // exec only returns on error
    error!("Failed to exec new binary: {}", err);
    Err(err.into())
}
