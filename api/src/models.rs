use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Client {
    pub id: String,
    pub name: String,
    pub host: String,
    pub color: String,
    pub compose_file_path: Option<String>,
    pub agent_version: Option<String>,
    pub agent_update_mode: String,
    pub last_seen: Option<String>,
    pub connected: bool,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
pub struct ClientWithStats {
    pub id: String,
    pub name: String,
    pub host: String,
    pub color: String,
    pub compose_file_path: Option<String>,
    pub agent_version: Option<String>,
    pub agent_update_mode: String,
    pub last_seen: Option<String>,
    pub connected: bool,
    pub created_at: String,
    pub updates_available: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Container {
    pub id: String,
    pub client_id: String,
    pub container_name: String,
    pub image: String,
    pub current_digest: Option<String>,
    pub latest_digest: Option<String>,
    pub update_available: bool,
    pub update_mode: String,
    pub status: String,
    pub checked_at: Option<String>,
    pub compose_service: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct UpdateJob {
    pub id: String,
    pub client_id: String,
    pub container_name: String,
    pub image: String,
    pub from_digest: Option<String>,
    pub to_digest: Option<String>,
    pub status: String,
    pub output: Option<String>,
    pub started_at: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct OnboardClientRequest {
    pub name: String,
    pub host: String,
    pub color: String,
    pub compose_file_path: Option<String>,
    pub ssh_user: String,
    pub ssh_password: String,
    pub agent_update_mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum ServerToAgent {
    CheckVersions,
    UpdateContainers { names: Vec<String> },
    UpdateAll,
    UpdateAgent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum ServerToUI {
    ClientConnected {
        client_id: String,
    },
    ClientDisconnected {
        client_id: String,
    },
    ContainerUpdate {
        client_id: String,
        container: Container,
    },
    JobProgress {
        job_id: String,
        chunk: String,
    },
    JobComplete {
        job_id: String,
        success: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerInfo {
    pub container_name: String,
    pub image: String,
    pub status: String,
    pub image_id: Option<String>,
    pub compose_service: Option<String>,
}
