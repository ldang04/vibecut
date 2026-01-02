use anyhow::Result;
use serde_json::{self, json};
use std::sync::Arc;

use crate::api::orchestrator::check_project_preconditions;
use crate::db::Database;
use crate::jobs::JobManager;
use crate::llm;

pub struct AgentContext {
    project_id: i64,
    db: Arc<Database>,
    job_manager: Arc<JobManager>,
}

impl AgentContext {
    pub fn new(project_id: i64, db: Arc<Database>, job_manager: Arc<JobManager>) -> Self {
        AgentContext {
            project_id,
            db,
            job_manager,
        }
    }

    /// Handle an event and generate proactive LLM message
    /// Control flow decisions remain deterministic in the orchestrator
    pub async fn handle_event(
        &self,
        event_type: &str,
        event_data: &serde_json::Value,
    ) -> Result<()> {
        // Get conversation history
        let history = self.db.get_orchestrator_messages(self.project_id, 20)?;
        
        // Get project state
        let state = check_project_preconditions(&self.db, self.project_id)?;
        
        // Get current goal if exists
        let goal = self.db.get_active_orchestrator_goals(self.project_id)?
            .first()
            .cloned();
        
        // Construct project state JSON
        let project_state_json = json!({
            "media_assets_count": state.media_assets_count,
            "segments_count": state.segments_count,
            "segments_with_text_embeddings": state.segments_with_text_embeddings,
            "segments_with_vision_embeddings": state.segments_with_vision_embeddings,
            "embedding_coverage": state.embedding_coverage,
            "jobs_running_count": state.jobs_running_count,
            "jobs_failed_count": state.jobs_failed_count,
        });
        
        // Construct context - include event data for proactive messages
        let mut context = json!({
            "event_type": event_type,
            "event_data": event_data,
            "goal": goal.as_ref().map(|(_, intent, status)| json!({
                "intent": intent,
                "status": status,
            })),
            "user_intent": goal.as_ref().map(|(_, intent, _)| intent),
        });
        
        // If event_data contains goal info, use it
        if let Some(event_data_obj) = event_data.as_object() {
            if let Some(goal_intent) = event_data_obj.get("user_intent").and_then(|v| v.as_str()) {
                if let Some(context_obj) = context.as_object_mut() {
                    context_obj.insert("user_intent".to_string(), json!(goal_intent));
                }
            }
        }
        
        // Generate LLM response (message only, no control decisions)
        let response = llm::generate_agent_response(
            &history,
            &project_state_json,
            &context,
            event_type,
        ).await?;
        
        // Store assistant message
        let message = response.get("message").and_then(|v| v.as_str()).unwrap_or("");
        self.db.store_orchestrator_message(
            self.project_id,
            "assistant",
            message,
            Some(&response),
        )?;
        
        // Control flow decisions are made deterministically by the orchestrator state machine
        // LLM only generates messages, not control decisions
        
        Ok(())
    }
}
