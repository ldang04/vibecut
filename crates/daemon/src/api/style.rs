use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
    routing::post,
    Router,
};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::Arc};

use crate::db::Database;
use crate::jobs::{JobManager, JobType};
use crate::media::ffmpeg::FFmpegWrapper;
use crate::media::compute_file_checksum;
use serde_json::json;

#[derive(Deserialize, Clone, Debug)]
#[serde(default)]
pub struct ImportReferenceRequest {
    pub folder_path: Option<String>,
    pub file_paths: Option<Vec<String>>,
}

impl Default for ImportReferenceRequest {
    fn default() -> Self {
        Self {
            folder_path: None,
            file_paths: None,
        }
    }
}

#[derive(Serialize)]
pub struct ImportReferenceResponse {
    job_id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    job_ids: Option<Vec<i64>>, // For multiple file uploads
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

    // Validate that at least one field is provided
    if req.file_paths.is_none() && req.folder_path.is_none() {
        eprintln!("Import reference request missing both file_paths and folder_path");
        return Err(StatusCode::BAD_REQUEST);
    }

    // Debug logging
    eprintln!("Import reference request received: file_paths={:?}, folder_path={:?}", req.file_paths, req.folder_path);

    // Handle individual file paths - create a separate job for each file
    if let Some(file_paths) = req.file_paths {
        if file_paths.is_empty() {
            return Err(StatusCode::BAD_REQUEST);
        }

        let mut job_ids = Vec::new();
        let db_clone = db.clone();
        let job_manager_clone = job_manager.clone();

        // Create a separate job for each file
        for file_path_str in file_paths {
            let video_path = PathBuf::from(&file_path_str);
            let job_payload = json!({
                "project_id": project_id,
                "file_path": file_path_str,
                "is_reference": true,
            });

            let job_id = job_manager
                .create_job(JobType::ImportRaw, Some(job_payload)) // Using ImportRaw job type but with is_reference flag
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

            job_ids.push(job_id);

            // Spawn async task to process this single file
            let db_task = db_clone.clone();
            let job_manager_task = job_manager_clone.clone();
            let path_for_task = video_path.clone();

            tokio::spawn(async move {
                if let Err(e) = process_single_file_import_reference(
                    db_task,
                    job_manager_task.clone(),
                    job_id,
                    path_for_task,
                )
                .await
                {
                    eprintln!("Import reference job {} failed: {:?}", job_id, e);
                    let _ = job_manager_task.update_job_status(job_id, crate::jobs::JobStatus::Failed, Some(0.0));
                }
            });
        }

        // Return the first job_id for backward compatibility, and all job_ids
        Ok(Json(ImportReferenceResponse {
            job_id: job_ids[0],
            job_ids: Some(job_ids),
            style_profile_id: None,
        }))
    } else if let Some(folder_path) = req.folder_path {
        // Folder scanning mode - single job for all files in folder
        let job_payload = json!({
            "project_id": project_id,
            "folder_path": folder_path,
            "is_reference": true,
        });

        let job_id = job_manager
            .create_job(JobType::ImportRaw, Some(job_payload))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        // Spawn async task to process import
        let db_clone = db.clone();
        let job_manager_clone = job_manager.clone();
        let folder = PathBuf::from(&folder_path);
        
        tokio::spawn(async move {
            if !folder.exists() {
                return;
            }
            if let Err(e) = process_import_reference(
                db_clone,
                job_manager_clone.clone(),
                job_id,
                folder,
            )
            .await
            {
                eprintln!("Import reference job {} failed: {:?}", job_id, e);
                let _ = job_manager_clone.update_job_status(job_id, crate::jobs::JobStatus::Failed, Some(0.0));
            }
        });

        Ok(Json(ImportReferenceResponse {
            job_id,
            job_ids: None,
            style_profile_id: None,
        }))
    } else {
        Err(StatusCode::BAD_REQUEST)
    }
}

/// Process a single file import for reference (one file per job)
async fn process_single_file_import_reference(
    db: Arc<Database>,
    job_manager: Arc<JobManager>,
    job_id: i64,
    video_path: PathBuf,
) -> anyhow::Result<()> {
    job_manager.update_job_status(job_id, crate::jobs::JobStatus::Running, Some(0.0))?;

    // Check if file exists before processing
    if !video_path.exists() {
        return Err(anyhow::anyhow!("File does not exist: {}", video_path.display()));
    }

    if !video_path.is_file() {
        return Err(anyhow::anyhow!("Path is not a file: {}", video_path.display()));
    }

    // Extract project_id from job payload
    let job = job_manager.get_job(job_id)?;
    let project_id = job
        .and_then(|j| j.payload)
        .and_then(|p| p.get("project_id").and_then(|v| v.as_i64()))
        .ok_or_else(|| anyhow::anyhow!("Missing project_id in job payload"))?;

    process_single_video_reference(
        db,
        job_manager.clone(),
        job_id,
        project_id,
        &video_path,
        0,
        1, // Only one file in this job
    )
    .await?;

    job_manager.update_job_status(job_id, crate::jobs::JobStatus::Completed, Some(1.0))?;
    Ok(())
}

/// Process a single reference video file
async fn process_single_video_reference(
    db: Arc<Database>,
    job_manager: Arc<JobManager>,
    job_id: i64,
    project_id: i64,
    video_path: &PathBuf,
    idx: usize,
    total_files: usize,
) -> anyhow::Result<()> {
    // Compute checksum
    let checksum: Option<String> = compute_file_checksum(video_path)
        .await
        .ok();

    // Probe media
    let media_info = FFmpegWrapper::probe(video_path).await?;

    // Register media asset with project_id and is_reference = true
    let asset_id = db.create_media_asset_with_reference_flag(
        project_id,
        video_path.to_str().unwrap(),
        checksum.as_ref().map(|s| s.as_str()),
        media_info.duration_ticks,
        media_info.fps_num,
        media_info.fps_den,
        media_info.width,
        media_info.height,
        media_info.has_audio,
        true, // This is a reference asset
    )?;

    // Queue proxy generation job
    let proxy_job_payload = json!({
        "media_asset_id": asset_id,
        "input_path": video_path.to_str().unwrap(),
    });
    let _proxy_job_id = job_manager.create_job(JobType::GenerateProxy, Some(proxy_job_payload))?;

    // Queue transcription job
    let transcribe_job_payload = json!({
        "media_asset_id": asset_id,
        "media_path": video_path.to_str().unwrap(),
    });
    let _transcribe_job_id = job_manager.create_job(JobType::Transcribe, Some(transcribe_job_payload))?;

    // Queue vision analysis job
    let vision_job_payload = json!({
        "media_asset_id": asset_id,
        "media_path": video_path.to_str().unwrap(),
    });
    let _vision_job_id = job_manager.create_job(JobType::AnalyzeVision, Some(vision_job_payload))?;

    // Update progress
    let progress = (idx + 1) as f64 / total_files as f64;
    job_manager.update_job_status(job_id, crate::jobs::JobStatus::Running, Some(progress))?;

    Ok(())
}

async fn process_import_reference(
    db: Arc<Database>,
    job_manager: Arc<JobManager>,
    job_id: i64,
    folder_path: PathBuf,
) -> anyhow::Result<()> {
    job_manager.update_job_status(job_id, crate::jobs::JobStatus::Running, Some(0.0))?;

    // Extract project_id from job payload
    let job = job_manager.get_job(job_id)?;
    let project_id = job
        .and_then(|j| j.payload)
        .and_then(|p| p.get("project_id").and_then(|v| v.as_i64()))
        .ok_or_else(|| anyhow::anyhow!("Missing project_id in job payload"))?;

    // Video file extensions
    let video_extensions: &[&str] = &["mp4", "mov", "avi", "mkv", "m4v", "webm"];

    // Scan for video files
    let mut video_files = Vec::new();
    if folder_path.is_dir() {
        let mut entries = tokio::fs::read_dir(&folder_path).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.is_file() {
                if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    let ext_lower = ext.to_lowercase();
                    if video_extensions.contains(&ext_lower.as_str()) {
                        video_files.push(path);
                    }
                }
            }
        }
    }

    let total_files = video_files.len();
    for (idx, video_path) in video_files.iter().enumerate() {
        process_single_video_reference(
            db.clone(),
            job_manager.clone(),
            job_id,
            project_id,
            video_path,
            idx,
            total_files,
        )
        .await?;
    }

    job_manager.update_job_status(job_id, crate::jobs::JobStatus::Completed, Some(1.0))?;
    Ok(())
}
