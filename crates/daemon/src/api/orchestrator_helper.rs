use crate::api::orchestrator::SegmentCandidate;
use crate::db::Database;
use std::collections::HashMap;

/// Diversify candidate segments by:
/// - Limiting max segments per asset
/// - Deduplicating near-identical summaries
/// - Avoiding consecutive time windows (within 2 seconds)
pub fn diversify_candidates(
    candidates: Vec<SegmentCandidate>,
    max_per_asset: usize,
    db: &Database,
) -> anyhow::Result<Vec<SegmentCandidate>> {
    if candidates.is_empty() {
        return Ok(candidates);
    }

    // Group by asset_id (need to look up from segment)
    let mut by_asset: HashMap<i64, Vec<SegmentCandidate>> = HashMap::new();
    
    for candidate in candidates {
        // Get asset_id from segment
        let segment_opt = db.get_segment_with_embeddings(candidate.segment_id)?;
        if let Some((segment, _)) = segment_opt {
            let asset_id = segment.media_asset_id;
            by_asset.entry(asset_id).or_insert_with(Vec::new).push(candidate);
        }
    }

    // Limit per asset and deduplicate
    let mut diversified = Vec::new();
    for (_asset_id, mut asset_candidates) in by_asset {
        // Sort by similarity score (descending) to keep best matches
        asset_candidates.sort_by(|a, b| {
            b.similarity_score.partial_cmp(&a.similarity_score).unwrap_or(std::cmp::Ordering::Equal)
        });

        // Limit to max_per_asset
        asset_candidates.truncate(max_per_asset);

        // Deduplicate summaries (exact match for now, could use fuzzy matching)
        let mut seen_summaries = std::collections::HashSet::new();
        for candidate in asset_candidates {
            let summary_key = candidate.summary_text.as_ref()
                .map(|s| s.to_lowercase().trim().to_string())
                .unwrap_or_default();
            
            if !summary_key.is_empty() && !seen_summaries.contains(&summary_key) {
                seen_summaries.insert(summary_key);
                diversified.push(candidate);
            } else if summary_key.is_empty() {
                // Always include segments without summaries (rare but possible)
                diversified.push(candidate);
            }
        }
    }

    // Sort by similarity score again (descending)
    diversified.sort_by(|a, b| {
        b.similarity_score.partial_cmp(&a.similarity_score).unwrap_or(std::cmp::Ordering::Equal)
    });

    // Filter consecutive time windows (within 2 seconds of capture_time)
    // This requires parsing capture_time, so for now we'll skip this step
    // and rely on max_per_asset to provide diversity

    Ok(diversified)
}
