use anyhow::Result;
use rusqlite::params;
use serde_json;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

use crate::db::Database;
use crate::jobs::{JobManager, JobStatus, JobType};

pub struct JobProcessor {
    db: Arc<Database>,
    job_manager: Arc<JobManager>,
}

impl JobProcessor {
    pub fn new(db: Arc<Database>, job_manager: Arc<JobManager>) -> Self {
        JobProcessor { db, job_manager }
    }

    /// Get pending jobs that are ready to run (prerequisites met)
    pub fn get_ready_jobs(&self) -> Result<Vec<i64>> {
        let status_str = serde_json::to_string(&JobStatus::Pending)?;
        let rows: Vec<_> = {
            let conn = self.db.conn.lock().unwrap();
            let mut stmt = conn.prepare(
                "SELECT id, type, payload_json FROM jobs WHERE status = ?1 ORDER BY created_at ASC"
            )?;
            
            let rows: Vec<_> = stmt.query_map(params![status_str], |row| {
                let job_id: i64 = row.get(0)?;
                let job_type_str: String = row.get(1)?;
                let payload_str: Option<String> = row.get(2)?;
                
                Ok((job_id, job_type_str, payload_str))
            })?.collect::<Result<Vec<_>, _>>()?;
            rows
        };
        
        let mut ready_jobs = Vec::new();
        for (job_id, job_type_str, payload_str) in rows {
            // Parse job type
            let job_type: JobType = serde_json::from_str(&job_type_str)?;
            
            // Check prerequisites based on job type
            if let Some(asset_id) = Self::extract_asset_id(&payload_str) {
                if Self::check_job_prerequisites(&self.db, &job_type, asset_id)? {
                    ready_jobs.push(job_id);
                }
            } else {
                // Jobs without asset_id requirements can run immediately
                match job_type {
                    JobType::ImportRaw | JobType::GenerateEdit | JobType::Export => {
                        ready_jobs.push(job_id);
                    }
                    _ => {
                        // Jobs that require asset_id but don't have it in payload - skip for now
                    }
                }
            }
        }
        
        Ok(ready_jobs)
    }

    /// Extract asset_id from job payload (string version)
    fn extract_asset_id(payload_str: &Option<String>) -> Option<i64> {
        if let Some(ref payload) = payload_str {
            if let Ok(payload_json) = serde_json::from_str::<serde_json::Value>(payload) {
                if let Some(asset_id) = payload_json.get("asset_id").and_then(|v| v.as_i64()) {
                    return Some(asset_id);
                }
                if let Some(asset_id) = payload_json.get("media_asset_id").and_then(|v| v.as_i64()) {
                    return Some(asset_id);
                }
            }
        }
        None
    }

    /// Extract asset_id from job payload (Value version)
    fn extract_asset_id_from_payload(payload: &Option<serde_json::Value>) -> Option<i64> {
        if let Some(ref payload_json) = payload {
            if let Some(asset_id) = payload_json.get("asset_id").and_then(|v| v.as_i64()) {
                return Some(asset_id);
            }
            if let Some(asset_id) = payload_json.get("media_asset_id").and_then(|v| v.as_i64()) {
                return Some(asset_id);
            }
        }
        None
    }

    /// Check if prerequisites are met for a job type
    fn check_job_prerequisites(
        db: &Database,
        job_type: &JobType,
        asset_id: i64,
    ) -> Result<bool> {
        match job_type {
            JobType::BuildSegments | JobType::TranscribeAsset | JobType::AnalyzeVisionAsset => {
                // These can run immediately (no prerequisites)
                Ok(true)
            }
            JobType::EnrichSegmentsFromTranscript => {
                // Requires segments_built_at AND transcript_ready_at
                db.check_asset_prerequisites(asset_id, &["segments_built", "transcript_ready"])
            }
            JobType::EnrichSegmentsFromVision => {
                // Requires segments_built_at AND vision_ready_at
                db.check_asset_prerequisites(asset_id, &["segments_built", "vision_ready"])
            }
            JobType::ComputeSegmentMetadata => {
                // Requires segments_built_at
                db.check_asset_prerequisites(asset_id, &["segments_built"])
            }
            JobType::EmbedSegments => {
                // Requires metadata_ready_at
                db.check_asset_prerequisites(asset_id, &["metadata_ready"])
            }
            JobType::GenerateProxy => {
                // Can run immediately (no prerequisites)
                Ok(true)
            }
            _ => {
                // Other job types - allow them to run (they'll handle their own prerequisites)
                Ok(true)
            }
        }
    }

    /// Process a single job
    pub async fn process_job(&self, job_id: i64) -> Result<()> {
        let job = self.job_manager.get_job(job_id)?
            .ok_or_else(|| anyhow::anyhow!("Job {} not found", job_id))?;
        
        // Update status to Running
        self.job_manager.update_job_status(job_id, JobStatus::Running, Some(0.0))?;
        
        // Process based on job type
        // Note: Actual processing logic will be implemented in separate modules
        // This is just the processor framework with gating logic
        
        match job.job_type {
            JobType::BuildSegments => {
                if let Some(asset_id) = Self::extract_asset_id_from_payload(&job.payload) {
                    if let Err(e) = crate::jobs::build_segments::process_build_segments(
                        self.db.clone(),
                        self.job_manager.clone(),
                        job_id,
                        asset_id,
                    ).await {
                        eprintln!("Error processing BuildSegments job {}: {:?}", job_id, e);
                        let _ = self.job_manager.update_job_status(job_id, JobStatus::Failed, None);
                        return Err(e);
                    }
                } else {
                    eprintln!("BuildSegments job {} missing asset_id", job_id);
                    let _ = self.job_manager.update_job_status(job_id, JobStatus::Failed, None);
                }
            }
            JobType::TranscribeAsset => {
                if let Some(asset_id) = Self::extract_asset_id_from_payload(&job.payload) {
                    let media_path = job.payload.as_ref()
                        .and_then(|p| p.get("media_path"))
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| anyhow::anyhow!("Missing media_path"))?;
                    
                    if let Err(e) = crate::jobs::transcribe::process_transcribe_asset(
                        self.db.clone(),
                        self.job_manager.clone(),
                        job_id,
                        asset_id,
                        media_path,
                    ).await {
                        eprintln!("Error processing TranscribeAsset job {}: {:?}", job_id, e);
                        let _ = self.job_manager.update_job_status(job_id, JobStatus::Failed, None);
                        return Err(e);
                    }
                } else {
                    eprintln!("TranscribeAsset job {} missing asset_id", job_id);
                    let _ = self.job_manager.update_job_status(job_id, JobStatus::Failed, None);
                }
            }
            JobType::AnalyzeVisionAsset => {
                if let Some(asset_id) = Self::extract_asset_id_from_payload(&job.payload) {
                    let media_path = job.payload.as_ref()
                        .and_then(|p| p.get("media_path"))
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| anyhow::anyhow!("Missing media_path"))?;
                    
                    if let Err(e) = crate::jobs::vision::process_analyze_vision_asset(
                        self.db.clone(),
                        self.job_manager.clone(),
                        job_id,
                        asset_id,
                        media_path,
                    ).await {
                        eprintln!("Error processing AnalyzeVisionAsset job {}: {:?}", job_id, e);
                        let _ = self.job_manager.update_job_status(job_id, JobStatus::Failed, None);
                        return Err(e);
                    }
                } else {
                    eprintln!("AnalyzeVisionAsset job {} missing asset_id", job_id);
                    let _ = self.job_manager.update_job_status(job_id, JobStatus::Failed, None);
                }
            }
            JobType::EnrichSegmentsFromTranscript => {
                if let Some(asset_id) = Self::extract_asset_id_from_payload(&job.payload) {
                    if let Err(e) = crate::jobs::enrichment::process_enrich_segments_from_transcript(
                        self.db.clone(),
                        self.job_manager.clone(),
                        job_id,
                        asset_id,
                    ).await {
                        eprintln!("Error processing EnrichSegmentsFromTranscript job {}: {:?}", job_id, e);
                        let _ = self.job_manager.update_job_status(job_id, JobStatus::Failed, None);
                        return Err(e);
                    }
                } else {
                    eprintln!("EnrichSegmentsFromTranscript job {} missing asset_id", job_id);
                    let _ = self.job_manager.update_job_status(job_id, JobStatus::Failed, None);
                }
            }
            JobType::EnrichSegmentsFromVision => {
                if let Some(asset_id) = Self::extract_asset_id_from_payload(&job.payload) {
                    if let Err(e) = crate::jobs::enrichment::process_enrich_segments_from_vision(
                        self.db.clone(),
                        self.job_manager.clone(),
                        job_id,
                        asset_id,
                    ).await {
                        eprintln!("Error processing EnrichSegmentsFromVision job {}: {:?}", job_id, e);
                        let _ = self.job_manager.update_job_status(job_id, JobStatus::Failed, None);
                        return Err(e);
                    }
                } else {
                    eprintln!("EnrichSegmentsFromVision job {} missing asset_id", job_id);
                    let _ = self.job_manager.update_job_status(job_id, JobStatus::Failed, None);
                }
            }
            JobType::ComputeSegmentMetadata => {
                if let Some(asset_id) = Self::extract_asset_id_from_payload(&job.payload) {
                    if let Err(e) = crate::jobs::metadata::process_compute_segment_metadata(
                        self.db.clone(),
                        self.job_manager.clone(),
                        job_id,
                        asset_id,
                    ).await {
                        eprintln!("Error processing ComputeSegmentMetadata job {}: {:?}", job_id, e);
                        let _ = self.job_manager.update_job_status(job_id, JobStatus::Failed, None);
                        return Err(e);
                    }
                } else {
                    eprintln!("ComputeSegmentMetadata job {} missing asset_id", job_id);
                    let _ = self.job_manager.update_job_status(job_id, JobStatus::Failed, None);
                }
            }
            JobType::EmbedSegments => {
                if let Some(asset_id) = Self::extract_asset_id_from_payload(&job.payload) {
                    if let Err(e) = crate::jobs::embeddings::process_embed_segments(
                        self.db.clone(),
                        self.job_manager.clone(),
                        job_id,
                        asset_id,
                    ).await {
                        eprintln!("Error processing EmbedSegments job {}: {:?}", job_id, e);
                        let _ = self.job_manager.update_job_status(job_id, JobStatus::Failed, None);
                        return Err(e);
                    }
                } else {
                    eprintln!("EmbedSegments job {} missing asset_id", job_id);
                    let _ = self.job_manager.update_job_status(job_id, JobStatus::Failed, None);
                }
            }
            _ => {
                // Other job types handled elsewhere
                // Don't mark as completed here - let the actual handlers do it
            }
        }
        
        // Only mark as completed if we didn't return early (for jobs that don't have handlers yet)
        // Actual implementations mark completion themselves
        
        Ok(())
    }

    /// Main processing loop
    pub async fn run(&self) {
        loop {
            // Get ready jobs (this locks the DB, but releases before await)
            let ready_jobs = match self.get_ready_jobs() {
                Ok(jobs) => jobs,
                Err(e) => {
                    eprintln!("Error getting ready jobs: {:?}", e);
                    sleep(Duration::from_secs(1)).await;
                    continue;
                }
            };
            
            // Process jobs (no DB locks held during await)
            for job_id in ready_jobs {
                if let Err(e) = self.process_job(job_id).await {
                    eprintln!("Error processing job {}: {:?}", job_id, e);
                    let _ = self.job_manager.update_job_status(
                        job_id,
                        JobStatus::Failed,
                        None,
                    );
                }
            }
            
            // Poll every 1-2 seconds
            sleep(Duration::from_secs(1)).await;
        }
    }
}

