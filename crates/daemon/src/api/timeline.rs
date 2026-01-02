use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::db::Database;
use engine::timeline::{Timeline, ProjectSettings, Resolution, TICKS_PER_SECOND};
use engine::ops::TimelineOperation;
use serde_json::{json, Value};
use rusqlite::params;

#[derive(Serialize)]
pub struct TimelineResponse {
    timeline: Value, // JSON representation of timeline
}

#[derive(Deserialize)]
pub struct ApplyOperationsRequest {
    operations: Vec<Value>, // Simplified - would be TimelineOperation enums
}

#[derive(Deserialize)]
pub struct DiffRequest {
    from: Value,
    to: Value,
}

pub fn router(db: Arc<Database>) -> Router {
    Router::new()
        .route("/:id/timeline", get(get_timeline))
        .route("/:id/timeline/apply", post(apply_operations))
        .route("/:id/timeline/consolidate", post(consolidate_timeline))
        .route("/timeline/consolidate-all", post(consolidate_all_timelines))
        .route("/:id/timeline/diff", post(log_diff))
        .route("/:id/timeline/test", post(test_timeline_serialization))
        .with_state(db)
}

async fn get_timeline(
    State(db): State<Arc<Database>>,
    Path(project_id): Path<i64>,
) -> Result<Json<TimelineResponse>, StatusCode> {
    // Load timeline from DB - return empty timeline if it doesn't exist yet
    let timeline = if let Some(timeline_json) = db
        .get_timeline(project_id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    {
        serde_json::from_str(&timeline_json)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    } else {
        // Return empty timeline structure if none exists
        json!({
            "settings": {
                "fps": 30.0,
                "resolution": { "width": 1920, "height": 1080 },
                "sample_rate": 48000,
                "ticks_per_second": 48000
            },
            "tracks": [],
            "captions": [],
            "music": [],
            "markers": []
        })
    };
    
    Ok(Json(TimelineResponse { timeline }))
}

async fn apply_operations(
    State(db): State<Arc<Database>>,
    Path(project_id): Path<i64>,
    Json(req): Json<ApplyOperationsRequest>,
) -> Result<Json<TimelineResponse>, StatusCode> {
    // Load timeline from database
    let timeline_json = db
        .get_timeline(project_id)
        .map_err(|e| {
            eprintln!("Failed to get timeline from database: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    // Deserialize timeline or create default
    let mut timeline: Timeline = if let Some(json_str) = timeline_json {
        eprintln!("Loading timeline from database, JSON length: {}", json_str.len());
        if json_str.len() < 200 {
            eprintln!("Timeline JSON from DB: {}", json_str);
        } else {
            eprintln!("Timeline JSON from DB (first 200 chars): {}", &json_str[..200.min(json_str.len())]);
        }
        
        match serde_json::from_str::<Timeline>(&json_str) {
            Ok(t) => {
                eprintln!("Successfully deserialized timeline from DB - tracks: {}, captions: {}, music: {}, markers: {}", 
                    t.tracks.len(), t.captions.len(), t.music.len(), t.markers.len());
                t
            }
            Err(e) => {
                eprintln!("Failed to deserialize timeline from DB: {:?}", e);
                eprintln!("Creating fresh timeline instead");
                // Create default timeline if deserialization fails
                let settings = ProjectSettings {
                    fps: 30.0,
                    resolution: Resolution {
                        width: 1920,
                        height: 1080,
                    },
                    sample_rate: 48000,
                    ticks_per_second: TICKS_PER_SECOND,
                };
                Timeline::new(settings)
            }
        }
    } else {
        eprintln!("No timeline found in database, creating new timeline");
        // Create default timeline if none exists
        let settings = ProjectSettings {
            fps: 30.0,
            resolution: Resolution {
                width: 1920,
                height: 1080,
            },
            sample_rate: 48000,
            ticks_per_second: TICKS_PER_SECOND,
        };
        Timeline::new(settings)
    };
    
    eprintln!("Timeline before operations - tracks: {}, settings: {:?}", 
        timeline.tracks.len(), timeline.settings);

    // Parse and apply each operation
    eprintln!("=== STARTING OPERATION APPLICATION ===");
    eprintln!("Number of operations to apply: {}", req.operations.len());
    
    for (i, op_value) in req.operations.iter().enumerate() {
        eprintln!("--- Processing operation {} ---", i);
        eprintln!("Operation value: {:?}", op_value);
        
        let op: TimelineOperation = serde_json::from_value(op_value.clone())
            .map_err(|e| {
                eprintln!("ERROR: Failed to deserialize operation {}: {:?}", i, e);
                eprintln!("Operation value that failed: {:?}", op_value);
                StatusCode::BAD_REQUEST
            })?;
        
        eprintln!("Successfully deserialized operation {}: {:?}", i, op);
        eprintln!("Timeline before applying operation {} - tracks: {}", i, timeline.tracks.len());
        
        timeline.apply_operation(op)
            .map_err(|e| {
                eprintln!("ERROR: Operation {} failed to apply: {}", i, e);
                StatusCode::BAD_REQUEST
            })?;
        
        eprintln!("Timeline after operation {} - tracks: {}", i, timeline.tracks.len());
        if timeline.tracks.len() > 0 {
            eprintln!("First track has {} clips", timeline.tracks[0].clips.len());
        }
    }
    
    eprintln!("=== OPERATION APPLICATION COMPLETE ===");
    
    // Automatically consolidate timeline after operations to ensure all primary clips are on track 1
    timeline.consolidate_timeline();
    eprintln!("Timeline after consolidation - tracks: {}", timeline.tracks.len());

    // Serialize and save updated timeline
    eprintln!("Timeline after all operations - tracks: {}, captions: {}, music: {}, markers: {}", 
        timeline.tracks.len(), timeline.captions.len(), timeline.music.len(), timeline.markers.len());
    
    let updated_timeline_json = serde_json::to_string(&timeline)
        .map_err(|e| {
            eprintln!("Failed to serialize timeline: {:?}", e);
            eprintln!("Timeline structure: tracks={}, captions={}, music={}, markers={}", 
                timeline.tracks.len(), timeline.captions.len(), timeline.music.len(), timeline.markers.len());
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    
    eprintln!("Serialized timeline JSON length: {}", updated_timeline_json.len());
    if updated_timeline_json.len() < 500 {
        eprintln!("Full serialized timeline: {}", updated_timeline_json);
    } else {
        eprintln!("Serialized timeline preview (first 500 chars): {}", &updated_timeline_json[..500]);
    }
    
    // Verify the serialized JSON contains expected fields
    if !updated_timeline_json.contains("\"tracks\"") {
        eprintln!("WARNING: Serialized timeline JSON does not contain 'tracks' field!");
    }
    if !updated_timeline_json.contains("\"settings\"") {
        eprintln!("WARNING: Serialized timeline JSON does not contain 'settings' field!");
    }
    
    db.store_timeline(project_id, &updated_timeline_json)
        .map_err(|e| {
            eprintln!("Failed to store timeline in database: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    // Convert timeline back to JSON Value for response
    // Try direct conversion first (more reliable), fallback to string parsing
    let timeline_value: Value = match serde_json::to_value(&timeline) {
        Ok(value) => {
            eprintln!("Successfully converted timeline to Value directly");
            // Verify the value is not empty
            if let Some(obj) = value.as_object() {
                if obj.is_empty() {
                    eprintln!("WARNING: Direct conversion produced empty object! Trying string parse fallback...");
                    // Fallback to string parsing
                    serde_json::from_str(&updated_timeline_json)
                        .map_err(|parse_err| {
                            eprintln!("String parse also failed: {:?}", parse_err);
                            eprintln!("JSON string: {}", updated_timeline_json);
                            StatusCode::INTERNAL_SERVER_ERROR
                        })?
                } else {
                    value
                }
            } else {
                eprintln!("WARNING: Direct conversion did not produce an object! Trying string parse fallback...");
                serde_json::from_str(&updated_timeline_json)
                    .map_err(|parse_err| {
                        eprintln!("String parse failed: {:?}", parse_err);
                        StatusCode::INTERNAL_SERVER_ERROR
                    })?
            }
        }
        Err(e) => {
            eprintln!("Direct conversion failed: {:?}, trying string parse", e);
            // Fallback: parse from the JSON string we just created
            serde_json::from_str(&updated_timeline_json)
                .map_err(|parse_err| {
                    eprintln!("Both direct conversion and string parsing failed!");
                    eprintln!("Direct conversion error: {:?}", e);
                    eprintln!("String parse error: {:?}", parse_err);
                    eprintln!("JSON string length: {}", updated_timeline_json.len());
                    eprintln!("JSON string (first 1000 chars): {}", &updated_timeline_json[..updated_timeline_json.len().min(1000)]);
                    StatusCode::INTERNAL_SERVER_ERROR
                })?
        }
    };

    // Validate the timeline value has all required fields
    if let Some(obj) = timeline_value.as_object() {
        let keys: Vec<_> = obj.keys().collect();
        eprintln!("Timeline value keys: {:?}", keys);
        
        // Verify all required fields are present
        let required_fields = ["settings", "tracks", "captions", "music", "markers"];
        for field in &required_fields {
            if !obj.contains_key(*field) {
                eprintln!("ERROR: Timeline value missing required field: {}", field);
            }
        }
        
        // Log track information
        if let Some(tracks) = obj.get("tracks") {
            if let Some(tracks_array) = tracks.as_array() {
                eprintln!("Tracks in timeline value: {} tracks", tracks_array.len());
                for (i, track) in tracks_array.iter().enumerate() {
                    if let Some(track_obj) = track.as_object() {
                        if let Some(clips) = track_obj.get("clips") {
                            if let Some(clips_array) = clips.as_array() {
                                eprintln!("  Track {}: {} clips", i, clips_array.len());
                            }
                        }
                    }
                }
            } else {
                eprintln!("WARNING: 'tracks' is not an array in timeline value");
            }
        } else {
            eprintln!("ERROR: 'tracks' field missing from timeline value");
        }
    } else {
        eprintln!("ERROR: Timeline value is not an object!");
        // Return error if timeline value is not properly structured
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    // Final validation: ensure timeline has all required fields before returning
    if let Some(obj) = timeline_value.as_object() {
        let has_settings = obj.contains_key("settings");
        let has_tracks = obj.contains_key("tracks");
        let has_captions = obj.contains_key("captions");
        let has_music = obj.contains_key("music");
        let has_markers = obj.contains_key("markers");
        
        if !has_settings || !has_tracks || !has_captions || !has_music || !has_markers {
            eprintln!("ERROR: Timeline value missing required fields - settings: {}, tracks: {}, captions: {}, music: {}, markers: {}", 
                has_settings, has_tracks, has_captions, has_music, has_markers);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
        
        eprintln!("Timeline response validated - all required fields present");
    }

    Ok(Json(TimelineResponse {
        timeline: timeline_value,
    }))
}

/// Internal helper: apply operations to timeline (used by orchestrator)
pub fn apply_ops_to_timeline(
    db: &Database,
    project_id: i64,
    operations: Vec<TimelineOperation>,
    is_new_version: bool,
) -> Result<Timeline, anyhow::Error> {
    // Load timeline from database
    let timeline_json = db.get_timeline(project_id)?;

    // Deserialize timeline or create default
    let mut timeline: Timeline = if let Some(json_str) = timeline_json {
        serde_json::from_str::<Timeline>(&json_str)
            .unwrap_or_else(|_| {
                // Create default timeline if deserialization fails
                let settings = ProjectSettings {
                    fps: 30.0,
                    resolution: Resolution {
                        width: 1920,
                        height: 1080,
                    },
                    sample_rate: 48000,
                    ticks_per_second: TICKS_PER_SECOND,
                };
                Timeline::new(settings)
            })
    } else {
        // Create default timeline if none exists
        let settings = ProjectSettings {
            fps: 30.0,
            resolution: Resolution {
                width: 1920,
                height: 1080,
            },
            sample_rate: 48000,
            ticks_per_second: TICKS_PER_SECOND,
        };
        Timeline::new(settings)
    };

    // Apply each operation
    for op in operations {
        timeline.apply_operation(op)
            .map_err(|e| anyhow::anyhow!("Failed to apply operation: {}", e))?;
    }

    // Consolidate timeline to ensure contiguity
    timeline.consolidate_timeline();

    // Serialize and save updated timeline
    let updated_timeline_json = serde_json::to_string(&timeline)?;
    
    // Get parent version ID if creating new version
    let parent_version_id = if is_new_version {
        // Get current version ID
        let conn = db.conn.lock().unwrap();
        let result: Result<String, rusqlite::Error> = conn.query_row(
            "SELECT version_id FROM timeline_versions WHERE project_id = ?1 AND is_current = 1",
            params![project_id],
            |row| row.get(0),
        );
        result.ok()
    } else {
        None
    };
    
    db.store_timeline_version(project_id, &updated_timeline_json, parent_version_id.as_deref(), is_new_version)?;

    Ok(timeline)
}

async fn consolidate_timeline(
    State(db): State<Arc<Database>>,
    Path(project_id): Path<i64>,
) -> Result<Json<TimelineResponse>, StatusCode> {
    // Load timeline from database
    let timeline_json = db
        .get_timeline(project_id)
        .map_err(|e| {
            eprintln!("Failed to get timeline from database: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    // Deserialize timeline or create default
    let mut timeline: Timeline = if let Some(json_str) = timeline_json {
        match serde_json::from_str::<Timeline>(&json_str) {
            Ok(t) => t,
            Err(_) => {
                // Create default timeline if deserialization fails
                let settings = ProjectSettings {
                    fps: 30.0,
                    resolution: Resolution {
                        width: 1920,
                        height: 1080,
                    },
                    sample_rate: 48000,
                    ticks_per_second: TICKS_PER_SECOND,
                };
                Timeline::new(settings)
            }
        }
    } else {
        // Create default timeline if none exists
        let settings = ProjectSettings {
            fps: 30.0,
            resolution: Resolution {
                width: 1920,
                height: 1080,
            },
            sample_rate: 48000,
            ticks_per_second: TICKS_PER_SECOND,
        };
        Timeline::new(settings)
    };
    
    // Consolidate timeline
    timeline.consolidate_timeline();
    
    // Save consolidated timeline
    let updated_timeline_json = serde_json::to_string(&timeline)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    db.store_timeline(project_id, &updated_timeline_json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let timeline_value: Value = serde_json::to_value(&timeline)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    Ok(Json(TimelineResponse { timeline: timeline_value }))
}

async fn consolidate_all_timelines(
    State(db): State<Arc<Database>>,
) -> Result<Json<Value>, StatusCode> {
    // Get all projects
    let projects = db.get_all_projects()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    let total_projects = projects.len();
    let mut consolidated_count = 0;
    
    for project in projects {
        let timeline_json = db
            .get_timeline(project.id)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        if let Some(json_str) = timeline_json {
            if let Ok(mut timeline) = serde_json::from_str::<Timeline>(&json_str) {
                // Consolidate timeline
                timeline.consolidate_timeline();
                
                // Save consolidated timeline
                if let Ok(updated_json) = serde_json::to_string(&timeline) {
                    if db.store_timeline(project.id, &updated_json).is_ok() {
                        consolidated_count += 1;
                    }
                }
            }
        }
    }
    
    Ok(Json(json!({
        "success": true,
        "consolidated_count": consolidated_count,
        "total_projects": total_projects
    })))
}

async fn log_diff(
    State(_db): State<Arc<Database>>,
    Path(_project_id): Path<i64>,
    Json(_req): Json<DiffRequest>,
) -> Result<Json<()>, StatusCode> {
    // Placeholder - would generate diff and log to edit_logs table
    Ok(Json(()))
}

// Test endpoint to verify timeline serialization works
async fn test_timeline_serialization() -> Result<Json<Value>, StatusCode> {
    eprintln!("=== TEST: Creating test timeline ===");
    let settings = ProjectSettings {
        fps: 30.0,
        resolution: Resolution {
            width: 1920,
            height: 1080,
        },
        sample_rate: 48000,
        ticks_per_second: TICKS_PER_SECOND,
    };
    let timeline = Timeline::new(settings);
    
    eprintln!("Test timeline - tracks: {}, captions: {}, music: {}, markers: {}", 
        timeline.tracks.len(), timeline.captions.len(), timeline.music.len(), timeline.markers.len());
    
    let json_str = serde_json::to_string(&timeline)
        .map_err(|e| {
            eprintln!("TEST FAILED: Serialization error: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    
    eprintln!("Test timeline JSON length: {}", json_str.len());
    eprintln!("Test timeline JSON: {}", json_str);
    
    let value = serde_json::to_value(&timeline)
        .map_err(|e| {
            eprintln!("TEST FAILED: to_value error: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    
    eprintln!("Test timeline value keys: {:?}", value.as_object().map(|o| o.keys().collect::<Vec<_>>()));
    eprintln!("=== TEST COMPLETE ===");
    
    Ok(Json(value))
}
