use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::db::Database;
use serde_json::{json, Value};

#[derive(Serialize)]
pub struct TimelineResponse {
    timeline: Value, // JSON representation of timeline
}

#[derive(Deserialize)]
pub struct ApplyOperationsRequest {
    operations: Vec<Value>, // Simplified - would be TimelineOperation enums
}

#[derive(Deserialize)]
pub struct DiffRequest {
    from: Value,
    to: Value,
}

pub fn router(db: Arc<Database>) -> Router {
    Router::new()
        .route("/:id/timeline", get(get_timeline))
        .route("/:id/timeline/apply", post(apply_operations))
        .route("/:id/timeline/diff", post(log_diff))
        .with_state(db)
}

async fn get_timeline(
    State(db): State<Arc<Database>>,
    Path(project_id): Path<i64>,
) -> Result<Json<TimelineResponse>, StatusCode> {
    // Load timeline from DB - return empty timeline if it doesn't exist yet
    let timeline = if let Some(timeline_json) = db
        .get_timeline(project_id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    {
        serde_json::from_str(&timeline_json)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    } else {
        // Return empty timeline structure if none exists
        json!({
            "tracks": [],
            "captions": [],
            "music": []
        })
    };
    
    Ok(Json(TimelineResponse { timeline }))
}

async fn apply_operations(
    State(_db): State<Arc<Database>>,
    Path(_project_id): Path<i64>,
    Json(_req): Json<ApplyOperationsRequest>,
) -> Result<Json<TimelineResponse>, StatusCode> {
    // Placeholder - would apply operations and update timeline
    Ok(Json(TimelineResponse {
        timeline: json!({}),
    }))
}

async fn log_diff(
    State(_db): State<Arc<Database>>,
    Path(_project_id): Path<i64>,
    Json(_req): Json<DiffRequest>,
) -> Result<Json<()>, StatusCode> {
    // Placeholder - would generate diff and log to edit_logs table
    Ok(Json(()))
}
