use anyhow::Result;
use serde_json;
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use tracing::{info, warn};

use crate::db::Database;
use crate::jobs::{JobEvent, JobManager};
use crate::orchestrator::agent::AgentContext;

/// Agent event loop that handles events and takes autonomous actions
pub async fn agent_event_loop(db: Arc<Database>, job_manager: Arc<JobManager>) {
    let mut rx = job_manager.subscribe();
    
    info!("[Agent] Event loop started");
    
    loop {
        match rx.recv().await {
            Ok(event) => {
                match event {
                    JobEvent::AnalysisComplete { project_id, .. } => {
                        // Generate proactive LLM message when analysis completes
                        let context = AgentContext::new(project_id, db.clone(), job_manager.clone());
                        
                        // Check if there's a goal waiting for analysis
                        if let Ok(Some((goal_id, user_intent))) = db.get_orchestrator_goal_by_status(project_id, "ready_to_propose") {
                            // Generate proactive message using LLM
                            if let Err(e) = context.handle_event("analysis_complete", &serde_json::json!({
                                "goal_id": goal_id,
                                "user_intent": user_intent,
                            })).await {
                                eprintln!("[Agent] Error generating proactive message: {:?}", e);
                            }
                        } else {
                            // No goal waiting, but still generate a general message
                            if let Err(e) = context.handle_event("analysis_complete", &serde_json::json!({})).await {
                                eprintln!("[Agent] Error generating proactive message: {:?}", e);
                            }
                        }
                    }
                    JobEvent::JobCompleted { job_id, job_type, asset_id: _, .. } => {
                        // Job completion events - could generate messages for specific job types
                        // For now, focus on AnalysisComplete which is more useful
                    }
                    _ => {
                        // Ignore other events for now
                    }
                }
            }
            Err(_) => {
                // Receiver closed or lagged - reconnect
                warn!("[Agent] Event receiver closed, reconnecting...");
                sleep(Duration::from_secs(1)).await;
                rx = job_manager.subscribe();
            }
        }
    }
}

