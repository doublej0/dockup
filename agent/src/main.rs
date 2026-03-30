use tracing::{error, info};

mod config;
mod docker;
mod registry;
mod selfupdate;
mod updater;
mod ws;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let config = match config::AgentConfig::load() {
        Ok(c) => {
            info!("Loaded config: server={}", c.server_url);
            c
        }
        Err(e) => {
            error!("Failed to load config from /etc/dockup-agent/config.toml: {}", e);
            std::process::exit(1);
        }
    };

    // Verify Docker socket is accessible
    match docker::list_running_containers().await {
        Ok(containers) => {
            info!(
                "Docker socket accessible — {} running containers",
                containers.len()
            );
        }
        Err(e) => {
            error!(
                "Cannot access Docker socket: {}. Is the socket mounted?",
                e
            );
            std::process::exit(1);
        }
    }

    // Spawn auto-update background task if enabled
    if config.agent_update_mode == "auto" {
        let _update_task = tokio::spawn(async {
            let arch = std::env::consts::ARCH;
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(86400));
            interval.tick().await; // skip immediate first tick
            loop {
                interval.tick().await;
                info!("Checking for agent updates...");
                if let Some(version) = selfupdate::check_for_update(env!("CARGO_PKG_VERSION")).await {
                    info!("Performing auto-update to agent version {}", version);
                    if let Err(e) = selfupdate::perform_update(&version, arch).await {
                        error!("Auto-update failed: {}", e);
                    }
                }
            }
        });
    }

    // Run WebSocket connection loop (this blocks forever, reconnecting on failure)
    ws::run_agent_loop(config).await;

    Ok(())
}
