use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::broadcast;

use crate::db::Database;

pub mod processor;
pub mod build_segments;
pub mod transcribe;
pub mod vision;
pub mod enrichment;
pub mod metadata;
pub mod embeddings;
pub mod twelvelabs_index;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JobType {
    ImportRaw,
    GenerateProxy,
    Transcribe,
    AnalyzeVision,
    GenerateEdit,
    Export,
    BuildSegments,
    TranscribeAsset,
    AnalyzeVisionAsset,
    EnrichSegmentsFromTranscript,
    EnrichSegmentsFromVision,
    ComputeSegmentMetadata,
    EmbedSegments,
    IndexAssetWithTwelveLabs,
}

impl JobType {
    /// Convert to plain string (variant name)
    pub fn to_string(&self) -> &'static str {
        match self {
            JobType::ImportRaw => "ImportRaw",
            JobType::GenerateProxy => "GenerateProxy",
            JobType::Transcribe => "Transcribe",
            JobType::AnalyzeVision => "AnalyzeVision",
            JobType::GenerateEdit => "GenerateEdit",
            JobType::Export => "Export",
            JobType::BuildSegments => "BuildSegments",
            JobType::TranscribeAsset => "TranscribeAsset",
            JobType::AnalyzeVisionAsset => "AnalyzeVisionAsset",
            JobType::EnrichSegmentsFromTranscript => "EnrichSegmentsFromTranscript",
            JobType::EnrichSegmentsFromVision => "EnrichSegmentsFromVision",
            JobType::ComputeSegmentMetadata => "ComputeSegmentMetadata",
            JobType::EmbedSegments => "EmbedSegments",
            JobType::IndexAssetWithTwelveLabs => "IndexAssetWithTwelveLabs",
        }
    }
    
    /// Parse from plain string (variant name)
    pub fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "ImportRaw" => Ok(JobType::ImportRaw),
            "GenerateProxy" => Ok(JobType::GenerateProxy),
            "Transcribe" => Ok(JobType::Transcribe),
            "AnalyzeVision" => Ok(JobType::AnalyzeVision),
            "GenerateEdit" => Ok(JobType::GenerateEdit),
            "Export" => Ok(JobType::Export),
            "BuildSegments" => Ok(JobType::BuildSegments),
            "TranscribeAsset" => Ok(JobType::TranscribeAsset),
            "AnalyzeVisionAsset" => Ok(JobType::AnalyzeVisionAsset),
            "EnrichSegmentsFromTranscript" => Ok(JobType::EnrichSegmentsFromTranscript),
            "EnrichSegmentsFromVision" => Ok(JobType::EnrichSegmentsFromVision),
            "ComputeSegmentMetadata" => Ok(JobType::ComputeSegmentMetadata),
            "EmbedSegments" => Ok(JobType::EmbedSegments),
            "IndexAssetWithTwelveLabs" => Ok(JobType::IndexAssetWithTwelveLabs),
            _ => Err(format!("Unknown job type: {}", s)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JobStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl JobStatus {
    /// Convert to plain string (variant name)
    pub fn to_string(&self) -> &'static str {
        match self {
            JobStatus::Pending => "Pending",
            JobStatus::Running => "Running",
            JobStatus::Completed => "Completed",
            JobStatus::Failed => "Failed",
            JobStatus::Cancelled => "Cancelled",
        }
    }
    
    /// Parse from plain string (variant name)
    pub fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "Pending" => Ok(JobStatus::Pending),
            "Running" => Ok(JobStatus::Running),
            "Completed" => Ok(JobStatus::Completed),
            "Failed" => Ok(JobStatus::Failed),
            "Cancelled" => Ok(JobStatus::Cancelled),
            _ => Err(format!("Unknown job status: {}", s)),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Job {
    pub id: i64,
    pub job_type: JobType,
    pub status: JobStatus,
    pub progress: f64,
    pub payload: Option<Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum JobEvent {
    JobCompleted {
        job_id: i64,
        job_type: String,
        asset_id: Option<i64>,
    },
    JobFailed {
        job_id: i64,
        job_type: String,
        asset_id: Option<i64>,
        error: String,
    },
    AnalysisComplete {
        asset_id: i64,
        readiness: String, // AssetReadiness as string
        project_id: i64,
    },
}

pub struct JobManager {
    db: Arc<Database>,
    event_sender: broadcast::Sender<JobEvent>,
}

impl JobManager {
    pub fn new(db: Arc<Database>) -> Self {
        let (event_sender, _) = broadcast::channel(1000); // Buffer up to 1000 events
        JobManager {
            db,
            event_sender,
        }
    }

    /// Get a receiver for job events
    pub fn subscribe(&self) -> broadcast::Receiver<JobEvent> {
        self.event_sender.subscribe()
    }

    /// Emit a job event (internal use)
    fn emit_event(&self, event: JobEvent) {
        // Ignore errors - receivers may not be listening
        let _ = self.event_sender.send(event);
    }

    /// Emit AnalysisComplete event (public, called from job processors)
    pub fn emit_analysis_complete(&self, asset_id: i64, project_id: i64, readiness: String) {
        self.emit_event(JobEvent::AnalysisComplete {
            asset_id,
            readiness,
            project_id,
        });
    }

    pub fn create_job(
        &self,
        job_type: JobType,
        payload: Option<Value>,
        dedupe_key: Option<String>,
    ) -> Result<i64> {
        let now = Utc::now().to_rfc3339();
        let status = JobStatus::Pending;
        let job_type_str = job_type.to_string(); // Plain string, not JSON
        let status_str = status.to_string(); // Plain string, not JSON
        let payload_str = payload.as_ref().map(|v| serde_json::to_string(v)).transpose()?;

        let conn = self.db.conn.lock().unwrap();
        
        // If dedupe_key provided, check for existing active job
        if let Some(ref key) = dedupe_key {
            let existing_id_result: Result<i64, rusqlite::Error> = conn.query_row(
                "SELECT id FROM jobs WHERE dedupe_key = ?1 AND is_active = 1 LIMIT 1",
                params![key],
                |row| row.get(0),
            );
            
            if let Ok(id) = existing_id_result {
                return Ok(id); // Return existing job_id
            }
            // If not found, continue to create new job
        }

        conn.execute(
            "INSERT INTO jobs (type, status, progress, payload_json, dedupe_key, is_active, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![job_type_str, status_str, 0.0, payload_str, dedupe_key, 1, now, now],
        )?;

        Ok(conn.last_insert_rowid())
    }

    pub fn get_job(&self, id: i64) -> Result<Option<Job>> {
        let conn = self.db.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, type, status, progress, payload_json, created_at, updated_at FROM jobs WHERE id = ?1"
        )?;

        let mut rows = stmt.query_map(params![id], |row| {
            let job_type_str: String = row.get(1)?;
            let status_str: String = row.get(2)?;
            let created_at_str: String = row.get(5)?;
            let updated_at_str: String = row.get(6)?;

            let job_type = JobType::from_str(&job_type_str)
                .map_err(|e| rusqlite::Error::InvalidColumnType(1, "TEXT".to_string(), rusqlite::types::Type::Text))?;
            let status = JobStatus::from_str(&status_str)
                .map_err(|e| rusqlite::Error::InvalidColumnType(2, "TEXT".to_string(), rusqlite::types::Type::Text))?;

            let payload_str: Option<String> = row.get(4)?;
            let payload = payload_str
                .map(|s| serde_json::from_str(&s))
                .transpose()
                .map_err(|_| rusqlite::Error::InvalidColumnType(4, "TEXT".to_string(), rusqlite::types::Type::Text))?;

            let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                .map_err(|_| rusqlite::Error::InvalidColumnType(5, "TEXT".to_string(), rusqlite::types::Type::Text))?
                .with_timezone(&Utc);
            let updated_at = DateTime::parse_from_rfc3339(&updated_at_str)
                .map_err(|_| rusqlite::Error::InvalidColumnType(6, "TEXT".to_string(), rusqlite::types::Type::Text))?
                .with_timezone(&Utc);

            Ok(Job {
                id: row.get(0)?,
                job_type,
                status,
                progress: row.get(3)?,
                payload,
                created_at,
                updated_at,
            })
        })?;

        match rows.next() {
            Some(Ok(job)) => Ok(Some(job)),
            Some(Err(e)) => Err(e.into()),
            None => Ok(None),
        }
    }

    pub fn update_job_status(
        &self,
        id: i64,
        status: JobStatus,
        progress: Option<f64>,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let status_str = status.to_string(); // Plain string, not JSON

        // Get job info for event emission (need to get job_type and asset_id)
        let job_opt = self.get_job(id)?;
        let job_type_str = job_opt.as_ref().map(|j| j.job_type.to_string());
        let asset_id = job_opt.as_ref()
            .and_then(|j| j.payload.as_ref())
            .and_then(|p| p.get("asset_id").and_then(|v| v.as_i64()));

        let conn = self.db.conn.lock().unwrap();
        
        // Set is_active = 0 when job completes, fails, or is cancelled
        let is_active = match status {
            JobStatus::Completed | JobStatus::Failed | JobStatus::Cancelled => 0,
            _ => 1,
        };
        
        if let Some(prog) = progress {
            conn.execute(
                "UPDATE jobs SET status = ?1, progress = ?2, is_active = ?3, updated_at = ?4 WHERE id = ?5",
                params![status_str, prog, is_active, now, id],
            )?;
        } else {
            conn.execute(
                "UPDATE jobs SET status = ?1, is_active = ?2, updated_at = ?3 WHERE id = ?4",
                params![status_str, is_active, now, id],
            )?;
        }

        // Emit events for completed/failed jobs
        if let Some(job_type) = job_type_str {
            match status {
                JobStatus::Completed => {
                    self.emit_event(JobEvent::JobCompleted {
                        job_id: id,
                        job_type: job_type.to_string(),
                        asset_id,
                    });
                },
                JobStatus::Failed => {
                    self.emit_event(JobEvent::JobFailed {
                        job_id: id,
                        job_type: job_type.to_string(),
                        asset_id,
                        error: "Job failed".to_string(), // Could extract from job if we store error messages
                    });
                },
                _ => {}
            }
        }

        Ok(())
    }

    pub fn cancel_job(&self, id: i64) -> Result<()> {
        self.update_job_status(id, JobStatus::Cancelled, None)
    }
}
