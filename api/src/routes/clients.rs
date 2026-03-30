use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
};
use serde::{Deserialize, Serialize};
use tracing::{error, info};

use crate::{db, AppState};

#[derive(Debug, Deserialize)]
pub struct UpdateClientRequest {
    pub name: String,
    pub color: String,
    pub compose_file_path: Option<String>,
    pub agent_update_mode: String,
}

#[derive(Debug, Serialize)]
pub struct ApiError {
    pub error: String,
}

pub async fn list_clients(
    State(state): State<AppState>,
) -> Result<Json<Vec<crate::models::Client>>, (StatusCode, Json<ApiError>)> {
    match db::get_all_clients(&state.db).await {
        Ok(clients) => {
            info!("Listed {} clients", clients.len());
            Ok(Json(clients))
        }
        Err(e) => {
            error!("Failed to list clients: {}", e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    error: e.to_string(),
                }),
            ))
        }
    }
}

pub async fn get_client(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<crate::models::Client>, (StatusCode, Json<ApiError>)> {
    match db::get_client(&state.db, &id).await {
        Ok(Some(client)) => {
            info!("Fetched client {}", id);
            Ok(Json(client))
        }
        Ok(None) => Err((
            StatusCode::NOT_FOUND,
            Json(ApiError {
                error: format!("Client {} not found", id),
            }),
        )),
        Err(e) => {
            error!("Failed to get client {}: {}", id, e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    error: e.to_string(),
                }),
            ))
        }
    }
}

pub async fn update_client(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateClientRequest>,
) -> Result<Json<crate::models::Client>, (StatusCode, Json<ApiError>)> {
    match db::update_client(
        &state.db,
        &id,
        &body.name,
        &body.color,
        body.compose_file_path.as_deref(),
        &body.agent_update_mode,
    )
    .await
    {
        Ok(()) => {}
        Err(e) => {
            error!("Failed to update client {}: {}", id, e);
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    error: e.to_string(),
                }),
            ));
        }
    }

    match db::get_client(&state.db, &id).await {
        Ok(Some(client)) => {
            info!("Updated client {}", id);
            Ok(Json(client))
        }
        Ok(None) => Err((
            StatusCode::NOT_FOUND,
            Json(ApiError {
                error: format!("Client {} not found", id),
            }),
        )),
        Err(e) => {
            error!("Failed to fetch updated client {}: {}", id, e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    error: e.to_string(),
                }),
            ))
        }
    }
}

pub async fn delete_client(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<ApiError>)> {
    match db::delete_client(&state.db, &id).await {
        Ok(()) => {
            info!("Deleted client {}", id);
            Ok(StatusCode::NO_CONTENT)
        }
        Err(e) => {
            error!("Failed to delete client {}: {}", id, e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    error: e.to_string(),
                }),
            ))
        }
    }
}
