use crate::timeline::{ClipInstance, Timeline, TrackKind, TICKS_PER_SECOND};
use std::path::PathBuf;
use std::collections::HashMap;

pub struct RenderCommand {
    pub ffmpeg_args: Vec<String>,
    pub output_path: PathBuf,
    pub concat_list_path: PathBuf, // Path to concat demuxer list file
}

/// Generate FFmpeg render command for timeline
/// V1: Hard cuts only, concatenate clips in order
pub fn generate_render_commands(
    timeline: &Timeline,
    output_path: PathBuf,
    proxy_paths: &HashMap<i64, String>, // Map asset_id -> proxy file path
) -> RenderCommand {
    // Get video track clips (sorted by timeline position)
    let video_track = timeline
        .tracks
        .iter()
        .find(|t| matches!(t.kind, TrackKind::Video) && t.id == 1);
    
    let mut clips: Vec<&ClipInstance> = if let Some(track) = video_track {
        let mut clips: Vec<&ClipInstance> = track.clips.iter().collect();
        // Sort by timeline_start_ticks
        clips.sort_by_key(|c| c.timeline_start_ticks);
        clips
    } else {
        Vec::new()
    };

    if clips.is_empty() {
        // Return minimal command if no clips
        return RenderCommand {
            ffmpeg_args: vec!["-f".to_string(), "lavfi".to_string(), "-i".to_string(), "color=black:size=1920x1080:d=1".to_string(), "-y".to_string(), output_path.to_string_lossy().to_string()],
            output_path: output_path.clone(),
            concat_list_path: PathBuf::new(),
        };
    }

    // Build input arguments and filter_complex for concatenation
    let mut input_args = Vec::new();
    
    for (idx, clip) in clips.iter().enumerate() {
        let proxy_path = proxy_paths.get(&clip.asset_id).cloned();
        if let Some(path) = proxy_path {
            // Use concat demuxer approach: create separate file for each clip segment
            // For V1, we'll use filter_complex concat instead (simpler)
            input_args.push("-i".to_string());
            input_args.push(path.clone());
        }
    }

    // Build filter_complex for concatenation with trim
    // For each clip, trim to in/out points, then concat
    if !clips.is_empty() {
        let num_inputs = clips.len();
        let mut filter_parts = Vec::new();
        
        // For each clip, add trim filter: [0:v]trim=start=0:end=5,setpts=PTS-STARTPTS[v0]
        for (idx, clip) in clips.iter().enumerate() {
            let start_sec = clip.in_ticks as f64 / TICKS_PER_SECOND as f64;
            let duration_sec = (clip.out_ticks - clip.in_ticks) as f64 / TICKS_PER_SECOND as f64;
            
            filter_parts.push(format!("[{}:v]trim=start={}:duration={},setpts=PTS-STARTPTS[v{}]", idx, start_sec, duration_sec, idx));
            filter_parts.push(format!("[{}:a]atrim=start={}:duration={},asetpts=PTS-STARTPTS[a{}]", idx, start_sec, duration_sec, idx));
        }
        
        // Concat all trimmed clips
        let mut concat_inputs = Vec::new();
        for i in 0..num_inputs {
            concat_inputs.push(format!("[v{}]", i));
            concat_inputs.push(format!("[a{}]", i));
        }
        filter_parts.push(format!("{}concat=n={}:v=1:a=1[outv][outa]", concat_inputs.join(""), num_inputs));
        
        let filter_complex = filter_parts.join(";");
        
        let mut args = input_args;
        args.push("-filter_complex".to_string());
        args.push(filter_complex);
        args.push("-map".to_string());
        args.push("[outv]".to_string());
        args.push("-map".to_string());
        args.push("[outa]".to_string());
        args.push("-c:v".to_string());
        args.push("libx264".to_string());
        args.push("-preset".to_string());
        args.push("medium".to_string());
        args.push("-crf".to_string());
        args.push("23".to_string());
        args.push("-c:a".to_string());
        args.push("aac".to_string());
        args.push("-b:a".to_string());
        args.push("128k".to_string());
        args.push("-y".to_string());
        args.push(output_path.to_string_lossy().to_string());

    RenderCommand {
        ffmpeg_args: args,
            output_path: output_path.clone(),
            concat_list_path: PathBuf::new(),
        }
    } else {
        // Fallback: empty timeline
        RenderCommand {
            ffmpeg_args: vec!["-f".to_string(), "lavfi".to_string(), "-i".to_string(), "color=black:size=1920x1080:d=1".to_string(), "-y".to_string(), output_path.to_string_lossy().to_string()],
            output_path: output_path.clone(),
            concat_list_path: PathBuf::new(),
        }
    }
}
