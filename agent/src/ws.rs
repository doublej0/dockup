use std::time::Duration;

use futures::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::{
    config::{AgentConfig, AGENT_VERSION},
    docker,
    registry::RegistryChecker,
    updater,
};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum ServerToAgent {
    CheckVersions,
    UpdateContainers { names: Vec<String> },
    UpdateAll,
    UpdateAgent,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum AgentToServer {
    ContainerList {
        containers: Vec<ContainerInfo>,
    },
    VersionCheckResult {
        container: String,
        current_digest: String,
        latest_digest: String,
        update_available: bool,
    },
    JobOutput {
        job_id: String,
        chunk: String,
    },
    JobComplete {
        job_id: String,
        success: bool,
    },
    AgentInfo {
        version: String,
        arch: String,
    },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ContainerInfo {
    pub container_name: String,
    pub image: String,
    pub status: String,
}


pub async fn run_agent_loop(config: AgentConfig) {
    let mut backoff_secs: u64 = 1;
    let registry = RegistryChecker::new();

    loop {
        let url = format!(
            "{}/api/ws/agent/{}?token={}",
            config.server_url.trim_end_matches('/'),
            config.client_id,
            config.jwt_token
        );

        info!("Connecting to server: {}", url);

        match connect_async(&url).await {
            Ok((ws_stream, _)) => {
                info!("Connected to server");
                backoff_secs = 1;

                handle_connection(ws_stream, &config, &registry).await;

                warn!("Disconnected from server, reconnecting...");
            }
            Err(e) => {
                error!(
                    "Failed to connect to server: {} — retrying in {}s",
                    e, backoff_secs
                );
            }
        }

        tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
        backoff_secs = (backoff_secs * 2).min(60);
    }
}

async fn handle_connection(
    ws_stream: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    config: &AgentConfig,
    registry: &RegistryChecker,
) {
    let (mut ws_tx, mut ws_rx) = ws_stream.split();
    let (out_tx, mut out_rx) = mpsc::channel::<AgentToServer>(64);

    // Task to send outgoing messages
    let send_task = tokio::spawn(async move {
        while let Some(msg) = out_rx.recv().await {
            let json = match serde_json::to_string(&msg) {
                Ok(j) => j,
                Err(e) => {
                    error!("Failed to serialize outgoing message: {}", e);
                    continue;
                }
            };
            if let Err(e) = ws_tx.send(Message::Text(json)).await {
                error!("Failed to send WebSocket message: {}", e);
                break;
            }
        }
    });

    // Send agent info immediately
    let arch = std::env::consts::ARCH.to_string();
    send_msg(
        &out_tx,
        AgentToServer::AgentInfo {
            version: AGENT_VERSION.to_string(),
            arch: arch.clone(),
        },
    )
    .await;

    // Send initial container list
    send_container_list(&out_tx).await;

    // Periodic container list broadcast (every 60s)
    let periodic_tx = out_tx.clone();
    let periodic_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        interval.tick().await; // skip first immediate tick
        loop {
            interval.tick().await;
            send_container_list(&periodic_tx).await;
        }
    });

    // Process incoming messages
    while let Some(result) = ws_rx.next().await {
        match result {
            Ok(Message::Text(text)) => {
                let msg: ServerToAgent = match serde_json::from_str(&text) {
                    Ok(m) => m,
                    Err(e) => {
                        warn!("Failed to parse server message: {}: {}", e, text);
                        continue;
                    }
                };
                handle_server_message(msg, config, registry, &out_tx).await;
            }
            Ok(Message::Ping(_)) => {
                // Pong handled by tungstenite automatically
            }
            Ok(Message::Close(_)) => {
                info!("Server sent close frame");
                break;
            }
            Ok(_) => {}
            Err(e) => {
                error!("WebSocket receive error: {}", e);
                break;
            }
        }
    }

    periodic_task.abort();
    send_task.abort();
}

async fn handle_server_message(
    msg: ServerToAgent,
    config: &AgentConfig,
    registry: &RegistryChecker,
    out_tx: &mpsc::Sender<AgentToServer>,
) {
    match msg {
        ServerToAgent::CheckVersions => {
            info!("Received CheckVersions request");
            let containers = match docker::list_running_containers().await {
                Ok(c) => c,
                Err(e) => {
                    error!("Failed to list containers for version check: {}", e);
                    return;
                }
            };

            for container in containers {
                let result = registry
                    .check_image(&container.image, None)
                    .await;
                match result {
                    Ok(check) => {
                        send_msg(
                            out_tx,
                            AgentToServer::VersionCheckResult {
                                container: container.container_name.clone(),
                                current_digest: String::new(),
                                latest_digest: check.latest_digest,
                                update_available: check.update_available,
                            },
                        )
                        .await;
                    }
                    Err(e) => {
                        warn!(
                            "Failed to check version for {}: {}",
                            container.container_name, e
                        );
                    }
                }
            }
        }

        ServerToAgent::UpdateContainers { names } => {
            info!("Received UpdateContainers for: {:?}", names);
            let containers = match docker::list_all_containers().await {
                Ok(c) => c,
                Err(e) => {
                    error!("Failed to list containers: {}", e);
                    return;
                }
            };

            for name in &names {
                let image = containers
                    .iter()
                    .find(|c| &c.container_name == name)
                    .map(|c| c.image.clone())
                    .unwrap_or_default();

                run_container_update(
                    name,
                    &image,
                    config.compose_file_path.as_deref(),
                    out_tx,
                )
                .await;
            }
        }

        ServerToAgent::UpdateAll => {
            info!("Received UpdateAll request");
            let containers = match docker::list_running_containers().await {
                Ok(c) => c,
                Err(e) => {
                    error!("Failed to list containers for UpdateAll: {}", e);
                    return;
                }
            };

            for container in containers {
                run_container_update(
                    &container.container_name,
                    &container.image,
                    config.compose_file_path.as_deref(),
                    out_tx,
                )
                .await;
            }
        }

        ServerToAgent::UpdateAgent => {
            info!("Received UpdateAgent request");
            let arch = std::env::consts::ARCH;
            match crate::selfupdate::check_for_update(AGENT_VERSION).await {
                Some(version) => {
                    if let Err(e) = crate::selfupdate::perform_update(&version, arch).await {
                        error!("Self-update failed: {}", e);
                    }
                }
                None => {
                    info!("No update available for agent");
                }
            }
        }
    }
}

async fn run_container_update(
    name: &str,
    image: &str,
    compose_path: Option<&str>,
    out_tx: &mpsc::Sender<AgentToServer>,
) {
    let job_id = Uuid::new_v4().to_string();
    info!("Starting update job {} for container {}", job_id, name);

    let (chunk_tx, mut chunk_rx) = mpsc::channel::<String>(64);
    let out_tx_clone = out_tx.clone();
    let job_id_clone = job_id.clone();

    // Spawn task to forward output chunks
    let forward_task = tokio::spawn(async move {
        while let Some(chunk) = chunk_rx.recv().await {
            send_msg(
                &out_tx_clone,
                AgentToServer::JobOutput {
                    job_id: job_id_clone.clone(),
                    chunk,
                },
            )
            .await;
        }
    });

    let success = updater::run_update(name, image, compose_path, chunk_tx).await;

    forward_task.await.ok();

    send_msg(
        out_tx,
        AgentToServer::JobComplete {
            job_id: job_id.clone(),
            success,
        },
    )
    .await;

    info!("Update job {} finished: success={}", job_id, success);
}

async fn send_container_list(out_tx: &mpsc::Sender<AgentToServer>) {
    match docker::list_all_containers().await {
        Ok(containers) => {
            let list: Vec<ContainerInfo> = containers
                .into_iter()
                .map(|c| ContainerInfo {
                    container_name: c.container_name,
                    image: c.image,
                    status: c.status,
                })
                .collect();
            info!("Sending container list: {} containers", list.len());
            send_msg(out_tx, AgentToServer::ContainerList { containers: list }).await;
        }
        Err(e) => {
            error!("Failed to list containers: {}", e);
        }
    }
}

async fn send_msg(tx: &mpsc::Sender<AgentToServer>, msg: AgentToServer) {
    if let Err(e) = tx.send(msg).await {
        error!("Failed to queue outgoing message: {}", e);
    }
}
