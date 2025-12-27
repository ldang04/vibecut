use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaInfo {
    pub duration_ticks: i64,
    pub fps_num: i32,
    pub fps_den: i32,
    pub width: i32,
    pub height: i32,
    pub has_audio: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeOutput {
    format: Option<FormatInfo>,
    streams: Vec<StreamInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FormatInfo {
    duration: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StreamInfo {
    codec_type: Option<String>,
    width: Option<i32>,
    height: Option<i32>,
    r_frame_rate: Option<String>,
    avg_frame_rate: Option<String>,
}

pub struct FFmpegWrapper;

impl FFmpegWrapper {
    pub async fn probe(media_path: &Path) -> Result<MediaInfo> {
        let output = Command::new("ffprobe")
            .args(&[
                "-v",
                "error",
                "-show_entries",
                "format=duration:stream=codec_type,width,height,r_frame_rate,avg_frame_rate",
                "-of",
                "json",
                media_path.to_str().unwrap(),
            ])
            .output()
            .await
            .context("Failed to execute ffprobe. Make sure FFmpeg is installed.")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("ffprobe failed: {}", stderr);
        }

        let probe_output: ProbeOutput = serde_json::from_slice(&output.stdout)
            .context("Failed to parse ffprobe JSON output")?;

        // Extract duration from format
        let duration_seconds = probe_output
            .format
            .and_then(|f| f.duration)
            .and_then(|d| d.parse::<f64>().ok())
            .unwrap_or(0.0);

        // Find video stream
        let video_stream = probe_output
            .streams
            .iter()
            .find(|s| s.codec_type.as_deref() == Some("video"));

        let (width, height, fps_num, fps_den) = if let Some(vs) = video_stream {
            let w = vs.width.unwrap_or(0);
            let h = vs.height.unwrap_or(0);

            // Parse frame rate (format: "30/1" or "30000/1001")
            let fps_str = vs.r_frame_rate.as_deref().or(vs.avg_frame_rate.as_deref());
            let (num, den) = fps_str
                .and_then(|s| {
                    let parts: Vec<&str> = s.split('/').collect();
                    if parts.len() == 2 {
                        Some((parts[0].parse::<i32>().ok()?, parts[1].parse::<i32>().ok()?))
                    } else {
                        None
                    }
                })
                .unwrap_or((30, 1));

            (w, h, num, den)
        } else {
            (0, 0, 30, 1)
        };

        // Check for audio stream
        let has_audio = probe_output
            .streams
            .iter()
            .any(|s| s.codec_type.as_deref() == Some("audio"));

        // Convert duration to ticks (48,000 ticks per second)
        const TICKS_PER_SECOND: i64 = 48000;
        let duration_ticks = (duration_seconds * TICKS_PER_SECOND as f64) as i64;

        Ok(MediaInfo {
            duration_ticks,
            fps_num,
            fps_den,
            width,
            height,
            has_audio,
        })
    }

    pub async fn generate_proxy(
        input_path: &Path,
        output_path: &Path,
        width: i32,
        height: i32,
    ) -> Result<()> {
        // Create parent directory if needed
        if let Some(parent) = output_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let status = Command::new("ffmpeg")
            .args(&[
                "-i",
                input_path.to_str().unwrap(),
                "-vf",
                &format!("scale={}:{}", width, height),
                "-c:v",
                "libx264",
                "-preset",
                "medium",
                "-crf",
                "23",
                "-c:a",
                "aac",
                "-b:a",
                "128k",
                "-y", // Overwrite output file
                output_path.to_str().unwrap(),
            ])
            .output()
            .await
            .context("Failed to execute ffmpeg. Make sure FFmpeg is installed.")?
            .status;

        if !status.success() {
            anyhow::bail!("ffmpeg failed to generate proxy");
        }

        Ok(())
    }

    pub async fn extract_audio(input_path: &Path, output_path: &Path) -> Result<()> {
        // Create parent directory if needed
        if let Some(parent) = output_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let status = Command::new("ffmpeg")
            .args(&[
                "-i",
                input_path.to_str().unwrap(),
                "-vn", // No video
                "-acodec",
                "pcm_s16le",
                "-ar",
                "44100",
                "-ac",
                "2",
                "-y",
                output_path.to_str().unwrap(),
            ])
            .output()
            .await
            .context("Failed to execute ffmpeg for audio extraction")?
            .status;

        if !status.success() {
            anyhow::bail!("ffmpeg failed to extract audio");
        }

        Ok(())
    }
}
