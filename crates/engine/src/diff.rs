use crate::timeline::Timeline;
use serde_json::Value;

pub fn generate_diff(from: &Timeline, to: &Timeline) -> Value {
    // Simplified diff generation - in production, would generate a detailed diff JSON
    // For now, return a placeholder structure
    serde_json::json!({
        "type": "timeline_diff",
        "tracks_changed": to.tracks.len() != from.tracks.len(),
        "clips_changed": true, // Placeholder
        "captions_changed": to.captions.len() != from.captions.len(),
        "music_changed": to.music.len() != from.music.len(),
    })
}
