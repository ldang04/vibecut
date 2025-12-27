use engine::compiler::{EditConstraints, EditEvent, EditPlan, EditSection};
use crate::db::{MediaAssetInfo, Segment};

const TICKS_PER_SECOND: i64 = 48000;

/// Generate an edit plan from segments
pub fn generate_edit_plan(
    segments_with_assets: &[(Segment, MediaAssetInfo)],
    constraints: EditConstraints,
) -> EditPlan {
    // V1: Simple greedy selection based on transcript quality
    
    // Filter segments that have transcripts and reasonable length
    let mut candidate_segments: Vec<_> = segments_with_assets
        .iter()
        .filter(|(segment, _)| {
            // Must have transcript
            if segment.transcript.is_none() {
                return false;
            }
            // Reasonable duration: 1-30 seconds
            let duration_ticks = segment.end_ticks - segment.start_ticks;
            let duration_sec = duration_ticks as f64 / TICKS_PER_SECOND as f64;
            duration_sec >= 1.0 && duration_sec <= 30.0
        })
        .collect();

    // Score segments: longer transcripts with reasonable length score higher
    candidate_segments.sort_by(|a, b| {
        let score_a = calculate_clarity_score(&**a);
        let score_b = calculate_clarity_score(&**b);
        score_b.partial_cmp(&score_a).unwrap_or(std::cmp::Ordering::Equal)
    });

    // Determine target length
    let target_length_ticks = constraints.target_length.unwrap_or(60 * TICKS_PER_SECOND); // Default 1 minute
    
    // Structure: intro, body, outro
    let intro_duration = 10 * TICKS_PER_SECOND; // 10 seconds
    let outro_duration = 5 * TICKS_PER_SECOND; // 5 seconds
    let body_duration = target_length_ticks - intro_duration - outro_duration;

    let mut timeline_position = 0i64;
    let mut selected_clips = Vec::new();

    // Select segments for body (most of the content)
    for (segment, asset) in candidate_segments.iter() {
        let clip_duration = segment.end_ticks - segment.start_ticks;
        
        // Check if we've filled the body
        let current_body_duration: i64 = selected_clips.iter().map(|c: &ClipInfo| c.duration).sum();
        if current_body_duration >= body_duration {
            break;
        }

        // Add clip for body
        selected_clips.push(ClipInfo {
            asset_id: asset.id,
            in_ticks: segment.start_ticks,
            out_ticks: segment.end_ticks,
            timeline_start: timeline_position,
            duration: clip_duration,
        });
        
        timeline_position += clip_duration;
    }

    // Build sections
    let mut sections = Vec::new();
    
    // Intro: first clip (if available)
    if let Some(first_clip) = selected_clips.first() {
        let intro_events = vec![EditEvent::Clip {
            asset_id: first_clip.asset_id,
            in_ticks: first_clip.in_ticks,
            out_ticks: first_clip.out_ticks.min(first_clip.in_ticks + intro_duration),
            timeline_start_ticks: 0,
            track_id: 1,
        }];
        sections.push(EditSection {
            section_type: "intro".to_string(),
            target_duration: intro_duration,
            events: intro_events,
        });
    }

    // Body: remaining clips
    let body_start = if !selected_clips.is_empty() { intro_duration } else { 0 };
    let mut body_position = body_start;
    let mut body_events = Vec::new();
    
    // Skip first clip for body (already in intro)
    for clip in selected_clips.iter().skip(1) {
        body_events.push(EditEvent::Clip {
            asset_id: clip.asset_id,
            in_ticks: clip.in_ticks,
            out_ticks: clip.out_ticks,
            timeline_start_ticks: body_position,
            track_id: 1,
        });
        body_position += clip.duration;
    }

    sections.push(EditSection {
        section_type: "body".to_string(),
        target_duration: body_duration,
        events: body_events,
    });

    // Outro: last clip (if available)
    if let Some(last_clip) = selected_clips.last() {
        let outro_events = vec![EditEvent::Clip {
            asset_id: last_clip.asset_id,
            in_ticks: last_clip.out_ticks.saturating_sub(outro_duration),
            out_ticks: last_clip.out_ticks,
            timeline_start_ticks: body_position,
            track_id: 1,
        }];
        sections.push(EditSection {
            section_type: "outro".to_string(),
            target_duration: outro_duration,
            events: outro_events,
        });
    }

    EditPlan {
        sections,
        constraints,
    }
}

struct ClipInfo {
    asset_id: i64,
    in_ticks: i64,
    out_ticks: i64,
    timeline_start: i64,
    duration: i64,
}

fn calculate_clarity_score((segment, _asset): &(Segment, MediaAssetInfo)) -> f64 {
    // Simple scoring: longer transcripts = better
    // Duration factor: prefer 3-10 second clips
    let transcript_score = segment
        .transcript
        .as_ref()
        .map(|t| t.len() as f64)
        .unwrap_or(0.0);

    let duration_ticks = segment.end_ticks - segment.start_ticks;
    let duration_sec = duration_ticks as f64 / TICKS_PER_SECOND as f64;
    
    // Duration factor: prefer clips around 5 seconds
    let duration_factor = if duration_sec >= 3.0 && duration_sec <= 10.0 {
        1.0
    } else if duration_sec < 3.0 {
        duration_sec / 3.0
    } else {
        10.0 / duration_sec
    };

    transcript_score * duration_factor
}
