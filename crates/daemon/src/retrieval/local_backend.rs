use anyhow::Result;
use std::sync::Arc;

use crate::api::orchestrator::{RetrievalFilters, SegmentCandidate, TimelineContext};
use crate::db::Database;
use crate::embeddings;
use crate::llm;
use crate::retrieval::{RetrievalBackend, RetrievalBackendKind, RetrievalResult};
use engine::timeline::TICKS_PER_SECOND;

pub struct LocalEmbeddingsBackend {
    db: Arc<Database>,
}

impl LocalEmbeddingsBackend {
    pub fn new(db: Arc<Database>) -> Self {
        LocalEmbeddingsBackend { db }
    }
}

#[async_trait::async_trait]
impl RetrievalBackend for LocalEmbeddingsBackend {
    async fn retrieve_candidates(
        &self,
        project_id: i64,
        user_intent: &str,
        filters: Option<&RetrievalFilters>,
        context: Option<&TimelineContext>,
    ) -> Result<RetrievalResult> {
        // Embed user intent using text embedding
        let query_embedding = llm::embed_text(user_intent).await?;
        
        // Oversample: retrieve 200 candidates first, then apply filters and diversity
        // Try to use fusion embeddings first, fallback to text embeddings if fusion not available
        // Search raw segments only (not reference segments for content)
        let mut search_results = embeddings::similarity_search(
            self.db.clone(),
            &query_embedding,
            "fusion",
            "fusion-0.6-0.4",
            200, // Oversample: get top 200 candidates
            Some(project_id),
            true, // raw_segments_only = true
        ).or_else(|_| {
            // Fallback to text embeddings if fusion not available
            embeddings::similarity_search(
                self.db.clone(),
                &query_embedding,
                "text",
                "all-MiniLM-L6-v2",
                200, // Oversample: get top 200 candidates
                Some(project_id),
                true, // raw_segments_only = true
            )
        })?;
        
        // Get segments and apply filters
        let mut candidate_segments = Vec::new();
        for (segment_id, similarity_score) in search_results {
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
                
                candidate_segments.push(SegmentCandidate {
                    segment_id: segment.id,
                    summary_text: segment.summary_text.clone(),
                    capture_time: segment.capture_time.clone(),
                    duration_sec,
                    similarity_score,
                });
            }
        }
        
        // Build debug info
        let debug = serde_json::json!({
            "backend_used": "local_embeddings",
            "tl_index_ready": false,
            "tl_results_count": 0,
            "mapping_stats": {
                "snapped_count": 0,
                "created_count": 0
            },
            "fallback_reason": null
        });
        
        Ok(RetrievalResult {
            candidates: candidate_segments,
            backend_used: RetrievalBackendKind::LocalEmbeddings,
            debug,
            warnings: Vec::new(),
        })
    }
}


