use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, Query, State,
    },
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::sync::{broadcast, mpsc};
use tracing::{error, info, warn};

use crate::{
    auth::validate_agent_jwt,
    db,
    models::{AgentToServer, Container, ServerToUI},
    AppState,
};

type AgentTx = mpsc::Sender<String>;

pub struct WsHub {
    agents: Mutex<HashMap<String, AgentTx>>,
    ui_tx: broadcast::Sender<ServerToUI>,
}

impl WsHub {
    pub fn new() -> Arc<Self> {
        let (ui_tx, _) = broadcast::channel(256);
        Arc::new(Self {
            agents: Mutex::new(HashMap::new()),
            ui_tx,
        })
    }

    pub async fn send_to_agent(&self, client_id: &str, msg: crate::models::ServerToAgent) {
        let tx = {
            let agents = self.agents.lock().unwrap();
            agents.get(client_id).cloned()
        };
        if let Some(tx) = tx {
            let json = match serde_json::to_string(&msg) {
                Ok(j) => j,
                Err(e) => {
                    error!("Failed to serialize agent message: {}", e);
                    return;
                }
            };
            if let Err(e) = tx.send(json).await {
                warn!("Failed to send to agent {}: {}", client_id, e);
            }
        } else {
            warn!("No agent connection for client {}", client_id);
        }
    }

    pub fn broadcast_ui(&self, msg: ServerToUI) {
        // Ignore send errors — no subscribers is fine
        let _ = self.ui_tx.send(msg);
    }

    pub fn subscribe_ui(&self) -> broadcast::Receiver<ServerToUI> {
        self.ui_tx.subscribe()
    }

    fn register_agent(&self, client_id: String, tx: AgentTx) {
        let mut agents = self.agents.lock().unwrap();
        agents.insert(client_id, tx);
    }

    fn unregister_agent(&self, client_id: &str) {
        let mut agents = self.agents.lock().unwrap();
        agents.remove(client_id);
    }

    pub fn get_connected_agent_ids(&self) -> Vec<String> {
        let agents = self.agents.lock().unwrap();
        agents.keys().cloned().collect()
    }
}

#[derive(Debug, Deserialize)]
pub struct AgentWsQuery {
    pub token: String,
}

pub async fn agent_ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Path(client_id): Path<String>,
    Query(query): Query<AgentWsQuery>,
) -> impl IntoResponse {
    let validated = validate_agent_jwt(&query.token, &state.jwt_secret);
    match validated {
        Ok(sub) if sub == client_id => {
            info!("Agent WebSocket upgrade for client {}", client_id);
            ws.on_upgrade(move |socket| handle_agent_socket(socket, state, client_id))
        }
        Ok(sub) => {
            warn!("JWT sub {} does not match client_id {}", sub, client_id);
            ws.on_upgrade(|mut socket| async move {
                let _ = socket.send(Message::Close(None)).await;
            })
        }
        Err(e) => {
            warn!("Invalid JWT for agent {}: {}", client_id, e);
            ws.on_upgrade(|mut socket| async move {
                let _ = socket.send(Message::Close(None)).await;
            })
        }
    }
}

async fn handle_agent_socket(socket: WebSocket, state: AppState, client_id: String) {
    let (mut ws_tx, mut ws_rx) = socket.split();
    let (cmd_tx, mut cmd_rx) = mpsc::channel::<String>(64);

    state.hub.register_agent(client_id.clone(), cmd_tx);

    let now = chrono::Utc::now().to_rfc3339();
    if let Err(e) = db::set_client_connected(&state.db, &client_id, true, Some(&now)).await {
        error!("Failed to set client connected for {}: {}", client_id, e);
    }

    state.hub.broadcast_ui(ServerToUI::ClientConnected {
        client_id: client_id.clone(),
    });

    info!("Agent connected: {}", client_id);

    // Trigger immediate version check on connect
    state.hub.send_to_agent(&client_id, crate::models::ServerToAgent::CheckVersions).await;
    info!("Sent CheckVersions to agent {}", client_id);

    // Spawn task to forward commands to the WebSocket
    let forward_task = tokio::spawn(async move {
        while let Some(msg) = cmd_rx.recv().await {
            if ws_tx.send(Message::Text(msg)).await.is_err() {
                break;
            }
        }
    });

    // Process incoming messages from the agent
    while let Some(result) = ws_rx.next().await {
        match result {
            Ok(Message::Text(text)) => {
                handle_agent_message(&state, &client_id, &text).await;
            }
            Ok(Message::Ping(data)) => {
                // Pong is handled automatically by axum
                let _ = data;
            }
            Ok(Message::Close(_)) => {
                info!("Agent {} sent close frame", client_id);
                break;
            }
            Ok(_) => {}
            Err(e) => {
                warn!("Agent {} WebSocket error: {}", client_id, e);
                break;
            }
        }
    }

    forward_task.abort();
    state.hub.unregister_agent(&client_id);

    let now = chrono::Utc::now().to_rfc3339();
    if let Err(e) =
        db::set_client_connected(&state.db, &client_id, false, Some(&now)).await
    {
        error!(
            "Failed to set client disconnected for {}: {}",
            client_id, e
        );
    }

    state.hub.broadcast_ui(ServerToUI::ClientDisconnected {
        client_id: client_id.clone(),
    });

    info!("Agent disconnected: {}", client_id);
}

async fn handle_agent_message(state: &AppState, client_id: &str, text: &str) {
    let msg: AgentToServer = match serde_json::from_str(text) {
        Ok(m) => m,
        Err(e) => {
            warn!(
                "Failed to parse agent message from {}: {} — raw: {}",
                client_id, e, text
            );
            return;
        }
    };

    match msg {
        AgentToServer::ContainerList { containers } => {
            info!(
                "Received container list from {}: {} containers",
                client_id,
                containers.len()
            );
            // Use the client's agent_update_mode as the default for newly inserted containers.
            // On conflict the UPDATE SET intentionally omits update_mode, so existing
            // user-set values are preserved.
            let default_update_mode = match db::get_client(&state.db, client_id).await {
                Ok(Some(ref c)) => c.agent_update_mode.clone(),
                _ => "manual".to_string(),
            };
            for info_item in containers {
                let container = Container {
                    id: uuid::Uuid::new_v4().to_string(),
                    client_id: client_id.to_string(),
                    container_name: info_item.container_name.clone(),
                    image: info_item.image.clone(),
                    current_digest: None,
                    latest_digest: None,
                    update_available: false,
                    update_mode: default_update_mode.clone(),
                    status: info_item.status.clone(),
                    checked_at: None,
                };
                if let Err(e) = db::upsert_container(&state.db, &container).await {
                    error!("Failed to upsert container {}: {}", info_item.container_name, e);
                }
                // Fetch the actual stored container to get real data
                if let Ok(containers) =
                    db::get_containers_for_client(&state.db, client_id, true).await
                {
                    if let Some(c) = containers
                        .into_iter()
                        .find(|c| c.container_name == info_item.container_name)
                    {
                        state.hub.broadcast_ui(ServerToUI::ContainerUpdate {
                            client_id: client_id.to_string(),
                            container: c,
                        });
                    }
                }
            }
        }

        AgentToServer::VersionCheckResult {
            container,
            current_digest,
            latest_digest,
            update_available,
        } => {
            info!(
                "Version check result for {}/{}: update_available={}",
                client_id, container, update_available
            );
            if let Err(e) = db::update_container_digest(
                &state.db,
                client_id,
                &container,
                &current_digest,
                &latest_digest,
                update_available,
            )
            .await
            {
                error!("Failed to update container digest: {}", e);
            }
            if let Ok(containers) =
                db::get_containers_for_client(&state.db, client_id, true).await
            {
                if let Some(c) = containers
                    .into_iter()
                    .find(|c| c.container_name == container)
                {
                    state.hub.broadcast_ui(ServerToUI::ContainerUpdate {
                        client_id: client_id.to_string(),
                        container: c,
                    });
                }
            }
        }

        AgentToServer::JobOutput { job_id, chunk } => {
            if let Err(e) = db::append_job_output(&state.db, &job_id, &chunk).await {
                error!("Failed to append job output for {}: {}", job_id, e);
            }
            state.hub.broadcast_ui(ServerToUI::JobProgress {
                job_id: job_id.clone(),
                chunk,
            });
        }

        AgentToServer::JobComplete { job_id, success } => {
            info!("Job {} complete: success={}", job_id, success);
            let status = if success { "success" } else { "failed" };
            let now = chrono::Utc::now().to_rfc3339();
            if let Err(e) =
                db::update_job_status(&state.db, &job_id, status, None, Some(&now)).await
            {
                error!("Failed to update job status for {}: {}", job_id, e);
            }
            state.hub.broadcast_ui(ServerToUI::JobComplete {
                job_id: job_id.clone(),
                success,
            });
        }

        AgentToServer::AgentInfo { version, arch: _ } => {
            info!("Agent info from {}: version={}", client_id, version);
            if let Err(e) = db::set_client_agent_version(&state.db, client_id, &version).await {
                error!("Failed to set agent version for {}: {}", client_id, e);
            }
        }
    }
}

pub async fn ui_ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    info!("UI WebSocket upgrade");
    ws.on_upgrade(move |socket| handle_ui_socket(socket, state))
}

async fn handle_ui_socket(socket: WebSocket, state: AppState) {
    let (mut ws_tx, mut ws_rx) = socket.split();
    let mut rx = state.hub.subscribe_ui();

    let forward_task = tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(msg) => {
                    let json = match serde_json::to_string(&msg) {
                        Ok(j) => j,
                        Err(e) => {
                            error!("Failed to serialize UI message: {}", e);
                            continue;
                        }
                    };
                    if ws_tx.send(Message::Text(json)).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Closed) => break,
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!("UI WebSocket lagged by {} messages", n);
                }
            }
        }
    });

    // Drain incoming messages (we don't use them from UI)
    while let Some(result) = ws_rx.next().await {
        match result {
            Ok(Message::Close(_)) => break,
            Ok(_) => {}
            Err(e) => {
                warn!("UI WebSocket error: {}", e);
                break;
            }
        }
    }

    forward_task.abort();
    info!("UI WebSocket disconnected");
}
