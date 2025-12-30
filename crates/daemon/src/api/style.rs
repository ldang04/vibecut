use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
    routing::post,
    Router,
};
use chrono::Utc;
use rusqlite;
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

#[derive(Deserialize)]
pub struct ProfileFromReferencesRequest {
    pub reference_asset_ids: Vec<i64>,
}

#[derive(Serialize)]
pub struct StyleProfileResponse {
    pacing: serde_json::Value,
    caption_templates: Vec<serde_json::Value>,
    music: serde_json::Value,
    structure: serde_json::Value,
}

pub fn router(db: Arc<Database>, job_manager: Arc<JobManager>) -> Router {
    Router::new()
        .route("/:id/import_reference", post(import_reference))
        .route("/:id/profile_from_references", post(profile_from_references))
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

/// Compute style profile from reference segments
async fn profile_from_references(
    State((db, _job_manager)): State<(Arc<Database>, Arc<JobManager>)>,
    Path(project_id): Path<i64>,
    Json(req): Json<ProfileFromReferencesRequest>,
) -> Result<Json<StyleProfileResponse>, StatusCode> {
    use engine::timeline::TICKS_PER_SECOND;
    
    // Get all segments from reference assets
    let mut all_segments = Vec::new();
    for asset_id in &req.reference_asset_ids {
        let segments = db.get_segments_by_asset(*asset_id)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        all_segments.extend(segments);
    }
    
    if all_segments.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    
    // Compute pacing stats from segment durations
    let durations: Vec<f64> = all_segments.iter()
        .map(|s| {
            let start = crate::db::Database::get_coalesced_src_in(s);
            let end = crate::db::Database::get_coalesced_src_out(s);
            (end - start) as f64 / TICKS_PER_SECOND as f64
        })
        .collect();
    
    let median_duration = if !durations.is_empty() {
        let mut sorted = durations.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        sorted[sorted.len() / 2]
    } else {
        5.0
    };
    
    let variance = if durations.len() > 1 {
        let mean = durations.iter().sum::<f64>() / durations.len() as f64;
        durations.iter()
            .map(|d| (d - mean).powi(2))
            .sum::<f64>() / durations.len() as f64
    } else {
        0.0
    };
    
    // Compute montage density (segments per minute)
    let total_duration_sec: f64 = durations.iter().sum();
    let montage_density = if total_duration_sec > 0.0 {
        (all_segments.len() as f64 / total_duration_sec) * 60.0
    } else {
        0.0
    };
    
    // Compute caption frequency (segments with transcript / total segments)
    let segments_with_transcript = all_segments.iter()
        .filter(|s| s.transcript.is_some())
        .count();
    let caption_frequency = if !all_segments.is_empty() {
        segments_with_transcript as f64 / all_segments.len() as f64
    } else {
        0.0
    };
    
    // Build style profile
    let style_profile = serde_json::json!({
        "pacing_stats": {
            "median_clip_length": median_duration,
            "variance": variance,
        },
        "montage_density": montage_density,
        "silence_cut_aggressiveness": 0.5, // Default, can be computed from gaps
        "caption_frequency": caption_frequency,
        "music_presence_ratio": 0.0, // Would need audio track analysis
        "typical_overlay_usage": 0.0, // Would need timeline analysis
    });
    
    // Store style profile
    let profile_name = format!("Reference Profile {}", chrono::Utc::now().to_rfc3339());
    let profile_id = db.create_style_profile(&profile_name, &style_profile.to_string())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    // Update style profile with project_id and reference_asset_ids
    let conn = db.conn.lock().unwrap();
    let reference_ids_json = serde_json::to_string(&req.reference_asset_ids)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    conn.execute(
        "UPDATE style_profiles SET project_id = ?1, reference_asset_ids_json = ?2 WHERE id = ?3",
        rusqlite::params![project_id, reference_ids_json, profile_id],
    ).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    drop(conn);
    
    // Return response matching ML service format
    Ok(Json(StyleProfileResponse {
        pacing: style_profile["pacing_stats"].clone(),
        caption_templates: vec![serde_json::json!({
            "placement": {"x": 0.5, "y": 0.9, "safe_area": true},
            "font_family": "Arial",
            "font_weight": "bold",
            "font_size": 48,
            "stroke": true,
            "shadow": true,
        })],
        music: serde_json::json!({
            "ducking_profile": {"duck_amount": 0.5, "fade_in": 0.2, "fade_out": 0.2},
            "loudness_curve": [],
            "bpm_tendencies": [],
        }),
        structure: serde_json::json!({
            "a_roll_b_roll_ratio": 0.6,
            "intro_duration_target": 10.0,
            "outro_duration_target": 5.0,
        }),
    }))
}
