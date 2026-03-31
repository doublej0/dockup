use std::io::Write;

use axum::{extract::State, http::StatusCode, response::Json};
use serde::Serialize;
use ssh2::Session;
use std::net::TcpStream;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::{
    auth::generate_agent_jwt,
    db,
    models::{Client, OnboardClientRequest},
    AppState,
};

#[derive(Debug, Serialize)]
pub struct ApiError {
    pub error: String,
}

pub async fn onboard_client(
    State(state): State<AppState>,
    Json(body): Json<OnboardClientRequest>,
) -> Result<Json<Client>, (StatusCode, Json<ApiError>)> {
    let client_id = Uuid::new_v4().to_string();
    let jwt = match generate_agent_jwt(&client_id, &state.jwt_secret) {
        Ok(t) => t,
        Err(e) => {
            error!("Failed to generate JWT: {}", e);
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    error: e.to_string(),
                }),
            ));
        }
    };

    info!("Starting onboarding for client {} ({})", body.name, body.host);

    // Clone SSH credentials into owned strings — we'll drop them at end of function
    let ssh_host = body.host.clone();
    let ssh_user = body.ssh_user.clone();
    let ssh_password = body.ssh_password.clone();
    let compose_file_path = body.compose_file_path.clone();
    let agent_update_mode = body.agent_update_mode.clone();
    let public_api_url = state.public_api_url.clone();
    let client_id_clone = client_id.clone();
    let jwt_clone = jwt.clone();

    let result = tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        // Connect TCP
        let addr = format!("{}:22", ssh_host);
        info!("Connecting to {} via SSH", addr);
        let tcp = TcpStream::connect(&addr)?;
        let mut sess = Session::new()?;
        sess.set_tcp_stream(tcp);
        sess.handshake()?;
        sess.userauth_password(&ssh_user, &ssh_password)?;

        if !sess.authenticated() {
            anyhow::bail!("SSH authentication failed");
        }
        info!("SSH authenticated for {}", ssh_host);

        // Detect architecture
        let arch = run_ssh_command(&sess, "uname -m")?;
        let arch = arch.trim().to_string();
        info!("Detected architecture: {}", arch);

        let binary_arch = match arch.as_str() {
            "x86_64" => "x86_64",
            "aarch64" | "arm64" => "aarch64",
            "armv7l" => "armv7",
            other => anyhow::bail!("Unsupported architecture: {}", other),
        };

        // Create config directory
        run_ssh_command(&sess, "sudo mkdir -p /etc/dockup-agent")?;
        info!("Created config directory");

        // Write config file
        let compose_line = if let Some(ref path) = compose_file_path {
            format!("compose_file_path = \"{}\"\n", path)
        } else {
            String::new()
        };

        let config_content = format!(
            "server_url = \"{}\"\nclient_id = \"{}\"\njwt_token = \"{}\"\nagent_update_mode = \"{}\"\n{}",
            public_api_url, client_id_clone, jwt_clone, agent_update_mode, compose_line
        );

        write_file_via_ssh(
            &sess,
            "/etc/dockup-agent/config.toml",
            config_content.as_bytes(),
            true,
        )?;
        info!("Wrote agent config");

        // Download binary
        let binary_url = format!("{}/api/agent/download/{}", public_api_url, binary_arch);
        info!("Downloading agent binary from {}", binary_url);

        let download_cmd = format!(
            "sudo curl -fsSL -o /usr/local/bin/dockup-agent '{}'",
            binary_url
        );
        run_ssh_command(&sess, &download_cmd)?;
        run_ssh_command(&sess, "sudo chmod +x /usr/local/bin/dockup-agent")?;
        info!("Agent binary installed");

        // Write systemd service
        let service_content = r#"[Unit]
Description=DockUp Agent
After=network.target docker.service
Requires=docker.service

[Service]
Type=simple
ExecStart=/usr/local/bin/dockup-agent
Restart=always
RestartSec=5
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
"#;

        write_file_via_ssh(
            &sess,
            "/etc/systemd/system/dockup-agent.service",
            service_content.as_bytes(),
            true,
        )?;
        info!("Wrote systemd service file");

        // Enable and start service
        run_ssh_command(&sess, "sudo systemctl daemon-reload")?;
        run_ssh_command(&sess, "sudo systemctl enable dockup-agent")?;
        run_ssh_command(&sess, "sudo systemctl restart dockup-agent")?;
        info!("Started dockup-agent service");

        // SSH credentials are dropped here as the closure ends
        drop(ssh_password);
        drop(ssh_user);

        Ok(())
    })
    .await;

    match result {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            error!("SSH onboarding failed: {}", e);
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    error: format!("SSH onboarding failed: {}", e),
                }),
            ));
        }
        Err(e) => {
            error!("SSH task panicked: {}", e);
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    error: "Internal error during onboarding".to_string(),
                }),
            ));
        }
    }

    // Wait up to 30 seconds for agent to connect
    info!("Waiting for agent {} to connect...", client_id);
    let now = chrono::Utc::now().to_rfc3339();

    let client = Client {
        id: client_id.clone(),
        name: body.name.clone(),
        host: body.host.clone(),
        color: body.color.clone(),
        compose_file_path: body.compose_file_path.clone(),
        agent_version: None,
        agent_update_mode: body.agent_update_mode.clone(),
        last_seen: None,
        connected: false,
        created_at: now,
    };

    if let Err(e) = db::insert_client(&state.db, &client).await {
        error!("Failed to insert client record: {}", e);
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: e.to_string(),
            }),
        ));
    }

    // Poll for up to 30 seconds
    let mut connected = false;
    for i in 0..30 {
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        match db::get_client(&state.db, &client_id).await {
            Ok(Some(ref c)) if c.connected => {
                info!("Agent {} connected after {}s", client_id, i + 1);
                connected = true;
                break;
            }
            _ => {}
        }
    }

    if !connected {
        warn!(
            "Agent {} did not connect within 30s — returning client anyway",
            client_id
        );
    }

    match db::get_client(&state.db, &client_id).await {
        Ok(Some(c)) => {
            info!("Onboarding complete for client {}", client_id);
            Ok(Json(c))
        }
        Ok(None) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: "Client record not found after insert".to_string(),
            }),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: e.to_string(),
            }),
        )),
    }
}

fn run_ssh_command(sess: &Session, cmd: &str) -> anyhow::Result<String> {
    let mut channel = sess.channel_session()?;
    channel.exec(cmd)?;

    let mut output = String::new();
    use std::io::Read;
    channel.read_to_string(&mut output)?;

    let mut stderr = String::new();
    channel.stderr().read_to_string(&mut stderr)?;

    channel.wait_close()?;
    let exit_code = channel.exit_status()?;

    if exit_code != 0 {
        anyhow::bail!(
            "Command '{}' exited with code {}: {}",
            cmd,
            exit_code,
            stderr.trim()
        );
    }

    Ok(output)
}

fn write_file_via_ssh(
    sess: &Session,
    remote_path: &str,
    content: &[u8],
    use_sudo: bool,
) -> anyhow::Result<()> {
    if use_sudo {
        // Write to a temp file then move with sudo
        let tmp_path = format!("/tmp/dockup_tmp_{}", uuid::Uuid::new_v4());
        {
            let mut channel = sess.scp_send(
                std::path::Path::new(&tmp_path),
                0o644,
                content.len() as u64,
                None,
            )?;
            channel.write_all(content)?;
            channel.send_eof()?;
            channel.wait_eof()?;
            channel.close()?;
            channel.wait_close()?;
        }
        run_ssh_command(sess, &format!("sudo mv {} {}", tmp_path, remote_path))?;
    } else {
        let mut channel = sess.scp_send(
            std::path::Path::new(remote_path),
            0o644,
            content.len() as u64,
            None,
        )?;
        channel.write_all(content)?;
        channel.send_eof()?;
        channel.wait_eof()?;
        channel.close()?;
        channel.wait_close()?;
    }
    Ok(())
}
