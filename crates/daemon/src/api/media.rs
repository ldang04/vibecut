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

#[derive(Serialize)]
pub struct AudioAssetResponse {
    id: i64,
    path: String,
    duration_ticks: i64,
}

pub fn router(db: Arc<Database>, job_manager: Arc<JobManager>) -> Router {
    Router::new()
        .route("/:id/import_raw", post(import_raw))
        .route("/:id/media", get(list_media))
        .route("/:id/references", get(list_references))
        .route("/:id/audio", get(list_audio))
        .route("/:id/media/:asset_id", delete(delete_media_asset))
        .route("/:id/media/:asset_id/proxy", get(get_proxy_file))
        .route("/:id/media/:asset_id/thumbnail/:timestamp_ms", get(get_thumbnail))
        .route("/:id/media/:asset_id/generate_thumbnails", post(generate_thumbnails_for_asset))
        .route("/proxy/:asset_id", get(get_proxy_file_legacy)) // Legacy route for compatibility
        .with_state((db, job_manager))
}

async fn list_media(
    State((db, _job_manager)): State<(Arc<Database>, Arc<JobManager>)>,
    Path(project_id): Path<i64>,
) -> Result<Json<Vec<MediaAssetResponse>>, StatusCode> {
    // Get media assets for this specific project (excluding references)
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

async fn list_references(
    State((db, _job_manager)): State<(Arc<Database>, Arc<JobManager>)>,
    Path(project_id): Path<i64>,
) -> Result<Json<Vec<MediaAssetResponse>>, StatusCode> {
    // Get reference assets for this specific project
    let assets = db
        .get_reference_assets_for_project(project_id)
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

async fn list_audio(
    State((db, _job_manager)): State<(Arc<Database>, Arc<JobManager>)>,
    Path(project_id): Path<i64>,
) -> Result<Json<Vec<AudioAssetResponse>>, StatusCode> {
    // Get audio-only assets for this specific project (has_audio = true, width = 0, height = 0)
    // For now, return empty array as audio assets are not separately stored
    // This endpoint exists to prevent 404 errors
    Ok(Json(vec![]))
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

/// Get thumbnail image for a specific timestamp
async fn get_thumbnail(
    State((db, _job_manager)): State<(Arc<Database>, Arc<JobManager>)>,
    Path((project_id, asset_id, timestamp_ms)): Path<(i64, i64, String)>,
) -> Result<Response, StatusCode> {
    // Get thumbnail directory for this asset
    let thumbnail_dir = db.get_thumbnail_dir(asset_id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    
    // Parse timestamp (format: "0000" for 0 seconds, "0100" for 1 second, etc.)
    // The timestamp_ms is actually the second number (e.g., "0000" = 0s, "0100" = 1s)
    let timestamp_sec: u64 = timestamp_ms.parse()
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    
    // Construct thumbnail file path: {thumbnail_dir}/t_{timestamp_sec:04d}.jpg
    let thumbnail_filename = format!("t_{:04}.jpg", timestamp_sec);
    let thumbnail_path = PathBuf::from(&thumbnail_dir).join(&thumbnail_filename);
    
    if !thumbnail_path.exists() {
        return Err(StatusCode::NOT_FOUND);
    }
    
    // Read thumbnail file
    let thumbnail_data = tokio::fs::read(&thumbnail_path)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    
    // Get file metadata
    let metadata = tokio::fs::metadata(&thumbnail_path)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    let file_size = metadata.len();
    
    // Build response with image/jpeg content type
    let response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "image/jpeg")
        .header(header::CONTENT_LENGTH, file_size.to_string())
        .header(header::CACHE_CONTROL, "public, max-age=31536000") // Cache for 1 year
        .body(Body::from(thumbnail_data))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    Ok(response)
}

/// Generate thumbnails for an asset that doesn't have them yet
async fn generate_thumbnails_for_asset(
    State((db, _job_manager)): State<(Arc<Database>, Arc<JobManager>)>,
    Path((project_id, asset_id)): Path<(i64, i64)>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use std::path::Path;
    
    // Get asset path
    let asset_path = db.get_media_asset_path(asset_id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    
    // Check if thumbnails already exist
    if let Ok(Some(_)) = db.get_thumbnail_dir(asset_id) {
        // Thumbnails already exist
        return Ok(Json(json!({ "status": "already_exists" })));
    }
    
    // Generate thumbnails
    let cache_dir = PathBuf::from(".cache");
    let thumbnails_dir = cache_dir.join("thumbs").join(format!("asset_{}", asset_id));
    
    let thumbnail_dir_path = FFmpegWrapper::extract_thumbnails(
        Path::new(&asset_path),
        &thumbnails_dir,
    ).await
    .map_err(|e| {
        eprintln!("Failed to extract thumbnails: {:?}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    
    // Store thumbnail directory in database
    db.set_thumbnail_dir(asset_id, &thumbnail_dir_path)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    Ok(Json(json!({ "status": "success", "thumbnail_dir": thumbnail_dir_path })))
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
        false, // Not a reference
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
    is_reference: bool,
) -> anyhow::Result<()> {
    // Compute checksum
    let checksum: Option<String> = compute_file_checksum(video_path)
        .await
        .ok();

    // Probe media
    let media_info = FFmpegWrapper::probe(video_path).await?;

    // Register media asset with project_id
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
        is_reference,
    )?;

    // Queue proxy generation job
    let proxy_job_payload = json!({
        "media_asset_id": asset_id,
        "input_path": video_path.to_str().unwrap(),
    });
    let _proxy_job_id = job_manager.create_job(JobType::GenerateProxy, Some(proxy_job_payload))?;

    // Queue BuildSegments job (can run immediately)
    let build_segments_payload = json!({
        "asset_id": asset_id,
    });
    let _build_segments_id = job_manager.create_job(JobType::BuildSegments, Some(build_segments_payload))?;

    // Queue transcription job (runs in parallel)
    let transcribe_job_payload = json!({
        "asset_id": asset_id,
        "media_path": video_path.to_str().unwrap(),
    });
    let _transcribe_job_id = job_manager.create_job(JobType::TranscribeAsset, Some(transcribe_job_payload))?;

    // Queue vision analysis job (runs in parallel)
    let vision_job_payload = json!({
        "asset_id": asset_id,
        "media_path": video_path.to_str().unwrap(),
    });
    let _vision_job_id = job_manager.create_job(JobType::AnalyzeVisionAsset, Some(vision_job_payload))?;

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
            false, // Not a reference
        )
        .await?;
    }

    job_manager.update_job_status(job_id, crate::jobs::JobStatus::Completed, Some(1.0))?;
    Ok(())
}

/// Process proxy generation job with thumbnail extraction
/// This function generates a proxy video and extracts thumbnails for a media asset
pub async fn process_proxy_generation_with_thumbnails(
    db: Arc<Database>,
    job_manager: Arc<JobManager>,
    job_id: i64,
    media_asset_id: i64,
    input_path: &str,
) -> anyhow::Result<()> {
    use std::path::Path;
    
    // Get media asset info to determine proxy dimensions
    let asset_path = db.get_media_asset_path(media_asset_id)?
        .ok_or_else(|| anyhow::anyhow!("Media asset not found"))?;
    
    // Probe to get dimensions
    let media_info = FFmpegWrapper::probe(Path::new(&asset_path)).await?;
    
    // Calculate proxy dimensions (scale down if large)
    let proxy_width = if media_info.width > 1920 { 1920 } else { media_info.width };
    let proxy_height = if media_info.height > 1080 { 1080 } else { media_info.height };
    
    // Determine proxy output path
    let cache_dir = PathBuf::from(".cache");
    let proxies_dir = cache_dir.join("proxies");
    tokio::fs::create_dir_all(&proxies_dir).await?;
    
    let proxy_filename = format!("proxy_{}.mp4", media_asset_id);
    let proxy_path = proxies_dir.join(&proxy_filename);
    
    // Generate proxy
    job_manager.update_job_status(
        job_id,
        crate::jobs::JobStatus::Running,
        Some(0.3),
    )?;
    
    FFmpegWrapper::generate_proxy(
        Path::new(input_path),
        &proxy_path,
        proxy_width,
        proxy_height,
    ).await?;
    
    // Store proxy path in database
    db.create_proxy(
        media_asset_id,
        proxy_path.to_str().unwrap(),
        "libx264",
        proxy_width,
        proxy_height,
    )?;
    
    // Generate thumbnails
    job_manager.update_job_status(
        job_id,
        crate::jobs::JobStatus::Running,
        Some(0.7),
    )?;
    
    let thumbnails_dir = cache_dir.join("thumbs").join(format!("asset_{}", media_asset_id));
    let thumbnail_dir_path = FFmpegWrapper::extract_thumbnails(
        Path::new(input_path),
        &thumbnails_dir,
    ).await?;
    
    // Store thumbnail directory in database
    db.set_thumbnail_dir(media_asset_id, &thumbnail_dir_path)?;
    
    // Mark job as completed
    job_manager.update_job_status(
        job_id,
        crate::jobs::JobStatus::Completed,
        Some(1.0),
    )?;
    
    Ok(())
}
