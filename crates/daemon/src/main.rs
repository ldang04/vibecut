use axum::{response::Json, routing::get, Router};
use serde::Serialize;
use std::{net::SocketAddr, path::PathBuf, sync::Arc};
use tracing::{info, level_filters::LevelFilter};
use tracing_subscriber;
use tower_http::cors::{CorsLayer, Any};

mod api;
mod db;
mod embeddings;
mod jobs;
mod llm;
mod media;
mod planner;
mod orchestrator;
mod retrieval;
mod twelvelabs;

#[derive(Serialize)]
struct HealthResponse {
    ok: bool,
    version: &'static str,
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        ok: true,
        version: "0.1.0",
    })
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::INFO)
        .init();

    // Initialize database
    // For now, use a local SQLite file. In production, this should be configurable
    let db_path = PathBuf::from(".cache/vibecut.db");
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let db = Arc::new(db::Database::new(&db_path)?);
    info!("Database initialized at {:?}", db_path);

    // Initialize job manager
    let job_manager = Arc::new(jobs::JobManager::new(db.clone()));

    // Initialize and spawn job processor
    let job_processor = jobs::processor::JobProcessor::new(db.clone(), job_manager.clone());
    let _processor_handle = tokio::spawn(async move {
        job_processor.run().await;
    });

    // Initialize and spawn agent event loop
    let agent_db = db.clone();
    let agent_job_manager = job_manager.clone();
    let _agent_handle = tokio::spawn(async move {
        orchestrator::events::agent_event_loop(agent_db, agent_job_manager).await;
    });

    // Build the router with CORS support
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any)
        .allow_credentials(false);
    
    let app = Router::new()
        .route("/health", get(health))
        .nest("/api", api::router(db.clone(), job_manager))
        .layer(cors);

    // Start the server
    let addr = SocketAddr::from(([127, 0, 0, 1], 7777));
    info!("Starting daemon server on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}