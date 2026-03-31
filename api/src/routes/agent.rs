use axum::{
    extract::Path,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};
use tracing::{error, info};

pub async fn download_agent(Path(arch): Path<String>) -> Response {
    let filename = match arch.as_str() {
        "x86_64" => "dockup-agent-x86_64",
        "aarch64" => "dockup-agent-aarch64",
        other => {
            error!("Unsupported arch requested: {}", other);
            return (
                StatusCode::BAD_REQUEST,
                format!("Unsupported arch: {}. Valid values: x86_64, aarch64", other),
            )
                .into_response();
        }
    };

    let path = format!("/app/agent-binaries/{}", filename);

    match tokio::fs::read(&path).await {
        Ok(bytes) => {
            info!("Serving agent binary: {}", filename);
            (
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, "application/octet-stream"),
                    (
                        header::CONTENT_DISPOSITION,
                        &format!("attachment; filename=\"{}\"", filename),
                    ),
                ],
                bytes,
            )
                .into_response()
        }
        Err(e) => {
            error!("Failed to read agent binary {}: {}", path, e);
            StatusCode::NOT_FOUND.into_response()
        }
    }
}
