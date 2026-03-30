use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
};
use serde::{Deserialize, Serialize};
use tracing::{error, info};

use crate::{db, models::Container, AppState};

#[derive(Debug, Deserialize)]
pub struct ListContainersQuery {
    pub show_stopped: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateContainerRequest {
    pub update_mode: String,
}

#[derive(Debug, Serialize)]
pub struct ApiError {
    pub error: String,
}

pub async fn list_containers(
    State(state): State<AppState>,
    Path(client_id): Path<String>,
    Query(query): Query<ListContainersQuery>,
) -> Result<Json<Vec<Container>>, (StatusCode, Json<ApiError>)> {
    let include_stopped = query.show_stopped.unwrap_or(false);

    match db::get_containers_for_client(&state.db, &client_id, include_stopped).await {
        Ok(containers) => {
            info!(
                "Listed {} containers for client {}",
                containers.len(),
                client_id
            );
            Ok(Json(containers))
        }
        Err(e) => {
            error!(
                "Failed to list containers for client {}: {}",
                client_id, e
            );
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    error: e.to_string(),
                }),
            ))
        }
    }
}

pub async fn update_container(
    State(state): State<AppState>,
    Path((client_id, container_name)): Path<(String, String)>,
    Json(body): Json<UpdateContainerRequest>,
) -> Result<Json<Container>, (StatusCode, Json<ApiError>)> {
    match db::update_container_mode(&state.db, &client_id, &container_name, &body.update_mode)
        .await
    {
        Ok(()) => {}
        Err(e) => {
            error!(
                "Failed to update container mode for {}/{}: {}",
                client_id, container_name, e
            );
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    error: e.to_string(),
                }),
            ));
        }
    }

    match db::get_containers_for_client(&state.db, &client_id, true).await {
        Ok(containers) => {
            if let Some(c) = containers
                .into_iter()
                .find(|c| c.container_name == container_name)
            {
                info!(
                    "Updated container mode for {}/{} to {}",
                    client_id, container_name, body.update_mode
                );
                Ok(Json(c))
            } else {
                Err((
                    StatusCode::NOT_FOUND,
                    Json(ApiError {
                        error: format!("Container {} not found", container_name),
                    }),
                ))
            }
        }
        Err(e) => {
            error!(
                "Failed to fetch updated container {}/{}: {}",
                client_id, container_name, e
            );
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    error: e.to_string(),
                }),
            ))
        }
    }
}
