use tokio::sync::mpsc;
use tracing::{error, info};

use crate::docker::recreate_container;

pub async fn run_update(
    container_name: &str,
    image: &str,
    compose_path: Option<&str>,
    compose_service: Option<&str>,
    tx: mpsc::Sender<String>,
) -> bool {
    info!("Starting update for container: {}", container_name);

    let send = |msg: String| {
        let tx = tx.clone();
        async move {
            if let Err(e) = tx.send(msg).await {
                error!("Failed to send update output: {}", e);
            }
        }
    };

    send(format!("Starting update for container: {}\n", container_name)).await;

    match recreate_container(container_name, image, compose_path, compose_service).await {
        Ok(output) => {
            send(output).await;
            send(format!(
                "Container {} updated successfully.\n",
                container_name
            ))
            .await;
            info!("Update complete for container: {}", container_name);
            true
        }
        Err(e) => {
            let msg = format!("Failed to update container {}: {}\n", container_name, e);
            error!("{}", msg);
            send(msg).await;
            false
        }
    }
}
