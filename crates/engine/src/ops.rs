use crate::timeline::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum TimelineOperation {
    SplitClip { clip_id: String, position_ticks: i64 },
    TrimClip {
        clip_id: String,
        new_in_ticks: i64,
        new_out_ticks: i64,
    },
    DeleteClip { clip_id: String },
    InsertClip {
        asset_id: i64,
        position_ticks: i64,
        track_id: i64,
        duration_ticks: i64,
    },
    MoveClip {
        clip_id: String,
        new_position_ticks: i64,
    },
    MoveClipToTrack {
        clip_id: String,
        new_track_id: i64,
    },
    RippleInsertClip {
        asset_id: i64,
        position_ticks: i64,
        duration_ticks: i64,
    },
    OverwriteClip {
        asset_id: i64,
        position_ticks: i64,
        duration_ticks: i64,
    },
    InsertLayeredClip {
        asset_id: i64,
        position_ticks: i64,
        duration_ticks: i64,
        base_track_id: i64,
    },
    ClearTimeline,
}

impl Timeline {
    pub fn apply_operation(&mut self, op: TimelineOperation) -> Result<(), String> {
        match op {
            TimelineOperation::SplitClip {
                clip_id,
                position_ticks,
            } => {
                // Find the clip across all tracks by UUID
                for track in &mut self.tracks {
                    if let Some(clip_index) = track.clips.iter().position(|c| c.id == clip_id) {
                        let clip = &mut track.clips[clip_index];
                        if position_ticks > clip.timeline_start_ticks
                            && position_ticks < clip.timeline_start_ticks + (clip.out_ticks - clip.in_ticks)
                        {
                            // Split the clip
                            let relative_pos = position_ticks - clip.timeline_start_ticks;
                            let split_in = clip.in_ticks + relative_pos;

                            let new_clip = ClipInstance {
                                id: uuid::Uuid::new_v4().to_string(),
                                asset_id: clip.asset_id,
                                in_ticks: split_in,
                                out_ticks: clip.out_ticks,
                                timeline_start_ticks: position_ticks,
                                speed: clip.speed,
                                track_id: clip.track_id,
                            };

                            clip.out_ticks = split_in;
                            track.clips.insert(clip_index + 1, new_clip);
                            return Ok(());
                        }
                    }
                }
                Err("Clip not found or position invalid".to_string())
            }
            TimelineOperation::TrimClip {
                clip_id,
                new_in_ticks,
                new_out_ticks,
            } => {
                for track in &mut self.tracks {
                    if let Some(clip) = track.clips.iter_mut().find(|c| c.id == clip_id) {
                        clip.in_ticks = new_in_ticks;
                        clip.out_ticks = new_out_ticks;
                        return Ok(());
                    }
                }
                Err("Clip not found".to_string())
            }
            TimelineOperation::DeleteClip { clip_id } => {
                for track in &mut self.tracks {
                    if let Some(clip_index) = track.clips.iter().position(|c| c.id == clip_id) {
                        track.clips.remove(clip_index);
                        return Ok(());
                    }
                }
                Err("Clip not found".to_string())
            }
            TimelineOperation::InsertClip {
                asset_id,
                position_ticks,
                track_id,
                duration_ticks,
            } => {
                // Find or create track
                let track = if let Some(t) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                    t
                } else {
                    // Create new track if it doesn't exist
                    let new_track = Track {
                        id: track_id,
                        kind: TrackKind::Video,
                        clips: Vec::new(),
                    };
                    self.tracks.push(new_track);
                    self.tracks.last_mut().unwrap()
                };

                let clip = ClipInstance {
                    id: uuid::Uuid::new_v4().to_string(),
                    asset_id,
                    in_ticks: 0,
                    out_ticks: duration_ticks,
                    timeline_start_ticks: position_ticks,
                    speed: 1.0,
                    track_id,
                };
                track.clips.push(clip);
                Ok(())
            }
            TimelineOperation::MoveClip {
                clip_id,
                new_position_ticks,
            } => {
                for track in &mut self.tracks {
                    if let Some(clip) = track.clips.iter_mut().find(|c| c.id == clip_id) {
                        clip.timeline_start_ticks = new_position_ticks;
                        return Ok(());
                    }
                }
                Err("Clip not found".to_string())
            }
            TimelineOperation::MoveClipToTrack {
                clip_id,
                new_track_id,
            } => {
                // Find the clip and remove it from current track
                let mut clip_to_move: Option<ClipInstance> = None;
                for track in &mut self.tracks {
                    if let Some(clip_index) = track.clips.iter().position(|c| c.id == clip_id) {
                        clip_to_move = Some(track.clips.remove(clip_index));
                        break;
                    }
                }

                if let Some(mut clip) = clip_to_move {
                    // Find or create the target track
                    let target_track = if let Some(t) = self.tracks.iter_mut().find(|t| t.id == new_track_id) {
                        t
                    } else {
                        // Create new track if it doesn't exist
                        let new_track = Track {
                            id: new_track_id,
                            kind: TrackKind::Video,
                            clips: Vec::new(),
                        };
                        self.tracks.push(new_track);
                        self.tracks.last_mut().unwrap()
                    };

                    clip.track_id = new_track_id;
                    target_track.clips.push(clip);
                    Ok(())
                } else {
                    Err("Clip not found".to_string())
                }
            }
            TimelineOperation::RippleInsertClip {
                asset_id,
                position_ticks,
                duration_ticks,
            } => {
                // Find primary storyline track (track with id == 1, or first track if no track 1)
                let primary_track = if let Some(t) = self.tracks.iter_mut().find(|t| t.id == 1) {
                    t
                } else if let Some(t) = self.tracks.first_mut() {
                    t
                } else {
                    // No tracks exist, create primary track
                    let new_track = Track {
                        id: 1,
                        kind: TrackKind::Video,
                        clips: Vec::new(),
                    };
                    self.tracks.push(new_track);
                    self.tracks.last_mut().unwrap()
                };

                // Find all clips that start at or after the insertion point
                // Shift them right by duration_ticks
                for clip in &mut primary_track.clips {
                    if clip.timeline_start_ticks >= position_ticks {
                        clip.timeline_start_ticks += duration_ticks;
                    }
                }

                // Insert new clip at position_ticks
                let new_clip = ClipInstance {
                    id: uuid::Uuid::new_v4().to_string(),
                    asset_id,
                    in_ticks: 0,
                    out_ticks: duration_ticks,
                    timeline_start_ticks: position_ticks,
                    speed: 1.0,
                    track_id: primary_track.id,
                };

                // Insert clip in sorted order by timeline_start_ticks
                let insert_index = primary_track.clips
                    .iter()
                    .position(|c| c.timeline_start_ticks > position_ticks)
                    .unwrap_or(primary_track.clips.len());
                primary_track.clips.insert(insert_index, new_clip);

                Ok(())
            }
            TimelineOperation::OverwriteClip {
                asset_id,
                position_ticks,
                duration_ticks,
            } => {
                // Find primary storyline track
                let primary_track = if let Some(t) = self.tracks.iter_mut().find(|t| t.id == 1) {
                    t
                } else if let Some(t) = self.tracks.first_mut() {
                    t
                } else {
                    // No tracks exist, create primary track
                    let new_track = Track {
                        id: 1,
                        kind: TrackKind::Video,
                        clips: Vec::new(),
                    };
                    self.tracks.push(new_track);
                    self.tracks.last_mut().unwrap()
                };

                let insert_end_ticks = position_ticks + duration_ticks;

                // Remove or trim clips that overlap with the insertion range
                primary_track.clips.retain_mut(|clip| {
                    let clip_end_ticks = clip.timeline_start_ticks + (clip.out_ticks - clip.in_ticks);
                    
                    // Check for overlap
                    if position_ticks < clip_end_ticks && insert_end_ticks > clip.timeline_start_ticks {
                        // Clip overlaps - check if it should be removed or trimmed
                        if position_ticks <= clip.timeline_start_ticks && insert_end_ticks >= clip_end_ticks {
                            // Completely covered - remove
                            return false;
                        } else if position_ticks > clip.timeline_start_ticks && insert_end_ticks < clip_end_ticks {
                            // Insertion is in the middle - split the clip (keep left part, right part handled separately)
                            clip.out_ticks = clip.in_ticks + (position_ticks - clip.timeline_start_ticks);
                            return true;
                        } else if position_ticks <= clip.timeline_start_ticks {
                            // Overlaps from the left - trim start
                            let trim_amount = insert_end_ticks - clip.timeline_start_ticks;
                            clip.timeline_start_ticks = insert_end_ticks;
                            clip.in_ticks += trim_amount;
                            return clip.out_ticks > clip.in_ticks; // Keep if still has duration
                        } else {
                            // Overlaps from the right - trim end
                            clip.out_ticks = clip.in_ticks + (position_ticks - clip.timeline_start_ticks);
                            return clip.out_ticks > clip.in_ticks; // Keep if still has duration
                        }
                    }
                    true // Keep clip if no overlap
                });

                // Insert new clip
                let new_clip = ClipInstance {
                    id: uuid::Uuid::new_v4().to_string(),
                    asset_id,
                    in_ticks: 0,
                    out_ticks: duration_ticks,
                    timeline_start_ticks: position_ticks,
                    speed: 1.0,
                    track_id: primary_track.id,
                };

                let insert_index = primary_track.clips
                    .iter()
                    .position(|c| c.timeline_start_ticks > position_ticks)
                    .unwrap_or(primary_track.clips.len());
                primary_track.clips.insert(insert_index, new_clip);

                Ok(())
            }
            TimelineOperation::InsertLayeredClip {
                asset_id,
                position_ticks,
                duration_ticks,
                base_track_id,
            } => {
                // Find base track
                let base_track = self.tracks.iter().find(|t| t.id == base_track_id);
                
                // Determine overlay track ID (base_track_id + 1, or find next available)
                let overlay_track_id = if let Some(_) = base_track {
                    // Find the highest track ID >= base_track_id + 1
                    let max_overlay_id = self.tracks
                        .iter()
                        .filter(|t| t.id > base_track_id)
                        .map(|t| t.id)
                        .max()
                        .unwrap_or(base_track_id);
                    max_overlay_id + 1
                } else {
                    base_track_id + 1
                };

                // Find or create overlay track
                let overlay_track = if let Some(t) = self.tracks.iter_mut().find(|t| t.id == overlay_track_id) {
                    t
                } else {
                    let new_track = Track {
                        id: overlay_track_id,
                        kind: TrackKind::Video,
                        clips: Vec::new(),
                    };
                    self.tracks.push(new_track);
                    self.tracks.last_mut().unwrap()
                };

                // Insert clip on overlay track (allows overlaps)
                let new_clip = ClipInstance {
                    id: uuid::Uuid::new_v4().to_string(),
                    asset_id,
                    in_ticks: 0,
                    out_ticks: duration_ticks,
                    timeline_start_ticks: position_ticks,
                    speed: 1.0,
                    track_id: overlay_track.id,
                };

                // Insert in sorted order
                let insert_index = overlay_track.clips
                    .iter()
                    .position(|c| c.timeline_start_ticks > position_ticks)
                    .unwrap_or(overlay_track.clips.len());
                overlay_track.clips.insert(insert_index, new_clip);

                Ok(())
            }
            TimelineOperation::ClearTimeline => {
                // Clear all clips from all tracks
                for track in &mut self.tracks {
                    track.clips.clear();
                }
                // Also clear captions, music, and markers
                self.captions.clear();
                self.music.clear();
                self.markers.clear();
                Ok(())
            }
        }
    }
}
