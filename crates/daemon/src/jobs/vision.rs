use anyhow::Result;
use reqwest;
use serde_json;
use std::sync::Arc;

use crate::db::Database;
use crate::jobs::JobManager;

const ML_SERVICE_URL: &str = "http://127.0.0.1:8001";

/// Process AnalyzeVisionAsset job - calls ML service and stores raw vision data
pub async fn process_analyze_vision_asset(
    db: Arc<Database>,
    job_manager: Arc<JobManager>,
    job_id: i64,
    asset_id: i64,
    media_path: &str,
) -> Result<()> {
    // Call ML service /vision/analyze endpoint
    let client = reqwest::Client::new();
    let response = client
        .post(&format!("{}/vision/analyze", ML_SERVICE_URL))
        .json(&serde_json::json!({
            "mediaPath": media_path
        }))
        .send()
        .await?;
    
    if !response.status().is_success() {
        return Err(anyhow::anyhow!("ML service vision analyze failed: {}", response.status()));
    }
    
    let vision_response: serde_json::Value = response.json().await?;
    
    // Store raw vision results in asset_vision table
    let vision_json = serde_json::to_string(&vision_response)?;
    db.store_asset_vision(asset_id, &vision_json)?;
    
    // Update asset analysis state
    db.update_asset_analysis_state(asset_id, "vision_ready_at", None)?;
    
    // Queue enrichment job (will be gated by processor)
    let enrich_payload = serde_json::json!({
        "asset_id": asset_id,
    });
    let _enrich_id = job_manager.create_job(
        crate::jobs::JobType::EnrichSegmentsFromVision,
        Some(enrich_payload),
        None,
    )?;
    
    job_manager.update_job_status(job_id, crate::jobs::JobStatus::Completed, Some(1.0))?;
    
    Ok(())
}

