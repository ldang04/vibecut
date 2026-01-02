use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{sse::Event, Json, Sse},
    routing::{get, post},
    Router,
};
use futures::stream::{self, Stream};
use std::convert::Infallible;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio_stream::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::Result;
use crate::db::Database;
use crate::embeddings;
use crate::jobs::{JobEvent, JobManager, JobStatus, JobType};
use crate::llm;
use crate::orchestrator::ensure::{ensure_ready, ReadinessGoal};
use crate::api::orchestrator_helper::diversify_candidates;
use crate::api::timeline;
use serde_json;
use rusqlite::params;

// Project state for precondition checking
pub struct ProjectState {
    pub media_assets_count: usize,
    pub segments_count: usize,
    pub segments_with_text_embeddings: usize,
    pub segments_with_vision_embeddings: usize,
    pub embedding_coverage: f32,
    pub jobs_running_count: usize,
    pub jobs_failed_count: usize,
}

// Agent mode enum
#[derive(Debug)]
pub enum AgentMode {
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

// Structured suggestion with action metadata
#[derive(Serialize, Deserialize, Clone)]
pub struct Suggestion {
    pub label: String,           // Display text
    pub action: String,          // "import_clips" | "analyze_clips" | "broaden_search" | "generate_plan" | "overwrite_timeline" | "create_new_version" | "cancel" | "show_progress"
    pub confirm_token: Option<String>,  // For destructive actions: "overwrite" | "new_version"
}

// Uniform response contract
#[derive(Serialize)]
pub struct AgentResponse<T> {
    pub mode: String,            // "talk" | "busy" | "act"
    pub message: String,         // Friendly assistant copy
    pub suggestions: Vec<Suggestion>, // Quick replies/buttons (structured)
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

#[derive(Serialize, Debug, Clone)]
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
    // Note: confirm_token removed - use query param instead
}

/// GET /projects/:id/orchestrator/messages - Get conversation history
async fn get_messages(
    State((db, _job_manager)): State<(Arc<Database>, Arc<JobManager>)>,
    Path(project_id): Path<i64>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let messages = db.get_orchestrator_messages(project_id, 50)
        .map_err(|e| {
            eprintln!("Error getting messages: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    
    Ok(Json(serde_json::json!({
        "messages": messages
    })))
}

#[derive(Deserialize)]
struct ParseIntentRequest {
    user_message: String,
    conversation_history: Option<Vec<serde_json::Value>>,
}

/// POST /projects/:id/orchestrator/parse_intent - Parse natural language to structured intent
async fn parse_intent_endpoint(
    State((db, _job_manager)): State<(Arc<Database>, Arc<JobManager>)>,
    Path(project_id): Path<i64>,
    Json(req): Json<ParseIntentRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Get conversation history if not provided
    let history = if let Some(provided_history) = req.conversation_history {
        provided_history
    } else {
        db.get_orchestrator_messages(project_id, 20)
            .map_err(|e| {
                eprintln!("Error getting messages: {:?}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?
    };
    
    // Call LLM to parse intent
    let parsed = llm::parse_intent(&req.user_message, Some(&history))
        .await
        .map_err(|e| {
            eprintln!("Error parsing intent: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    
    Ok(Json(parsed))
}

pub fn router(db: Arc<Database>, job_manager: Arc<JobManager>) -> Router {
    Router::new()
        .route("/:id/orchestrator/propose", post(propose))
        .route("/:id/orchestrator/plan", post(plan))
        .route("/:id/orchestrator/apply", post(apply))
        .route("/:id/orchestrator/events", get(events))
        .route("/:id/orchestrator/messages", get(get_messages))
        .route("/:id/orchestrator/parse_intent", post(parse_intent_endpoint))
        .with_state((db, job_manager))
}

// Check project preconditions with accurate embedding coverage
pub fn check_project_preconditions(db: &Database, project_id: i64) -> Result<ProjectState, anyhow::Error> {
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
            let running_status_str = JobStatus::Running.to_string();
            let pending_status_str = JobStatus::Pending.to_string();
            
            // Query for Running jobs
            let mut count = 0;
            let mut stmt = conn.prepare("SELECT id, payload_json, type FROM jobs WHERE status = ?1 OR status = ?2")?;
            let rows = stmt.query_map(params![running_status_str, pending_status_str], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, Option<String>>(1)?, row.get::<_, String>(2)?))
            })?;
            
            for row_result in rows {
                if let Ok((job_id, payload_str_opt, job_type_str)) = row_result {
                    // Parse job type from plain string to check if it's an analysis job
                    let job_type_parsed = JobType::from_str(&job_type_str).ok();
                    let is_analysis_job = job_type_parsed.as_ref().map_or(false, |jt| {
                        matches!(jt,
                            JobType::TranscribeAsset | JobType::AnalyzeVisionAsset | JobType::BuildSegments |
                            JobType::EnrichSegmentsFromTranscript | JobType::EnrichSegmentsFromVision |
                            JobType::ComputeSegmentMetadata | JobType::EmbedSegments
                        )
                    });
                    
                    // For analysis jobs, check if payload contains asset_id
                    if let Some(Some(payload_str)) = payload_str_opt.as_ref().map(|s| Some(s)) {
                        if let Ok(payload) = serde_json::from_str::<serde_json::Value>(payload_str) {
                            if let Some(asset_id) = payload.get("asset_id").and_then(|v| v.as_i64()) {
                                if asset_ids.contains(&asset_id) {
                                    eprintln!("[ORCHESTRATOR] Found running job {} (type: {}, asset_id: {})", job_id, job_type_str, asset_id);
                                    count += 1;
                                    continue;
                                }
                            }
                        }
                    }
                    
                    // If no asset_id in payload but it's an analysis job, it might still be relevant
                    // (some jobs might not have asset_id in payload)
                    if is_analysis_job {
                        eprintln!("[ORCHESTRATOR] Found running analysis job {} (type: {}) without asset_id in payload", job_id, job_type_str);
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
            let failed_status_str = JobStatus::Failed.to_string();
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
pub fn determine_mode(
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

// Generate agent response using LLM (replaces hardcoded messages)
async fn generate_agent_response_with_llm(
    mode: &AgentMode,
    state: &ProjectState,
    user_intent: &str,
    candidate_count: usize,
    conversation_history: Vec<serde_json::Value>,
    event_type: &str,
    db: &Database,
    project_id: i64,
) -> Result<(String, Vec<Suggestion>, Vec<String>)> {
    // Construct project state JSON
    let project_state_json = serde_json::json!({
        "media_assets_count": state.media_assets_count,
        "segments_count": state.segments_count,
        "segments_with_text_embeddings": state.segments_with_text_embeddings,
        "segments_with_vision_embeddings": state.segments_with_vision_embeddings,
        "embedding_coverage": state.embedding_coverage,
        "jobs_running_count": state.jobs_running_count,
        "jobs_failed_count": state.jobs_failed_count,
    });
    
    // Get current goal if exists
    let goal = db.get_active_orchestrator_goals(project_id)
        .ok()
        .and_then(|goals| goals.first().cloned());
    
    // Get latest edit plan if it exists (for when user asks "what's the plan?")
    let latest_plan = db.get_latest_edit_plan(project_id).ok().flatten();
    
    // Construct context JSON
    let mut context_json = serde_json::json!({
        "user_intent": user_intent,
        "candidate_count": candidate_count,
        "mode": mode_to_string(mode),
        "goal": goal.as_ref().map(|(_, intent, status)| serde_json::json!({
            "intent": intent,
            "status": status,
        })),
    });
    
    // If candidate_count > 0, try to get segment descriptions from the most recent proposal
    // This allows the LLM to describe what segments were actually found
    // Note: This is best-effort - if no proposal exists yet, segment_descriptions won't be in context
    if candidate_count > 0 {
        // Try to get the most recent proposal - extract the JSON string first to avoid lifetime issues
        let proposal_json: Option<String> = {
            let conn = db.conn.lock().unwrap();
            conn.prepare(
                "SELECT proposal_json FROM orchestrator_proposals WHERE project_id = ?1 ORDER BY created_at DESC LIMIT 1"
            )
            .ok()
            .and_then(|mut stmt| {
                stmt.query_map(params![project_id], |row| {
                    Ok(row.get::<_, String>(0)?)
                })
                .ok()
                .and_then(|mut rows| rows.next().and_then(|r| r.ok()))
            })
        };
        
        if let Some(proposal_json_str) = proposal_json {
            if let Ok(proposal) = serde_json::from_str::<serde_json::Value>(&proposal_json_str) {
                if let Some(segments) = proposal.get("segments").and_then(|s| s.as_array()) {
                    let segment_descriptions: Vec<String> = segments.iter()
                        .take(10)
                        .filter_map(|seg| {
                            seg.get("description")
                                .or_else(|| seg.get("summary_text"))
                                .and_then(|d| d.as_str())
                                .map(|s| s.to_string())
                        })
                        .collect();
                    if !segment_descriptions.is_empty() {
                        context_json["segment_descriptions"] = serde_json::json!(segment_descriptions);
                    }
                }
            }
        }
    }
    
    // Include plan summary with semantic descriptions if available
    if let Some(plan) = latest_plan {
        let primary_segments = plan.get("primary_segments")
            .and_then(|v| v.as_array())
            .map(|arr| arr.len())
            .unwrap_or(0);
        let total_duration = plan.get("primary_segments")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|seg| seg.get("duration_sec").and_then(|d| d.as_f64()))
                    .sum::<f64>()
            })
            .unwrap_or(0.0);
        
        // Extract segment descriptions for narrative description
        let segment_descriptions: Vec<String> = plan.get("primary_segments")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|seg| seg.get("description").and_then(|d| d.as_str()).map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        
        context_json["edit_plan"] = serde_json::json!({
            "has_plan": true,
            "segment_count": primary_segments,
            "total_duration_sec": total_duration,
            "plan_summary": format!("{} segments, {:.1}s total", primary_segments, total_duration),
            "segment_descriptions": segment_descriptions,
        });
    } else {
        context_json["edit_plan"] = serde_json::json!({
            "has_plan": false,
        });
    }
    
    // Call LLM to generate response
    let response = match llm::generate_agent_response(
        &conversation_history,
        &project_state_json,
        &context_json,
        event_type,
    ).await {
        Ok(resp) => resp,
        Err(e) => {
            eprintln!("[ERROR] LLM call failed: {:?}", e);
            eprintln!("[ERROR] This means the agent will use fallback messages instead of LLM-generated ones.");
            eprintln!("[ERROR] Check: 1) ML service is running, 2) OpenAI API key is set, 3) Network connectivity");
            return Err(e);
        }
    };
    
    // Extract message
    let message = response.get("message")
        .and_then(|v| v.as_str())
        .unwrap_or("I'm here to help!")
        .to_string();
    
    // Extract and validate suggestions
    let empty_vec: Vec<serde_json::Value> = Vec::new();
    let suggestions_raw = response.get("suggestions")
        .and_then(|v| v.as_array())
        .unwrap_or(&empty_vec);
    
    let mut suggestions = Vec::new();
    let allowed_actions = [
        "import_clips", "analyze_clips", "generate_plan", "apply_plan",
        "overwrite_timeline", "create_new_version", "broaden_search",
        "show_all_moments", "show_progress", "cancel",
    ];
    
    for sug in suggestions_raw {
        if let Some(sug_obj) = sug.as_object() {
            if let (Some(label), Some(action)) = (
                sug_obj.get("label").and_then(|v| v.as_str()),
                sug_obj.get("action").and_then(|v| v.as_str()),
            ) {
                // Validate action is allowed
                if allowed_actions.contains(&action) {
                    let confirm_token = sug_obj.get("confirm_token")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    
                    suggestions.push(Suggestion {
                        label: label.to_string(),
                        action: action.to_string(),
                        confirm_token,
                    });
                }
            }
        }
    }
    
    // Extract questions
    let questions = response.get("questions")
        .and_then(|v| v.as_array())
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .collect();
    
    // Store assistant message in database
    let _ = db.store_orchestrator_message(
        project_id,
        "assistant",
        &message,
        Some(&response),
    );
    
    Ok((message, suggestions, questions))
}

// Generate mode-specific friendly messages (fallback if LLM fails)
fn generate_response_for_mode(
    mode: &AgentMode,
    state: &ProjectState,
    user_intent: &str,
    candidate_count: usize,
) -> (String, Vec<Suggestion>, Vec<String>) {
    match mode {
        AgentMode::TalkImport => (
            "Hey! Your library is empty right now. Click Import Video Clips to add footage — then I'll scan it and suggest a first cut.".to_string(),
            vec![Suggestion {
                label: "Import clips".to_string(),
                action: "import_clips".to_string(),
                confirm_token: None,
            }],
            vec![],
        ),
        AgentMode::TalkAnalyze => (
            "Nice — I see your clips. Next step is analyzing them into moments I can edit with. Want me to start the scan?".to_string(),
            vec![Suggestion {
                label: "Analyze clips".to_string(),
                action: "analyze_clips".to_string(),
                confirm_token: None,
            }],
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
                vec![Suggestion {
                    label: "Show progress".to_string(),
                    action: "show_progress".to_string(),
                    confirm_token: None,
                }],
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
                Suggestion {
                    label: "Overwrite timeline".to_string(),
                    action: "overwrite_timeline".to_string(),
                    confirm_token: Some("overwrite".to_string()),
                },
                Suggestion {
                    label: "Create new version".to_string(),
                    action: "create_new_version".to_string(),
                    confirm_token: Some("new_version".to_string()),
                },
                Suggestion {
                    label: "Cancel".to_string(),
                    action: "cancel".to_string(),
                    confirm_token: None,
                },
            ],
            vec![],
        ),
        AgentMode::Act => {
            if candidate_count == 0 {
                (
                    "I couldn't find moments that match that request yet. Want me to broaden the search, or are you aiming for a specific vibe (funny / cinematic / cozy)?".to_string(),
                    vec![
                        Suggestion {
                            label: "Broaden search".to_string(),
                            action: "broaden_search".to_string(),
                            confirm_token: None,
                        },
                        Suggestion {
                            label: "Show all moments".to_string(),
                            action: "show_all_moments".to_string(),
                            confirm_token: None,
                        },
                    ],
                    vec!["What kind of moments are you looking for?".to_string()],
                )
            } else {
                (
                    format!("I found {} good moments based on speech and visual interest. I'll start with a short hook, then build the main section around these scenes.", candidate_count),
                    vec![Suggestion {
                        label: "Generate Plan".to_string(),
                        action: "generate_plan".to_string(),
                        confirm_token: None,
                    }],
                    vec![],
                )
            }
        },
    }
}

/// POST /projects/:id/orchestrator/propose - Combined retrieval + narrative reasoning
async fn propose(
    State((db, job_manager)): State<(Arc<Database>, Arc<JobManager>)>,
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
    
    // Create or update goal based on user intent
    if !req.user_intent.is_empty() {
        let initial_status = match mode {
            AgentMode::TalkImport => "needs_analysis",
            AgentMode::TalkAnalyze | AgentMode::Busy => "needs_analysis",
            AgentMode::Act => "ready_to_propose",
            _ => "needs_analysis",
        };
        
        // Check for existing active goals
        let active_goals = db.get_active_orchestrator_goals(project_id)
            .unwrap_or_default();
        
        if active_goals.is_empty() {
            // Create new goal
            let _ = db.create_orchestrator_goal(project_id, &req.user_intent, initial_status);
        } else {
            // Update most recent goal
            if let Some((goal_id, _, _)) = active_goals.first() {
                let _ = db.update_orchestrator_goal_status(*goal_id, initial_status);
            }
        }
    }
    
    // Auto-enqueue missing jobs for TalkAnalyze or Busy modes
    match mode {
        AgentMode::TalkAnalyze => {
            // Enqueue jobs to reach Segmented state
            let ensure_result = ensure_ready(&db, &job_manager, project_id, ReadinessGoal::Segmented)
                .map_err(|e| {
                    eprintln!("Error ensuring ready for Segmented: {:?}", e);
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;
            
            if !ensure_result.enqueued_jobs.is_empty() {
                // Jobs were enqueued, return BUSY mode with LLM response
                let history = db.get_orchestrator_messages(project_id, 20).unwrap_or_default();
                match generate_agent_response_with_llm(
                    &AgentMode::Busy,
                    &state,
                    &req.user_intent,
                    0,
                    history,
                    "user_message",
                    &db,
                    project_id,
                ).await {
                    Ok((message, suggestions, questions)) => {
                        return Ok(Json(AgentResponse {
                            mode: "busy".to_string(),
                            message,
                            suggestions,
                            questions,
                            data: None,
                            debug: None,
                        }));
                    }
                    Err(e) => {
                        eprintln!("[ERROR] Failed to generate LLM response: {:?}", e);
                        return Err(StatusCode::INTERNAL_SERVER_ERROR);
                    }
                }
            }
        },
        AgentMode::Busy => {
            // Enqueue jobs to reach Embedded state (what we need for proposals)
            let ensure_result = ensure_ready(&db, &job_manager, project_id, ReadinessGoal::Embedded)
                .map_err(|e| {
                    eprintln!("Error ensuring ready for Embedded: {:?}", e);
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;
            
            if !ensure_result.enqueued_jobs.is_empty() {
                let history = db.get_orchestrator_messages(project_id, 20).unwrap_or_default();
                match generate_agent_response_with_llm(
                    &AgentMode::Busy,
                    &state,
                    &req.user_intent,
                    0,
                    history,
                    "user_message",
                    &db,
                    project_id,
                ).await {
                    Ok((message, suggestions, questions)) => {
                        return Ok(Json(AgentResponse {
                            mode: "busy".to_string(),
                            message,
                            suggestions,
                            questions,
                            data: None,
                            debug: None,
                        }));
                    }
                Err(e) => {
                    // No fallback - return error
                    eprintln!("[ERROR] Failed to generate LLM response: {:?}", e);
                    return Err(StatusCode::INTERNAL_SERVER_ERROR);
                }
                }
            }
        },
        _ => {}
    }
    
    match mode {
        AgentMode::TalkImport | AgentMode::TalkAnalyze | AgentMode::TalkClarify | AgentMode::Busy => {
            let history = db.get_orchestrator_messages(project_id, 20).unwrap_or_default();
            match generate_agent_response_with_llm(
                &mode,
                &state,
                &req.user_intent,
                0,
                history,
                "user_message",
                &db,
                project_id,
            ).await {
                Ok((message, suggestions, questions)) => {
                    return Ok(Json(AgentResponse {
                        mode: mode_to_string(&mode),
                        message,
                        suggestions,
                        questions,
                        data: None,
                        debug: None,
                    }));
                }
                Err(e) => {
                    // No fallback - return error
                    eprintln!("[ERROR] Failed to generate LLM response: {:?}", e);
                    return Err(StatusCode::INTERNAL_SERVER_ERROR);
                }
            }
        },
        AgentMode::Act => {
            // Continue with retrieval + reasoning
            // Use retrieval module (handles TwelveLabs + fallback to local embeddings)
            let retrieval_result = crate::retrieval::retrieve_candidates(
                db.clone(),
                project_id,
                &req.user_intent,
                req.filters.as_ref(),
                req.context.as_ref(),
            ).await.map_err(|e| {
                eprintln!("Error in retrieval: {:?}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
            
            let mut candidate_segments = retrieval_result.candidates;
            
            // Apply diversity filtering (max 3 segments per asset, dedupe summaries)
            candidate_segments = diversify_candidates(candidate_segments, 3, &db)
                .map_err(|e| {
                    eprintln!("Error diversifying candidates: {:?}", e);
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;
            
            // Build warning message if fallback was used
            let mut warning_message = None;
            if let Some(debug_obj) = retrieval_result.debug.as_object() {
                if let Some(fallback_reason) = debug_obj.get("fallback_reason") {
                    if !fallback_reason.is_null() {
                        warning_message = Some("I'm still indexing your footage for better search; results will improve shortly.".to_string());
                    }
                }
            }
            
            // If 0 candidates after filtering, return TALK mode
            if candidate_segments.is_empty() {
                let history = db.get_orchestrator_messages(project_id, 20).unwrap_or_default();
                match generate_agent_response_with_llm(
                    &AgentMode::Act,
                    &state,
                    &req.user_intent,
                    0,
                    history,
                    "user_message",
                    &db,
                    project_id,
                ).await {
                    Ok((message, suggestions, questions)) => {
                        return Ok(Json(AgentResponse {
                            mode: "talk".to_string(),
                            message,
                            suggestions,
                            questions,
                            data: None,
                            debug: None,
                        }));
                    }
                    Err(e) => {
                        eprintln!("[ERROR] Failed to generate LLM response: {:?}", e);
                        return Err(StatusCode::INTERNAL_SERVER_ERROR);
                    }
                }
            }
            
            // Prepare segment metadata for LLM (without embeddings) - include rich semantic descriptions
            let segment_metadata: Vec<serde_json::Value> = candidate_segments.iter()
                .take(20) // Limit to top 20 for LLM
                .map(|c| {
                    // Get full segment data for richer description
                    let mut description = c.summary_text.clone().unwrap_or_else(|| "video segment".to_string());
                    
                    // Try to get full segment to enrich description
                    if let Ok(Some((segment, _))) = db.get_segment_with_embeddings(c.segment_id) {
                        let mut desc_parts = Vec::new();
                        
                        if let Some(ref summary) = segment.summary_text {
                            desc_parts.push(summary.clone());
                        }
                        
                        if let Some(ref transcript) = segment.transcript {
                            let transcript_excerpt = transcript.split('.').next()
                                .unwrap_or(transcript)
                                .chars()
                                .take(80)
                                .collect::<String>();
                            if !transcript_excerpt.trim().is_empty() {
                                desc_parts.push(format!("spoken: {}", transcript_excerpt));
                            }
                        }
                        
                        if let Some(ref scene_json) = segment.scene_json {
                            if let Ok(scene) = serde_json::from_str::<serde_json::Value>(scene_json) {
                                if let Some(tags) = scene.get("tags").and_then(|t| t.as_array()) {
                                    let tag_str: Vec<String> = tags.iter()
                                        .filter_map(|t| t.as_str().map(|s| s.to_string()))
                                        .collect();
                                    if !tag_str.is_empty() {
                                        desc_parts.push(format!("scene: {}", tag_str.join(", ")));
                                    }
                                }
                            }
                        }
                        
                        if !desc_parts.is_empty() {
                            description = desc_parts.join(" | ");
                        }
                    }
                    
                    serde_json::json!({
                        "segment_id": c.segment_id,
                        "description": description,
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
            
            // Update goal status to "proposed"
            if let Ok(Some((goal_id, _))) = db.get_orchestrator_goal_by_status(project_id, "ready_to_propose") {
                let _ = db.update_orchestrator_goal_status(goal_id, "proposed");
            }
            
            // Generate friendly message using LLM - include segment descriptions
            let history = db.get_orchestrator_messages(project_id, 20).unwrap_or_default();
            
            // Build segment descriptions for context
            let segment_descriptions: Vec<String> = segment_metadata.iter()
                .take(10) // Top 10 for description
                .filter_map(|seg| {
                    seg.get("description")
                        .or_else(|| seg.get("summary_text"))
                        .and_then(|d| d.as_str())
                        .map(|s| s.to_string())
                })
                .collect();
            
            let (message, suggestions, questions) = match generate_agent_response_with_llm(
                &AgentMode::Act,
                &state,
                &req.user_intent,
                candidate_segments.len(),
                history,
                "user_message",
                &db,
                project_id,
            ).await {
                Ok((msg, sug, q)) => (msg, sug, q),
                Err(e) => {
                    eprintln!("[ERROR] Failed to generate LLM response: {:?}", e);
                    return Err(StatusCode::INTERNAL_SERVER_ERROR);
                }
            };
            
            // Include warning in message if fallback was used
            let final_message = if let Some(warning) = warning_message {
                format!("{}\n\n{}", message, warning)
            } else {
                message
            };
            
            Ok(Json(AgentResponse {
                mode: "act".to_string(),
                message: final_message,
                suggestions,
                questions,
                data: Some(ProposeData {
                    candidate_segments,
                    narrative_structure: narrative_proposal.get("narrative_structure")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                }),
                debug: Some(retrieval_result.debug),
            }))
        },
        AgentMode::TalkConfirm => {
            // Should not happen in propose, but handle with LLM
            let history = db.get_orchestrator_messages(project_id, 20).unwrap_or_default();
            match generate_agent_response_with_llm(
                &mode,
                &state,
                &req.user_intent,
                0,
                history,
                "user_message",
                &db,
                project_id,
            ).await {
                Ok((message, suggestions, questions)) => {
                    Ok(Json(AgentResponse {
                        mode: "talk".to_string(),
                        message,
                        suggestions,
                        questions,
                        data: None,
                        debug: None,
                    }))
                }
                Err(e) => {
                    eprintln!("[ERROR] Failed to generate LLM response: {:?}", e);
                    Err(StatusCode::INTERNAL_SERVER_ERROR)
                }
            }
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
        // Get LLM response for missing segments
        let history = db.get_orchestrator_messages(project_id, 20).unwrap_or_default();
        match generate_agent_response_with_llm(
            &AgentMode::TalkAnalyze,
            &state,
            "",
            0,
            history,
            "generate_plan",
            &db,
            project_id,
        ).await {
            Ok((message, suggestions, questions)) => {
                return Ok(Json(AgentResponse {
                    mode: "talk".to_string(),
                    message,
                    suggestions,
                    questions,
                    data: None,
                    debug: None,
                }));
            }
            Err(e) => {
                eprintln!("[ERROR] Failed to generate LLM response: {:?}", e);
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }
        }
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
    
    // Update goal status to "planned"
    if let Ok(Some((goal_id, _))) = db.get_orchestrator_goal_by_status(project_id, "proposed") {
        let _ = db.update_orchestrator_goal_status(goal_id, "planned");
    }
    
    // Get LLM response for plan generated - include context about what was generated
    let history = db.get_orchestrator_messages(project_id, 20).unwrap_or_default();
    
    // Get the user's original intent from the most recent user message or goal
    let user_intent = history.iter()
        .rev()
        .find(|msg| msg.get("role").and_then(|r| r.as_str()) == Some("user"))
        .and_then(|msg| msg.get("content").and_then(|c| c.as_str()))
        .unwrap_or("create an edit")
        .to_string();
    
    // Count segments in the plan
    let segment_count = edit_plan.get("primary_segments")
        .and_then(|v| v.as_array())
        .map(|arr| arr.len())
        .unwrap_or(0);
    
    let (message, suggestions, questions) = generate_agent_response_with_llm(
        &AgentMode::Act,
        &state,
        &format!("I've generated an edit plan for: {}", user_intent),
        segment_count,
        history,
        "plan_generated",
        &db,
        project_id,
    ).await.map_err(|e| {
        eprintln!("[ERROR] Failed to generate LLM response: {:?}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    
    // Store the plan in database so it can be retrieved later
    let edit_plan_json = serde_json::to_string(&edit_plan)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let _ = db.store_orchestrator_apply(project_id, &edit_plan_json);
    
    Ok(Json(AgentResponse {
        mode: "act".to_string(),
        message,
        suggestions,
        questions,
        data: Some(PlanData { edit_plan }),
        debug: None,
    }))
}

/// POST /projects/:id/orchestrator/apply - Apply EditPlan to timeline
async fn apply(
    State((db, _job_manager)): State<(Arc<Database>, Arc<JobManager>)>,
    Path(project_id): Path<i64>,
    Query(query_params): Query<HashMap<String, String>>,
    Json(req): Json<ApplyRequest>,
) -> Result<Json<ApplyResponse>, StatusCode> {
    // Check if timeline has existing clips (destructive action)
    let has_existing_clips = {
        let timeline_json = db.get_timeline(project_id)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        if let Some(json_str) = timeline_json {
            if let Ok(timeline_obj) = serde_json::from_str::<engine::timeline::Timeline>(&json_str) {
                timeline_obj.tracks.iter().any(|track| !track.clips.is_empty())
            } else {
                false
            }
        } else {
            false
        }
    };
    
    // Check if destructive and needs confirmation
    let confirm_token = query_params.get("confirm").map(|s| s.as_str());
    let is_new_version = confirm_token == Some("new_version");
    let is_overwrite = confirm_token == Some("overwrite");
    
    if has_existing_clips && !is_new_version && !is_overwrite {
        let state = check_project_preconditions(&db, project_id)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        
        // Get LLM response for confirmation - include context about applying
        let history = db.get_orchestrator_messages(project_id, 20).unwrap_or_default();
        
        // Get the user's intent from conversation history
        let user_intent = history.iter()
            .rev()
            .find(|msg| msg.get("role").and_then(|r| r.as_str()) == Some("user"))
            .and_then(|msg| msg.get("content").and_then(|c| c.as_str()))
            .unwrap_or("apply the edit")
            .to_string();
        
        let (message, suggestions, questions) = generate_agent_response_with_llm(
            &AgentMode::TalkConfirm,
            &state,
            &format!("User wants to apply the edit plan: {}", user_intent),
            0,
            history,
            "apply_plan",
            &db,
            project_id,
        ).await.map_err(|e| {
            eprintln!("[ERROR] Failed to generate LLM response: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
        
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
    
    // Update goal status to "applied" -> "completed"
    if let Ok(Some((goal_id, _))) = db.get_orchestrator_goal_by_status(project_id, "planned") {
        let _ = db.update_orchestrator_goal_status(goal_id, "applied");
        let _ = db.update_orchestrator_goal_status(goal_id, "completed");
    }
    
    // TODO: Convert EditPlan to TimelineOperations
    // This function needs to be implemented based on the EditPlan structure from the ML service
    // For now, return an error indicating this is not yet implemented
    eprintln!("[ORCHESTRATOR] EditPlan to TimelineOperations conversion not yet implemented");
    return Err(StatusCode::NOT_IMPLEMENTED);
}

/// GET /projects/:id/orchestrator/events - SSE endpoint for orchestrator events
async fn events(
    State((_db, job_manager)): State<(Arc<Database>, Arc<JobManager>)>,
    Path(project_id): Path<i64>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    // Subscribe to job events
    let mut rx = job_manager.subscribe();
    
    // Create a stream from the broadcast receiver using unfold
    let event_stream = stream::unfold((rx, project_id), |(mut rx, project_id)| async move {
        match rx.recv().await {
            Ok(event) => {
                // Filter events for this project
                let should_include = match &event {
                    JobEvent::AnalysisComplete { project_id: pid, .. } => *pid == project_id,
                    JobEvent::JobCompleted { .. } | JobEvent::JobFailed { .. } => {
                        // For now, accept all job events (we can improve filtering later)
                        true
                    }
                };
                
                if should_include {
                    let json = serde_json::to_string(&event).unwrap_or_else(|_| "{}".to_string());
                    Some((Ok(Event::default().data(json)), (rx, project_id)))
                } else {
                    // Continue stream with next event
                    Some((Ok(Event::default().comment("filtered")), (rx, project_id)))
                }
            }
            Err(_) => {
                // Receiver closed or lagged, end stream
                None
            }
        }
    });

    // Combine with keep-alive stream
    let keep_alive = stream::unfold((), |_| async {
        tokio::time::sleep(Duration::from_secs(30)).await;
        Some((Ok(Event::default().comment("keep-alive")), ()))
    });

    let combined = stream::select(event_stream, keep_alive);

    Sse::new(combined).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive-text"),
    )
}

