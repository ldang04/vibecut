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
use crate::planner::generate_edit_plan;
use engine::compiler::{compile_edit_plan, EditConstraints};
use engine::timeline::{ProjectSettings, Resolution, TICKS_PER_SECOND};
use serde_json;

#[derive(Deserialize)]
pub struct GenerateRequest {
    target_length: Option<i64>,
    vibe: Option<String>,
    captions_on: Option<bool>,
    music_on: Option<bool>,
}

#[derive(Serialize)]
pub struct GenerateResponse {
    job_id: i64,
}

pub fn router(db: Arc<Database>) -> Router {
    Router::new()
        .route("/:id/generate", post(generate))
        .with_state(db)
}

async fn generate(
    State(db): State<Arc<Database>>,
    Path(project_id): Path<i64>,
    Json(req): Json<GenerateRequest>,
) -> Result<Json<GenerateResponse>, StatusCode> {
    // Verify project exists
    let _project = db
        .get_project(project_id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    // Load segments for project
    let segments_with_assets = db
        .get_segments_for_project(project_id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if segments_with_assets.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    // Create constraints
    let constraints = EditConstraints {
        target_length: req.target_length,
        vibe: req.vibe,
        captions_on: req.captions_on.unwrap_or(true),
        music_on: req.music_on.unwrap_or(true),
    };

    // Generate edit plan
    let plan = generate_edit_plan(&segments_with_assets, constraints);

    // Create project settings from first media asset
    let first_asset = &segments_with_assets[0].1;
    let fps = first_asset.fps_num as f64 / first_asset.fps_den as f64;
    let settings = ProjectSettings {
        fps,
        resolution: Resolution {
            width: first_asset.width,
            height: first_asset.height,
        },
        sample_rate: 48000,
        ticks_per_second: TICKS_PER_SECOND,
    };

    // Compile to timeline
    let timeline = compile_edit_plan(plan, settings);

    // Serialize and store timeline
    let timeline_json = serde_json::to_string(&timeline)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    db.store_timeline(project_id, &timeline_json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Return success (for now, synchronous. Can make async with job later)
    Ok(Json(GenerateResponse { job_id: 0 }))
}
