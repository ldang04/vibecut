use anyhow::Result;
use rusqlite::params;
use std::sync::Arc;

use crate::db::Database;
use crate::jobs::JobManager;
use crate::media::ffmpeg::FFmpegWrapper;

use engine::timeline::TICKS_PER_SECOND;

const SEGMENT_DURATION_SECONDS: f64 = 5.0; // Fixed 5 second segments for v1

/// Process BuildSegments job - creates segments from fixed time windows
pub async fn process_build_segments(
    db: Arc<Database>,
    job_manager: Arc<JobManager>,
    job_id: i64,
    asset_id: i64,
) -> Result<()> {
    // Get asset info
    let asset_path = db.get_media_asset_path(asset_id)?
        .ok_or_else(|| anyhow::anyhow!("Media asset {} not found", asset_id))?;
    
    // Get project_id from asset
    let project_id: i64 = {
        let conn = db.conn.lock().unwrap();
        conn.query_row(
            "SELECT project_id FROM media_assets WHERE id = ?1",
            params![asset_id],
            |row| row.get(0),
        )?
    };
    
    // Probe media to get duration
    let media_info = FFmpegWrapper::probe(&std::path::PathBuf::from(&asset_path)).await?;
    let duration_ticks = media_info.duration_ticks;
    let duration_seconds = duration_ticks as f64 / TICKS_PER_SECOND as f64;
    
    // Create segments with fixed 5s windows (deterministic chunking)
    let mut segments_created = 0;
    let mut current_time_ticks = 0i64;
    let segment_duration_ticks = (SEGMENT_DURATION_SECONDS * TICKS_PER_SECOND as f64) as i64;
    
    while current_time_ticks < duration_ticks {
        let segment_end_ticks = (current_time_ticks + segment_duration_ticks).min(duration_ticks);
        
        // Create segment with stable identity (write only to src_in_ticks/src_out_ticks)
        let _segment_id = db.create_segment(
            project_id,
            asset_id,
            current_time_ticks,
            segment_end_ticks,
        )?;
        
        segments_created += 1;
        current_time_ticks = segment_end_ticks;
        
        // Update progress
        let progress = current_time_ticks as f64 / duration_ticks as f64;
        job_manager.update_job_status(job_id, crate::jobs::JobStatus::Running, Some(progress))?;
    }
    
    // Update asset analysis state
    db.update_asset_analysis_state(asset_id, "segments_built_at", None)?;
    
    // Mark job as completed
    job_manager.update_job_status(job_id, crate::jobs::JobStatus::Completed, Some(1.0))?;
    
    eprintln!("Created {} segments for asset {}", segments_created, asset_id);
    
    Ok(())
}

