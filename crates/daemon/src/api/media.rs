use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode, HeaderMap},
    response::{Json, Response},
    routing::{delete, get, post},
    Router,
    body::Body,
};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::Arc};
use tokio_util::codec::{BytesCodec, FramedRead};
use futures::StreamExt;
use bytes::Bytes;
use tokio::io::{AsyncSeekExt, AsyncReadExt, SeekFrom};

use crate::db::Database;
use crate::jobs::{JobManager, JobType};
use crate::media::ffmpeg::FFmpegWrapper;
use crate::media::compute_file_checksum;
use serde_json::json;

#[derive(Deserialize, Clone, Debug)]
#[serde(default)]
pub struct ImportRawRequest {
    pub folder_path: Option<String>,
    pub file_paths: Option<Vec<String>>,
}

impl Default for ImportRawRequest {
    fn default() -> Self {
        Self {
            folder_path: None,
            file_paths: None,
        }
    }
}

#[derive(Serialize)]
pub struct ImportRawResponse {
    job_id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    job_ids: Option<Vec<i64>>, // For multiple file uploads
}

#[derive(Serialize)]
pub struct MediaAssetResponse {
    id: i64,
    path: String,
    duration_ticks: i64,
    width: i32,
    height: i32,
}

pub fn router(db: Arc<Database>, job_manager: Arc<JobManager>) -> Router {
    Router::new()
        .route("/:id/import_raw", post(import_raw))
        .route("/:id/media", get(list_media))
        .route("/:id/media/:asset_id", delete(delete_media_asset))
        .route("/:id/media/:asset_id/proxy", get(get_proxy_file))
        .route("/proxy/:asset_id", get(get_proxy_file_legacy)) // Legacy route for compatibility
        .with_state((db, job_manager))
}

async fn list_media(
    State((db, _job_manager)): State<(Arc<Database>, Arc<JobManager>)>,
    Path(project_id): Path<i64>,
) -> Result<Json<Vec<MediaAssetResponse>>, StatusCode> {
    // Get media assets for this specific project
    let assets = db
        .get_media_assets_for_project(project_id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    let response: Vec<MediaAssetResponse> = assets
        .into_iter()
        .map(|asset| MediaAssetResponse {
            id: asset.id,
            path: asset.path,
            duration_ticks: asset.duration_ticks,
            width: asset.width,
            height: asset.height,
        })
        .collect();
    
    Ok(Json(response))
}

async fn delete_media_asset(
    State((db, _job_manager)): State<(Arc<Database>, Arc<JobManager>)>,
    Path(params): Path<(i64, i64)>, // (project_id, asset_id)
) -> Result<StatusCode, StatusCode> {
    let (project_id, asset_id) = params;
    
    db.delete_media_asset(project_id, asset_id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct ProxyQuery {
    thumbnail: Option<bool>,
}

async fn get_proxy_file(
    State((db, _job_manager)): State<(Arc<Database>, Arc<JobManager>)>,
    Path(params): Path<(i64, i64)>, // (project_id, asset_id) for /:id/media/:asset_id/proxy
    Query(_query): Query<ProxyQuery>,
    headers: HeaderMap,
) -> Result<Response, StatusCode> {
    let (_project_id, asset_id) = params;
    serve_video_file(db, asset_id, headers).await
}

/// Legacy handler for /proxy/:asset_id route (without project_id)
async fn get_proxy_file_legacy(
    State((db, _job_manager)): State<(Arc<Database>, Arc<JobManager>)>,
    Path(asset_id): Path<i64>,
    Query(_query): Query<ProxyQuery>,
    headers: HeaderMap,
) -> Result<Response, StatusCode> {
    serve_video_file(db, asset_id, headers).await
}

/// Common logic to serve video file with range request support
async fn serve_video_file(
    db: Arc<Database>,
    asset_id: i64,
    headers: HeaderMap,
) -> Result<Response, StatusCode> {
    
    // Try to get proxy path, fallback to original file path
    let file_path = match db
        .get_proxy_path(asset_id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    {
        Some(proxy_path) => {
            let path = PathBuf::from(&proxy_path);
            if path.exists() {
                path
            } else {
                // Proxy file doesn't exist, fallback to original
                db.get_media_asset_path(asset_id)
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
                    .ok_or(StatusCode::NOT_FOUND)
                    .map(PathBuf::from)?
            }
        }
        None => {
            // No proxy exists, use original file
            db.get_media_asset_path(asset_id)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
                .ok_or(StatusCode::NOT_FOUND)
                .map(PathBuf::from)?
        }
    };

    if !file_path.exists() {
        return Err(StatusCode::NOT_FOUND);
    }

    // Get file metadata
    let metadata = tokio::fs::metadata(&file_path)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    let file_size = metadata.len();

    // Handle empty file
    if file_size == 0 {
        return Ok(Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "video/mp4")
            .header(header::ACCEPT_RANGES, "bytes")
            .header(header::CONTENT_LENGTH, "0")
            .body(Body::empty())
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?);
    }

    // Parse Range header if present
    let range_header = headers.get(header::RANGE);
    let (start, end, status_code) = if let Some(range_value) = range_header {
        if let Ok(range_str) = range_value.to_str() {
            if let Some(range) = parse_range(range_str, file_size) {
                (range.0, range.1, StatusCode::PARTIAL_CONTENT)
            } else {
                // Invalid range, return full file
                (0, file_size.saturating_sub(1), StatusCode::OK)
            }
        } else {
            (0, file_size.saturating_sub(1), StatusCode::OK)
        }
    } else {
        (0, file_size.saturating_sub(1), StatusCode::OK)
    };

    let content_length = end - start + 1;

    // Open file and seek to start position
    let mut file = tokio::fs::File::open(&file_path)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    
    file.seek(SeekFrom::Start(start))
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Create a limited reader for the range
    let limited_file = file.take(content_length);
    let stream = FramedRead::new(limited_file, BytesCodec::new());
    let body_stream = stream.map(|result| {
        result.map(|bytes| Bytes::from(bytes.freeze()))
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err))
    });
    let body = Body::from_stream(body_stream);

    // Build response with appropriate headers
    let mut response_builder = Response::builder()
        .status(status_code)
        .header(header::CONTENT_TYPE, "video/mp4")
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_LENGTH, content_length.to_string());

    // Add Content-Range header for partial content
    if status_code == StatusCode::PARTIAL_CONTENT {
        response_builder = response_builder.header(
            header::CONTENT_RANGE,
            format!("bytes {}-{}/{}", start, end, file_size),
        );
    }

    Ok(response_builder
        .body(body)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?)
}

/// Parse Range header value (e.g., "bytes=0-1023")
/// Returns (start, end) inclusive range, or None if invalid
fn parse_range(range_str: &str, file_size: u64) -> Option<(u64, u64)> {
    if !range_str.starts_with("bytes=") {
        return None;
    }

    let range = &range_str[6..]; // Skip "bytes="
    let parts: Vec<&str> = range.split('-').collect();
    if parts.len() != 2 {
        return None;
    }

    let start_str = parts[0].trim();
    let end_str = parts[1].trim();

    if start_str.is_empty() && end_str.is_empty() {
        return None;
    }

    let start = if start_str.is_empty() {
        // Suffix range: "-500" means last 500 bytes
        if file_size == 0 {
            return None; // Can't have suffix range for empty file
        }
        if let Ok(suffix) = end_str.parse::<u64>() {
            if suffix > file_size {
                0
            } else {
                file_size - suffix
            }
        } else {
            return None;
        }
    } else {
        start_str.parse::<u64>().ok()?
    };

    let end = if end_str.is_empty() {
        // Prefix range: "500-" means from byte 500 to end
        if file_size > 0 {
            file_size - 1
        } else {
            return None; // Can't have range for empty file
        }
    } else {
        end_str.parse::<u64>().ok()?
    };

    // Validate range
    if file_size == 0 || start > end || end >= file_size {
        return None;
    }

    Some((start, end))
}

async fn import_raw(
    State((db, job_manager)): State<(Arc<Database>, Arc<JobManager>)>,
    Path(project_id): Path<i64>,
    Json(req): Json<ImportRawRequest>,
) -> Result<Json<ImportRawResponse>, StatusCode> {
    // Verify project exists
    let _project = db
        .get_project(project_id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    // Validate that at least one field is provided
    if req.file_paths.is_none() && req.folder_path.is_none() {
        eprintln!("Import request missing both file_paths and folder_path");
        return Err(StatusCode::BAD_REQUEST);
    }

    // Debug logging
    eprintln!("Import request received: file_paths={:?}, folder_path={:?}", req.file_paths, req.folder_path);

    // Handle individual file paths - create a separate job for each file
    if let Some(file_paths) = req.file_paths {
        if file_paths.is_empty() {
            return Err(StatusCode::BAD_REQUEST);
        }

        let mut job_ids = Vec::new();
        let db_clone = db.clone();
        let job_manager_clone = job_manager.clone();

        // Create a separate job for each file (don't filter by existence here - let the job handle it)
        for file_path_str in file_paths {
            let video_path = PathBuf::from(&file_path_str);
            let job_payload = json!({
                "project_id": project_id,
                "file_path": file_path_str,
            });

            let job_id = job_manager
                .create_job(JobType::ImportRaw, Some(job_payload))
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

            job_ids.push(job_id);

            // Spawn async task to process this single file
            let db_task = db_clone.clone();
            let job_manager_task = job_manager_clone.clone();
            let path_for_task = video_path.clone();

            tokio::spawn(async move {
                if let Err(e) = process_single_file_import(
                    db_task,
                    job_manager_task.clone(),
                    job_id,
                    path_for_task,
                )
                .await
                {
                    eprintln!("Import job {} failed: {:?}", job_id, e);
                    let _ = job_manager_task.update_job_status(job_id, crate::jobs::JobStatus::Failed, Some(0.0));
                }
            });
        }

        // Return the first job_id for backward compatibility, and all job_ids
        Ok(Json(ImportRawResponse {
            job_id: job_ids[0],
            job_ids: Some(job_ids),
        }))
    } else if let Some(folder_path) = req.folder_path {
        // Folder scanning mode - single job for all files in folder
        let job_payload = json!({
            "project_id": project_id,
            "folder_path": folder_path,
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
            if let Err(e) = process_import(
                db_clone,
                job_manager_clone.clone(),
                job_id,
                folder,
            )
            .await
            {
                eprintln!("Import job {} failed: {:?}", job_id, e);
                let _ = job_manager_clone.update_job_status(job_id, crate::jobs::JobStatus::Failed, Some(0.0));
            }
        });

        Ok(Json(ImportRawResponse {
            job_id,
            job_ids: None,
        }))
    } else {
        Err(StatusCode::BAD_REQUEST)
    }
}

/// Process a single file import (one file per job)
async fn process_single_file_import(
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

    process_single_video(
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

/// Process a single video file
async fn process_single_video(
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

    // Register media asset with project_id
    let asset_id = db.create_media_asset(
        project_id,
        video_path.to_str().unwrap(),
        checksum.as_ref().map(|s| s.as_str()),
        media_info.duration_ticks,
        media_info.fps_num,
        media_info.fps_den,
        media_info.width,
        media_info.height,
        media_info.has_audio,
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

async fn process_import(
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
        process_single_video(
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
