use anyhow::Result;
use serde_json;
use std::sync::Arc;

use crate::db::Database;
use crate::jobs::JobManager;
use engine::timeline::TICKS_PER_SECOND;

/// Helper: Convert seconds to ticks
fn secs_to_ticks(seconds: f64) -> i64 {
    (seconds * TICKS_PER_SECOND as f64) as i64
}

/// Process EnrichSegmentsFromTranscript job - attaches transcript to segments by time intersection
pub async fn process_enrich_segments_from_transcript(
    db: Arc<Database>,
    job_manager: Arc<JobManager>,
    job_id: i64,
    asset_id: i64,
) -> Result<()> {
    // Load raw transcript from asset_transcripts table
    let transcript_json = db.get_asset_transcript(asset_id)?
        .ok_or_else(|| anyhow::anyhow!("Transcript not found for asset {}", asset_id))?;
    
    let transcript_data: serde_json::Value = serde_json::from_str(&transcript_json)?;
    let segments_data = transcript_data.get("segments")
        .and_then(|s| s.as_array())
        .ok_or_else(|| anyhow::anyhow!("Invalid transcript format"))?;
    
    // Get all segments for this asset
    let segments = db.get_segments_by_asset(asset_id)?;
    
    let mut enriched_count = 0;
    for segment in &segments {
        let segment_start_ticks = Database::get_coalesced_src_in(segment);
        let segment_end_ticks = Database::get_coalesced_src_out(segment);
        
        // Find intersecting transcript segments
        let mut transcript_texts = Vec::new();
        for transcript_seg in segments_data {
            if let (Some(start_sec), Some(end_sec)) = (
                transcript_seg.get("start").and_then(|v| v.as_f64()),
                transcript_seg.get("end").and_then(|v| v.as_f64()),
            ) {
                let transcript_start_ticks = secs_to_ticks(start_sec);
                let transcript_end_ticks = secs_to_ticks(end_sec);
                
                // Check for intersection
                if transcript_start_ticks < segment_end_ticks && transcript_end_ticks > segment_start_ticks {
                    if let Some(text) = transcript_seg.get("text").and_then(|v| v.as_str()) {
                        transcript_texts.push(text);
                    }
                }
            }
        }
        
        // Combine transcript texts
        if !transcript_texts.is_empty() {
            let combined_text = transcript_texts.join(" ");
            db.update_segment_metadata(
                segment.id,
                None, // summary_text
                None, // keywords_json
                None, // quality_json
                None, // subject_json
                None, // scene_json
                Some(&combined_text), // transcript
                None, // segment_kind
            )?;
            enriched_count += 1;
        }
        
        // Update progress
        let progress = enriched_count as f64 / segments.len() as f64;
        job_manager.update_job_status(job_id, crate::jobs::JobStatus::Running, Some(progress))?;
    }
    
    // Queue metadata computation job
    let metadata_payload = serde_json::json!({
        "asset_id": asset_id,
    });
    let _metadata_id = job_manager.create_job(
        crate::jobs::JobType::ComputeSegmentMetadata,
        Some(metadata_payload),
        None,
    )?;
    
    job_manager.update_job_status(job_id, crate::jobs::JobStatus::Completed, Some(1.0))?;
    
    Ok(())
}

/// Process EnrichSegmentsFromVision job - attaches vision data to segments by time intersection
pub async fn process_enrich_segments_from_vision(
    db: Arc<Database>,
    job_manager: Arc<JobManager>,
    job_id: i64,
    asset_id: i64,
) -> Result<()> {
    // Load raw vision data from asset_vision table
    let vision_json = db.get_asset_vision(asset_id)?
        .ok_or_else(|| anyhow::anyhow!("Vision data not found for asset {}", asset_id))?;
    
    let vision_data: serde_json::Value = serde_json::from_str(&vision_json)?;
    let vision_segments = vision_data.get("segments")
        .and_then(|s| s.as_array())
        .ok_or_else(|| anyhow::anyhow!("Invalid vision format"))?;
    
    // Get all segments for this asset
    let segments = db.get_segments_by_asset(asset_id)?;
    
    let mut enriched_count = 0;
    for segment in &segments {
        let segment_start_ticks = Database::get_coalesced_src_in(segment);
        let segment_end_ticks = Database::get_coalesced_src_out(segment);
        
        // Find intersecting vision segments and aggregate data
        let mut blur_scores = Vec::new();
        let mut motion_scores = Vec::new();
        let mut tags = Vec::new();
        let mut has_face = false;
        let mut face_bbox = None;
        
        for vision_seg in vision_segments {
            if let (Some(start_sec), Some(end_sec)) = (
                vision_seg.get("start").and_then(|v| v.as_f64()),
                vision_seg.get("end").and_then(|v| v.as_f64()),
            ) {
                let vision_start_ticks = secs_to_ticks(start_sec);
                let vision_end_ticks = secs_to_ticks(end_sec);
                
                // Check for intersection
                if vision_start_ticks < segment_end_ticks && vision_end_ticks > segment_start_ticks {
                    if let Some(blur) = vision_seg.get("blur_score").and_then(|v| v.as_f64()) {
                        blur_scores.push(blur);
                    }
                    if let Some(motion) = vision_seg.get("motion_score").and_then(|v| v.as_f64()) {
                        motion_scores.push(motion);
                    }
                    if let Some(vision_tags) = vision_seg.get("tags").and_then(|v| v.as_array()) {
                        for tag in vision_tags {
                            if let Some(tag_str) = tag.as_str() {
                                if !tags.contains(&tag_str.to_string()) {
                                    tags.push(tag_str.to_string());
                                }
                            }
                        }
                    }
                    if let Some(has_face_val) = vision_seg.get("has_face").and_then(|v| v.as_bool()) {
                        if has_face_val {
                            has_face = true;
                            if let Some(bbox) = vision_seg.get("face_bbox") {
                                face_bbox = Some(bbox.clone());
                            }
                        }
                    }
                }
            }
        }
        
        // Aggregate quality and scene data
        let avg_blur = if !blur_scores.is_empty() {
            blur_scores.iter().sum::<f64>() / blur_scores.len() as f64
        } else {
            0.0
        };
        let avg_motion = if !motion_scores.is_empty() {
            motion_scores.iter().sum::<f64>() / motion_scores.len() as f64
        } else {
            0.0
        };
        
        let quality_json = serde_json::json!({
            "blur_score": avg_blur,
            "motion_score": avg_motion,
        });
        
        let scene_json = serde_json::json!({
            "tags": tags,
            "has_face": has_face,
            "face_bbox": face_bbox,
        });
        
        // Update segment metadata
        db.update_segment_metadata(
            segment.id,
            None, // summary_text
            None, // keywords_json
            Some(&quality_json.to_string()), // quality_json
            None, // subject_json
            Some(&scene_json.to_string()), // scene_json
            None, // transcript
            None, // segment_kind
        )?;
        enriched_count += 1;
        
        // Update progress
        let progress = enriched_count as f64 / segments.len() as f64;
        job_manager.update_job_status(job_id, crate::jobs::JobStatus::Running, Some(progress))?;
    }
    
    // Queue metadata computation job (if not already queued)
    let metadata_payload = serde_json::json!({
        "asset_id": asset_id,
    });
    let _metadata_id = job_manager.create_job(
        crate::jobs::JobType::ComputeSegmentMetadata,
        Some(metadata_payload),
        None,
    )?;
    
    job_manager.update_job_status(job_id, crate::jobs::JobStatus::Completed, Some(1.0))?;
    
    Ok(())
}

