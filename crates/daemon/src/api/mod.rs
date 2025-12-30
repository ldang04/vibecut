use axum::Router;
use std::sync::Arc;

use crate::db::Database;
use crate::jobs::JobManager;

pub mod export;
pub mod generate;
pub mod jobs;
pub mod media;
pub mod orchestrator;
pub mod projects;
pub mod style;
pub mod timeline;

pub fn router(db: Arc<Database>, job_manager: Arc<JobManager>) -> Router {
    Router::new()
        .nest("/projects", {
            Router::new()
                .merge(projects::router(db.clone()))
                .merge(media::router(db.clone(), job_manager.clone()))
                .merge(style::router(db.clone(), job_manager.clone()))
                .merge(generate::router(db.clone()))
                .merge(timeline::router(db.clone()))
                .merge(orchestrator::router(db.clone(), job_manager.clone()))
                .merge(export::router(db, job_manager.clone()))
        })
        .nest("/jobs", jobs::router(job_manager))
}
