use std::sync::Arc;

use axum::{
    routing::{delete, get, post, put},
    Router,
};
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::SqlitePool;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::info;

mod auth;
mod db;
mod models;
mod routes;

use routes::ws::WsHub;

#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
    pub hub: Arc<WsHub>,
    pub jwt_secret: String,
    pub public_api_url: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "sqlite:///data/dockup.db".to_string());

    let jwt_secret = std::env::var("JWT_SECRET").expect("JWT_SECRET must be set");
    let public_api_url = std::env::var("DOCKUP_PUBLIC_API_URL")
        .unwrap_or_else(|_| "http://localhost:3101".to_string());

    info!("Connecting to database: {}", database_url);

    let connect_options = database_url
        .parse::<SqliteConnectOptions>()?
        .create_if_missing(true);

    let pool = SqlitePool::connect_with(connect_options).await?;

    info!("Running migrations");
    sqlx::migrate!("./migrations").run(&pool).await?;

    let hub = WsHub::new();

    let state = AppState {
        db: pool,
        hub,
        jwt_secret,
        public_api_url,
    };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        // Clients
        .route("/api/clients", get(routes::clients::list_clients))
        .route("/api/clients/:id", get(routes::clients::get_client))
        .route("/api/clients/:id", put(routes::clients::update_client))
        .route("/api/clients/:id", delete(routes::clients::delete_client))
        // Containers
        .route(
            "/api/clients/:id/containers",
            get(routes::containers::list_containers),
        )
        .route(
            "/api/clients/:id/containers/:name",
            put(routes::containers::update_container),
        )
        // Updates
        .route(
            "/api/clients/:id/check-versions",
            post(routes::updates::check_versions),
        )
        .route(
            "/api/clients/:id/update",
            post(routes::updates::trigger_update),
        )
        .route(
            "/api/clients/:id/jobs",
            get(routes::updates::list_jobs),
        )
        .route("/api/jobs", get(routes::updates::get_recent_jobs_handler))
        .route("/api/jobs/:id", get(routes::updates::get_job_handler))
        // Agent binary distribution
        .route(
            "/api/agent/download/:arch",
            get(routes::agent::download_agent),
        )
        // Onboarding
        .route("/api/onboard", post(routes::onboarding::onboard_client))
        // WebSocket
        .route(
            "/api/ws/agent/:client_id",
            get(routes::ws::agent_ws_handler),
        )
        .route("/api/ws/ui", get(routes::ws::ui_ws_handler))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state.clone());

    let hub_clone = state.hub.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(6 * 60 * 60));
        interval.tick().await; // skip first tick — agents send on connect already
        loop {
            interval.tick().await;
            let agent_ids = hub_clone.get_connected_agent_ids();
            for id in agent_ids {
                hub_clone.send_to_agent(&id, crate::models::ServerToAgent::CheckVersions).await;
                tracing::info!("Periodic CheckVersions sent to agent {}", id);
            }
        }
    });

    let addr = "0.0.0.0:3101";
    info!("Listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
