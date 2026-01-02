use anyhow::Result;
use std::sync::Arc;

use crate::api::orchestrator::{RetrievalFilters, SegmentCandidate, TimelineContext};
use crate::db::Database;
use crate::retrieval::{RetrievalBackend, RetrievalBackendKind, RetrievalResult};
use crate::twelvelabs;
use engine::timeline::TICKS_PER_SECOND;

pub struct TwelveLabsBackend {
    db: Arc<Database>,
}

impl TwelveLabsBackend {
    pub fn new(db: Arc<Database>) -> Self {
        TwelveLabsBackend { db }
    }
}

#[async_trait::async_trait]
impl RetrievalBackend for TwelveLabsBackend {
    async fn retrieve_candidates(
        &self,
        project_id: i64,
        user_intent: &str,
        filters: Option<&RetrievalFilters>,
        context: Option<&TimelineContext>,
    ) -> Result<RetrievalResult> {
        // Get project index_id
        let index_id = {
            let index_id: Result<String, rusqlite::Error> = {
                let conn = self.db.conn.lock().unwrap();
                conn.query_row(
                    "SELECT twelvelabs_index_id FROM projects WHERE id = ?1",
                    rusqlite::params![project_id],
                    |row| row.get(0),
                )
            };
            
            match index_id {
                Ok(id) if !id.is_empty() => Some(id),
                _ => None,
            }
        };
        
        let index_id = match index_id {
            Some(id) => id,
            None => {
                // Index not ready - return empty result with debug info
                let debug = serde_json::json!({
                    "backend_used": "twelvelabs",
                    "tl_index_ready": false,
                    "tl_results_count": 0,
                    "mapping_stats": {
                        "snapped_count": 0,
                        "created_count": 0
                    },
                    "fallback_reason": "index_not_ready"
                });
                
                return Ok(RetrievalResult {
                    candidates: Vec::new(),
                    backend_used: RetrievalBackendKind::TwelveLabs,
                    debug,
                    warnings: vec!["TwelveLabs index not ready for this project. Indexing in progress.".to_string()],
                });
            }
        };
        
        // Check if assets are indexed
        let indexed_assets_count: i64 = {
            let conn = self.db.conn.lock().unwrap();
            conn.query_row(
                "SELECT COUNT(*) FROM media_assets WHERE project_id = ?1 AND twelvelabs_indexed_at IS NOT NULL",
                rusqlite::params![project_id],
                |row| row.get(0),
            ).unwrap_or(0)
        };
        
        if indexed_assets_count == 0 {
            // No assets indexed yet
            let debug = serde_json::json!({
                "backend_used": "twelvelabs",
                "tl_index_ready": false,
                "tl_results_count": 0,
                "mapping_stats": {
                    "snapped_count": 0,
                    "created_count": 0
                },
                "fallback_reason": "no_assets_indexed"
            });
            
            return Ok(RetrievalResult {
                candidates: Vec::new(),
                backend_used: RetrievalBackendKind::TwelveLabs,
                debug,
                warnings: vec!["No assets indexed with TwelveLabs yet. Indexing in progress.".to_string()],
            });
        }
        
        // Search TwelveLabs
        let search_results = match twelvelabs::search(&index_id, user_intent, 200).await {
            Ok(results) => results,
            Err(e) => {
                // Search failed - return error (will trigger fallback in retrieval module)
                return Err(anyhow::anyhow!("TwelveLabs search failed: {}", e));
            }
        };
        
        let results_count = search_results.len();
        
        // Map search results to segments
        let mut candidates = Vec::new();
        let mut snapped_count = 0;
        let mut created_count = 0;
        
        for search_result in search_results {
            // Convert seconds to ticks
            let start_ticks = (search_result.start * TICKS_PER_SECOND as f64) as i64;
            let end_ticks = (search_result.end * TICKS_PER_SECOND as f64) as i64;
            
            // Find the asset by video_id
            let asset_id = {
                let asset_id: Result<i64, rusqlite::Error> = {
                    let conn = self.db.conn.lock().unwrap();
                    conn.query_row(
                        "SELECT id FROM media_assets WHERE twelvelabs_video_id = ?1 AND project_id = ?2",
                        rusqlite::params![search_result.video_id, project_id],
                        |row| row.get(0),
                    )
                };
                
                match asset_id {
                    Ok(id) => id,
                    Err(_) => {
                        eprintln!("[TWELVELABS_BACKEND] Video ID {} not found in database", search_result.video_id);
                        continue;
                    }
                }
            };
            
            // Try to snap to existing segment
            let segment_id = {
                let segments = self.db.get_segments_by_asset(asset_id)?;
                
                // Find overlapping segments
                let mut best_overlap = 0i64;
                let mut best_segment_id = None;
                let tl_range = end_ticks - start_ticks;
                let tl_midpoint = start_ticks + tl_range / 2;
                
                for segment in &segments {
                    let seg_start = segment.start_ticks;
                    let seg_end = segment.end_ticks;
                    
                    // Calculate overlap
                    let overlap_start = start_ticks.max(seg_start);
                    let overlap_end = end_ticks.min(seg_end);
                    
                    if overlap_start < overlap_end {
                        let overlap = overlap_end - overlap_start;
                        
                        // Check if midpoint is inside segment
                        let midpoint_inside = tl_midpoint >= seg_start && tl_midpoint <= seg_end;
                        
                        // Snap if: overlap >= 40% of TL range OR midpoint inside segment
                        let overlap_percent = if tl_range > 0 {
                            (overlap as f64 / tl_range as f64) * 100.0
                        } else {
                            100.0
                        };
                        
                        if overlap > best_overlap && (overlap_percent >= 40.0 || midpoint_inside) {
                            best_overlap = overlap;
                            best_segment_id = Some(segment.id);
                        }
                    }
                }
                
                if let Some(seg_id) = best_segment_id {
                    snapped_count += 1;
                    seg_id
                } else {
                    // Create dynamic segment
                    let dedupe_key = format!("{}:{}:{}:twelvelabs", asset_id, start_ticks, end_ticks);
                    let external_ref = format!("{}:{}:{}", search_result.video_id, search_result.start, search_result.end);
                    
                    let seg_id = self.db.get_or_create_dynamic_segment(
                        asset_id,
                        project_id,
                        start_ticks,
                        end_ticks,
                        &dedupe_key,
                        "twelvelabs",
                        &external_ref,
                    )?;
                    
                    created_count += 1;
                    seg_id
                }
            };
            
            // Get segment info
            let segment_opt = self.db.get_segment_with_embeddings(segment_id)?;
            
            if let Some((segment, _embeddings)) = segment_opt {
                // Apply filters
                if let Some(ref filters) = filters {
                    if let Some(ref kind) = filters.segment_kind {
                        if segment.segment_kind.as_ref() != Some(kind) {
                            continue;
                        }
                    }
                    // Additional filters can be applied here
                }
                
                let duration_sec = {
                    let start = Database::get_coalesced_src_in(&segment);
                    let end = Database::get_coalesced_src_out(&segment);
                    (end - start) as f64 / TICKS_PER_SECOND as f64
                };
                
                candidates.push(SegmentCandidate {
                    segment_id: segment.id,
                    summary_text: segment.summary_text.clone(),
                    capture_time: segment.capture_time.clone(),
                    duration_sec,
                    similarity_score: search_result.score as f32,
                });
            }
        }
        
        // Build debug info
        let debug = serde_json::json!({
            "backend_used": "twelvelabs",
            "tl_index_ready": true,
            "tl_results_count": results_count,
            "mapping_stats": {
                "snapped_count": snapped_count,
                "created_count": created_count
            },
            "fallback_reason": null
        });
        
        Ok(RetrievalResult {
            candidates,
            backend_used: RetrievalBackendKind::TwelveLabs,
            debug,
            warnings: Vec::new(),
        })
    }
}

