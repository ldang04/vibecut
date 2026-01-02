use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::time::Duration;

const TWELVELABS_API_BASE: &str = "https://api.twelvelabs.io/v1.3";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStatus {
    pub status: String, // "pending" | "ready" | "failed"
    pub video_id: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub video_id: String,
    pub start: f64, // seconds
    pub end: f64,   // seconds
    pub score: f64,
    pub transcript: Option<String>,
}

/// Get API key from environment
fn get_api_key() -> Result<String> {
    std::env::var("TWELVELABS_API_KEY")
        .map_err(|_| anyhow::anyhow!("TWELVELABS_API_KEY environment variable not set"))
}

/// Create a per-project index
pub async fn create_index(project_id: i64, index_name: Option<String>) -> Result<String> {
    let api_key = get_api_key()?;
    let name = index_name.unwrap_or_else(|| format!("vibecut-project-{}", project_id));
    
    let client = reqwest::Client::new();
    let response = client
        .post(&format!("{}/indexes", TWELVELABS_API_BASE))
        .header("x-api-key", &api_key)
        .json(&serde_json::json!({
            "index_name": name,
            "engines": [
                {
                    "engine_name": "marengo2.7",
                    "engine_options": ["visual", "audio", "conversation", "text_in_video", "logo"]
                }
            ]
        }))
        .send()
        .await?;
    
    let status = response.status();
    if status.is_success() {
        let result: serde_json::Value = response.json().await?;
        if let Some(index_id) = result.get("_id").and_then(|v| v.as_str()) {
            Ok(index_id.to_string())
        } else {
            Err(anyhow::anyhow!("Invalid response format: missing _id"))
        }
    } else {
        let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
        Err(anyhow::anyhow!("TwelveLabs API error: {} - {}", status, error_text))
    }
}

/// Create a task to upload and index a video
pub async fn create_task_upload(index_id: &str, video_path: &str) -> Result<String> {
    let api_key = get_api_key()?;
    
    // For now, we'll use video_url. In production, you might want to upload the file directly
    // This assumes the video is accessible via HTTP URL
    let video_url = if video_path.starts_with("http://") || video_path.starts_with("https://") {
        video_path.to_string()
    } else {
        // For local files, we'd need to upload them or serve them via a proxy
        // For now, return an error indicating we need a URL
        return Err(anyhow::anyhow!("Local file paths not yet supported. Video must be accessible via HTTP URL"));
    };
    
    let client = reqwest::Client::new();
    let response = client
        .post(&format!("{}/tasks", TWELVELABS_API_BASE))
        .header("x-api-key", &api_key)
        .json(&serde_json::json!({
            "index_id": index_id,
            "video_url": video_url
        }))
        .send()
        .await?;
    
    let status = response.status();
    if status.is_success() {
        let result: serde_json::Value = response.json().await?;
        if let Some(task_id) = result.get("_id").and_then(|v| v.as_str()) {
            Ok(task_id.to_string())
        } else {
            Err(anyhow::anyhow!("Invalid response format: missing _id"))
        }
    } else {
        let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
        Err(anyhow::anyhow!("TwelveLabs API error: {} - {}", status, error_text))
    }
}

/// Get task status (for polling)
pub async fn get_task_status(task_id: &str) -> Result<TaskStatus> {
    let api_key = get_api_key()?;
    
    let client = reqwest::Client::new();
    let response = client
        .get(&format!("{}/tasks/{}", TWELVELABS_API_BASE, task_id))
        .header("x-api-key", &api_key)
        .send()
        .await?;
    
    let status_code = response.status();
    if status_code.is_success() {
        let result: serde_json::Value = response.json().await?;
        
        let status = result.get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        
        let video_id = result.get("video_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        
        let error = result.get("error")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        
        Ok(TaskStatus {
            status,
            video_id,
            error,
        })
    } else {
        let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
        Err(anyhow::anyhow!("TwelveLabs API error: {} - {}", status_code, error_text))
    }
}

/// Search for matching moments in an index
pub async fn search(
    index_id: &str,
    query_text: &str,
    limit: usize,
) -> Result<Vec<SearchResult>> {
    let api_key = get_api_key()?;
    
    let client = reqwest::Client::new();
    let response = client
        .post(&format!("{}/search", TWELVELABS_API_BASE))
        .header("x-api-key", &api_key)
        .timeout(Duration::from_secs(10))
        .json(&serde_json::json!({
            "query": query_text,
            "index_id": index_id,
            "search_options": ["visual", "audio", "conversation", "text_in_video"],
            "filter": {},
            "threshold": 0.5,
            "limit": limit
        }))
        .send()
        .await?;
    
    let status_code = response.status();
    if status_code.is_success() {
        let result: serde_json::Value = response.json().await?;
        
        let mut search_results = Vec::new();
        
        if let Some(data) = result.get("data").and_then(|v| v.as_array()) {
            for item in data {
                if let Some(video_id) = item.get("video_id").and_then(|v| v.as_str()) {
                    if let Some(matches) = item.get("matches").and_then(|v| v.as_array()) {
                        for m in matches {
                            if let (Some(start), Some(end), Some(score)) = (
                                m.get("start").and_then(|v| v.as_f64()),
                                m.get("end").and_then(|v| v.as_f64()),
                                m.get("score").and_then(|v| v.as_f64()),
                            ) {
                                let transcript = m.get("transcript")
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string());
                                
                                search_results.push(SearchResult {
                                    video_id: video_id.to_string(),
                                    start,
                                    end,
                                    score,
                                    transcript,
                                });
                            }
                        }
                    }
                }
            }
        }
        
        Ok(search_results)
    } else {
        let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
        Err(anyhow::anyhow!("TwelveLabs API error: {} - {}", status_code, error_text))
    }
}

