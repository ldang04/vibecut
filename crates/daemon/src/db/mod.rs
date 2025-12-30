use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, Row};
use std::path::Path;
use std::sync::Mutex;

pub struct Database {
    pub(crate) conn: Mutex<Connection>,
}

impl Database {
    pub fn new(db_path: &Path) -> Result<Self> {
        let conn = Connection::open(db_path)?;
        let db = Database {
            conn: Mutex::new(conn),
        };
        db.init_schema()?;
        Ok(db)
    }

    fn init_schema(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "CREATE TABLE IF NOT EXISTS projects (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                created_at TEXT NOT NULL,
                cache_dir TEXT NOT NULL,
                style_profile_id INTEGER,
                FOREIGN KEY (style_profile_id) REFERENCES style_profiles(id)
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS media_assets (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id INTEGER NOT NULL,
                path TEXT NOT NULL,
                checksum TEXT,
                duration_ticks INTEGER NOT NULL,
                fps_num INTEGER NOT NULL,
                fps_den INTEGER NOT NULL,
                width INTEGER NOT NULL,
                height INTEGER NOT NULL,
                has_audio INTEGER NOT NULL,
                FOREIGN KEY (project_id) REFERENCES projects(id),
                UNIQUE(project_id, path)
            )",
            [],
        )?;
        
        // Migration: Check if table needs migration from old schema
        // Check if project_id column exists
        let has_project_id = conn
            .prepare("SELECT project_id FROM media_assets LIMIT 1")
            .is_ok();
        
        if !has_project_id {
            // Old schema detected - need to migrate
            // SQLite doesn't support dropping UNIQUE constraints, so we recreate the table
            conn.execute(
                "CREATE TABLE IF NOT EXISTS media_assets_migration (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    project_id INTEGER NOT NULL,
                    path TEXT NOT NULL,
                    checksum TEXT,
                    duration_ticks INTEGER NOT NULL,
                    fps_num INTEGER NOT NULL,
                    fps_den INTEGER NOT NULL,
                    width INTEGER NOT NULL,
                    height INTEGER NOT NULL,
                    has_audio INTEGER NOT NULL,
                    FOREIGN KEY (project_id) REFERENCES projects(id),
                    UNIQUE(project_id, path)
                )",
                [],
            )?;
            
            // Copy data with default project_id of 1 for existing rows
            // (or they can be manually assigned later)
            let _ = conn.execute(
                "INSERT INTO media_assets_migration 
                 SELECT id, 1, path, checksum, duration_ticks, fps_num, fps_den, width, height, has_audio 
                 FROM media_assets",
                [],
            );
            
            // Drop old table
            let _ = conn.execute("DROP TABLE media_assets", []);
            
            // Rename new table
            let _ = conn.execute("ALTER TABLE media_assets_migration RENAME TO media_assets", []);
        } else {
            // Check if old UNIQUE constraint on path alone exists
            // If the table was created with the new schema, it should have UNIQUE(project_id, path)
            // If it has the old schema, we'd need to recreate, but this is complex to detect
            // For now, assume if project_id exists, the schema is correct
        }
        
        // Migration: Add is_reference column if it doesn't exist
        let has_is_reference = conn
            .prepare("SELECT is_reference FROM media_assets LIMIT 1")
            .is_ok();
        
        if !has_is_reference {
            // Add is_reference column with default value of 0 (not a reference)
            let _ = conn.execute(
                "ALTER TABLE media_assets ADD COLUMN is_reference INTEGER NOT NULL DEFAULT 0",
                [],
            );
        }

        // Migration: Add thumbnail_dir column if it doesn't exist
        let has_thumbnail_dir = conn
            .prepare("SELECT thumbnail_dir FROM media_assets LIMIT 1")
            .is_ok();
        
        if !has_thumbnail_dir {
            // Add thumbnail_dir column (nullable, stores path to thumbnail directory)
            let _ = conn.execute(
                "ALTER TABLE media_assets ADD COLUMN thumbnail_dir TEXT",
                [],
            );
        }

        // Migration: Add analysis state tracking columns to media_assets
        let has_segments_built_at = conn
            .prepare("SELECT segments_built_at FROM media_assets LIMIT 1")
            .is_ok();
        
        if !has_segments_built_at {
            let _ = conn.execute(
                "ALTER TABLE media_assets ADD COLUMN segments_built_at TEXT",
                [],
            );
            let _ = conn.execute(
                "ALTER TABLE media_assets ADD COLUMN transcript_ready_at TEXT",
                [],
            );
            let _ = conn.execute(
                "ALTER TABLE media_assets ADD COLUMN vision_ready_at TEXT",
                [],
            );
            let _ = conn.execute(
                "ALTER TABLE media_assets ADD COLUMN metadata_ready_at TEXT",
                [],
            );
            let _ = conn.execute(
                "ALTER TABLE media_assets ADD COLUMN embeddings_ready_at TEXT",
                [],
            );
        }

        conn.execute(
            "CREATE TABLE IF NOT EXISTS proxies (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                media_asset_id INTEGER NOT NULL,
                path TEXT NOT NULL,
                codec TEXT NOT NULL,
                width INTEGER NOT NULL,
                height INTEGER NOT NULL,
                FOREIGN KEY (media_asset_id) REFERENCES media_assets(id)
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS segments (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                media_asset_id INTEGER NOT NULL,
                project_id INTEGER NOT NULL,
                start_ticks INTEGER NOT NULL,
                end_ticks INTEGER NOT NULL,
                src_in_ticks INTEGER,
                src_out_ticks INTEGER,
                segment_kind TEXT,
                summary_text TEXT,
                keywords_json TEXT,
                quality_json TEXT,
                subject_json TEXT,
                scene_json TEXT,
                capture_time TEXT,
                transcript TEXT,
                speaker TEXT,
                scores_json TEXT,
                tags_json TEXT,
                FOREIGN KEY (media_asset_id) REFERENCES media_assets(id),
                FOREIGN KEY (project_id) REFERENCES projects(id)
            )",
            [],
        )?;

        // Migration: Add new segment columns if they don't exist
        let has_project_id = conn
            .prepare("SELECT project_id FROM segments LIMIT 1")
            .is_ok();
        
        if !has_project_id {
            // Add project_id column (default to 1 for existing rows, will be backfilled properly)
            let _ = conn.execute(
                "ALTER TABLE segments ADD COLUMN project_id INTEGER NOT NULL DEFAULT 1",
                [],
            );
            let _ = conn.execute(
                "ALTER TABLE segments ADD COLUMN src_in_ticks INTEGER",
                [],
            );
            let _ = conn.execute(
                "ALTER TABLE segments ADD COLUMN src_out_ticks INTEGER",
                [],
            );
            let _ = conn.execute(
                "ALTER TABLE segments ADD COLUMN segment_kind TEXT",
                [],
            );
            let _ = conn.execute(
                "ALTER TABLE segments ADD COLUMN summary_text TEXT",
                [],
            );
            let _ = conn.execute(
                "ALTER TABLE segments ADD COLUMN keywords_json TEXT",
                [],
            );
            let _ = conn.execute(
                "ALTER TABLE segments ADD COLUMN quality_json TEXT",
                [],
            );
            let _ = conn.execute(
                "ALTER TABLE segments ADD COLUMN subject_json TEXT",
                [],
            );
            let _ = conn.execute(
                "ALTER TABLE segments ADD COLUMN scene_json TEXT",
                [],
            );
            let _ = conn.execute(
                "ALTER TABLE segments ADD COLUMN capture_time TEXT",
                [],
            );
            
            // Backfill src_in_ticks and src_out_ticks from start_ticks and end_ticks
            let _ = conn.execute(
                "UPDATE segments SET src_in_ticks = start_ticks WHERE src_in_ticks IS NULL",
                [],
            );
            let _ = conn.execute(
                "UPDATE segments SET src_out_ticks = end_ticks WHERE src_out_ticks IS NULL",
                [],
            );
        }

        conn.execute(
            "CREATE TABLE IF NOT EXISTS embeddings (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                segment_id INTEGER NOT NULL,
                embedding_type TEXT NOT NULL,
                model_name TEXT NOT NULL,
                model_version TEXT,
                vector_blob BLOB NOT NULL,
                semantic_text TEXT,
                FOREIGN KEY (segment_id) REFERENCES segments(id),
                UNIQUE(segment_id, embedding_type, model_name)
            )",
            [],
        )?;

        // Migration: Update embeddings table if it has old schema
        let has_embedding_type = conn
            .prepare("SELECT embedding_type FROM embeddings LIMIT 1")
            .is_ok();
        
        if !has_embedding_type {
            // Add new columns
            let _ = conn.execute(
                "ALTER TABLE embeddings ADD COLUMN embedding_type TEXT",
                [],
            );
            let _ = conn.execute(
                "ALTER TABLE embeddings ADD COLUMN model_name TEXT",
                [],
            );
            
            // Migrate existing embeddings to semantic type
            let _ = conn.execute(
                "UPDATE embeddings SET embedding_type = 'semantic', model_name = 'text-embedding-3-small' WHERE embedding_type IS NULL",
                [],
            );
            
            // Make columns NOT NULL after migration
            // SQLite doesn't support ALTER COLUMN, so we'll handle NULLs in code
        }
        
        // Migration: Add semantic_text column if it doesn't exist
        let has_semantic_text = conn
            .prepare("SELECT semantic_text FROM embeddings LIMIT 1")
            .is_ok();
        
        if !has_semantic_text {
            let _ = conn.execute(
                "ALTER TABLE embeddings ADD COLUMN semantic_text TEXT",
                [],
            );
        }

        conn.execute(
            "CREATE TABLE IF NOT EXISTS style_profiles (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                project_id INTEGER,
                reference_asset_ids_json TEXT,
                json_blob TEXT NOT NULL,
                created_at TEXT NOT NULL,
                FOREIGN KEY (project_id) REFERENCES projects(id)
            )",
            [],
        )?;

        // Migration: Add new columns to style_profiles if they don't exist
        let has_project_id = conn
            .prepare("SELECT project_id FROM style_profiles LIMIT 1")
            .is_ok();
        
        if !has_project_id {
            let _ = conn.execute(
                "ALTER TABLE style_profiles ADD COLUMN project_id INTEGER",
                [],
            );
            let _ = conn.execute(
                "ALTER TABLE style_profiles ADD COLUMN reference_asset_ids_json TEXT",
                [],
            );
        }

        conn.execute(
            "CREATE TABLE IF NOT EXISTS timeline_projects (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id INTEGER NOT NULL,
                json_blob TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                FOREIGN KEY (project_id) REFERENCES projects(id)
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS jobs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                type TEXT NOT NULL,
                status TEXT NOT NULL,
                progress REAL NOT NULL,
                payload_json TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS edit_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id INTEGER NOT NULL,
                diff_json TEXT NOT NULL,
                created_at TEXT NOT NULL,
                FOREIGN KEY (project_id) REFERENCES projects(id)
            )",
            [],
        )?;

        // New tables for raw analysis results
        conn.execute(
            "CREATE TABLE IF NOT EXISTS asset_transcripts (
                asset_id INTEGER PRIMARY KEY,
                transcript_json TEXT NOT NULL,
                FOREIGN KEY (asset_id) REFERENCES media_assets(id)
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS asset_vision (
                asset_id INTEGER PRIMARY KEY,
                vision_json TEXT NOT NULL,
                FOREIGN KEY (asset_id) REFERENCES media_assets(id)
            )",
            [],
        )?;

        // New tables for orchestrator history
        conn.execute(
            "CREATE TABLE IF NOT EXISTS orchestrator_messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id INTEGER NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                created_at TEXT NOT NULL,
                FOREIGN KEY (project_id) REFERENCES projects(id)
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS orchestrator_proposals (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id INTEGER NOT NULL,
                proposal_json TEXT NOT NULL,
                created_at TEXT NOT NULL,
                FOREIGN KEY (project_id) REFERENCES projects(id)
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS orchestrator_applies (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id INTEGER NOT NULL,
                edit_plan_json TEXT NOT NULL,
                created_at TEXT NOT NULL,
                FOREIGN KEY (project_id) REFERENCES projects(id)
            )",
            [],
        )?;

        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct Project {
    pub id: i64,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub cache_dir: String,
    pub style_profile_id: Option<i64>,
}

impl Project {
    pub fn from_row(row: &Row) -> rusqlite::Result<Self> {
        let created_at_str: String = row.get(2)?;
        let created_at = DateTime::parse_from_rfc3339(&created_at_str)
            .map_err(|_| rusqlite::Error::InvalidColumnType(2, "TEXT".to_string(), rusqlite::types::Type::Text))?
            .with_timezone(&Utc);
        
        Ok(Project {
            id: row.get(0)?,
            name: row.get(1)?,
            created_at,
            cache_dir: row.get(3)?,
            style_profile_id: row.get(4)?,
        })
    }
}

impl Database {
    pub fn create_project(&self, name: &str, cache_dir: &str) -> Result<i64> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO projects (name, created_at, cache_dir) VALUES (?1, ?2, ?3)",
            params![name, now, cache_dir],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn get_project(&self, id: i64) -> Result<Option<Project>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, created_at, cache_dir, style_profile_id FROM projects WHERE id = ?1"
        )?;
        let mut rows = stmt.query_map(params![id], |row| Project::from_row(row))?;
        
        match rows.next() {
            Some(Ok(project)) => Ok(Some(project)),
            Some(Err(e)) => Err(e.into()),
            None => Ok(None),
        }
    }

    pub fn get_all_projects(&self) -> Result<Vec<Project>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, created_at, cache_dir, style_profile_id FROM projects ORDER BY created_at DESC"
        )?;
        let rows = stmt.query_map([], |row| Project::from_row(row))?;
        
        let mut projects = Vec::new();
        for row in rows {
            projects.push(row?);
        }
        Ok(projects)
    }

    pub fn delete_project(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM projects WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn create_media_asset(
        &self,
        project_id: i64,
        path: &str,
        checksum: Option<&str>,
        duration_ticks: i64,
        fps_num: i32,
        fps_den: i32,
        width: i32,
        height: i32,
        has_audio: bool,
    ) -> Result<i64> {
        self.create_media_asset_with_reference_flag(
            project_id, path, checksum, duration_ticks, fps_num, fps_den, width, height, has_audio, false,
        )
    }
    
    pub fn create_media_asset_with_reference_flag(
        &self,
        project_id: i64,
        path: &str,
        checksum: Option<&str>,
        duration_ticks: i64,
        fps_num: i32,
        fps_den: i32,
        width: i32,
        height: i32,
        has_audio: bool,
        is_reference: bool,
    ) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        
        // Check if asset already exists for this project
        let existing_id: Result<i64, rusqlite::Error> = conn.query_row(
            "SELECT id FROM media_assets WHERE project_id = ?1 AND path = ?2",
            params![project_id, path],
            |row| row.get::<_, i64>(0),
        );
        
        match existing_id {
            Ok(id) => {
                // Update existing asset
                conn.execute(
                    "UPDATE media_assets SET checksum = ?1, duration_ticks = ?2, fps_num = ?3, fps_den = ?4, width = ?5, height = ?6, has_audio = ?7, is_reference = ?8 WHERE id = ?9",
                    params![checksum, duration_ticks, fps_num, fps_den, width, height, if has_audio { 1 } else { 0 }, if is_reference { 1 } else { 0 }, id],
                )?;
                Ok(id)
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                // Insert new asset
                conn.execute(
                    "INSERT INTO media_assets (project_id, path, checksum, duration_ticks, fps_num, fps_den, width, height, has_audio, is_reference) 
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                    params![project_id, path, checksum, duration_ticks, fps_num, fps_den, width, height, if has_audio { 1 } else { 0 }, if is_reference { 1 } else { 0 }],
                )?;
                Ok(conn.last_insert_rowid())
            }
            Err(e) => Err(e.into()),
        }
    }

    pub fn create_proxy(
        &self,
        media_asset_id: i64,
        path: &str,
        codec: &str,
        width: i32,
        height: i32,
    ) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO proxies (media_asset_id, path, codec, width, height) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![media_asset_id, path, codec, width, height],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn create_style_profile(&self, name: &str, json_blob: &str) -> Result<i64> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO style_profiles (name, json_blob, created_at) VALUES (?1, ?2, ?3)",
            params![name, json_blob, now],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn get_style_profile(&self, id: i64) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT json_blob FROM style_profiles WHERE id = ?1")?;
        let mut rows = stmt.query_map(params![id], |row| {
            Ok(row.get::<_, String>(0)?)
        })?;
        
        match rows.next() {
            Some(Ok(blob)) => Ok(Some(blob)),
            Some(Err(e)) => Err(e.into()),
            None => Ok(None),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Segment {
    pub id: i64,
    pub media_asset_id: i64,
    pub project_id: i64,
    pub start_ticks: i64,
    pub end_ticks: i64,
    pub src_in_ticks: Option<i64>,
    pub src_out_ticks: Option<i64>,
    pub segment_kind: Option<String>,
    pub summary_text: Option<String>,
    pub keywords_json: Option<String>,
    pub quality_json: Option<String>,
    pub subject_json: Option<String>,
    pub scene_json: Option<String>,
    pub capture_time: Option<String>,
    pub transcript: Option<String>,
    pub speaker: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MediaAssetInfo {
    pub id: i64,
    pub path: String,
    pub duration_ticks: i64,
    pub fps_num: i32,
    pub fps_den: i32,
    pub width: i32,
    pub height: i32,
}

impl Database {
    /// Get all segments with their media asset info for a project
    pub fn get_segments_for_project(&self, project_id: i64) -> Result<Vec<(Segment, MediaAssetInfo)>> {
        let conn = self.conn.lock().unwrap();
        
        // Join segments with media_assets to get full info, filter by project_id
        let mut stmt = conn.prepare(
            "SELECT s.id, s.media_asset_id, s.project_id, s.start_ticks, s.end_ticks, 
                    s.src_in_ticks, s.src_out_ticks, s.segment_kind, s.summary_text, 
                    s.keywords_json, s.quality_json, s.subject_json, s.scene_json, 
                    s.capture_time, s.transcript, s.speaker,
                    ma.id, ma.path, ma.duration_ticks, ma.fps_num, ma.fps_den, ma.width, ma.height
             FROM segments s
             INNER JOIN media_assets ma ON s.media_asset_id = ma.id
             WHERE s.project_id = ?1
             ORDER BY ma.id, s.start_ticks"
        )?;
        
        let rows = stmt.query_map(params![project_id], |row| {
            let segment = Segment {
                id: row.get(0)?,
                media_asset_id: row.get(1)?,
                project_id: row.get(2)?,
                start_ticks: row.get(3)?,
                end_ticks: row.get(4)?,
                src_in_ticks: row.get(5)?,
                src_out_ticks: row.get(6)?,
                segment_kind: row.get(7)?,
                summary_text: row.get(8)?,
                keywords_json: row.get(9)?,
                quality_json: row.get(10)?,
                subject_json: row.get(11)?,
                scene_json: row.get(12)?,
                capture_time: row.get(13)?,
                transcript: row.get(14)?,
                speaker: row.get(15)?,
            };
            
            let media_asset = MediaAssetInfo {
                id: row.get(16)?,
                path: row.get(17)?,
                duration_ticks: row.get(18)?,
                fps_num: row.get(19)?,
                fps_den: row.get(20)?,
                width: row.get(21)?,
                height: row.get(22)?,
            };
            
            Ok((segment, media_asset))
        })?;
        
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        
        Ok(result)
    }

    /// Get coalesced src_in_ticks (single source of truth for reading)
    pub fn get_coalesced_src_in(segment: &Segment) -> i64 {
        segment.src_in_ticks.unwrap_or(segment.start_ticks)
    }

    /// Get coalesced src_out_ticks (single source of truth for reading)
    pub fn get_coalesced_src_out(segment: &Segment) -> i64 {
        segment.src_out_ticks.unwrap_or(segment.end_ticks)
    }

    /// Create a new segment with stable identity fields
    pub fn create_segment(
        &self,
        project_id: i64,
        media_asset_id: i64,
        src_in_ticks: i64,
        src_out_ticks: i64,
    ) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO segments (project_id, media_asset_id, src_in_ticks, src_out_ticks, start_ticks, end_ticks) 
             VALUES (?1, ?2, ?3, ?4, ?3, ?4)",
            params![project_id, media_asset_id, src_in_ticks, src_out_ticks],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Update segment metadata fields (enrichable fields)
    pub fn update_segment_metadata(
        &self,
        segment_id: i64,
        summary_text: Option<&str>,
        keywords_json: Option<&str>,
        quality_json: Option<&str>,
        subject_json: Option<&str>,
        scene_json: Option<&str>,
        transcript: Option<&str>,
        segment_kind: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE segments SET 
                summary_text = COALESCE(?1, summary_text),
                keywords_json = COALESCE(?2, keywords_json),
                quality_json = COALESCE(?3, quality_json),
                subject_json = COALESCE(?4, subject_json),
                scene_json = COALESCE(?5, scene_json),
                transcript = COALESCE(?6, transcript),
                segment_kind = COALESCE(?7, segment_kind)
             WHERE id = ?8",
            params![summary_text, keywords_json, quality_json, subject_json, scene_json, transcript, segment_kind, segment_id],
        )?;
        Ok(())
    }

    /// Get segments for a specific asset
    pub fn get_segments_by_asset(&self, asset_id: i64) -> Result<Vec<Segment>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, media_asset_id, project_id, start_ticks, end_ticks, 
                    src_in_ticks, src_out_ticks, segment_kind, summary_text, 
                    keywords_json, quality_json, subject_json, scene_json, 
                    capture_time, transcript, speaker
             FROM segments
             WHERE media_asset_id = ?1
             ORDER BY start_ticks"
        )?;
        
        let rows = stmt.query_map(params![asset_id], |row| {
            Ok(Segment {
                id: row.get(0)?,
                media_asset_id: row.get(1)?,
                project_id: row.get(2)?,
                start_ticks: row.get(3)?,
                end_ticks: row.get(4)?,
                src_in_ticks: row.get(5)?,
                src_out_ticks: row.get(6)?,
                segment_kind: row.get(7)?,
                summary_text: row.get(8)?,
                keywords_json: row.get(9)?,
                quality_json: row.get(10)?,
                subject_json: row.get(11)?,
                scene_json: row.get(12)?,
                capture_time: row.get(13)?,
                transcript: row.get(14)?,
                speaker: row.get(15)?,
            })
        })?;
        
        let mut segments = Vec::new();
        for row in rows {
            segments.push(row?);
        }
        Ok(segments)
    }

    /// Get segment with its embeddings
    pub fn get_segment_with_embeddings(&self, segment_id: i64) -> Result<Option<(Segment, Vec<(String, String, Vec<u8>)>)>> {
        let conn = self.conn.lock().unwrap();
        
        // Get segment
        let mut stmt = conn.prepare(
            "SELECT id, media_asset_id, project_id, start_ticks, end_ticks, 
                    src_in_ticks, src_out_ticks, segment_kind, summary_text, 
                    keywords_json, quality_json, subject_json, scene_json, 
                    capture_time, transcript, speaker
             FROM segments
             WHERE id = ?1"
        )?;
        
        let segment_opt: Option<Segment> = stmt.query_row(params![segment_id], |row| {
            Ok(Segment {
                id: row.get(0)?,
                media_asset_id: row.get(1)?,
                project_id: row.get(2)?,
                start_ticks: row.get(3)?,
                end_ticks: row.get(4)?,
                src_in_ticks: row.get(5)?,
                src_out_ticks: row.get(6)?,
                segment_kind: row.get(7)?,
                summary_text: row.get(8)?,
                keywords_json: row.get(9)?,
                quality_json: row.get(10)?,
                subject_json: row.get(11)?,
                scene_json: row.get(12)?,
                capture_time: row.get(13)?,
                transcript: row.get(14)?,
                speaker: row.get(15)?,
            })
        }).ok();
        
        if let Some(segment) = segment_opt {
            // Get embeddings
            let mut emb_stmt = conn.prepare(
                "SELECT embedding_type, model_name, vector_blob
                 FROM embeddings
                 WHERE segment_id = ?1"
            )?;
            
            let emb_rows = emb_stmt.query_map(params![segment_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                ))
            })?;
            
            let mut embeddings = Vec::new();
            for row in emb_rows {
                embeddings.push(row?);
            }
            
            Ok(Some((segment, embeddings)))
        } else {
            Ok(None)
        }
    }

    /// Update asset analysis state timestamp
    pub fn update_asset_analysis_state(
        &self,
        asset_id: i64,
        field: &str,
        timestamp: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let timestamp_str = timestamp.map(|s| s.to_string()).unwrap_or_else(|| {
            Utc::now().to_rfc3339()
        });
        
        match field {
            "segments_built_at" => {
                conn.execute(
                    "UPDATE media_assets SET segments_built_at = ?1 WHERE id = ?2",
                    params![timestamp_str, asset_id],
                )?;
            }
            "transcript_ready_at" => {
                conn.execute(
                    "UPDATE media_assets SET transcript_ready_at = ?1 WHERE id = ?2",
                    params![timestamp_str, asset_id],
                )?;
            }
            "vision_ready_at" => {
                conn.execute(
                    "UPDATE media_assets SET vision_ready_at = ?1 WHERE id = ?2",
                    params![timestamp_str, asset_id],
                )?;
            }
            "metadata_ready_at" => {
                conn.execute(
                    "UPDATE media_assets SET metadata_ready_at = ?1 WHERE id = ?2",
                    params![timestamp_str, asset_id],
                )?;
            }
            "embeddings_ready_at" => {
                conn.execute(
                    "UPDATE media_assets SET embeddings_ready_at = ?1 WHERE id = ?2",
                    params![timestamp_str, asset_id],
                )?;
            }
            _ => return Err(anyhow::anyhow!("Unknown analysis state field: {}", field)),
        }
        Ok(())
    }

    /// Check if asset prerequisites are ready for job gating
    pub fn check_asset_prerequisites(
        &self,
        asset_id: i64,
        required_states: &[&str],
    ) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        
        for state in required_states {
            let column = match *state {
                "segments_built" => "segments_built_at",
                "transcript_ready" => "transcript_ready_at",
                "vision_ready" => "vision_ready_at",
                "metadata_ready" => "metadata_ready_at",
                "embeddings_ready" => "embeddings_ready_at",
                _ => return Err(anyhow::anyhow!("Unknown state: {}", state)),
            };
            
            let is_ready: bool = conn.query_row(
                &format!("SELECT {} IS NOT NULL FROM media_assets WHERE id = ?1", column),
                params![asset_id],
                |row| row.get(0),
            )?;
            
            if !is_ready {
                return Ok(false);
            }
        }
        
        Ok(true)
    }

    pub fn get_media_assets_for_project(&self, project_id: i64) -> Result<Vec<MediaAssetInfo>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, path, duration_ticks, fps_num, fps_den, width, height
             FROM media_assets
             WHERE project_id = ?1 AND project_id IS NOT NULL AND (is_reference IS NULL OR is_reference = 0)
             ORDER BY id DESC"
        )?;
        
        let rows = stmt.query_map(params![project_id], |row| {
            Ok(MediaAssetInfo {
                id: row.get(0)?,
                path: row.get(1)?,
                duration_ticks: row.get(2)?,
                fps_num: row.get(3)?,
                fps_den: row.get(4)?,
                width: row.get(5)?,
                height: row.get(6)?,
            })
        })?;
        
        let mut assets = Vec::new();
        for row in rows {
            assets.push(row?);
        }
        Ok(assets)
    }

    pub fn get_reference_assets_for_project(&self, project_id: i64) -> Result<Vec<MediaAssetInfo>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, path, duration_ticks, fps_num, fps_den, width, height
             FROM media_assets
             WHERE project_id = ?1 AND project_id IS NOT NULL AND is_reference = 1
             ORDER BY id DESC"
        )?;
        
        let rows = stmt.query_map(params![project_id], |row| {
            Ok(MediaAssetInfo {
                id: row.get(0)?,
                path: row.get(1)?,
                duration_ticks: row.get(2)?,
                fps_num: row.get(3)?,
                fps_den: row.get(4)?,
                width: row.get(5)?,
                height: row.get(6)?,
            })
        })?;
        
        let mut assets = Vec::new();
        for row in rows {
            assets.push(row?);
        }
        Ok(assets)
    }

    pub fn delete_media_asset(&self, project_id: i64, asset_id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        // Verify the asset belongs to the project before deleting
        let asset_exists: Result<i64, rusqlite::Error> = conn.query_row(
            "SELECT id FROM media_assets WHERE id = ?1 AND project_id = ?2",
            params![asset_id, project_id],
            |row| row.get::<_, i64>(0),
        );
        
        match asset_exists {
            Ok(_) => {
                // Delete the media asset (cascade will handle related records if foreign keys are set up)
                conn.execute(
                    "DELETE FROM media_assets WHERE id = ?1 AND project_id = ?2",
                    params![asset_id, project_id],
                )?;
                Ok(())
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                // Asset doesn't exist or doesn't belong to this project
                Err(anyhow::anyhow!("Media asset not found or doesn't belong to this project"))
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Store timeline for a project
    pub fn store_timeline(&self, project_id: i64, timeline_json: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().unwrap();
        
        // Check if timeline already exists for this project
        let existing = conn.query_row(
            "SELECT id FROM timeline_projects WHERE project_id = ?1",
            params![project_id],
            |row| row.get::<_, i64>(0),
        );
        
        match existing {
            Ok(_id) => {
                // Update existing
                conn.execute(
                    "UPDATE timeline_projects SET json_blob = ?1, updated_at = ?2 WHERE project_id = ?3",
                    params![timeline_json, now, project_id],
                )?;
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                // Insert new
                conn.execute(
                    "INSERT INTO timeline_projects (project_id, json_blob, created_at, updated_at) VALUES (?1, ?2, ?3, ?4)",
                    params![project_id, timeline_json, now, now],
                )?;
            }
            Err(e) => return Err(e.into()),
        }
        
        Ok(())
    }

    /// Get timeline for a project
    pub fn get_timeline(&self, project_id: i64) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT json_blob FROM timeline_projects WHERE project_id = ?1")?;
        let mut rows = stmt.query_map(params![project_id], |row| {
            Ok(row.get::<_, String>(0)?)
        })?;
        
        match rows.next() {
            Some(Ok(blob)) => Ok(Some(blob)),
            Some(Err(e)) => Err(e.into()),
            None => Ok(None),
        }
    }

    /// Get proxy path for a media asset
    pub fn get_proxy_path(&self, media_asset_id: i64) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT path FROM proxies WHERE media_asset_id = ?1 LIMIT 1")?;
        let mut rows = stmt.query_map(params![media_asset_id], |row| {
            Ok(row.get::<_, String>(0)?)
        })?;
        
        match rows.next() {
            Some(Ok(path)) => Ok(Some(path)),
            Some(Err(e)) => Err(e.into()),
            None => Ok(None),
        }
    }

    /// Get original media asset path by ID
    pub fn get_media_asset_path(&self, media_asset_id: i64) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT path FROM media_assets WHERE id = ?1 LIMIT 1")?;
        let mut rows = stmt.query_map(params![media_asset_id], |row| {
            Ok(row.get::<_, String>(0)?)
        })?;
        
        match rows.next() {
            Some(Ok(path)) => Ok(Some(path)),
            Some(Err(e)) => Err(e.into()),
            None => Ok(None),
        }
    }

    /// Set thumbnail directory path for a media asset
    pub fn set_thumbnail_dir(&self, media_asset_id: i64, thumbnail_dir: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE media_assets SET thumbnail_dir = ?1 WHERE id = ?2",
            params![thumbnail_dir, media_asset_id],
        )?;
        Ok(())
    }

    /// Get thumbnail directory path for a media asset
    pub fn get_thumbnail_dir(&self, media_asset_id: i64) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT thumbnail_dir FROM media_assets WHERE id = ?1 LIMIT 1")?;
        let mut rows = stmt.query_map(params![media_asset_id], |row| {
            Ok(row.get::<_, Option<String>>(0)?)
        })?;
        
        match rows.next() {
            Some(Ok(dir)) => Ok(dir),
            Some(Err(e)) => Err(e.into()),
            None => Ok(None),
        }
    }

    /// Store raw transcript results for an asset
    pub fn store_asset_transcript(&self, asset_id: i64, transcript_json: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO asset_transcripts (asset_id, transcript_json) VALUES (?1, ?2)",
            params![asset_id, transcript_json],
        )?;
        Ok(())
    }

    /// Get raw transcript results for an asset
    pub fn get_asset_transcript(&self, asset_id: i64) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT transcript_json FROM asset_transcripts WHERE asset_id = ?1")?;
        let mut rows = stmt.query_map(params![asset_id], |row| {
            Ok(row.get::<_, String>(0)?)
        })?;
        
        match rows.next() {
            Some(Ok(json)) => Ok(Some(json)),
            Some(Err(e)) => Err(e.into()),
            None => Ok(None),
        }
    }

    /// Store raw vision analysis results for an asset
    pub fn store_asset_vision(&self, asset_id: i64, vision_json: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO asset_vision (asset_id, vision_json) VALUES (?1, ?2)",
            params![asset_id, vision_json],
        )?;
        Ok(())
    }

    /// Get raw vision analysis results for an asset
    pub fn get_asset_vision(&self, asset_id: i64) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT vision_json FROM asset_vision WHERE asset_id = ?1")?;
        let mut rows = stmt.query_map(params![asset_id], |row| {
            Ok(row.get::<_, String>(0)?)
        })?;
        
        match rows.next() {
            Some(Ok(json)) => Ok(Some(json)),
            Some(Err(e)) => Err(e.into()),
            None => Ok(None),
        }
    }

    /// Store orchestrator message
    pub fn store_orchestrator_message(
        &self,
        project_id: i64,
        role: &str,
        content: &str,
    ) -> Result<i64> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO orchestrator_messages (project_id, role, content, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![project_id, role, content, now],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Store orchestrator proposal
    pub fn store_orchestrator_proposal(
        &self,
        project_id: i64,
        proposal_json: &str,
    ) -> Result<i64> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO orchestrator_proposals (project_id, proposal_json, created_at) VALUES (?1, ?2, ?3)",
            params![project_id, proposal_json, now],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Store orchestrator applied plan
    pub fn store_orchestrator_apply(
        &self,
        project_id: i64,
        edit_plan_json: &str,
    ) -> Result<i64> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO orchestrator_applies (project_id, edit_plan_json, created_at) VALUES (?1, ?2, ?3)",
            params![project_id, edit_plan_json, now],
        )?;
        Ok(conn.last_insert_rowid())
    }
}
