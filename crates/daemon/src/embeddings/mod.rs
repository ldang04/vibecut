use anyhow::Result;
use rusqlite::params;
use std::sync::Arc;

use crate::db::Database;

/// Perform similarity search using cosine similarity
/// Supports multiple embedding types (text, vision, fusion) and filters by raw vs reference segments
pub fn similarity_search(
    db: Arc<Database>,
    query_embedding: &[f32],
    embedding_type: &str, // 'text', 'vision', or 'fusion'
    model_name: &str,
    limit: usize,
    project_id: Option<i64>,
    raw_segments_only: bool, // If true, only search raw segments (not references)
) -> Result<Vec<(i64, f32)>> {
    // Build query with optional filtering
    let query = if raw_segments_only {
        // Only search segments from non-reference assets
        "SELECT e.segment_id, e.vector_blob 
         FROM embeddings e
         JOIN segments s ON e.segment_id = s.id
         JOIN media_assets m ON s.media_asset_id = m.id
         WHERE e.embedding_type = ?1 AND e.model_name = ?2
           AND (m.is_reference IS NULL OR m.is_reference = 0)
           AND (?3 IS NULL OR s.project_id = ?3)"
    } else {
        // Search all segments (raw + reference)
        "SELECT e.segment_id, e.vector_blob 
         FROM embeddings e
         JOIN segments s ON e.segment_id = s.id
         WHERE e.embedding_type = ?1 AND e.model_name = ?2
           AND (?3 IS NULL OR s.project_id = ?3)"
    };
    
    // Load all embeddings of the specified type
    let conn = db.conn.lock().unwrap();
    let mut stmt = conn.prepare(query)?;
    
    let rows: Vec<_> = stmt.query_map(params![embedding_type, model_name, project_id], |row| {
        let segment_id: i64 = row.get(0)?;
        let vector_blob: Vec<u8> = row.get(1)?;
        Ok((segment_id, vector_blob))
    })?.collect::<Result<Vec<_>, _>>()?;
    drop(stmt);
    drop(conn);
    
    let mut results = Vec::new();
    for (segment_id, vector_blob) in rows {
        // Deserialize embedding vector (assuming f32 array stored as bytes)
        let embedding: Vec<f32> = vector_blob.chunks(4)
            .map(|chunk| {
                let bytes: [u8; 4] = [chunk[0], chunk[1], chunk[2], chunk[3]];
                f32::from_le_bytes(bytes)
            })
            .collect();
        
        // Handle dimension mismatch gracefully
        let min_dim = query_embedding.len().min(embedding.len());
        if min_dim == 0 {
            continue;
        }
        
        let query_trimmed: Vec<f32> = query_embedding.iter().take(min_dim).copied().collect();
        let emb_trimmed: Vec<f32> = embedding.iter().take(min_dim).copied().collect();
        
        // Compute cosine similarity
        let similarity = cosine_similarity(&query_trimmed, &emb_trimmed);
        results.push((segment_id, similarity));
    }
    
    // Sort by similarity (descending) and take top N
    results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    results.truncate(limit);
    
    Ok(results)
}

/// Search only reference segments (for style matching)
pub fn similarity_search_references(
    db: Arc<Database>,
    query_embedding: &[f32],
    embedding_type: &str,
    model_name: &str,
    limit: usize,
    project_id: Option<i64>,
) -> Result<Vec<(i64, f32)>> {
    let query = "SELECT e.segment_id, e.vector_blob 
                 FROM embeddings e
                 JOIN segments s ON e.segment_id = s.id
                 JOIN media_assets m ON s.media_asset_id = m.id
                 WHERE e.embedding_type = ?1 AND e.model_name = ?2
                   AND m.is_reference = 1
                   AND (?3 IS NULL OR s.project_id = ?3)";
    
    let conn = db.conn.lock().unwrap();
    let mut stmt = conn.prepare(query)?;
    
    let rows: Vec<_> = stmt.query_map(params![embedding_type, model_name, project_id], |row| {
        let segment_id: i64 = row.get(0)?;
        let vector_blob: Vec<u8> = row.get(1)?;
        Ok((segment_id, vector_blob))
    })?.collect::<Result<Vec<_>, _>>()?;
    drop(stmt);
    drop(conn);
    
    let mut results = Vec::new();
    for (segment_id, vector_blob) in rows {
        let embedding: Vec<f32> = vector_blob.chunks(4)
            .map(|chunk| {
                let bytes: [u8; 4] = [chunk[0], chunk[1], chunk[2], chunk[3]];
                f32::from_le_bytes(bytes)
            })
            .collect();
        
        let min_dim = query_embedding.len().min(embedding.len());
        if min_dim == 0 {
            continue;
        }
        
        let query_trimmed: Vec<f32> = query_embedding.iter().take(min_dim).copied().collect();
        let emb_trimmed: Vec<f32> = embedding.iter().take(min_dim).copied().collect();
        
        let similarity = cosine_similarity(&query_trimmed, &emb_trimmed);
        results.push((segment_id, similarity));
    }
    
    results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    results.truncate(limit);
    
    Ok(results)
}

/// Compute cosine similarity between two vectors
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }
    
    let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    
    dot_product / (norm_a * norm_b)
}
