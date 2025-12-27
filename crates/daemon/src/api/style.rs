use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
    routing::post,
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::db::Database;
use crate::jobs::{JobManager, JobType};
use serde_json::json;

#[derive(Deserialize)]
pub struct ImportReferenceRequest {
    folder_path: String,
}

#[derive(Serialize)]
pub struct ImportReferenceResponse {
    job_id: i64,
    style_profile_id: Option<i64>,
}

pub fn router(db: Arc<Database>, job_manager: Arc<JobManager>) -> Router {
    Router::new()
        .route("/:id/import_reference", post(import_reference))
        .with_state((db, job_manager))
}

async fn import_reference(
    State((db, job_manager)): State<(Arc<Database>, Arc<JobManager>)>,
    Path(project_id): Path<i64>,
    Json(req): Json<ImportReferenceRequest>,
) -> Result<Json<ImportReferenceResponse>, StatusCode> {
    // Verify project exists
    let _project = db
        .get_project(project_id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    // Create job for style profile analysis
    let job_payload = json!({
        "project_id": project_id,
        "folder_path": req.folder_path,
    });

    let job_id = job_manager
        .create_job(JobType::ImportRaw, Some(job_payload)) // Using ImportRaw as placeholder
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // In production, would call ML service to analyze references and create style profile
    // For now, return job_id
    Ok(Json(ImportReferenceResponse {
        job_id,
        style_profile_id: None,
    }))
}
