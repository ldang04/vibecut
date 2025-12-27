use serde::{Deserialize, Serialize};

pub const TICKS_PER_SECOND: i64 = 48000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSettings {
    pub fps: f64,
    pub resolution: Resolution,
    pub sample_rate: i32,
    #[serde(default = "default_ticks_per_second")]
    pub ticks_per_second: i64,
}

fn default_ticks_per_second() -> i64 {
    TICKS_PER_SECOND
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resolution {
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaAssetRef {
    pub id: i64,
    pub path: String,
    pub duration_ticks: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipInstance {
    pub asset_id: i64,
    pub in_ticks: i64,
    pub out_ticks: i64,
    pub timeline_start_ticks: i64,
    pub speed: f64,
    pub track_id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TrackKind {
    Video,
    Audio,
    Caption,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub id: i64,
    pub kind: TrackKind,
    pub clips: Vec<ClipInstance>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptionEvent {
    pub start_ticks: i64,
    pub end_ticks: i64,
    pub text: String,
    pub template_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MusicEvent {
    pub start_ticks: i64,
    pub end_ticks: i64,
    pub track_path: String,
    pub ducking_profile_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Marker {
    pub position_ticks: i64,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Timeline {
    pub settings: ProjectSettings,
    pub tracks: Vec<Track>,
    pub captions: Vec<CaptionEvent>,
    pub music: Vec<MusicEvent>,
    pub markers: Vec<Marker>,
}

impl Timeline {
    pub fn new(settings: ProjectSettings) -> Self {
        Timeline {
            settings,
            tracks: Vec::new(),
            captions: Vec::new(),
            music: Vec::new(),
            markers: Vec::new(),
        }
    }
}
