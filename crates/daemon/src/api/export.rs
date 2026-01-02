use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
    routing::post,
    Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::db::Database;
use crate::jobs::{JobManager, JobType};
use engine::render::generate_render_commands;
use engine::timeline::Timeline;
use serde_json::json;

#[derive(Deserialize)]
pub struct ExportRequest {
    preset: Option<String>,
    out_path: String,
}

#[derive(Serialize)]
pub struct ExportResponse {
    job_id: i64,
}

pub fn router(db: Arc<Database>, job_manager: Arc<JobManager>) -> Router {
    Router::new()
        .route("/:id/export", post(export))
        .with_state((db, job_manager))
}

async fn export(
    State((db, job_manager)): State<(Arc<Database>, Arc<JobManager>)>,
    Path(project_id): Path<i64>,
    Json(req): Json<ExportRequest>,
) -> Result<Json<ExportResponse>, StatusCode> {
    // Load timeline
    let timeline_json = db
        .get_timeline(project_id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    
    let timeline: Timeline = serde_json::from_str(&timeline_json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Get proxy paths for all asset IDs in timeline
    let mut proxy_paths = HashMap::new();
    for track in &timeline.tracks {
        for clip in &track.clips {
            if !proxy_paths.contains_key(&clip.asset_id) {
                if let Ok(Some(path)) = db.get_proxy_path(clip.asset_id) {
                    proxy_paths.insert(clip.asset_id, path);
                }
            }
        }
    }

    // Generate render command
    let output_path = PathBuf::from(&req.out_path);
    let render_cmd = generate_render_commands(&timeline, output_path.clone(), &proxy_paths);

    // Create export job with render command
    let job_payload = json!({
        "preset": req.preset,
        "out_path": req.out_path,
        "ffmpeg_args": render_cmd.ffmpeg_args,
    });

    let job_id = job_manager
        .create_job(JobType::Export, Some(job_payload), None)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // TODO: Spawn async task to execute FFmpeg command
    // For V1, just return job_id - execution can be added later

    Ok(Json(ExportResponse { job_id }))
}
