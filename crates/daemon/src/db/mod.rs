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
                start_ticks INTEGER NOT NULL,
                end_ticks INTEGER NOT NULL,
                transcript TEXT,
                speaker TEXT,
                scores_json TEXT,
                tags_json TEXT,
                FOREIGN KEY (media_asset_id) REFERENCES media_assets(id)
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS embeddings (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                segment_id INTEGER NOT NULL,
                model_version TEXT NOT NULL,
                vector_blob BLOB NOT NULL,
                FOREIGN KEY (segment_id) REFERENCES segments(id)
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS style_profiles (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                json_blob TEXT NOT NULL,
                created_at TEXT NOT NULL
            )",
            [],
        )?;

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
    pub start_ticks: i64,
    pub end_ticks: i64,
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
    /// Note: For V1, we get all segments. In the future, we'd link media_assets to projects.
    pub fn get_segments_for_project(&self, _project_id: i64) -> Result<Vec<(Segment, MediaAssetInfo)>> {
        let conn = self.conn.lock().unwrap();
        
        // Join segments with media_assets to get full info
        let mut stmt = conn.prepare(
            "SELECT s.id, s.media_asset_id, s.start_ticks, s.end_ticks, s.transcript, s.speaker,
                    ma.id, ma.path, ma.duration_ticks, ma.fps_num, ma.fps_den, ma.width, ma.height
             FROM segments s
             INNER JOIN media_assets ma ON s.media_asset_id = ma.id
             ORDER BY ma.id, s.start_ticks"
        )?;
        
        let rows = stmt.query_map([], |row| {
            let segment = Segment {
                id: row.get(0)?,
                media_asset_id: row.get(1)?,
                start_ticks: row.get(2)?,
                end_ticks: row.get(3)?,
                transcript: row.get(4)?,
                speaker: row.get(5)?,
            };
            
            let media_asset = MediaAssetInfo {
                id: row.get(6)?,
                path: row.get(7)?,
                duration_ticks: row.get(8)?,
                fps_num: row.get(9)?,
                fps_den: row.get(10)?,
                width: row.get(11)?,
                height: row.get(12)?,
            };
            
            Ok((segment, media_asset))
        })?;
        
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        
        Ok(result)
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
}
