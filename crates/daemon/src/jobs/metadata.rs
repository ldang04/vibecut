use anyhow::Result;
use serde_json;
use std::sync::Arc;

use crate::db::Database;
use crate::jobs::JobManager;

/// Process ComputeSegmentMetadata job - generates deterministic metadata
pub async fn process_compute_segment_metadata(
    db: Arc<Database>,
    job_manager: Arc<JobManager>,
    job_id: i64,
    asset_id: i64,
) -> Result<()> {
    // Get all segments for this asset
    let segments = db.get_segments_by_asset(asset_id)?;
    
    let mut processed_count = 0;
    for segment in &segments {
        // Generate summary_text deterministically
        let summary_text = if let Some(ref transcript) = segment.transcript {
            // Use first clause or first 50 chars
            let first_clause = transcript.split('.').next().unwrap_or(transcript);
            if first_clause.len() > 50 {
                format!("{}...", &first_clause[..50])
            } else {
                first_clause.to_string()
            }
        } else {
            // Use vision tags if available
            if let Some(ref scene_json) = segment.scene_json {
                if let Ok(scene) = serde_json::from_str::<serde_json::Value>(scene_json) {
                    if let Some(tags) = scene.get("tags").and_then(|t| t.as_array()) {
                        let tag_str = tags.iter()
                            .filter_map(|t| t.as_str())
                            .collect::<Vec<_>>()
                            .join(" ");
                        if !tag_str.is_empty() {
                            tag_str
                        } else {
                            "video segment".to_string()
                        }
                    } else {
                        "video segment".to_string()
                    }
                } else {
                    "video segment".to_string()
                }
            } else {
                "video segment".to_string()
            }
        };
        
        // Extract keywords from transcript (simple: first few words)
        let keywords_json = if let Some(ref transcript) = segment.transcript {
            let words: Vec<&str> = transcript.split_whitespace().take(5).collect();
            serde_json::json!({
                "keywords": words
            }).to_string()
        } else {
            serde_json::json!({
                "keywords": []
            }).to_string()
        };
        
        // Generate subject_json from face detection and scene analysis
        let subject_json = if let Some(ref scene_json) = segment.scene_json {
            if let Ok(scene) = serde_json::from_str::<serde_json::Value>(scene_json) {
                let has_face = scene.get("has_face").and_then(|v| v.as_bool()).unwrap_or(false);
                let face_bbox = scene.get("face_bbox").cloned();
                serde_json::json!({
                    "has_face": has_face,
                    "face_bbox": face_bbox,
                    "subject_present": has_face,
                }).to_string()
            } else {
                serde_json::json!({
                    "has_face": false,
                    "subject_present": false,
                }).to_string()
            }
        } else {
            serde_json::json!({
                "has_face": false,
                "subject_present": false,
            }).to_string()
        };
        
        // Determine segment_kind using heuristics
        let segment_kind = {
            let has_transcript = segment.transcript.is_some();
            let has_face = if let Some(ref scene_json) = segment.scene_json {
                serde_json::from_str::<serde_json::Value>(scene_json)
                    .ok()
                    .and_then(|s| s.get("has_face").and_then(|v| v.as_bool()))
                    .unwrap_or(false)
            } else {
                false
            };
            let motion_high = if let Some(ref quality_json) = segment.quality_json {
                serde_json::from_str::<serde_json::Value>(quality_json)
                    .ok()
                    .and_then(|q| q.get("motion_score").and_then(|v| v.as_f64()))
                    .map(|m| m > 50.0)
                    .unwrap_or(false)
            } else {
                false
            };
            
            if has_transcript && has_face {
                Some("talking_head".to_string())
            } else if !has_transcript && motion_high {
                Some("action".to_string())
            } else if !has_transcript && !motion_high {
                Some("broll".to_string())
            } else {
                None
            }
        };
        
        // Update segment metadata
        db.update_segment_metadata(
            segment.id,
            Some(&summary_text),
            Some(&keywords_json),
            None, // quality_json (already set)
            Some(&subject_json),
            None, // scene_json (already set)
            None, // transcript (already set)
            segment_kind.as_deref(),
        )?;
        
        processed_count += 1;
        
        // Update progress
        let progress = processed_count as f64 / segments.len() as f64;
        job_manager.update_job_status(job_id, crate::jobs::JobStatus::Running, Some(progress))?;
    }
    
    // Update asset analysis state
    db.update_asset_analysis_state(asset_id, "metadata_ready_at", None)?;
    
    // Queue embedding job
    let embed_payload = serde_json::json!({
        "asset_id": asset_id,
    });
    let embed_id = job_manager.create_job(
        crate::jobs::JobType::EmbedSegments,
        Some(embed_payload),
    )?;
    eprintln!("[METADATA] Queued EmbedSegments job {} for asset_id: {}", embed_id, asset_id);
    
    job_manager.update_job_status(job_id, crate::jobs::JobStatus::Completed, Some(1.0))?;
    
    Ok(())
}

