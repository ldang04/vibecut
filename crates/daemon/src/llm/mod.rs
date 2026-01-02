use anyhow::Result;
use reqwest;
use serde_json;

const ML_SERVICE_URL: &str = "http://127.0.0.1:8001";

/// Embed text using the ML service /embeddings/text endpoint
/// Returns a 384-dimensional vector (all-MiniLM-L6-v2)
pub async fn embed_text(text: &str) -> Result<Vec<f32>> {
    let client = reqwest::Client::new();
    let response = client
        .post(&format!("{}/embeddings/text", ML_SERVICE_URL))
        .json(&serde_json::json!({
            "text": text
        }))
        .send()
        .await?;
    
    if response.status().is_success() {
        let embedding_response: serde_json::Value = response.json().await?;
        if let Some(embedding_vec) = embedding_response.get("embedding")
            .and_then(|e| e.as_array())
        {
            let embedding: Vec<f32> = embedding_vec.iter()
                .filter_map(|v| v.as_f64().map(|f| f as f32))
                .collect();
            Ok(embedding)
        } else {
            Err(anyhow::anyhow!("Invalid response format from ML service"))
        }
    } else {
        Err(anyhow::anyhow!("ML service returned error: {}", response.status()))
    }
}

/// Call the orchestrator reason endpoint (placeholder for now)
pub async fn reason_narrative(
    segments: &[serde_json::Value],
    style_profile: Option<&serde_json::Value>,
    timeline_context: Option<&serde_json::Value>,
) -> Result<serde_json::Value> {
    let client = reqwest::Client::new();
    let mut request_body = serde_json::json!({
        "segments": segments,
    });
    
    if let Some(profile) = style_profile {
        request_body["style_profile"] = profile.clone();
    }
    if let Some(context) = timeline_context {
        request_body["timeline_context"] = context.clone();
    }
    
    let response = client
        .post(&format!("{}/orchestrator/reason", ML_SERVICE_URL))
        .json(&request_body)
        .send()
        .await?;
    
    if response.status().is_success() {
        Ok(response.json().await?)
    } else {
        Err(anyhow::anyhow!("ML service returned error: {}", response.status()))
    }
}

/// Generate EditPlan from beats and constraints
pub async fn generate_edit_plan(
    narrative_structure: &str,
    beats: &serde_json::Value, // JSON array of beats
    constraints: &serde_json::Value,
    style_profile_id: Option<i64>,
) -> Result<serde_json::Value> {
    let client = reqwest::Client::new();
    let mut request_body = serde_json::json!({
        "beats": beats,
        "constraints": constraints,
        "narrative_structure": narrative_structure,
    });
    
    if let Some(profile_id) = style_profile_id {
        request_body["style_profile_id"] = serde_json::json!(profile_id);
    }
    
    let response = client
        .post(&format!("{}/orchestrator/generate_plan", ML_SERVICE_URL))
        .json(&request_body)
        .send()
        .await?;
    
    if response.status().is_success() {
        Ok(response.json().await?)
    } else {
        Err(anyhow::anyhow!("ML service returned error: {}", response.status()))
    }
}

/// Generate agent response using LLM
pub async fn generate_agent_response(
    conversation_history: &[serde_json::Value],
    project_state: &serde_json::Value,
    context: &serde_json::Value,
    event_type: &str,
) -> Result<serde_json::Value> {
    let client = reqwest::Client::new();
    let request_body = serde_json::json!({
        "conversation_history": conversation_history,
        "project_state": project_state,
        "context": context,
        "event_type": event_type,
    });
    
    let response = client
        .post(&format!("{}/orchestrator/generate_response", ML_SERVICE_URL))
        .json(&request_body)
        .send()
        .await?;
    
    let status = response.status();
    if status.is_success() {
        Ok(response.json().await?)
    } else {
        let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
        eprintln!("[ERROR] ML service returned error {}: {}", status, error_text);
        Err(anyhow::anyhow!("ML service returned error {}: {}", status, error_text))
    }
}

/// Parse user intent from natural language using LLM
pub async fn parse_intent(
    user_message: &str,
    conversation_history: Option<&[serde_json::Value]>,
) -> Result<serde_json::Value> {
    let client = reqwest::Client::new();
    let mut request_body = serde_json::json!({
        "user_message": user_message,
    });
    
    if let Some(history) = conversation_history {
        request_body["conversation_history"] = serde_json::json!(history);
    }
    
    let response = client
        .post(&format!("{}/orchestrator/parse_intent", ML_SERVICE_URL))
        .json(&request_body)
        .send()
        .await?;
    
    if response.status().is_success() {
        Ok(response.json().await?)
    } else {
        Err(anyhow::anyhow!("ML service returned error: {}", response.status()))
    }
}
