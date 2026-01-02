use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::params;
use serde_json;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

use crate::db::Database;
use crate::jobs::{JobManager, JobStatus};
use crate::twelvelabs;

/// Process IndexAssetWithTwelveLabs job
pub async fn process_index_asset_with_twelvelabs(
    db: Arc<Database>,
    job_manager: Arc<JobManager>,
    job_id: i64,
    asset_id: i64,
    project_id: i64,
) -> Result<()> {
    eprintln!("[TWELVELABS_INDEX] Starting indexing job {} for asset {}", job_id, asset_id);
    
    // Get asset info
    let asset = db.get_media_asset(asset_id)?
        .ok_or_else(|| anyhow::anyhow!("Asset {} not found", asset_id))?;
    
    // Get or create project index
    let index_id = {
        let existing_index_id: Result<String, rusqlite::Error> = {
            let conn = db.conn.lock().unwrap();
            conn.query_row(
                "SELECT twelvelabs_index_id FROM projects WHERE id = ?1",
                params![project_id],
                |row| row.get(0),
            )
        };
        
        match existing_index_id {
            Ok(id) if !id.is_empty() => id,
            _ => {
                // Create new index
                eprintln!("[TWELVELABS_INDEX] Creating new index for project {}", project_id);
                let new_index_id = twelvelabs::create_index(project_id, None).await?;
                
                // Store in database
                {
                    let conn = db.conn.lock().unwrap();
                    conn.execute(
                        "UPDATE projects SET twelvelabs_index_id = ?1, twelvelabs_indexed_at = ?2 WHERE id = ?3",
                        params![new_index_id.clone(), Utc::now().to_rfc3339(), project_id],
                    )?;
                }
                
                eprintln!("[TWELVELABS_INDEX] Created index {} for project {}", new_index_id, project_id);
                new_index_id
            }
        }
    };
    
    // Check if already indexed
    let already_indexed: bool = {
        let conn = db.conn.lock().unwrap();
        conn.query_row(
            "SELECT twelvelabs_indexed_at IS NOT NULL FROM media_assets WHERE id = ?1",
            params![asset_id],
            |row| row.get(0),
        ).unwrap_or(false)
    };
    
    if already_indexed {
        eprintln!("[TWELVELABS_INDEX] Asset {} already indexed, skipping", asset_id);
        job_manager.update_job_status(job_id, JobStatus::Completed, Some(1.0))?;
        return Ok(());
    }
    
    // Check if we have a task_id (job was interrupted)
    let existing_task_id: Option<String> = {
        let conn = db.conn.lock().unwrap();
        conn.query_row(
            "SELECT twelvelabs_task_id FROM media_assets WHERE id = ?1",
            params![asset_id],
            |row| row.get(0),
        ).ok()
    };
    
    let task_id = if let Some(task_id) = existing_task_id {
        eprintln!("[TWELVELABS_INDEX] Resuming existing task {}", task_id);
        task_id
    } else {
        // Create upload task
        // Note: For now, we assume the video is accessible via HTTP URL
        // In production, you might need to upload the file or serve it via a proxy
        let video_url = if asset.path.starts_with("http://") || asset.path.starts_with("https://") {
            asset.path.clone()
        } else {
            // For local files, construct a proxy URL
            format!("http://127.0.0.1:7777/api/projects/{}/media/{}/proxy", project_id, asset_id)
        };
        
        eprintln!("[TWELVELABS_INDEX] Creating upload task for asset {} with URL {}", asset_id, video_url);
        let new_task_id = twelvelabs::create_task_upload(&index_id, &video_url).await?;
        
        // Store task_id
        {
            let conn = db.conn.lock().unwrap();
            conn.execute(
                "UPDATE media_assets SET twelvelabs_task_id = ?1 WHERE id = ?2",
                params![new_task_id.clone(), asset_id],
            )?;
        }
        
        eprintln!("[TWELVELABS_INDEX] Created task {} for asset {}", new_task_id, asset_id);
        new_task_id
    };
    
    // Poll task status with exponential backoff
    let mut backoff_seconds = 5;
    let max_backoff = 60;
    let max_attempts = 120; // 10 minutes max (120 * 5s)
    let mut attempts = 0;
    
    loop {
        attempts += 1;
        if attempts > max_attempts {
            return Err(anyhow::anyhow!("Task {} did not complete within timeout", task_id));
        }
        
        // Update job progress
        let progress = 0.1 + (attempts as f64 / max_attempts as f64) * 0.8; // 10% to 90%
        job_manager.update_job_status(job_id, JobStatus::Running, Some(progress))?;
        
        // Check task status
        match twelvelabs::get_task_status(&task_id).await {
            Ok(status) => {
                match status.status.as_str() {
                    "ready" => {
                        // Task completed successfully
                        if let Some(video_id) = status.video_id {
                            eprintln!("[TWELVELABS_INDEX] Task {} completed, video_id: {}", task_id, video_id);
                            
                            // Store video_id and mark as indexed
                            {
                                let conn = db.conn.lock().unwrap();
                                conn.execute(
                                    "UPDATE media_assets SET twelvelabs_video_id = ?1, twelvelabs_indexed_at = ?2, twelvelabs_task_id = NULL, twelvelabs_last_error = NULL WHERE id = ?3",
                                    params![video_id, Utc::now().to_rfc3339(), asset_id],
                                )?;
                            }
                            
                            job_manager.update_job_status(job_id, JobStatus::Completed, Some(1.0))?;
                            return Ok(());
                        } else {
                            return Err(anyhow::anyhow!("Task ready but no video_id returned"));
                        }
                    }
                    "failed" => {
                        let error_msg = status.error.unwrap_or_else(|| "Unknown error".to_string());
                        eprintln!("[TWELVELABS_INDEX] Task {} failed: {}", task_id, error_msg);
                        
                        // Store error
                        {
                            let conn = db.conn.lock().unwrap();
                            conn.execute(
                                "UPDATE media_assets SET twelvelabs_last_error = ?1 WHERE id = ?2",
                                params![error_msg.clone(), asset_id],
                            )?;
                        }
                        
                        job_manager.update_job_status(job_id, JobStatus::Failed, None)?;
                        return Err(anyhow::anyhow!("Task failed: {}", error_msg));
                    }
                    "pending" | "processing" => {
                        // Still processing, wait and retry
                        eprintln!("[TWELVELABS_INDEX] Task {} still processing (attempt {}/{})", task_id, attempts, max_attempts);
                        sleep(Duration::from_secs(backoff_seconds)).await;
                        
                        // Exponential backoff with cap
                        backoff_seconds = (backoff_seconds * 2).min(max_backoff);
                    }
                    _ => {
                        eprintln!("[TWELVELABS_INDEX] Unknown task status: {}", status.status);
                        sleep(Duration::from_secs(backoff_seconds)).await;
                        backoff_seconds = (backoff_seconds * 2).min(max_backoff);
                    }
                }
            }
            Err(e) => {
                eprintln!("[TWELVELABS_INDEX] Error checking task status: {:?}", e);
                // On error, wait and retry (might be transient network issue)
                sleep(Duration::from_secs(backoff_seconds)).await;
                backoff_seconds = (backoff_seconds * 2).min(max_backoff);
            }
        }
    }
}


