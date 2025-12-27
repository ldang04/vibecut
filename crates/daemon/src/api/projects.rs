use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
    routing::{delete, get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::db::Database;

#[derive(Deserialize)]
pub struct CreateProjectRequest {
    name: String,
    cache_dir: String,
}

#[derive(Serialize)]
pub struct CreateProjectResponse {
    id: i64,
}

#[derive(Serialize)]
pub struct ProjectResponse {
    id: i64,
    name: String,
    created_at: String,
    cache_dir: String,
    style_profile_id: Option<i64>,
}

pub fn router(db: Arc<Database>) -> Router {
    Router::new()
        .route("/", get(list_projects))
        .route("/", post(create_project))
        .route("/:id", get(get_project))
        .route("/:id", delete(delete_project))
        .with_state(db.clone())
}

async fn list_projects(
    State(db): State<Arc<Database>>,
) -> Result<Json<Vec<ProjectResponse>>, StatusCode> {
    let projects = db
        .get_all_projects()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    let responses: Vec<ProjectResponse> = projects
        .into_iter()
        .map(|project| ProjectResponse {
            id: project.id,
            name: project.name,
            created_at: project.created_at.to_rfc3339(),
            cache_dir: project.cache_dir,
            style_profile_id: project.style_profile_id,
        })
        .collect();
    
    Ok(Json(responses))
}

async fn create_project(
    State(db): State<Arc<Database>>,
    Json(req): Json<CreateProjectRequest>,
) -> Result<Json<CreateProjectResponse>, StatusCode> {
    let id = db
        .create_project(&req.name, &req.cache_dir)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    Ok(Json(CreateProjectResponse { id }))
}

async fn get_project(
    State(db): State<Arc<Database>>,
    Path(id): Path<i64>,
) -> Result<Json<ProjectResponse>, StatusCode> {
    let project = db
        .get_project(id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    Ok(Json(ProjectResponse {
        id: project.id,
        name: project.name,
        created_at: project.created_at.to_rfc3339(),
        cache_dir: project.cache_dir,
        style_profile_id: project.style_profile_id,
    }))
}

async fn delete_project(
    State(db): State<Arc<Database>>,
    Path(id): Path<i64>,
) -> Result<StatusCode, StatusCode> {
    db.delete_project(id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    Ok(StatusCode::NO_CONTENT)
}
