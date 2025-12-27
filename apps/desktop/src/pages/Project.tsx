import { useState, useEffect, useRef } from 'react';
import { useDaemon } from '../hooks/useDaemon';
import { VideoPlayer } from '../components/VideoPlayer';

interface TimelineData {
  tracks: any[];
  captions: any[];
  music: any[];
}

interface TimelineResponse {
  timeline: TimelineData;
}

interface ProjectProps {
  projectId: number;
}

export function Project({ projectId }: ProjectProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [timeline, setTimeline] = useState<TimelineData | null>(null);
  const [selectedClip, setSelectedClip] = useState<any | null>(null);
  const [videoSrc, setVideoSrc] = useState<string>('');
  const timelineData = useDaemon<TimelineResponse>(`/projects/${projectId}/timeline`, { method: 'GET' });

  useEffect(() => {
    // Fetch timeline data on mount
    timelineData.execute();
  }, [projectId]);

  useEffect(() => {
    if (timelineData.data?.timeline) {
      setTimeline(timelineData.data.timeline);
    }
  }, [timelineData.data]);

  useEffect(() => {
    if (canvasRef.current && timeline) {
      const canvas = canvasRef.current;
      const ctx = canvas.getContext('2d');
      if (!ctx) return;

      // Clear canvas
      ctx.clearRect(0, 0, canvas.width, canvas.height);

      // Draw timeline tracks
      const trackHeight = 60;
      let y = 50;

      timeline.tracks.forEach((track, index) => {
        // Draw track label
        ctx.fillStyle = '#333';
        ctx.font = '14px sans-serif';
        ctx.fillText(`Track ${index + 1}`, 10, y + 30);

        // Draw track background
        ctx.fillStyle = '#f0f0f0';
        ctx.fillRect(100, y, canvas.width - 110, trackHeight);

        // Draw clips
        track.clips?.forEach((clip: any, clipIndex: number) => {
          const isSelected = selectedClip && selectedClip.asset_id === clip.asset_id && selectedClip.timeline_start_ticks === clip.timeline_start_ticks;
          ctx.fillStyle = isSelected ? '#10b981' : '#3b82f6';
          const x = 100 + (clip.timeline_start_ticks / 48000) * 10; // Scale
          const width = ((clip.out_ticks - clip.in_ticks) / 48000) * 10;
          ctx.fillRect(x, y + 5, width, trackHeight - 10);
          
          // Store clip info for click detection (simplified - would need proper hit testing)
          (clip as any)._x = x;
          (clip as any)._y = y;
          (clip as any)._width = width;
          (clip as any)._trackIndex = index;
        });

        y += trackHeight + 10;
      });

      // Draw time ruler
      ctx.strokeStyle = '#666';
      ctx.beginPath();
      ctx.moveTo(100, 40);
      ctx.lineTo(canvas.width - 10, 40);
      ctx.stroke();

      // Draw time markers
      for (let i = 0; i < 10; i++) {
        const x = 100 + (i * (canvas.width - 110)) / 10;
        ctx.beginPath();
        ctx.moveTo(x, 35);
        ctx.lineTo(x, 45);
        ctx.stroke();
        ctx.fillText(`${i}s`, x - 5, 30);
      }
    }
  }, [timeline, canvasRef, selectedClip]);

  const handleCanvasClick = (e: React.MouseEvent<HTMLCanvasElement>) => {
    if (!timeline || !canvasRef.current) return;

    const canvas = canvasRef.current;
    const rect = canvas.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top;

    // Find clicked clip (simplified - would need proper hit testing)
    for (const track of timeline.tracks) {
      for (const clip of track.clips || []) {
        const clipX = 100 + (clip.timeline_start_ticks / 48000) * 10;
        const clipWidth = ((clip.out_ticks - clip.in_ticks) / 48000) * 10;
        // Rough hit test (would need to account for track Y position)
        if (x >= clipX && x <= clipX + clipWidth) {
          setSelectedClip(clip);
          // For V1, construct file path (will need proper proxy URL handling)
          // For now, use placeholder - will need to fetch proxy path from API
          setVideoSrc(`file:///path/to/proxy/${clip.asset_id}.mp4`);
          break;
        }
      }
    }
  };

  return (
    <div style={{ padding: '2rem', maxWidth: '1400px', margin: '0 auto' }}>
      <h1>Project Timeline</h1>
      
      <div style={{ marginBottom: '1rem' }}>
        <button
          onClick={() => timelineData.execute()}
          style={{
            padding: '0.5rem 1rem',
            backgroundColor: '#3b82f6',
            color: 'white',
            border: 'none',
            borderRadius: '0.25rem',
            cursor: 'pointer',
          }}
        >
          Refresh Timeline
        </button>
      </div>

      <div
        style={{
          border: '1px solid #e5e7eb',
          borderRadius: '0.5rem',
          overflow: 'auto',
          backgroundColor: 'white',
        }}
      >
        <canvas
          ref={canvasRef}
          width={1200}
          height={400}
          style={{ display: 'block', cursor: 'pointer' }}
          onClick={handleCanvasClick}
        />
      </div>

      {selectedClip && videoSrc && (
        <div style={{ marginTop: '2rem' }}>
          <h2>Video Player</h2>
          <VideoPlayer
            src={videoSrc}
            startTime={selectedClip.in_ticks / 48000}
            endTime={selectedClip.out_ticks / 48000}
          />
        </div>
      )}

      {timelineData.error && (
        <div style={{ marginTop: '1rem', color: '#ef4444' }}>
          Error: {timelineData.error.message}
        </div>
      )}
    </div>
  );
}
