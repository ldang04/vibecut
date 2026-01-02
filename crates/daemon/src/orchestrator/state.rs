use anyhow::Result;
use rusqlite::params;
use std::sync::Arc;

use crate::db::Database;
use crate::jobs::JobType;

#[derive(Debug, Clone, PartialEq)]
pub enum AssetReadiness {
    Imported,      // Asset exists, no segments
    Segmented,     // segments_built_at IS NOT NULL
    Enriched,      // transcript_ready_at IS NOT NULL AND vision_ready_at IS NOT NULL
    MetadataReady, // metadata_ready_at IS NOT NULL (after ComputeSegmentMetadata)
    Embedded,      // embeddings_ready_at IS NOT NULL
    IndexedExternal, // twelvelabs_indexed_at IS NOT NULL
}

#[derive(Debug, Clone)]
pub struct AssetState {
    pub asset_id: i64,
    pub readiness: AssetReadiness,
    pub missing_steps: Vec<JobType>, // What's needed to reach next level
    pub active_job_ids: Vec<i64>,    // Jobs currently running for this asset
}

#[derive(Debug, Clone)]
pub struct SegmentSanity {
    pub count: usize,
    pub have_src_bounds: usize, // src_in/out populated
    pub have_transcript: usize,  // transcript populated (if needed)
    pub have_vision: usize,      // scene_json populated (if needed)
}

#[derive(Debug, Clone)]
pub struct ProjectState {
    pub media_assets_count: usize,
    pub segments_count: usize,
    pub asset_states: Vec<AssetState>,
    pub analysis_coverage: f32, // Assets with embeddings / total assets
    pub jobs_running_count: usize,
    pub segments_sanity: SegmentSanity, // Separate check
}

/// Derive AssetReadiness from media_assets timestamp columns
pub fn get_asset_readiness(db: &Database, asset_id: i64) -> Result<AssetReadiness> {
    let conn = db.conn.lock().unwrap();
    
    // Check timestamps in order of readiness levels
    // First check if indexed externally (TwelveLabs)
    let indexed_external: bool = conn.query_row(
        "SELECT twelvelabs_indexed_at IS NOT NULL FROM media_assets WHERE id = ?1",
        params![asset_id],
        |row| row.get(0),
    ).unwrap_or(false);
    
    if indexed_external {
        return Ok(AssetReadiness::IndexedExternal);
    }
    
    let embeddings_ready: bool = conn.query_row(
        "SELECT embeddings_ready_at IS NOT NULL FROM media_assets WHERE id = ?1",
        params![asset_id],
        |row| row.get(0),
    ).unwrap_or(false);
    
    if embeddings_ready {
        return Ok(AssetReadiness::Embedded);
    }
    
    let metadata_ready: bool = conn.query_row(
        "SELECT metadata_ready_at IS NOT NULL FROM media_assets WHERE id = ?1",
        params![asset_id],
        |row| row.get(0),
    ).unwrap_or(false);
    
    if metadata_ready {
        return Ok(AssetReadiness::MetadataReady);
    }
    
    let transcript_ready: bool = conn.query_row(
        "SELECT transcript_ready_at IS NOT NULL FROM media_assets WHERE id = ?1",
        params![asset_id],
        |row| row.get(0),
    ).unwrap_or(false);
    
    let vision_ready: bool = conn.query_row(
        "SELECT vision_ready_at IS NOT NULL FROM media_assets WHERE id = ?1",
        params![asset_id],
        |row| row.get(0),
    ).unwrap_or(false);
    
    if transcript_ready && vision_ready {
        return Ok(AssetReadiness::Enriched);
    }
    
    let segments_built: bool = conn.query_row(
        "SELECT segments_built_at IS NOT NULL FROM media_assets WHERE id = ?1",
        params![asset_id],
        |row| row.get(0),
    ).unwrap_or(false);
    
    if segments_built {
        return Ok(AssetReadiness::Segmented);
    }
    
    Ok(AssetReadiness::Imported)
}

/// Get asset states for all raw assets in a project
pub fn get_asset_states(db: &Database, project_id: i64) -> Result<Vec<AssetState>> {
    let conn = db.conn.lock().unwrap();
    
    // Get all raw (non-reference) assets for this project
    let asset_ids: Vec<i64> = {
        let mut stmt = conn.prepare(
            "SELECT id FROM media_assets WHERE project_id = ?1 AND (is_reference IS NULL OR is_reference = 0)"
        )?;
        let rows = stmt.query_map(params![project_id], |row| row.get(0))?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    
    let mut asset_states = Vec::new();
    
    for asset_id in asset_ids {
        let readiness = get_asset_readiness(db, asset_id)?;
        
        // Compute missing steps (simplified for now - will be expanded in ensure.rs)
        let missing_steps = Vec::new(); // TODO: compute based on readiness
        let active_job_ids = Vec::new(); // TODO: query active jobs for this asset
        
        asset_states.push(AssetState {
            asset_id,
            readiness,
            missing_steps,
            active_job_ids,
        });
    }
    
    Ok(asset_states)
}

/// Get segment sanity checks
pub fn get_segment_sanity(db: &Database, project_id: i64) -> Result<SegmentSanity> {
    let conn = db.conn.lock().unwrap();
    
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM segments WHERE project_id = ?1",
        params![project_id],
        |row| row.get(0),
    ).unwrap_or(0);
    
    let have_src_bounds: i64 = conn.query_row(
        "SELECT COUNT(*) FROM segments WHERE project_id = ?1 AND src_in_ticks IS NOT NULL AND src_out_ticks IS NOT NULL",
        params![project_id],
        |row| row.get(0),
    ).unwrap_or(0);
    
    let have_transcript: i64 = conn.query_row(
        "SELECT COUNT(*) FROM segments WHERE project_id = ?1 AND transcript IS NOT NULL",
        params![project_id],
        |row| row.get(0),
    ).unwrap_or(0);
    
    let have_vision: i64 = conn.query_row(
        "SELECT COUNT(*) FROM segments WHERE project_id = ?1 AND scene_json IS NOT NULL",
        params![project_id],
        |row| row.get(0),
    ).unwrap_or(0);
    
    Ok(SegmentSanity {
        count: count as usize,
        have_src_bounds: have_src_bounds as usize,
        have_transcript: have_transcript as usize,
        have_vision: have_vision as usize,
    })
}

/// Get comprehensive project state
pub fn get_project_state(db: &Database, project_id: i64) -> Result<ProjectState> {
    let conn = db.conn.lock().unwrap();
    
    // Count media assets (raw only)
    let media_assets_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM media_assets WHERE project_id = ?1 AND (is_reference IS NULL OR is_reference = 0)",
        params![project_id],
        |row| row.get(0),
    ).unwrap_or(0);
    
    // Count segments
    let segments_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM segments WHERE project_id = ?1",
        params![project_id],
        |row| row.get(0),
    ).unwrap_or(0);
    
    // Get asset states
    let asset_states = get_asset_states(db, project_id)?;
    
    // Compute analysis coverage (assets with embeddings / total assets)
    let assets_with_embeddings: i64 = conn.query_row(
        "SELECT COUNT(*) FROM media_assets WHERE project_id = ?1 AND (is_reference IS NULL OR is_reference = 0) AND embeddings_ready_at IS NOT NULL",
        params![project_id],
        |row| row.get(0),
    ).unwrap_or(0);
    
    let analysis_coverage = if media_assets_count > 0 {
        assets_with_embeddings as f32 / media_assets_count as f32
    } else {
        0.0
    };
    
    // Count running jobs (simplified - will be improved in ensure.rs)
    let jobs_running_count = 0; // TODO: query active jobs for this project
    
    // Get segment sanity
    let segments_sanity = get_segment_sanity(db, project_id)?;
    
    Ok(ProjectState {
        media_assets_count: media_assets_count as usize,
        segments_count: segments_count as usize,
        asset_states,
        analysis_coverage,
        jobs_running_count,
        segments_sanity,
    })
}

