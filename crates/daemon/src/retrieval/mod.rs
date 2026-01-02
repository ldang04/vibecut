use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::api::orchestrator::{RetrievalFilters, SegmentCandidate, TimelineContext};
use crate::db::Database;

/// Backend kind identifier
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RetrievalBackendKind {
    #[serde(rename = "twelvelabs")]
    TwelveLabs,
    #[serde(rename = "local_embeddings")]
    LocalEmbeddings,
}

impl RetrievalBackendKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            RetrievalBackendKind::TwelveLabs => "twelvelabs",
            RetrievalBackendKind::LocalEmbeddings => "local_embeddings",
        }
    }
}

/// Result from retrieval backend
#[derive(Debug, Clone, Serialize)]
pub struct RetrievalResult {
    pub candidates: Vec<SegmentCandidate>,
    pub backend_used: RetrievalBackendKind,
    pub debug: serde_json::Value,
    pub warnings: Vec<String>,
}

/// Trait for retrieval backends
#[async_trait::async_trait]
pub trait RetrievalBackend: Send + Sync {
    async fn retrieve_candidates(
        &self,
        project_id: i64,
        user_intent: &str,
        filters: Option<&RetrievalFilters>,
        context: Option<&TimelineContext>,
    ) -> Result<RetrievalResult>;
}

/// Main retrieval function that selects backend and retrieves candidates
pub async fn retrieve_candidates(
    db: Arc<Database>,
    project_id: i64,
    user_intent: &str,
    filters: Option<&RetrievalFilters>,
    context: Option<&TimelineContext>,
) -> Result<RetrievalResult> {
    // Read backend selection from environment
    let backend_str = std::env::var("RETRIEVAL_BACKEND")
        .unwrap_or_else(|_| "twelvelabs_then_local".to_string());
    
    match backend_str.as_str() {
        "twelvelabs" => {
            // Try TwelveLabs only
            match crate::retrieval::twelvelabs_backend::TwelveLabsBackend::new(db.clone()).retrieve_candidates(
                project_id,
                user_intent,
                filters,
                context,
            ).await {
                Ok(result) => Ok(result),
                Err(e) => {
                    // If TwelveLabs fails, return error (no fallback)
                    Err(e)
                }
            }
        }
        "local" => {
            // Use local embeddings only
            crate::retrieval::local_backend::LocalEmbeddingsBackend::new(db).retrieve_candidates(
                project_id,
                user_intent,
                filters,
                context,
            ).await
        }
        "twelvelabs_then_local" | _ => {
            // Try TwelveLabs first, fallback to local
            match crate::retrieval::twelvelabs_backend::TwelveLabsBackend::new(db.clone()).retrieve_candidates(
                project_id,
                user_intent,
                filters,
                context,
            ).await {
                Ok(result) => Ok(result),
                Err(e) => {
                    // Fallback to local embeddings
                    eprintln!("[RETRIEVAL] TwelveLabs failed, falling back to local embeddings: {:?}", e);
                    let mut local_result = crate::retrieval::local_backend::LocalEmbeddingsBackend::new(db).retrieve_candidates(
                        project_id,
                        user_intent,
                        filters,
                        context,
                    ).await?;
                    
                    // Update debug to indicate fallback
                    if let Some(debug_obj) = local_result.debug.as_object_mut() {
                        debug_obj.insert("fallback_reason".to_string(), serde_json::json!(e.to_string()));
                    }
                    local_result.warnings.push(format!("TwelveLabs unavailable, using local embeddings: {}", e));
                    
                    Ok(local_result)
                }
            }
        }
    }
}

pub mod local_backend;
pub mod twelvelabs_backend;

