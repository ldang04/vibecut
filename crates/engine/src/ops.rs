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
    ReorderClip {
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
    ConvertPrimaryToOverlay {
        clip_id: String,
        position_ticks: i64,
    },
    ConvertOverlayToPrimary {
        clip_id: String,
        position_ticks: i64,
    },
    ConsolidateTimeline,
    ClearTimeline,
}

impl Timeline {
    /// Ensures the primary timeline (track 1) is contiguous with no gaps
    /// Packs all clips together starting from 0, removing any gaps
    fn repack_primary_timeline(&mut self) {
        if let Some(primary_track) = self.tracks.iter_mut().find(|t| t.id == 1) {
            // Sort clips by timeline_start_ticks
            primary_track.clips.sort_by_key(|c| c.timeline_start_ticks);
            
            // Repack clips contiguously starting from 0
            let mut current_time = 0i64;
            for clip in &mut primary_track.clips {
                clip.timeline_start_ticks = current_time;
                current_time += clip.out_ticks - clip.in_ticks;
            }
        }
    }

    /// Find the first available overlay lane that has no clip overlapping with the insertion time range
    /// Returns the track ID to use for the overlay clip
    fn find_available_overlay_lane(
        &self,
        base_track_id: i64,
        position_ticks: i64,
        duration_ticks: i64,
    ) -> i64 {
        let insert_end_ticks = position_ticks + duration_ticks;
        
        // Check existing overlay tracks (id > base_track_id)
        for track in self.tracks.iter().filter(|t| t.id > base_track_id) {
            let has_overlap = track.clips.iter().any(|clip| {
                let clip_end = clip.timeline_start_ticks + (clip.out_ticks - clip.in_ticks);
                position_ticks < clip_end && insert_end_ticks > clip.timeline_start_ticks
            });
            
            if !has_overlap {
                return track.id; // Reuse this lane
            }
        }
        
        // No available lane, create new one
        let max_id = self.tracks
            .iter()
            .filter(|t| t.id > base_track_id)
            .map(|t| t.id)
            .max()
            .unwrap_or(base_track_id);
        max_id + 1
    }

    /// Consolidates all primary clips to track 1 and removes empty tracks
    /// This ensures the magnetic timeline model is maintained
    /// NOTE: Overlay tracks (id > 1) are preserved - only clips that should be on primary are moved
    pub fn consolidate_timeline(&mut self) {
        // First, collect all clips from other video tracks that should be on primary track
        // BUT: preserve overlay tracks (tracks with id > 1 that have clips) - these are intentional overlays
        let mut clips_to_move: Vec<ClipInstance> = Vec::new();
        
        // Collect clips from non-primary video tracks
        // BUT: preserve overlay tracks (id > 1) - these are intentional overlays and should not be moved
        // In the current implementation, overlay tracks are tracks with id > 1
        // We should NOT move clips from overlay tracks back to primary
        // For now, we preserve all overlay tracks by not collecting their clips
        // This means clips_to_move will remain empty, preserving all overlay tracks

        // Find or create primary track (track 1) - do this after collecting clips
        let has_primary = self.tracks.iter().any(|t| t.id == 1);
        if !has_primary {
            let new_track = Track {
                id: 1,
                kind: TrackKind::Video,
                clips: Vec::new(),
            };
            self.tracks.push(new_track);
        }

        // Now add all moved clips to primary track and update their track_id
        // Note: clips_to_move should be empty since we're preserving overlay tracks
        if let Some(primary_track) = self.tracks.iter_mut().find(|t| t.id == 1) {
            // Update track_id for all clips being moved
            for clip in &mut clips_to_move {
                clip.track_id = 1;
            }
            primary_track.clips.append(&mut clips_to_move);
        }

        // Remove empty tracks (except primary track and overlay tracks)
        // Overlay tracks (id > 1) should be preserved even if empty (they might be needed later)
        // But for now, we'll remove empty overlay tracks to keep things clean
        self.tracks.retain(|t| t.id == 1 || !t.clips.is_empty());

        // Repack primary timeline to ensure contiguity
        // This only affects track 1, preserving overlay tracks
        self.repack_primary_timeline();
    }

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
                        // When extending left edge outward (in_ticks decreases), adjust timeline_start_ticks
                        // to move the clip earlier on the timeline by the same amount
                        let in_delta = new_in_ticks - clip.in_ticks;
                        clip.in_ticks = new_in_ticks;
                        clip.out_ticks = new_out_ticks;
                        // Adjust timeline position when left edge changes (extending outward or trimming inward)
                        clip.timeline_start_ticks += in_delta;
                        return Ok(());
                    }
                }
                Err("Clip not found".to_string())
            }
            TimelineOperation::DeleteClip { clip_id } => {
                // Find the clip and determine if it's on primary track
                let mut deleted_clip: Option<(i64, i64, i64)> = None; // (track_id, timeline_start_ticks, duration)
                
                for track in &mut self.tracks {
                    if let Some(clip_index) = track.clips.iter().position(|c| c.id == clip_id) {
                        let clip = &track.clips[clip_index];
                        let duration = clip.out_ticks - clip.in_ticks;
                        deleted_clip = Some((track.id, clip.timeline_start_ticks, duration));
                        track.clips.remove(clip_index);
                        break;
                    }
                }
                
                if let Some((track_id, deleted_start, duration)) = deleted_clip {
                    // If deleted from primary track (track_id == 1), implement ripple delete
                    if track_id == 1 {
                        // Find primary track and shift all clips to the right left by duration
                        if let Some(primary_track) = self.tracks.iter_mut().find(|t| t.id == 1) {
                            for clip in &mut primary_track.clips {
                                if clip.timeline_start_ticks > deleted_start {
                                    clip.timeline_start_ticks -= duration;
                                }
                            }
                            // Ensure contiguity
                            self.repack_primary_timeline();
                        }
                    }
                    Ok(())
                } else {
                    Err("Clip not found".to_string())
                }
            }
            TimelineOperation::InsertClip {
                asset_id,
                position_ticks,
                track_id,
                duration_ticks,
            } => {
                // Force primary storyline clips to track 1
                // Only allow non-primary tracks for overlays (track_id > 1)
                let actual_track_id = if track_id == 1 || track_id <= 0 {
                    1
                } else {
                    track_id
                };

                // Find or create track
                let track = if let Some(t) = self.tracks.iter_mut().find(|t| t.id == actual_track_id) {
                    t
                } else {
                    // Only create new track if it's an overlay (track_id > 1)
                    if actual_track_id > 1 {
                        let new_track = Track {
                            id: actual_track_id,
                            kind: TrackKind::Video,
                            clips: Vec::new(),
                        };
                        self.tracks.push(new_track);
                        self.tracks.last_mut().unwrap()
                    } else {
                        // For primary track, create it
                        let new_track = Track {
                            id: 1,
                            kind: TrackKind::Video,
                            clips: Vec::new(),
                        };
                        self.tracks.push(new_track);
                        self.tracks.last_mut().unwrap()
                    }
                };

                let clip = ClipInstance {
                    id: uuid::Uuid::new_v4().to_string(),
                    asset_id,
                    in_ticks: 0,
                    out_ticks: duration_ticks,
                    timeline_start_ticks: position_ticks,
                    speed: 1.0,
                    track_id: actual_track_id,
                };
                track.clips.push(clip);
                
                // If inserted into primary track, ensure contiguity
                if actual_track_id == 1 {
                    self.repack_primary_timeline();
                }
                
                Ok(())
            }
            TimelineOperation::MoveClip {
                clip_id,
                new_position_ticks,
            } => {
                // Find the clip and remove it temporarily
                let mut clip_to_move: Option<ClipInstance> = None;
                let mut original_track_id: Option<i64> = None;
                
                for track in &mut self.tracks {
                    if let Some(clip_index) = track.clips.iter().position(|c| c.id == clip_id) {
                        original_track_id = Some(track.id);
                        let clip = &track.clips[clip_index];
                        let clip_original_position = clip.timeline_start_ticks;
                        let duration = clip.out_ticks - clip.in_ticks;
                        clip_to_move = Some(track.clips.remove(clip_index));
                        
                        // If on primary track, collapse the gap
                        if track.id == 1 {
                            // Shift all clips to the right of original position left by duration
                            for other_clip in &mut track.clips {
                                if other_clip.timeline_start_ticks > clip_original_position {
                                    other_clip.timeline_start_ticks -= duration;
                                }
                            }
                        }
                        break;
                    }
                }
                
                if let Some(mut clip) = clip_to_move {
                    let track_id = original_track_id.unwrap();
                    let duration = clip.out_ticks - clip.in_ticks;
                    
                    // Only apply magnetic behavior to primary track
                    if track_id == 1 {
                        // Find primary track
                        if let Some(primary_track) = self.tracks.iter_mut().find(|t| t.id == 1) {
                            // Clamp new position to valid bounds (0 to end of timeline)
                            let timeline_end = primary_track.clips.iter()
                                .map(|c| c.timeline_start_ticks + (c.out_ticks - c.in_ticks))
                                .max()
                                .unwrap_or(0);
                            
                            let clamped_position = new_position_ticks.max(0).min(timeline_end);
                            
                            // Shift clips at/after insertion point right by clip duration
                            for other_clip in &mut primary_track.clips {
                                if other_clip.timeline_start_ticks >= clamped_position {
                                    other_clip.timeline_start_ticks += duration;
                                }
                            }
                            
                            // Set clip's new position
                            clip.timeline_start_ticks = clamped_position;
                            
                            // Insert clip in sorted order
                            let insert_index = primary_track.clips
                                .iter()
                                .position(|c| c.timeline_start_ticks > clamped_position)
                                .unwrap_or(primary_track.clips.len());
                            primary_track.clips.insert(insert_index, clip);
                            
                            // Ensure contiguity
                            self.repack_primary_timeline();
                        } else {
                            return Err("Primary track not found".to_string());
                        }
                    } else {
                        // For non-primary tracks, just update position (overlay behavior)
                        clip.timeline_start_ticks = new_position_ticks;
                        if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                            let insert_index = track.clips
                                .iter()
                                .position(|c| c.timeline_start_ticks > new_position_ticks)
                                .unwrap_or(track.clips.len());
                            track.clips.insert(insert_index, clip);
                        }
                    }
                    Ok(())
                } else {
                    Err("Clip not found".to_string())
                }
            }
            TimelineOperation::ReorderClip {
                clip_id,
                new_position_ticks,
            } => {
                // Find the clip in primary track
                let mut clip_to_move: Option<ClipInstance> = None;
                
                if let Some(primary_track) = self.tracks.iter_mut().find(|t| t.id == 1) {
                    if let Some(clip_index) = primary_track.clips.iter().position(|c| c.id == clip_id) {
                        let clip = &primary_track.clips[clip_index];
                        let clip_original_position = clip.timeline_start_ticks;
                        let duration = clip.out_ticks - clip.in_ticks;
                        clip_to_move = Some(primary_track.clips.remove(clip_index));
                        
                        // Collapse gap: shift clips to the right of original position left by duration
                        for other_clip in &mut primary_track.clips {
                            if other_clip.timeline_start_ticks > clip_original_position {
                                other_clip.timeline_start_ticks -= duration;
                            }
                        }
                    }
                }
                
                if let Some(mut clip) = clip_to_move {
                    let duration = clip.out_ticks - clip.in_ticks;
                    
                    if let Some(primary_track) = self.tracks.iter_mut().find(|t| t.id == 1) {
                        // Clamp new position to valid bounds (0 to end of timeline)
                        let timeline_end = primary_track.clips.iter()
                            .map(|c| c.timeline_start_ticks + (c.out_ticks - c.in_ticks))
                            .max()
                            .unwrap_or(0);
                        
                        let clamped_position = new_position_ticks.max(0).min(timeline_end);
                        
                        // Shift clips at/after insertion point right by clip duration
                        for other_clip in &mut primary_track.clips {
                            if other_clip.timeline_start_ticks >= clamped_position {
                                other_clip.timeline_start_ticks += duration;
                            }
                        }
                        
                        // Set clip's new position
                        clip.timeline_start_ticks = clamped_position;
                        
                        // Insert clip in sorted order
                        let insert_index = primary_track.clips
                            .iter()
                            .position(|c| c.timeline_start_ticks > clamped_position)
                            .unwrap_or(primary_track.clips.len());
                        primary_track.clips.insert(insert_index, clip);
                        
                        // Ensure contiguity
                        self.repack_primary_timeline();
                    }
                    Ok(())
                } else {
                    Err("Clip not found in primary track".to_string())
                }
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

                // Ensure contiguity after insertion
                self.repack_primary_timeline();

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
                // Use dynamic lane algorithm to find available overlay track
                let overlay_track_id = self.find_available_overlay_lane(
                    base_track_id,
                    position_ticks,
                    duration_ticks,
                );

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
            TimelineOperation::ConvertPrimaryToOverlay {
                clip_id,
                position_ticks,
            } => {
                // Find the clip in primary track (track 1)
                let mut clip_to_convert: Option<ClipInstance> = None;
                let mut clip_original_position: Option<i64> = None;
                
                if let Some(primary_track) = self.tracks.iter_mut().find(|t| t.id == 1) {
                    if let Some(clip_index) = primary_track.clips.iter().position(|c| c.id == clip_id) {
                        let clip = &primary_track.clips[clip_index];
                        clip_original_position = Some(clip.timeline_start_ticks);
                        let duration = clip.out_ticks - clip.in_ticks;
                        clip_to_convert = Some(primary_track.clips.remove(clip_index));
                        
                        // Collapse primary: shift all clips after removed clip left by duration
                        for other_clip in &mut primary_track.clips {
                            if let Some(original_pos) = clip_original_position {
                                if other_clip.timeline_start_ticks > original_pos {
                                    other_clip.timeline_start_ticks -= duration;
                                }
                            }
                        }
                        
                        // Ensure contiguity
                        self.repack_primary_timeline();
                    }
                }
                
                if let Some(mut clip) = clip_to_convert {
                    let duration = clip.out_ticks - clip.in_ticks;
                    
                    // Use dynamic lane algorithm to find available overlay track
                    let overlay_track_id = self.find_available_overlay_lane(
                        1, // base_track_id is primary track (1)
                        position_ticks,
                        duration,
                    );
                    
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
                    
                    // Update clip position and track_id
                    clip.timeline_start_ticks = position_ticks;
                    clip.track_id = overlay_track.id;
                    
                    // Insert in sorted order
                    let insert_index = overlay_track.clips
                        .iter()
                        .position(|c| c.timeline_start_ticks > position_ticks)
                        .unwrap_or(overlay_track.clips.len());
                    overlay_track.clips.insert(insert_index, clip);
                    
                    Ok(())
                } else {
                    Err("Clip not found in primary track".to_string())
                }
            }
            TimelineOperation::ConvertOverlayToPrimary {
                clip_id,
                position_ticks,
            } => {
                // Find the clip in an overlay track (track id > 1)
                let mut clip_to_convert: Option<ClipInstance> = None;
                let mut source_track_id: Option<i64> = None;
                
                // Find clip in any overlay track (id > 1)
                for track in &mut self.tracks {
                    if track.id > 1 && track.kind == TrackKind::Video {
                        if let Some(clip_index) = track.clips.iter().position(|c| c.id == clip_id) {
                            source_track_id = Some(track.id);
                            clip_to_convert = Some(track.clips.remove(clip_index));
                            break;
                        }
                    }
                }
                
                if let Some(mut clip) = clip_to_convert {
                    let duration = clip.out_ticks - clip.in_ticks;
                    
                    // Find or create primary track (track 1)
                    let primary_track = if let Some(t) = self.tracks.iter_mut().find(|t| t.id == 1) {
                        t
                    } else {
                        let new_track = Track {
                            id: 1,
                            kind: TrackKind::Video,
                            clips: Vec::new(),
                        };
                        self.tracks.push(new_track);
                        self.tracks.last_mut().unwrap()
                    };
                    
                    // Clamp new position to valid bounds (0 to end of timeline)
                    let timeline_end = primary_track.clips.iter()
                        .map(|c| c.timeline_start_ticks + (c.out_ticks - c.in_ticks))
                        .max()
                        .unwrap_or(0);
                    
                    let clamped_position = position_ticks.max(0).min(timeline_end);
                    
                    // Shift clips at/after insertion point right by clip duration (ripple effect)
                    for other_clip in &mut primary_track.clips {
                        if other_clip.timeline_start_ticks >= clamped_position {
                            other_clip.timeline_start_ticks += duration;
                        }
                    }
                    
                    // Update clip's position and track_id
                    clip.timeline_start_ticks = clamped_position;
                    clip.track_id = 1;
                    
                    // Insert clip in sorted order
                    let insert_index = primary_track.clips
                        .iter()
                        .position(|c| c.timeline_start_ticks > clamped_position)
                        .unwrap_or(primary_track.clips.len());
                    primary_track.clips.insert(insert_index, clip);
                    
                    // Ensure contiguity
                    self.repack_primary_timeline();
                    
                    // Remove empty overlay track if it exists and is now empty
                    if let Some(track_id) = source_track_id {
                        if let Some(track) = self.tracks.iter().find(|t| t.id == track_id) {
                            if track.clips.is_empty() {
                                self.tracks.retain(|t| t.id != track_id);
                            }
                        }
                    }
                    
                    Ok(())
                } else {
                    Err("Clip not found in overlay track".to_string())
                }
            }
            TimelineOperation::ConsolidateTimeline => {
                self.consolidate_timeline();
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
