use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;

pub const AGENT_VERSION: &str = "0.1.0";

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentConfig {
    pub server_url: String,
    pub client_id: String,
    pub jwt_token: String,
    pub compose_file_path: Option<String>,
    pub agent_update_mode: String,
}

impl AgentConfig {
    pub fn load() -> Result<Self> {
        let contents = fs::read_to_string("/etc/dockup-agent/config.toml")?;
        let config: AgentConfig = toml::from_str(&contents)?;
        Ok(config)
    }
}
