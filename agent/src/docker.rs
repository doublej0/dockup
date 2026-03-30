use anyhow::Result;
use bollard::{container::ListContainersOptions, Docker};
use futures::StreamExt;
use std::collections::HashMap;
use tracing::{error, info};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ContainerInfo {
    pub container_name: String,
    pub image: String,
    pub status: String,
}

fn get_docker() -> Result<Docker> {
    Ok(Docker::connect_with_local_defaults()?)
}

fn parse_status(state: Option<&str>) -> String {
    match state {
        Some("running") => "running".to_string(),
        Some(s) => s.to_string(),
        None => "unknown".to_string(),
    }
}

pub async fn list_running_containers() -> Result<Vec<ContainerInfo>> {
    let docker = get_docker()?;
    let mut filters = HashMap::new();
    filters.insert("status", vec!["running"]);

    let options = ListContainersOptions {
        all: false,
        filters,
        ..Default::default()
    };

    let containers = docker.list_containers(Some(options)).await?;

    let result = containers
        .into_iter()
        .map(|c| {
            let name = c
                .names
                .unwrap_or_default()
                .first()
                .cloned()
                .unwrap_or_default()
                .trim_start_matches('/')
                .to_string();
            ContainerInfo {
                container_name: name,
                image: c.image.unwrap_or_default(),
                status: parse_status(c.state.as_deref()),
            }
        })
        .collect();

    Ok(result)
}

pub async fn list_all_containers() -> Result<Vec<ContainerInfo>> {
    let docker = get_docker()?;

    let options = ListContainersOptions::<String> {
        all: true,
        ..Default::default()
    };

    let containers = docker.list_containers(Some(options)).await?;

    let result = containers
        .into_iter()
        .map(|c| {
            let name = c
                .names
                .unwrap_or_default()
                .first()
                .cloned()
                .unwrap_or_default()
                .trim_start_matches('/')
                .to_string();
            ContainerInfo {
                container_name: name,
                image: c.image.unwrap_or_default(),
                status: parse_status(c.state.as_deref()),
            }
        })
        .collect();

    Ok(result)
}

#[allow(dead_code)]
pub async fn pull_image(image: &str) -> Result<()> {
    info!("Pulling image: {}", image);
    let docker = get_docker()?;

    // Parse image into name and tag
    let (image_name, tag) = if let Some(idx) = image.rfind(':') {
        (&image[..idx], &image[idx + 1..])
    } else {
        (image, "latest")
    };

    let options = bollard::image::CreateImageOptions {
        from_image: image_name,
        tag,
        ..Default::default()
    };

    let mut stream = docker.create_image(Some(options), None, None);
    while let Some(result) = stream.next().await {
        match result {
            Ok(info) => {
                if let Some(status) = info.status {
                    tracing::debug!("Pull progress: {}", status);
                }
            }
            Err(e) => {
                error!("Error pulling image {}: {}", image, e);
                return Err(e.into());
            }
        }
    }

    info!("Successfully pulled image: {}", image);
    Ok(())
}

pub async fn recreate_container(
    name: &str,
    image: &str,
    compose_path: Option<&str>,
) -> Result<String> {
    info!("Recreating container: {} (image: {})", name, image);

    if let Some(path) = compose_path {
        // Use docker compose
        let pull_output = std::process::Command::new("docker")
            .args(["compose", "-f", path, "pull", name])
            .output()?;

        let up_output = std::process::Command::new("docker")
            .args(["compose", "-f", path, "up", "-d", name])
            .output()?;

        let combined = format!(
            "{}{}{}{}",
            String::from_utf8_lossy(&pull_output.stdout),
            String::from_utf8_lossy(&pull_output.stderr),
            String::from_utf8_lossy(&up_output.stdout),
            String::from_utf8_lossy(&up_output.stderr),
        );

        info!("docker compose recreate output for {}: {}", name, combined);
        Ok(combined)
    } else {
        // Standalone container: pull, stop, remove, recreate
        let mut output = String::new();

        // Pull the new image
        let pull_out = std::process::Command::new("docker")
            .args(["pull", image])
            .output()?;
        output.push_str(&String::from_utf8_lossy(&pull_out.stdout));
        output.push_str(&String::from_utf8_lossy(&pull_out.stderr));

        // Get container details before stopping
        let inspect_out = std::process::Command::new("docker")
            .args(["inspect", "--format", "{{json .}}", name])
            .output()?;
        let inspect_str = String::from_utf8_lossy(&inspect_out.stdout);

        // Stop container
        let stop_out = std::process::Command::new("docker")
            .args(["stop", name])
            .output()?;
        output.push_str(&String::from_utf8_lossy(&stop_out.stdout));
        output.push_str(&String::from_utf8_lossy(&stop_out.stderr));

        // Remove container
        let rm_out = std::process::Command::new("docker")
            .args(["rm", name])
            .output()?;
        output.push_str(&String::from_utf8_lossy(&rm_out.stdout));
        output.push_str(&String::from_utf8_lossy(&rm_out.stderr));

        // Try to parse run args from inspect output and re-run
        // This is a best-effort approach for standalone containers
        if let Ok(inspect_json) = serde_json::from_str::<serde_json::Value>(&inspect_str) {
            let run_cmd = build_run_command_from_inspect(name, image, &inspect_json);
            let run_out = std::process::Command::new("sh")
                .arg("-c")
                .arg(&run_cmd)
                .output()?;
            output.push_str(&format!("Running: {}\n", run_cmd));
            output.push_str(&String::from_utf8_lossy(&run_out.stdout));
            output.push_str(&String::from_utf8_lossy(&run_out.stderr));
        } else {
            // Fallback: just run the container with minimal args
            let run_out = std::process::Command::new("docker")
                .args(["run", "-d", "--name", name, image])
                .output()?;
            output.push_str(&String::from_utf8_lossy(&run_out.stdout));
            output.push_str(&String::from_utf8_lossy(&run_out.stderr));
        }

        info!("Container {} recreated", name);
        Ok(output)
    }
}

fn build_run_command_from_inspect(
    name: &str,
    image: &str,
    inspect: &serde_json::Value,
) -> String {
    let mut args = vec!["docker".to_string(), "run".to_string(), "-d".to_string()];
    args.push(format!("--name={}", name));

    // Restart policy
    if let Some(restart) = inspect
        .pointer("/HostConfig/RestartPolicy/Name")
        .and_then(|v| v.as_str())
    {
        if !restart.is_empty() && restart != "no" {
            args.push(format!("--restart={}", restart));
        }
    }

    // Port bindings
    if let Some(ports) = inspect
        .pointer("/HostConfig/PortBindings")
        .and_then(|v| v.as_object())
    {
        for (container_port, bindings) in ports {
            if let Some(bindings_arr) = bindings.as_array() {
                for binding in bindings_arr {
                    let host_port = binding
                        .get("HostPort")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if !host_port.is_empty() {
                        args.push(format!("-p={}:{}", host_port, container_port));
                    }
                }
            }
        }
    }

    // Environment variables
    if let Some(env) = inspect
        .pointer("/Config/Env")
        .and_then(|v| v.as_array())
    {
        for e in env {
            if let Some(env_str) = e.as_str() {
                args.push(format!("-e={}", env_str));
            }
        }
    }

    // Volume bindings
    if let Some(binds) = inspect
        .pointer("/HostConfig/Binds")
        .and_then(|v| v.as_array())
    {
        for bind in binds {
            if let Some(bind_str) = bind.as_str() {
                args.push(format!("-v={}", bind_str));
            }
        }
    }

    args.push(image.to_string());

    args.join(" ")
}
