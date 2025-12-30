use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
    routing::post,
    Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::db::Database;
use crate::embeddings;
use crate::jobs::{JobManager, JobStatus};
use crate::llm;
use serde_json;
use rusqlite::params;

// Project state for precondition checking
struct ProjectState {
    media_assets_count: usize,
    segments_count: usize,
    segments_with_text_embeddings: usize,
    segments_with_vision_embeddings: usize,
    embedding_coverage: f32,
    jobs_running_count: usize,
    jobs_failed_count: usize,
}

// Agent mode enum
#[derive(Debug)]
enum AgentMode {
    TalkConfirm,    // Destructive action needs confirmation
    TalkImport,     // No media assets
    TalkAnalyze,    // No segments
    TalkClarify,    // Ambiguous intent
    Busy,           // Jobs running or coverage incomplete
    Act,            // Ready to execute
}

#[derive(Deserialize)]
pub struct ProposeRequest {
    pub user_intent: String,
    pub filters: Option<RetrievalFilters>,
    pub context: Option<TimelineContext>,
}

#[derive(Deserialize)]
pub struct RetrievalFilters {
    pub capture_time_range: Option<(String, String)>,
    pub quality_threshold: Option<f64>,
    pub unused_only: Option<bool>,
    pub segment_kind: Option<String>,
}

#[derive(Deserialize, Serialize)]
pub struct TimelineContext {
    pub current_clips: Vec<ClipInfo>,
    pub selected_range: Option<TimeRange>,
    pub user_selected_clips: Option<Vec<i64>>,
}

#[derive(Deserialize, Serialize)]
pub struct ClipInfo {
    pub segment_id: i64,
    pub timeline_start_ticks: i64,
}

#[derive(Deserialize, Serialize)]
pub struct TimeRange {
    pub start_ticks: i64,
    pub end_ticks: i64,
}

// Uniform response contract
#[derive(Serialize)]
pub struct AgentResponse<T> {
    pub mode: String,            // "talk" | "busy" | "act"
    pub message: String,         // Friendly assistant copy
    pub suggestions: Vec<String>, // Quick replies/buttons
    pub questions: Vec<String>,  // Optional prompts
    pub data: Option<T>,         // Propose candidates / plan / etc
    pub debug: Option<serde_json::Value>, // Optional in dev
}

#[derive(Serialize)]
pub struct ProposeData {
    pub candidate_segments: Vec<SegmentCandidate>,
    pub narrative_structure: Option<String>,
}

#[derive(Serialize)]
pub struct PlanData {
    pub edit_plan: serde_json::Value,
}

#[derive(Serialize)]
pub struct ApplyData {
    pub timeline: serde_json::Value,
}

// Type aliases for convenience
pub type ProposeResponse = AgentResponse<ProposeData>;
pub type PlanResponse = AgentResponse<PlanData>;
pub type ApplyResponse = AgentResponse<ApplyData>;

#[derive(Serialize)]
pub struct SegmentCandidate {
    pub segment_id: i64,
    pub summary_text: Option<String>,
    pub capture_time: Option<String>,
    pub duration_sec: f64,
    pub similarity_score: f32,
}

#[derive(Deserialize)]
pub struct PlanRequest {
    pub beats: Vec<Beat>,
    pub constraints: EditConstraints,
    pub style_profile_id: Option<i64>,
    pub narrative_structure: String,
}

#[derive(Deserialize)]
pub struct Beat {
    pub beat_id: String,
    pub segment_ids: Vec<i64>,
    pub target_sec: Option<f64>,
}

#[derive(Deserialize)]
pub struct EditConstraints {
    pub target_length: Option<i64>,
    pub vibe: Option<String>,
    pub captions_on: bool,
    pub music_on: bool,
}

#[derive(Deserialize)]
pub struct ApplyRequest {
    pub edit_plan: serde_json::Value,
    pub confirm_token: Option<String>, // "overwrite" | "new_version" | null
}

pub fn router(db: Arc<Database>, job_manager: Arc<JobManager>) -> Router {
    Router::new()
        .route("/:id/orchestrator/propose", post(propose))
        .route("/:id/orchestrator/plan", post(plan))
        .route("/:id/orchestrator/apply", post(apply))
        .with_state((db, job_manager))
}

// Check project preconditions with accurate embedding coverage
fn check_project_preconditions(db: &Database, project_id: i64) -> Result<ProjectState, anyhow::Error> {
    let conn = db.conn.lock().unwrap();
    
    // Count media assets
    let media_assets_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM media_assets WHERE project_id = ?1 AND (is_reference IS NULL OR is_reference = 0)",
        params![project_id],
        |row| row.get(0),
    ).unwrap_or(0) as i64;
    
    // Count segments
    let segments_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM segments WHERE project_id = ?1",
        params![project_id],
        |row| row.get(0),
    ).unwrap_or(0) as i64;
    
    // Debug: Check segments and their project_ids
    let segment_ids: Vec<i64> = {
        let mut stmt = conn.prepare("SELECT id FROM segments WHERE project_id = ?1 LIMIT 5")?;
        let rows = stmt.query_map(params![project_id], |row| row.get(0))?;
        rows.collect::<Result<Vec<_>, _>>().unwrap_or_default()
    };
    eprintln!("[ORCHESTRATOR] Found {} segments for project {}, sample IDs: {:?}", segments_count, project_id, segment_ids);
    
    // Debug: Check if embeddings table has any rows at all
    let total_embeddings_any_type: i64 = conn.query_row(
        "SELECT COUNT(*) FROM embeddings",
        params![],
        |row| row.get(0),
    ).unwrap_or(0) as i64;
    eprintln!("[ORCHESTRATOR] Total embeddings in database (any type): {}", total_embeddings_any_type);
    
    // Count segments with text embeddings (must match the model_name used when storing)
    // Debug: First check if embeddings exist at all
    let total_embeddings: i64 = conn.query_row(
        "SELECT COUNT(*) FROM embeddings WHERE embedding_type = 'text' AND model_name = 'all-MiniLM-L6-v2'",
        params![],
        |row| row.get(0),
    ).unwrap_or(0) as i64;
    
    // Debug: Check embeddings for sample segments
    if !segment_ids.is_empty() {
        let sample_segment_id = segment_ids[0];
        let has_emb_for_sample: bool = conn.query_row(
            "SELECT COUNT(*) > 0 FROM embeddings WHERE segment_id = ?1 AND embedding_type = 'text' AND model_name = 'all-MiniLM-L6-v2'",
            params![sample_segment_id],
            |row| row.get(0),
        ).unwrap_or(false);
        eprintln!("[ORCHESTRATOR] Sample segment {} has text embedding: {}", sample_segment_id, has_emb_for_sample);
    }
    
    let segments_with_text_embeddings: i64 = conn.query_row(
        "SELECT COUNT(DISTINCT s.id) FROM segments s
         JOIN embeddings e ON s.id = e.segment_id
         WHERE s.project_id = ?1 AND e.embedding_type = 'text' AND e.model_name = 'all-MiniLM-L6-v2'",
        params![project_id],
        |row| row.get(0),
    ).unwrap_or(0) as i64;
    
    eprintln!("[ORCHESTRATOR] Embedding debug: total_text_embeddings={}, segments_with_text_embeddings={}, segments_count={}", 
        total_embeddings, segments_with_text_embeddings, segments_count);
    
    // Count segments with vision embeddings (must match the model_name used when storing)
    let segments_with_vision_embeddings: i64 = conn.query_row(
        "SELECT COUNT(DISTINCT s.id) FROM segments s
         JOIN embeddings e ON s.id = e.segment_id
         WHERE s.project_id = ?1 AND e.embedding_type = 'vision' AND e.model_name = 'clip-vit-b-32'",
        params![project_id],
        |row| row.get(0),
    ).unwrap_or(0) as i64;
    
    // Calculate embedding coverage
    let embedding_coverage = if segments_count > 0 {
        segments_with_text_embeddings as f32 / segments_count as f32
    } else {
        0.0
    };
    
    // Count running jobs for this project's assets
    let jobs_running_count: i64 = {
        // Get all media asset IDs for this project
        let asset_ids: Vec<i64> = {
            let mut stmt = conn.prepare("SELECT id FROM media_assets WHERE project_id = ?1")?;
            let rows = stmt.query_map(params![project_id], |row| row.get(0))?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        
        if asset_ids.is_empty() {
            0
        } else {
            // Query jobs with Running or Pending status and check if payload contains asset_id
            // Note: status is stored as JSON string, so we need to serialize it
            let running_status_str = serde_json::to_string(&JobStatus::Running)?;
            let pending_status_str = serde_json::to_string(&JobStatus::Pending)?;
            
            // Query for Running jobs
            let mut count = 0;
            let mut stmt = conn.prepare("SELECT id, payload_json, type FROM jobs WHERE status = ?1 OR status = ?2")?;
            let rows = stmt.query_map(params![running_status_str, pending_status_str], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, Option<String>>(1)?, row.get::<_, String>(2)?))
            })?;
            
            for row_result in rows {
                if let Ok((job_id, payload_str_opt, job_type_str)) = row_result {
                    // Parse job type to check if it's an analysis job
                    let job_type: Option<String> = serde_json::from_str(&job_type_str).ok();
                    let is_analysis_job = job_type.as_ref().map_or(false, |jt| {
                        matches!(jt.as_str(), 
                            "TranscribeAsset" | "AnalyzeVisionAsset" | "BuildSegments" |
                            "EnrichSegmentsFromTranscript" | "EnrichSegmentsFromVision" |
                            "ComputeSegmentMetadata" | "EmbedSegments"
                        )
                    });
                    
                    // For analysis jobs, check if payload contains asset_id
                    if let Some(Some(payload_str)) = payload_str_opt.as_ref().map(|s| Some(s)) {
                        if let Ok(payload) = serde_json::from_str::<serde_json::Value>(payload_str) {
                            if let Some(asset_id) = payload.get("asset_id").and_then(|v| v.as_i64()) {
                                if asset_ids.contains(&asset_id) {
                                    eprintln!("[ORCHESTRATOR] Found running job {} (type: {:?}, asset_id: {})", job_id, job_type, asset_id);
                                    count += 1;
                                    continue;
                                }
                            }
                        }
                    }
                    
                    // If no asset_id in payload but it's an analysis job, it might still be relevant
                    // (some jobs might not have asset_id in payload)
                    if is_analysis_job {
                        eprintln!("[ORCHESTRATOR] Found running analysis job {} (type: {:?}) without asset_id in payload", job_id, job_type);
                        // Don't count it unless we can verify it's for this project
                        // For now, we'll be conservative and not count jobs without asset_id
                    }
                }
            }
            count
        }
    };
    
    // Count failed jobs (same approach)
    let jobs_failed_count: i64 = {
        let asset_ids: Vec<i64> = {
            let mut stmt = conn.prepare("SELECT id FROM media_assets WHERE project_id = ?1")?;
            let rows = stmt.query_map(params![project_id], |row| row.get(0))?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        
        if asset_ids.is_empty() {
            0
        } else {
            // Note: status is stored as JSON string, so we need to serialize it
            let failed_status_str = serde_json::to_string(&JobStatus::Failed)?;
            let mut stmt = conn.prepare("SELECT payload_json FROM jobs WHERE status = ?1")?;
            let rows = stmt.query_map(params![failed_status_str], |row| row.get::<_, Option<String>>(0))?;
            let mut count = 0;
            for row_result in rows {
                if let Ok(Some(payload_str)) = row_result {
                    if let Ok(payload) = serde_json::from_str::<serde_json::Value>(&payload_str) {
                        if let Some(asset_id) = payload.get("asset_id").and_then(|v| v.as_i64()) {
                            if asset_ids.contains(&asset_id) {
                                count += 1;
                            }
                        }
                    }
                }
            }
            count
        }
    };
    
    // Debug logging with more detail
    eprintln!("[ORCHESTRATOR] Project {} state: media={}, segments={}, text_emb={}, vision_emb={}, coverage={:.2}%, jobs_running={}, jobs_failed={}",
        project_id, media_assets_count, segments_count, segments_with_text_embeddings, segments_with_vision_embeddings,
        embedding_coverage * 100.0, jobs_running_count, jobs_failed_count);
    eprintln!("[ORCHESTRATOR] BUSY mode check: jobs_running={}, coverage={:.2}% (threshold=80%), will_be_busy={}",
        jobs_running_count, embedding_coverage * 100.0, 
        jobs_running_count > 0 || embedding_coverage < 0.8);
    
    drop(conn);
    
    Ok(ProjectState {
        media_assets_count: media_assets_count as usize,
        segments_count: segments_count as usize,
        segments_with_text_embeddings: segments_with_text_embeddings as usize,
        segments_with_vision_embeddings: segments_with_vision_embeddings as usize,
        embedding_coverage,
        jobs_running_count: jobs_running_count as usize,
        jobs_failed_count: jobs_failed_count as usize,
    })
}

// Determine agent mode with ordered logic
fn determine_mode(
    user_intent: &str,
    state: &ProjectState,
    is_destructive: bool,
    confirm_token: Option<&str>,
) -> AgentMode {
    // 1. Destructive actions need confirmation
    if is_destructive && confirm_token.is_none() {
        return AgentMode::TalkConfirm;
    }
    
    // 2. No media assets
    if state.media_assets_count == 0 {
        return AgentMode::TalkImport;
    }
    
    // 3. No segments
    if state.segments_count == 0 {
        return AgentMode::TalkAnalyze;
    }
    
    // 4. Jobs running or embedding coverage incomplete
    const COVERAGE_THRESHOLD: f32 = 0.8;
    if state.jobs_running_count > 0 || state.embedding_coverage < COVERAGE_THRESHOLD {
        return AgentMode::Busy;
    }
    
    // 5. Ambiguous intent
    let intent_lower = user_intent.to_lowercase();
    let ambiguous_phrases = [
        "make this good",
        "do your thing",
        "edit my vlog",
        "fix this",
        "improve this",
    ];
    
    if ambiguous_phrases.iter().any(|phrase| intent_lower.contains(phrase)) {
        return AgentMode::TalkClarify;
    }
    
    // 6. Ready to act
    AgentMode::Act
}

// Convert mode to string
fn mode_to_string(mode: &AgentMode) -> String {
    match mode {
        AgentMode::TalkConfirm | AgentMode::TalkImport | AgentMode::TalkAnalyze | AgentMode::TalkClarify => "talk".to_string(),
        AgentMode::Busy => "busy".to_string(),
        AgentMode::Act => "act".to_string(),
    }
}

// Generate mode-specific friendly messages
fn generate_response_for_mode(
    mode: &AgentMode,
    state: &ProjectState,
    user_intent: &str,
    candidate_count: usize,
) -> (String, Vec<String>, Vec<String>) {
    match mode {
        AgentMode::TalkImport => (
            "Hey! Your library is empty right now. Click Import Video Clips to add footage — then I'll scan it and suggest a first cut.".to_string(),
            vec!["Import clips".to_string()],
            vec![],
        ),
        AgentMode::TalkAnalyze => (
            "Nice — I see your clips. Next step is analyzing them into moments I can edit with. Want me to start the scan?".to_string(),
            vec!["Analyze clips".to_string()],
            vec![],
        ),
        AgentMode::Busy => {
            let jobs_msg = if state.jobs_running_count > 0 {
                format!("I'm scanning your footage now ({} jobs running).", state.jobs_running_count)
            } else {
                format!("I'm still analyzing your footage ({}% complete).", (state.embedding_coverage * 100.0) as u32)
            };
            (
                format!("{}. You can keep browsing — I'll tell you when I'm ready to propose an edit.", jobs_msg),
                vec!["Show progress".to_string()],
                vec![],
            )
        },
        AgentMode::TalkClarify => (
            "Got it — before I start, what kind of vibe are you going for? Casual vlog, cinematic montage, or something fast-paced?".to_string(),
            vec![],
            vec![
                "What's the main story you want to tell?".to_string(),
                "How long should the final video be?".to_string(),
            ],
        ),
        AgentMode::TalkConfirm => (
            "This will replace your current timeline. Do you want to overwrite it, or create a new version?".to_string(),
            vec![
                "Overwrite timeline".to_string(),
                "Create new version".to_string(),
                "Cancel".to_string(),
            ],
            vec![],
        ),
        AgentMode::Act => {
            if candidate_count == 0 {
                (
                    "I couldn't find moments that match that request yet. Want me to broaden the search, or are you aiming for a specific vibe (funny / cinematic / cozy)?".to_string(),
                    vec![
                        "Broaden search".to_string(),
                        "Show all moments".to_string(),
                    ],
                    vec!["What kind of moments are you looking for?".to_string()],
                )
            } else {
                (
                    format!("I found {} good moments based on speech and visual interest. I'll start with a short hook, then build the main section around these scenes.", candidate_count),
                    vec!["Generate Plan".to_string()],
                    vec![],
                )
            }
        },
    }
}

/// POST /projects/:id/orchestrator/propose - Combined retrieval + narrative reasoning
async fn propose(
    State((db, _job_manager)): State<(Arc<Database>, Arc<JobManager>)>,
    Path(project_id): Path<i64>,
    Query(params): Query<HashMap<String, String>>,
    Json(req): Json<ProposeRequest>,
) -> Result<Json<ProposeResponse>, StatusCode> {
    use engine::timeline::TICKS_PER_SECOND;
    
    // Preflight check
    let state = check_project_preconditions(&db, project_id)
        .map_err(|e| {
            eprintln!("Error checking preconditions: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    
    // Determine mode
    let confirm_token = params.get("confirm").map(|s| s.as_str());
    let mode = determine_mode(&req.user_intent, &state, false, confirm_token);
    
    match mode {
        AgentMode::TalkImport | AgentMode::TalkAnalyze | AgentMode::TalkClarify | AgentMode::Busy => {
            let (message, suggestions, questions) = generate_response_for_mode(&mode, &state, &req.user_intent, 0);
            return Ok(Json(AgentResponse {
                mode: mode_to_string(&mode),
                message,
                suggestions,
                questions,
                data: None,
                debug: None,
            }));
        },
        AgentMode::Act => {
            // Continue with retrieval + reasoning
            // Embed user intent using text embedding
            let query_embedding = llm::embed_text(&req.user_intent)
                .await
                .map_err(|e| {
                    eprintln!("Error embedding text: {:?}", e);
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;
            
            // Try to use fusion embeddings first, fallback to text embeddings if fusion not available
            // Search raw segments only (not reference segments for content)
            let search_results = embeddings::similarity_search(
                db.clone(),
                &query_embedding,
                "fusion",
                "fusion-0.6-0.4",
                50, // Get top 50 candidates
                Some(project_id),
                true, // raw_segments_only = true
            ).or_else(|_| {
                // Fallback to text embeddings if fusion not available
                embeddings::similarity_search(
                    db.clone(),
                    &query_embedding,
                    "text",
                    "all-MiniLM-L6-v2",
                    50,
                    Some(project_id),
                    true, // raw_segments_only = true
                )
            }).map_err(|e| {
                eprintln!("Error in similarity search: {:?}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
            
            // Get segments and apply filters
            let mut candidate_segments = Vec::new();
            for (segment_id, similarity_score) in search_results {
                let segment_opt = db.get_segment_with_embeddings(segment_id)
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                
                if let Some((segment, _embeddings)) = segment_opt {
                    // Apply filters
                    if let Some(ref filters) = req.filters {
                        if let Some(ref kind) = filters.segment_kind {
                            if segment.segment_kind.as_ref() != Some(kind) {
                                continue;
                            }
                        }
                        // Additional filters can be applied here
                    }
                    
                    let duration_sec = {
                        let start = crate::db::Database::get_coalesced_src_in(&segment);
                        let end = crate::db::Database::get_coalesced_src_out(&segment);
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
            
            // If 0 candidates, return TALK mode
            if candidate_segments.is_empty() {
                let (message, suggestions, questions) = generate_response_for_mode(
                    &AgentMode::Act, &state, &req.user_intent, 0
                );
                return Ok(Json(AgentResponse {
                    mode: "talk".to_string(),
                    message,
                    suggestions,
                    questions,
                    data: None,
                    debug: None,
                }));
            }
            
            // Prepare segment metadata for LLM (without embeddings)
            let segment_metadata: Vec<serde_json::Value> = candidate_segments.iter()
                .take(20) // Limit to top 20 for LLM
                .map(|c| {
                    serde_json::json!({
                        "segment_id": c.segment_id,
                        "summary_text": c.summary_text,
                        "capture_time": c.capture_time,
                        "duration_sec": c.duration_sec,
                    })
                })
                .collect();
            
            // Load style profile if available
            let style_profile = if let Some(profile_id) = db.get_project(project_id)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
                .and_then(|p| p.style_profile_id)
            {
                db.get_style_profile(profile_id)
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
                    .and_then(|json| serde_json::from_str::<serde_json::Value>(&json).ok())
            } else {
                None
            };
            
            // Call LLM for structured reasoning (not user-facing copy)
            let timeline_context_json = req.context.as_ref()
                .map(|c| serde_json::to_value(c).ok())
                .flatten();
            
            let narrative_proposal = llm::reason_narrative(
                &segment_metadata,
                style_profile.as_ref(),
                timeline_context_json.as_ref(),
            ).await.map_err(|e| {
                eprintln!("Error in narrative reasoning: {:?}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
            
            // Store proposal in database
            let proposal_json = serde_json::to_string(&narrative_proposal)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            let _ = db.store_orchestrator_proposal(project_id, &proposal_json);
            
            // Generate friendly message
            let (message, suggestions, questions) = generate_response_for_mode(
                &AgentMode::Act, &state, &req.user_intent, candidate_segments.len()
            );
            
            Ok(Json(AgentResponse {
                mode: "act".to_string(),
                message,
                suggestions,
                questions,
                data: Some(ProposeData {
                    candidate_segments,
                    narrative_structure: narrative_proposal.get("narrative_structure")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                }),
                debug: None,
            }))
        },
        AgentMode::TalkConfirm => {
            // Should not happen in propose, but handle gracefully
            let (message, suggestions, questions) = generate_response_for_mode(&mode, &state, &req.user_intent, 0);
            Ok(Json(AgentResponse {
                mode: "talk".to_string(),
                message,
                suggestions,
                questions,
                data: None,
                debug: None,
            }))
        },
    }
}

/// POST /projects/:id/orchestrator/plan - Generate EditPlan
async fn plan(
    State((db, _job_manager)): State<(Arc<Database>, Arc<JobManager>)>,
    Path(project_id): Path<i64>,
    Json(req): Json<PlanRequest>,
) -> Result<Json<PlanResponse>, StatusCode> {
    // Check preconditions
    let state = check_project_preconditions(&db, project_id)
        .map_err(|e| {
            eprintln!("Error checking preconditions: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    
    if state.segments_count == 0 || req.beats.is_empty() {
        let (message, suggestions, questions) = generate_response_for_mode(
            &AgentMode::TalkAnalyze, &state, "", 0
        );
        return Ok(Json(AgentResponse {
            mode: "talk".to_string(),
            message,
            suggestions,
            questions,
            data: None,
            debug: None,
        }));
    }
    
    // Convert beats to JSON
    let beats_json: Vec<serde_json::Value> = req.beats.iter()
        .map(|b| serde_json::json!({
            "beat_id": b.beat_id,
            "segment_ids": b.segment_ids,
            "target_sec": b.target_sec,
        }))
        .collect();
    
    // Convert constraints to JSON
    let constraints_json = serde_json::json!({
        "target_length": req.constraints.target_length,
        "vibe": req.constraints.vibe,
        "captions_on": req.constraints.captions_on,
        "music_on": req.constraints.music_on,
    });
    
    // Call LLM to generate EditPlan
    let beats_json_value = serde_json::json!(beats_json);
    let edit_plan = llm::generate_edit_plan(
        &req.narrative_structure,
        &beats_json_value,
        &constraints_json,
        req.style_profile_id,
    ).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    Ok(Json(AgentResponse {
        mode: "act".to_string(),
        message: "I've generated an edit plan based on your segments. Ready to apply it to your timeline?".to_string(),
        suggestions: vec!["Apply Plan".to_string()],
        questions: vec![],
        data: Some(PlanData { edit_plan }),
        debug: None,
    }))
}

/// POST /projects/:id/orchestrator/apply - Apply EditPlan to timeline
async fn apply(
    State((db, _job_manager)): State<(Arc<Database>, Arc<JobManager>)>,
    Path(project_id): Path<i64>,
    Json(req): Json<ApplyRequest>,
) -> Result<Json<ApplyResponse>, StatusCode> {
    use engine::timeline::TICKS_PER_SECOND;
    
    // Get current timeline
    let current_timeline_json = db.get_timeline(project_id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .unwrap_or_else(|| "{}".to_string());
    
    let timeline: serde_json::Value = serde_json::from_str(&current_timeline_json)
        .unwrap_or_else(|_| serde_json::json!({}));
    
    // Check if timeline has existing clips (destructive action)
    let has_existing_clips = {
        if let Some(tracks) = timeline.get("tracks").and_then(|t| t.as_array()) {
            tracks.iter().any(|track| {
                track.get("clips")
                    .and_then(|c| c.as_array())
                    .map(|clips| !clips.is_empty())
                    .unwrap_or(false)
            })
        } else {
            false
        }
    };
    
    // Check if destructive and needs confirmation
    if has_existing_clips && req.confirm_token.is_none() {
        let state = check_project_preconditions(&db, project_id)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let (message, suggestions, questions) = generate_response_for_mode(
            &AgentMode::TalkConfirm,
            &state,
            "",
            0
        );
        return Ok(Json(AgentResponse {
            mode: "talk".to_string(),
            message,
            suggestions,
            questions,
            data: None,
            debug: None,
        }));
    }
    
    // Store applied plan in database
    let edit_plan_json = serde_json::to_string(&req.edit_plan)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let _ = db.store_orchestrator_apply(project_id, &edit_plan_json);
    
    let mut timeline: serde_json::Value = timeline;
    
    // Parse EditPlan and apply to timeline
    if let Some(primary_segments) = req.edit_plan.get("primary_segments")
        .and_then(|p| p.as_array())
    {
        // Get or create tracks
        if !timeline.get("tracks").is_some() {
            timeline["tracks"] = serde_json::json!([]);
        }
        
        let tracks = timeline.get_mut("tracks")
            .and_then(|t| t.as_array_mut())
            .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
        
        // Ensure primary track exists
        if tracks.is_empty() {
            tracks.push(serde_json::json!({
                "kind": "video",
                "clips": []
            }));
        }
        
        let primary_track = tracks.get_mut(0)
            .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
        
        let clips = primary_track.get_mut("clips")
            .and_then(|c| c.as_array_mut())
            .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
        
        // Add segments sequentially
        let mut current_time_ticks = 0i64;
        for segment_ref in primary_segments {
            if let (Some(segment_id), Some(trim_in), Some(trim_out)) = (
                segment_ref.get("segment_id").and_then(|s| s.as_i64()),
                segment_ref.get("trim_in_offset_ticks").and_then(|t| t.as_i64()),
                segment_ref.get("trim_out_offset_ticks").and_then(|t| t.as_i64()),
            ) {
                // Get segment from database
                let segment_opt = db.get_segment_with_embeddings(segment_id)
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                
                if let Some((segment, _embeddings)) = segment_opt {
                    let src_in = crate::db::Database::get_coalesced_src_in(&segment);
                    let src_out = crate::db::Database::get_coalesced_src_out(&segment);
                    
                    // Apply trim offsets
                    let final_in = src_in + trim_in;
                    let final_out = src_out - trim_out;
                    
                    // Get asset path
                    let asset_path = db.get_media_asset_path(segment.media_asset_id)
                        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
                        .ok_or(StatusCode::NOT_FOUND)?;
                    
                    // Create clip
                    let clip = serde_json::json!({
                        "asset_path": asset_path,
                        "in_ticks": final_in,
                        "out_ticks": final_out,
                        "start_ticks": current_time_ticks,
                        "segment_id": segment_id,
                    });
                    
                    clips.push(clip);
                    
                    // Update current time
                    current_time_ticks += final_out - final_in;
                }
            }
        }
    }
    
    // Store updated timeline
    let updated_timeline_json = serde_json::to_string(&timeline)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    db.store_timeline(project_id, &updated_timeline_json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    Ok(Json(AgentResponse {
        mode: "act".to_string(),
        message: "Done! I've applied the edit to your timeline.".to_string(),
        suggestions: vec![],
        questions: vec![],
        data: Some(ApplyData { timeline }),
        debug: None,
    }))
}

