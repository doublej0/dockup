use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
};
use serde::{Deserialize, Serialize};
use tracing::{error, info};
use uuid::Uuid;

use crate::{
    db,
    models::{ServerToAgent, UpdateJob},
    AppState,
};

#[derive(Debug, Deserialize)]
pub struct TriggerUpdateRequest {
    pub container_names: Option<Vec<String>>,
    pub all: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct RecentJobsQuery {
    pub client_id: Option<String>,
    pub status: Option<String>,
    pub page: Option<i64>,
    pub per_page: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct ApiError {
    pub error: String,
}

pub async fn trigger_update(
    State(state): State<AppState>,
    Path(client_id): Path<String>,
    Json(body): Json<TriggerUpdateRequest>,
) -> Result<Json<Vec<UpdateJob>>, (StatusCode, Json<ApiError>)> {
    let client = match db::get_client(&state.db, &client_id).await {
        Ok(Some(c)) => c,
        Ok(None) => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(ApiError {
                    error: format!("Client {} not found", client_id),
                }),
            ))
        }
        Err(e) => {
            error!("Failed to fetch client {}: {}", client_id, e);
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    error: e.to_string(),
                }),
            ));
        }
    };

    let containers = match db::get_containers_for_client(&state.db, &client_id, false).await {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to get containers for client {}: {}", client_id, e);
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    error: e.to_string(),
                }),
            ));
        }
    };

    let target_containers: Vec<_> = if body.all.unwrap_or(false) {
        containers
            .iter()
            .filter(|c| c.update_available)
            .collect()
    } else if let Some(ref names) = body.container_names {
        containers
            .iter()
            .filter(|c| names.contains(&c.container_name))
            .collect()
    } else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                error: "Must specify container_names or all=true".to_string(),
            }),
        ));
    };

    if target_containers.is_empty() {
        return Ok(Json(vec![]));
    }

    let now = chrono::Utc::now().to_rfc3339();
    let mut jobs = vec![];

    for container in &target_containers {
        let job = UpdateJob {
            id: Uuid::new_v4().to_string(),
            client_id: client_id.clone(),
            container_name: container.container_name.clone(),
            image: container.image.clone(),
            from_digest: container.current_digest.clone(),
            to_digest: container.latest_digest.clone(),
            status: "pending".to_string(),
            output: None,
            started_at: now.clone(),
            completed_at: None,
        };

        if let Err(e) = db::insert_update_job(&state.db, &job).await {
            error!("Failed to insert update job: {}", e);
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    error: e.to_string(),
                }),
            ));
        }

        jobs.push(job);
    }

    let msg = if body.all.unwrap_or(false) {
        ServerToAgent::UpdateAll
    } else {
        ServerToAgent::UpdateContainers {
            names: target_containers
                .iter()
                .map(|c| c.container_name.clone())
                .collect(),
        }
    };

    state.hub.send_to_agent(&client.id, msg).await;

    info!(
        "Triggered update for {} containers on client {}",
        jobs.len(),
        client_id
    );

    Ok(Json(jobs))
}

pub async fn list_jobs(
    State(state): State<AppState>,
    Path(client_id): Path<String>,
) -> Result<Json<Vec<UpdateJob>>, (StatusCode, Json<ApiError>)> {
    match db::get_jobs_for_client(&state.db, &client_id).await {
        Ok(jobs) => {
            info!("Listed {} jobs for client {}", jobs.len(), client_id);
            Ok(Json(jobs))
        }
        Err(e) => {
            error!("Failed to list jobs for client {}: {}", client_id, e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    error: e.to_string(),
                }),
            ))
        }
    }
}

pub async fn get_job_handler(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Result<Json<UpdateJob>, (StatusCode, Json<ApiError>)> {
    match db::get_job(&state.db, &job_id).await {
        Ok(Some(job)) => Ok(Json(job)),
        Ok(None) => Err((
            StatusCode::NOT_FOUND,
            Json(ApiError {
                error: format!("Job {} not found", job_id),
            }),
        )),
        Err(e) => {
            error!("Failed to get job {}: {}", job_id, e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    error: e.to_string(),
                }),
            ))
        }
    }
}

pub async fn check_versions(
    State(state): State<AppState>,
    Path(client_id): Path<String>,
) -> Result<axum::http::StatusCode, (axum::http::StatusCode, Json<ApiError>)> {
    match db::get_client(&state.db, &client_id).await {
        Ok(Some(_)) => {
            state
                .hub
                .send_to_agent(&client_id, crate::models::ServerToAgent::CheckVersions)
                .await;
            info!("Sent CheckVersions to agent {}", client_id);
            Ok(axum::http::StatusCode::OK)
        }
        Ok(None) => Err((
            axum::http::StatusCode::NOT_FOUND,
            Json(ApiError {
                error: format!("Client {} not found", client_id),
            }),
        )),
        Err(e) => Err((
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: e.to_string(),
            }),
        )),
    }
}

pub async fn get_recent_jobs_handler(
    State(state): State<AppState>,
    Query(query): Query<RecentJobsQuery>,
) -> Result<Json<Vec<UpdateJob>>, (StatusCode, Json<ApiError>)> {
    let per_page = query.per_page.unwrap_or(50).min(200);
    let page = query.page.unwrap_or(1).max(1);
    let offset = (page - 1) * per_page;

    match db::get_recent_jobs_filtered(
        &state.db,
        query.client_id.as_deref(),
        query.status.as_deref(),
        per_page,
        offset,
    )
    .await
    {
        Ok(jobs) => {
            info!("Fetched {} recent jobs", jobs.len());
            Ok(Json(jobs))
        }
        Err(e) => {
            error!("Failed to get recent jobs: {}", e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    error: e.to_string(),
                }),
            ))
        }
    }
}
