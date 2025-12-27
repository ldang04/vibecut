use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

use crate::db::Database;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JobType {
    ImportRaw,
    GenerateProxy,
    Transcribe,
    AnalyzeVision,
    GenerateEdit,
    Export,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JobStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
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

pub struct JobManager {
    db: Arc<Database>,
}

impl JobManager {
    pub fn new(db: Arc<Database>) -> Self {
        JobManager { db }
    }

    pub fn create_job(
        &self,
        job_type: JobType,
        payload: Option<Value>,
    ) -> Result<i64> {
        let now = Utc::now().to_rfc3339();
        let status = JobStatus::Pending;
        let job_type_str = serde_json::to_string(&job_type)?;
        let status_str = serde_json::to_string(&status)?;
        let payload_str = payload.as_ref().map(|v| serde_json::to_string(v)).transpose()?;

        let conn = self.db.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO jobs (type, status, progress, payload_json, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![job_type_str, status_str, 0.0, payload_str, now, now],
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

            let job_type = serde_json::from_str(&job_type_str)
                .map_err(|_e| rusqlite::Error::InvalidColumnType(1, "TEXT".to_string(), rusqlite::types::Type::Text))?;
            let status = serde_json::from_str(&status_str)
                .map_err(|_e| rusqlite::Error::InvalidColumnType(2, "TEXT".to_string(), rusqlite::types::Type::Text))?;

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
        let status_str = serde_json::to_string(&status)?;

        let conn = self.db.conn.lock().unwrap();
        if let Some(prog) = progress {
            conn.execute(
                "UPDATE jobs SET status = ?1, progress = ?2, updated_at = ?3 WHERE id = ?4",
                params![status_str, prog, now, id],
            )?;
        } else {
            let mut stmt = conn.prepare(
                "UPDATE jobs SET status = ?1, updated_at = ?2 WHERE id = ?3"
            )?;
            stmt.execute(params![status_str, now, id])?;
        }

        Ok(())
    }

    pub fn cancel_job(&self, id: i64) -> Result<()> {
        self.update_job_status(id, JobStatus::Cancelled, None)
    }
}
