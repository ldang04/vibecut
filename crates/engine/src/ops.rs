use crate::timeline::*;

#[derive(Debug, Clone)]
pub enum TimelineOperation {
    SplitClip { clip_id: usize, position_ticks: i64 },
    TrimClip {
        clip_id: usize,
        new_in_ticks: i64,
        new_out_ticks: i64,
    },
    RippleDelete { clip_id: usize },
    InsertClip {
        asset_id: i64,
        position_ticks: i64,
        track_id: i64,
    },
    MoveClip {
        clip_id: usize,
        new_position_ticks: i64,
    },
}

impl Timeline {
    pub fn apply_operation(&mut self, op: TimelineOperation) -> Result<(), String> {
        match op {
            TimelineOperation::SplitClip {
                clip_id,
                position_ticks,
            } => {
                // Find the clip across all tracks
                for track in &mut self.tracks {
                    if clip_id < track.clips.len() {
                        let clip = &mut track.clips[clip_id];
                        if position_ticks > clip.timeline_start_ticks
                            && position_ticks < clip.timeline_start_ticks + (clip.out_ticks - clip.in_ticks)
                        {
                            // Split the clip
                            let relative_pos = position_ticks - clip.timeline_start_ticks;
                            let split_in = clip.in_ticks + relative_pos;

                            let new_clip = ClipInstance {
                                asset_id: clip.asset_id,
                                in_ticks: split_in,
                                out_ticks: clip.out_ticks,
                                timeline_start_ticks: position_ticks,
                                speed: clip.speed,
                                track_id: clip.track_id,
                            };

                            clip.out_ticks = split_in;
                            track.clips.insert(clip_id + 1, new_clip);
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
                    if clip_id < track.clips.len() {
                        let clip = &mut track.clips[clip_id];
                        clip.in_ticks = new_in_ticks;
                        clip.out_ticks = new_out_ticks;
                        return Ok(());
                    }
                }
                Err("Clip not found".to_string())
            }
            TimelineOperation::RippleDelete { clip_id } => {
                for track in &mut self.tracks {
                    if clip_id < track.clips.len() {
                        track.clips.remove(clip_id);
                        return Ok(());
                    }
                }
                Err("Clip not found".to_string())
            }
            TimelineOperation::InsertClip {
                asset_id,
                position_ticks,
                track_id,
            } => {
                let track = self.tracks.iter_mut().find(|t| t.id == track_id);
                if let Some(track) = track {
                    // Simple insertion - in production, would need to handle overlaps
                    let clip = ClipInstance {
                        asset_id,
                        in_ticks: 0,
                        out_ticks: 1000 * TICKS_PER_SECOND, // Placeholder
                        timeline_start_ticks: position_ticks,
                        speed: 1.0,
                        track_id,
                    };
                    track.clips.push(clip);
                    Ok(())
                } else {
                    Err("Track not found".to_string())
                }
            }
            TimelineOperation::MoveClip {
                clip_id,
                new_position_ticks,
            } => {
                for track in &mut self.tracks {
                    if clip_id < track.clips.len() {
                        track.clips[clip_id].timeline_start_ticks = new_position_ticks;
                        return Ok(());
                    }
                }
                Err("Clip not found".to_string())
            }
        }
    }
}
