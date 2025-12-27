use crate::timeline::*;

// EditPlan structure (simplified - full definition would match edit_plan.json schema)
pub struct EditPlan {
    pub sections: Vec<EditSection>,
    pub constraints: EditConstraints,
}

pub struct EditSection {
    pub section_type: String,
    pub target_duration: i64,
    pub events: Vec<EditEvent>,
}

pub enum EditEvent {
    Clip {
        asset_id: i64,
        in_ticks: i64,
        out_ticks: i64,
        timeline_start_ticks: i64,
        track_id: i64,
    },
    Caption {
        text: String,
        template_id: Option<i64>,
        start_ticks: i64,
        end_ticks: i64,
    },
    Music {
        track_path: String,
        ducking_profile_id: Option<i64>,
        start_ticks: i64,
        end_ticks: i64,
    },
}

pub struct EditConstraints {
    pub target_length: Option<i64>,
    pub vibe: Option<String>,
    pub captions_on: bool,
    pub music_on: bool,
}

pub fn compile_edit_plan(plan: EditPlan, settings: ProjectSettings) -> Timeline {
    let mut timeline = Timeline::new(settings);

    // Create tracks (one video track, one b-roll track, one audio bed track)
    let video_track = Track {
        id: 1,
        kind: TrackKind::Video,
        clips: Vec::new(),
    };
    let broll_track = Track {
        id: 2,
        kind: TrackKind::Video,
        clips: Vec::new(),
    };
    let audio_track = Track {
        id: 3,
        kind: TrackKind::Audio,
        clips: Vec::new(),
    };

    let mut tracks = vec![video_track, broll_track, audio_track];

    // Process each section and compile events
    for section in plan.sections {
        for event in section.events {
            match event {
                EditEvent::Clip {
                    asset_id,
                    in_ticks,
                    out_ticks,
                    timeline_start_ticks,
                    track_id,
                } => {
                    let track = tracks.iter_mut().find(|t| t.id == track_id);
                    if let Some(track) = track {
                        track.clips.push(ClipInstance {
                            asset_id,
                            in_ticks,
                            out_ticks,
                            timeline_start_ticks,
                            speed: 1.0,
                            track_id,
                        });
                    }
                }
                EditEvent::Caption {
                    text,
                    template_id,
                    start_ticks,
                    end_ticks,
                } => {
                    timeline.captions.push(CaptionEvent {
                        start_ticks,
                        end_ticks,
                        text,
                        template_id,
                    });
                }
                EditEvent::Music {
                    track_path,
                    ducking_profile_id,
                    start_ticks,
                    end_ticks,
                } => {
                    timeline.music.push(MusicEvent {
                        start_ticks,
                        end_ticks,
                        track_path,
                        ducking_profile_id,
                    });
                }
            }
        }
    }

    timeline.tracks = tracks;
    timeline
}
