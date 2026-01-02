use anyhow::Result;
use rusqlite::params;
use std::sync::Arc;
use serde_json;

use crate::db::Database;
use crate::jobs::{JobManager, JobType};
use crate::orchestrator::state::{AssetReadiness, AssetState, get_asset_states};

pub enum ReadinessGoal {
    Segmented,
    Enriched,
    MetadataReady,
    Embedded,
    IndexedExternal,
}

impl ReadinessGoal {
    fn to_readiness(&self) -> AssetReadiness {
        match self {
            ReadinessGoal::Segmented => AssetReadiness::Segmented,
            ReadinessGoal::Enriched => AssetReadiness::Enriched,
            ReadinessGoal::MetadataReady => AssetReadiness::MetadataReady,
            ReadinessGoal::Embedded => AssetReadiness::Embedded,
            ReadinessGoal::IndexedExternal => AssetReadiness::IndexedExternal,
        }
    }
}

#[derive(Debug, Clone)]
pub struct EnsureAssetStatus {
    pub asset_id: i64,
    pub current_readiness: AssetReadiness,
    pub target_readiness: AssetReadiness,
    pub missing_steps: Vec<JobType>,
    pub active_job_ids: Vec<i64>,
    pub enqueued_job_ids: Vec<i64>, // Jobs just enqueued by ensure_ready()
}

#[derive(Debug, Clone)]
pub struct EnsureResult {
    pub enqueued_jobs: Vec<i64>,
    pub assets: Vec<EnsureAssetStatus>, // Per-asset progress
    pub waiting_for: Vec<String>, // Human-readable what we're waiting for
    pub next_poll_ms: u64,
    pub will_be_ready: bool,
}

/// Compute missing steps needed to reach target readiness from current state
fn compute_missing_steps(current: &AssetReadiness, target: &AssetReadiness) -> Vec<JobType> {
    let mut steps = Vec::new();
    
    match (current, target) {
        (AssetReadiness::Imported, _) => {
            steps.push(JobType::BuildSegments);
            // Continue to next level
            let next = AssetReadiness::Segmented;
            steps.extend(compute_missing_steps(&next, target));
        }
        (AssetReadiness::Segmented, t) if *t != AssetReadiness::Segmented => {
            steps.push(JobType::TranscribeAsset);
            steps.push(JobType::AnalyzeVisionAsset);
            // Continue to next level
            let next = AssetReadiness::Enriched;
            steps.extend(compute_missing_steps(&next, target));
        }
        (AssetReadiness::Enriched, t) if *t != AssetReadiness::Enriched && *t != AssetReadiness::Segmented => {
            steps.push(JobType::ComputeSegmentMetadata);
            // Continue to next level
            let next = AssetReadiness::MetadataReady;
            steps.extend(compute_missing_steps(&next, target));
        }
        (AssetReadiness::MetadataReady, t) if *t != AssetReadiness::MetadataReady && *t != AssetReadiness::Enriched && *t != AssetReadiness::Segmented => {
            steps.push(JobType::EmbedSegments);
            // Continue to next level
            let next = AssetReadiness::Embedded;
            steps.extend(compute_missing_steps(&next, target));
        }
        (AssetReadiness::Embedded, t) if *t != AssetReadiness::Embedded && *t != AssetReadiness::MetadataReady && *t != AssetReadiness::Enriched && *t != AssetReadiness::Segmented => {
            steps.push(JobType::IndexAssetWithTwelveLabs);
        }
        _ => {
            // Already at or past target
        }
    }
    
    steps
}

/// Ensure project assets are ready for the given goal by enqueueing missing jobs
pub fn ensure_ready(
    db: &Database,
    job_manager: &JobManager,
    project_id: i64,
    goal: ReadinessGoal,
) -> Result<EnsureResult> {
    let target_readiness = goal.to_readiness();
    let asset_states = get_asset_states(db, project_id)?;
    
    let mut enqueued_jobs = Vec::new();
    let mut asset_statuses = Vec::new();
    let mut waiting_for = Vec::new();
    
    for asset_state in asset_states {
        if asset_state.readiness == target_readiness {
            // Already at target
            asset_statuses.push(EnsureAssetStatus {
                asset_id: asset_state.asset_id,
                current_readiness: asset_state.readiness.clone(),
                target_readiness: target_readiness.clone(),
                missing_steps: Vec::new(),
                active_job_ids: asset_state.active_job_ids.clone(),
                enqueued_job_ids: Vec::new(),
            });
            continue;
        }
        
        // Compute missing steps
        let missing_steps = compute_missing_steps(&asset_state.readiness, &target_readiness);
        
        let mut enqueued_for_asset = Vec::new();
        
        for job_type in &missing_steps {
            // Generate dedupe_key: format!("{}:{}", job_type_variant_name, asset_id)
            let dedupe_key = format!("{}:{}", job_type.to_string(), asset_state.asset_id);
            
            // Check if job already exists and is active
            let existing_job_exists = {
                let conn = db.conn.lock().unwrap();
                let existing_id_result: Result<i64, rusqlite::Error> = conn.query_row(
                    "SELECT id FROM jobs WHERE dedupe_key = ?1 AND is_active = 1 LIMIT 1",
                    params![dedupe_key.clone()],
                    |row| row.get(0),
                );
                existing_id_result.is_ok()
            };
            
            if existing_job_exists {
                // Job already exists and is active
                continue;
            }
            
            // Create job payload
            let payload = serde_json::json!({
                "asset_id": asset_state.asset_id,
                "project_id": project_id,
            });
            
            // Enqueue job with dedupe_key
            match job_manager.create_job(job_type.clone(), Some(payload), Some(dedupe_key)) {
                Ok(job_id) => {
                    enqueued_jobs.push(job_id);
                    enqueued_for_asset.push(job_id);
                    waiting_for.push(format!("{} for asset {}", job_type.to_string(), asset_state.asset_id));
                }
                Err(e) => {
                    eprintln!("[ENSURE] Failed to enqueue {} for asset {}: {:?}", job_type.to_string(), asset_state.asset_id, e);
                }
            }
        }
        
        asset_statuses.push(EnsureAssetStatus {
            asset_id: asset_state.asset_id,
            current_readiness: asset_state.readiness.clone(),
            target_readiness: target_readiness.clone(),
            missing_steps,
            active_job_ids: asset_state.active_job_ids.clone(),
            enqueued_job_ids: enqueued_for_asset,
        });
    }
    
    let will_be_ready = asset_statuses.iter().all(|a| {
        a.current_readiness == target_readiness || !a.enqueued_job_ids.is_empty()
    });
    
    Ok(EnsureResult {
        enqueued_jobs,
        assets: asset_statuses,
        waiting_for,
        next_poll_ms: 5000, // Poll every 5 seconds
        will_be_ready,
    })
}
