use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use serde::Serialize;
use std::sync::Arc;

use crate::jobs::JobManager;

#[derive(Serialize)]
pub struct JobResponse {
    id: i64,
    job_type: String,
    status: String,
    progress: f64,
    payload: Option<serde_json::Value>,
    created_at: String,
    updated_at: String,
}

pub fn router(job_manager: Arc<JobManager>) -> Router {
    Router::new()
        .route("/:id", get(get_job))
        .route("/:id/cancel", post(cancel_job))
        .with_state(job_manager)
}

async fn get_job(
    State(job_manager): State<Arc<JobManager>>,
    Path(id): Path<i64>,
) -> Result<Json<JobResponse>, StatusCode> {
    let job = job_manager
        .get_job(id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    Ok(Json(JobResponse {
        id: job.id,
        job_type: serde_json::to_string(&job.job_type).unwrap_or_default(),
        status: serde_json::to_string(&job.status).unwrap_or_default(),
        progress: job.progress,
        payload: job.payload,
        created_at: job.created_at.to_rfc3339(),
        updated_at: job.updated_at.to_rfc3339(),
    }))
}

async fn cancel_job(
    State(job_manager): State<Arc<JobManager>>,
    Path(id): Path<i64>,
) -> Result<Json<()>, StatusCode> {
    job_manager
        .cancel_job(id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(()))
}
