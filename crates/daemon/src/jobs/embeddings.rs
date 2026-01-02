use anyhow::Result;
use reqwest;
use rusqlite::params;
use serde_json;
use std::sync::Arc;

use crate::db::Database;
use crate::jobs::JobManager;

const ML_SERVICE_URL: &str = "http://127.0.0.1:8001";
const TICKS_PER_SECOND: i64 = 48000;

/// Convert ticks to seconds
fn ticks_to_seconds(ticks: i64) -> f64 {
    ticks as f64 / TICKS_PER_SECOND as f64
}

/// Compute fusion embedding by combining text and vision embeddings with weighted combination
/// fusion = normalize(Wt * text_emb + Wv * vision_emb)
/// Default weights: Wt=0.6, Wv=0.4
fn compute_fusion_embedding(
    text_emb: &[f32],
    vision_emb: &[f32],
    weight_text: f32,
    weight_vision: f32,
) -> Vec<f32> {
    // Normalize text embedding
    let text_norm: f32 = text_emb.iter().map(|x| x * x).sum::<f32>().sqrt();
    let text_emb_norm: Vec<f32> = if text_norm > 0.0 {
        text_emb.iter().map(|x| x / text_norm).collect()
    } else {
        text_emb.to_vec()
    };
    
    // Normalize vision embedding
    let vision_norm: f32 = vision_emb.iter().map(|x| x * x).sum::<f32>().sqrt();
    let vision_emb_norm: Vec<f32> = if vision_norm > 0.0 {
        vision_emb.iter().map(|x| x / vision_norm).collect()
    } else {
        vision_emb.to_vec()
    };
    
    // Handle dimension mismatch: use the smaller dimension or pad/truncate
    let min_dim = text_emb_norm.len().min(vision_emb_norm.len());
    let text_trimmed: Vec<f32> = text_emb_norm.iter().take(min_dim).copied().collect();
    let vision_trimmed: Vec<f32> = vision_emb_norm.iter().take(min_dim).copied().collect();
    
    // Weighted combination
    let fusion: Vec<f32> = text_trimmed.iter()
        .zip(vision_trimmed.iter())
        .map(|(t, v)| weight_text * t + weight_vision * v)
        .collect();
    
    // Renormalize result
    let fusion_norm: f32 = fusion.iter().map(|x| x * x).sum::<f32>().sqrt();
    if fusion_norm > 0.0 {
        fusion.iter().map(|x| x / fusion_norm).collect()
    } else {
        fusion
    }
}

/// Construct structured text for embedding from segment metadata
fn construct_semantic_text(segment: &crate::db::Segment) -> String {
    let mut parts = Vec::new();
    
    // Format as structured text: spoken, summary, keywords
    if let Some(ref transcript) = segment.transcript {
        // Use full transcript (not truncated)
        parts.push(format!("spoken: {}", transcript));
    }
    
    if let Some(ref summary) = segment.summary_text {
        parts.push(format!("summary: {}", summary));
    }
    
    if let Some(ref keywords_json) = segment.keywords_json {
        if let Ok(keywords) = serde_json::from_str::<serde_json::Value>(keywords_json) {
            if let Some(kw_array) = keywords.get("keywords").and_then(|k| k.as_array()) {
                let kw_str: Vec<String> = kw_array.iter()
                    .filter_map(|k| k.as_str().map(|s| s.to_string()))
                    .collect();
                if !kw_str.is_empty() {
                    parts.push(format!("keywords: {}", kw_str.join(", ")));
                }
            }
        }
    }
    
    if parts.is_empty() {
        "video segment".to_string()
    } else {
        parts.join("\n")
    }
}

/// Process EmbedSegments job - generates text, vision, and fusion embeddings (idempotent)
pub async fn process_embed_segments(
    db: Arc<Database>,
    job_manager: Arc<JobManager>,
    job_id: i64,
    asset_id: i64,
) -> Result<()> {
    eprintln!("[EMBEDDING] Starting EmbedSegments job {} for asset_id: {}", job_id, asset_id);
    
    // Get media asset path for vision embeddings
    let media_path = db.get_media_asset_path(asset_id)?
        .ok_or_else(|| anyhow::anyhow!("Media asset {} not found", asset_id))?;
    
    // Get all segments for this asset
    let segments = db.get_segments_by_asset(asset_id)?;
    eprintln!("[EMBEDDING] Found {} segments for asset_id: {}", segments.len(), asset_id);
    
    let client = reqwest::Client::new();
    let mut processed_count = 0;
    
    for segment in &segments {
        // Get segment time boundaries (using coalesced helpers)
        let src_in = Database::get_coalesced_src_in(segment);
        let src_out = Database::get_coalesced_src_out(segment);
        let start_time = ticks_to_seconds(src_in);
        let end_time = ticks_to_seconds(src_out);
        
        // 1. Generate text embedding
        let has_text_emb: bool = {
            let conn = db.conn.lock().unwrap();
            let result = conn.query_row(
                "SELECT COUNT(*) > 0 FROM embeddings WHERE segment_id = ?1 AND embedding_type = 'text' AND model_name = 'all-MiniLM-L6-v2'",
                params![segment.id],
                |row| row.get(0),
            ).unwrap_or(false);
            result
        };
        
        if !has_text_emb {
            let semantic_text = construct_semantic_text(segment);
            
            // Call ML service /embeddings/text endpoint
            let response = client
                .post(&format!("{}/embeddings/text", ML_SERVICE_URL))
                .json(&serde_json::json!({
                    "text": semantic_text
                }))
                .send()
                .await?;
            
            if response.status().is_success() {
                let embedding_response: serde_json::Value = response.json().await?;
                if let Some(embedding_vec) = embedding_response.get("embedding")
                    .and_then(|e| e.as_array())
                {
                    // Convert to bytes for storage (384 dimensions)
                    let embedding: Vec<f32> = embedding_vec.iter()
                        .filter_map(|v| v.as_f64().map(|f| f as f32))
                        .collect();
                    
                    eprintln!("[EMBEDDING] Segment {}: Generated text embedding ({} dims)", segment.id, embedding.len());
                    
                    let embedding_bytes: Vec<u8> = embedding.iter()
                        .flat_map(|f| f.to_le_bytes().to_vec())
                        .collect();
                    
                    // Store in database
                    {
                        let conn = db.conn.lock().unwrap();
                        // Check if embedding already exists
                        let exists: bool = conn.query_row(
                            "SELECT COUNT(*) > 0 FROM embeddings WHERE segment_id = ?1 AND embedding_type = 'text' AND model_name = 'all-MiniLM-L6-v2'",
                            params![segment.id],
                            |row| row.get(0),
                        ).unwrap_or(false);
                        
                        if exists {
                            eprintln!("[EMBEDDING] Text embedding for segment {} already exists, skipping insert", segment.id);
                        } else {
                            let result = conn.execute(
                                "INSERT INTO embeddings (segment_id, embedding_type, model_name, model_version, vector_blob) VALUES (?1, ?2, ?3, ?4, ?5)",
                                params![segment.id, "text", "all-MiniLM-L6-v2", "1", embedding_bytes],
                            );
                            match result {
                                Ok(rows_affected) => {
                                    eprintln!("[EMBEDDING] Successfully stored text embedding for segment {} ({} rows affected)", segment.id, rows_affected);
                                }
                                Err(e) => {
                                    eprintln!("[EMBEDDING] Error storing text embedding for segment {}: {:?}", segment.id, e);
                                }
                            }
                        }
                    }
                }
            }
        }
        
        // 2. Generate vision embedding
        let has_vision_emb: bool = {
            let conn = db.conn.lock().unwrap();
            let result = conn.query_row(
                "SELECT COUNT(*) > 0 FROM embeddings WHERE segment_id = ?1 AND embedding_type = 'vision' AND model_name = 'clip-vit-b-32'",
                params![segment.id],
                |row| row.get(0),
            ).unwrap_or(false);
            result
        };
        
        if !has_vision_emb {
            // Call ML service /embeddings/vision endpoint
            let response = client
                .post(&format!("{}/embeddings/vision", ML_SERVICE_URL))
                .json(&serde_json::json!({
                    "media_path": media_path,
                    "start_time": start_time,
                    "end_time": end_time
                }))
                .send()
                .await?;
            
            if response.status().is_success() {
                let embedding_response: serde_json::Value = response.json().await?;
                if let Some(embedding_vec) = embedding_response.get("embedding")
                    .and_then(|e| e.as_array())
                {
                    // Convert to bytes for storage (512 dimensions)
                    let embedding: Vec<f32> = embedding_vec.iter()
                        .filter_map(|v| v.as_f64().map(|f| f as f32))
                        .collect();
                    
                    eprintln!("[EMBEDDING] Segment {}: Generated vision embedding ({} dims)", segment.id, embedding.len());
                    
                    let embedding_bytes: Vec<u8> = embedding.iter()
                        .flat_map(|f| f.to_le_bytes().to_vec())
                        .collect();
                    
                    // Store in database
                    {
                        let conn = db.conn.lock().unwrap();
                        // Check if embedding already exists
                        let exists: bool = conn.query_row(
                            "SELECT COUNT(*) > 0 FROM embeddings WHERE segment_id = ?1 AND embedding_type = 'vision' AND model_name = 'clip-vit-b-32'",
                            params![segment.id],
                            |row| row.get(0),
                        ).unwrap_or(false);
                        
                        if exists {
                            eprintln!("[EMBEDDING] Vision embedding for segment {} already exists, skipping insert", segment.id);
                        } else {
                            let result = conn.execute(
                                "INSERT INTO embeddings (segment_id, embedding_type, model_name, model_version, vector_blob) VALUES (?1, ?2, ?3, ?4, ?5)",
                                params![segment.id, "vision", "clip-vit-b-32", "1", embedding_bytes],
                            );
                            match result {
                                Ok(rows_affected) => {
                                    eprintln!("[EMBEDDING] Successfully stored vision embedding for segment {} ({} rows affected)", segment.id, rows_affected);
                                }
                                Err(e) => {
                                    eprintln!("[EMBEDDING] Error storing vision embedding for segment {}: {:?}", segment.id, e);
                                }
                            }
                        }
                    }
                }
            }
        }
        
        // 3. Generate fusion embedding (requires both text and vision)
        // Note: We need to retrieve embeddings after they're stored, so use a fresh connection
        let has_fusion_emb: bool = {
            let conn = db.conn.lock().unwrap();
            let result = conn.query_row(
                "SELECT COUNT(*) > 0 FROM embeddings WHERE segment_id = ?1 AND embedding_type = 'fusion' AND model_name = 'fusion-0.6-0.4'",
                params![segment.id],
                |row| row.get(0),
            ).unwrap_or(false);
            result
        };
        
        if !has_fusion_emb {
            // Retrieve text and vision embeddings (use fresh connection to ensure we see the just-stored embeddings)
            let (text_emb, vision_emb) = {
                let conn = db.conn.lock().unwrap();
                
                // Get text embedding
                let text_emb_blob: Option<Vec<u8>> = conn.query_row(
                    "SELECT vector_blob FROM embeddings WHERE segment_id = ?1 AND embedding_type = 'text' AND model_name = 'all-MiniLM-L6-v2'",
                    params![segment.id],
                    |row| row.get(0),
                ).ok();
                
                // Get vision embedding
                let vision_emb_blob: Option<Vec<u8>> = conn.query_row(
                    "SELECT vector_blob FROM embeddings WHERE segment_id = ?1 AND embedding_type = 'vision' AND model_name = 'clip-vit-b-32'",
                    params![segment.id],
                    |row| row.get(0),
                ).ok();
                
                eprintln!("[EMBEDDING] Segment {}: Retrieving embeddings for fusion - text: {}, vision: {}", 
                    segment.id, text_emb_blob.is_some(), vision_emb_blob.is_some());
                
                // Convert blobs back to f32 vectors
                let text_emb = text_emb_blob.map(|blob| {
                    blob.chunks(4)
                        .map(|chunk| {
                            let bytes: [u8; 4] = [chunk[0], chunk[1], chunk[2], chunk[3]];
                            f32::from_le_bytes(bytes)
                        })
                        .collect::<Vec<f32>>()
                });
                
                let vision_emb = vision_emb_blob.map(|blob| {
                    blob.chunks(4)
                        .map(|chunk| {
                            let bytes: [u8; 4] = [chunk[0], chunk[1], chunk[2], chunk[3]];
                            f32::from_le_bytes(bytes)
                        })
                        .collect::<Vec<f32>>()
                });
                
                (text_emb, vision_emb)
            };
            
            // Compute fusion if both embeddings exist
            if let (Some(text_vec), Some(vision_vec)) = (text_emb, vision_emb) {
                let fusion_vec = compute_fusion_embedding(&text_vec, &vision_vec, 0.6, 0.4);
                
                eprintln!("[EMBEDDING] Segment {}: Generated fusion embedding ({} dims)", segment.id, fusion_vec.len());
                
                let embedding_bytes: Vec<u8> = fusion_vec.iter()
                    .flat_map(|f| f.to_le_bytes().to_vec())
                    .collect();
                
                // Store in database
                {
                    let conn = db.conn.lock().unwrap();
                    let result = conn.execute(
                        "INSERT INTO embeddings (segment_id, embedding_type, model_name, model_version, vector_blob) VALUES (?1, ?2, ?3, ?4, ?5)",
                        params![segment.id, "fusion", "fusion-0.6-0.4", "1", embedding_bytes],
                    );
                    match result {
                        Ok(rows_affected) => {
                            eprintln!("[EMBEDDING] Successfully stored fusion embedding for segment {} ({} rows affected)", segment.id, rows_affected);
                        }
                        Err(e) => {
                            eprintln!("[EMBEDDING] Error storing fusion embedding for segment {}: {:?}", segment.id, e);
                        }
                    }
                }
            } else {
                eprintln!("[EMBEDDING] Segment {}: Skipping fusion embedding (missing text or vision embedding)", segment.id);
            }
        }
        
        processed_count += 1;
        
        // Update progress
        let progress = processed_count as f64 / segments.len() as f64;
        job_manager.update_job_status(job_id, crate::jobs::JobStatus::Running, Some(progress))?;
    }
    
    // Update asset analysis state
    db.update_asset_analysis_state(asset_id, "embeddings_ready_at", None)?;
    
    // Get project_id from asset to emit AnalysisComplete event
    let project_id = {
        let conn = db.conn.lock().unwrap();
        conn.query_row(
            "SELECT project_id FROM media_assets WHERE id = ?1",
            params![asset_id],
            |row| row.get::<_, i64>(0),
        ).unwrap_or(0)
    };
    
    if project_id > 0 {
        // Emit AnalysisComplete event for orchestrator
        job_manager.emit_analysis_complete(asset_id, project_id, "Embedded".to_string());
    }
    
    eprintln!("[EMBEDDING] Completed EmbedSegments job {} for asset_id: {} (processed {} segments)", job_id, asset_id, processed_count);
    job_manager.update_job_status(job_id, crate::jobs::JobStatus::Completed, Some(1.0))?;
    
    Ok(())
}
