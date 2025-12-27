import { useEffect, useRef, useState } from 'react';

interface TimelineData {
  tracks: any[];
  captions: any[];
  music: any[];
}

interface TimelineProps {
  timeline: TimelineData | null;
  selectedClip: any | null;
  onClipClick: (clip: any) => void;
  playheadPosition?: number; // in ticks
}

const TICKS_PER_SECOND = 48000;
const MIN_PIXELS_PER_SECOND = 10;
const MAX_PIXELS_PER_SECOND = 500;
const ZOOM_SENSITIVITY = 0.01;
const PIXELS_PER_REM = 16; // 1rem = 16px

export function Timeline({ timeline, selectedClip, onClipClick, playheadPosition = 0 }: TimelineProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [pixelsPerSecond, setPixelsPerSecond] = useState(20); // Initial zoom level
  const [scrollX, setScrollX] = useState(0); // Horizontal scroll offset in pixels
  const [hoverX, setHoverX] = useState<number | null>(null); // Cursor X position for zoom anchoring

  useEffect(() => {
    if (!canvasRef.current || !timeline) return;

    const canvas = canvasRef.current;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    // Clear canvas
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    ctx.fillStyle = '#1e1e1e';
    ctx.fillRect(0, 0, canvas.width, canvas.height);

    const leftMargin = 3 * PIXELS_PER_REM; // 3rem margin (1rem + 2rem)
    
    // Calculate adaptive tick intervals based on zoom level
    let majorTickInterval: number;
    let minorTickInterval: number;
    if (pixelsPerSecond < 20) {
      majorTickInterval = 10; // Every 10 seconds
      minorTickInterval = 5;
    } else if (pixelsPerSecond < 50) {
      majorTickInterval = 5; // Every 5 seconds
      minorTickInterval = 1;
    } else if (pixelsPerSecond < 100) {
      majorTickInterval = 1; // Every 1 second
      minorTickInterval = 0.5;
    } else if (pixelsPerSecond < 200) {
      majorTickInterval = 0.5; // Every 500ms
      minorTickInterval = 0.1;
    } else {
      majorTickInterval = 0.1; // Every 100ms
      minorTickInterval = 0.05;
    }

    // Calculate visible time range
    const startTime = scrollX / pixelsPerSecond;
    const endTime = startTime + (canvas.width - leftMargin) / pixelsPerSecond;
    
    // Draw time ruler line
    ctx.strokeStyle = '#404040';
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(leftMargin, 30);
    ctx.lineTo(canvas.width, 30);
    ctx.stroke();
    
    // Draw minor and major ticks
    ctx.strokeStyle = '#404040';
    ctx.lineWidth = 1;
    
    // Start from the first tick before visible range
    const firstMajorTick = Math.floor(startTime / majorTickInterval) * majorTickInterval;
    const firstMinorTick = Math.floor(startTime / minorTickInterval) * minorTickInterval;
    
    // Draw minor ticks
    for (let time = firstMinorTick; time <= endTime; time += minorTickInterval) {
      const x = leftMargin + (time * pixelsPerSecond) - scrollX;
      if (x >= leftMargin && x <= canvas.width) {
        const isMajorTick = Math.abs(time % majorTickInterval) < 0.001;
        ctx.beginPath();
        if (isMajorTick) {
          // Major tick - taller
          ctx.moveTo(x, 25);
          ctx.lineTo(x, 35);
        } else {
          // Minor tick - shorter
          ctx.moveTo(x, 28);
          ctx.lineTo(x, 32);
        }
        ctx.stroke();
      }
    }
    
    // Draw time labels (on major ticks)
    ctx.fillStyle = '#a0a0a0';
    ctx.font = '11px sans-serif';
    ctx.textAlign = 'center';
    for (let time = firstMajorTick; time <= endTime; time += majorTickInterval) {
      const x = leftMargin + (time * pixelsPerSecond) - scrollX;
      if (x >= leftMargin && x <= canvas.width) {
        // Format time label based on zoom level
        let label: string;
        if (majorTickInterval >= 1) {
          label = `${Math.floor(time)}s`;
        } else if (majorTickInterval >= 0.1) {
          label = `${time.toFixed(1)}s`;
        } else {
          label = `${time.toFixed(2)}s`;
        }
        ctx.fillText(label, x, 20);
      }
    }

    // Draw timeline tracks
    const trackHeight = 60;
    let y = 50;

    if (!timeline.tracks) return;
    
    timeline.tracks.forEach((track, trackIndex) => {
      // Draw track label
      ctx.fillStyle = '#a0a0a0';
      ctx.font = '12px sans-serif';
      ctx.textAlign = 'left';
      ctx.fillText(`Track ${trackIndex + 1}`, 10, y + 35);

      // Draw track background
      ctx.fillStyle = '#252525';
      ctx.fillRect(leftMargin, y, canvas.width - leftMargin, trackHeight);

      // Draw track border
      ctx.strokeStyle = '#404040';
      ctx.strokeRect(leftMargin, y, canvas.width - leftMargin, trackHeight);

      // Draw clips
      track.clips?.forEach((clip: any) => {
        const isSelected = selectedClip && 
          selectedClip.asset_id === clip.asset_id && 
          selectedClip.timeline_start_ticks === clip.timeline_start_ticks;
        
        ctx.fillStyle = isSelected ? '#10b981' : '#3b82f6';
        const pixelsPerTick = pixelsPerSecond / TICKS_PER_SECOND;
        const clipStartTime = clip.timeline_start_ticks / TICKS_PER_SECOND;
        const clipDuration = (clip.out_ticks - clip.in_ticks) / TICKS_PER_SECOND;
        const x = leftMargin + (clipStartTime * pixelsPerSecond) - scrollX;
        const width = clipDuration * pixelsPerSecond;
        ctx.fillRect(x, y + 5, width, trackHeight - 10);

        // Clip border
        ctx.strokeStyle = isSelected ? '#0ea66e' : '#2563eb';
        ctx.strokeRect(x, y + 5, width, trackHeight - 10);
      });

      y += trackHeight + 10;
    });

    // Draw playhead
    if (playheadPosition > 0) {
      const playheadTime = playheadPosition / TICKS_PER_SECOND;
      const playheadX = leftMargin + (playheadTime * pixelsPerSecond) - scrollX;
      if (playheadX >= leftMargin && playheadX <= canvas.width) {
        ctx.strokeStyle = '#ef4444';
        ctx.lineWidth = 2;
        ctx.beginPath();
        ctx.moveTo(playheadX, 30);
        ctx.lineTo(playheadX, y);
        ctx.stroke();
      }
    }
  }, [timeline, selectedClip, playheadPosition, pixelsPerSecond, scrollX]);

  const handleCanvasClick = (e: React.MouseEvent<HTMLCanvasElement>) => {
    if (!timeline || !canvasRef.current) return;

    const canvas = canvasRef.current;
    const rect = canvas.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top;
    const leftMargin = PIXELS_PER_REM;

    // Find clicked clip
    const trackHeight = 60;
    const trackSpacing = 10;
    let currentY = 50;
    
    if (!timeline.tracks) return;
    
    for (const track of timeline.tracks) {
      if (y >= currentY && y <= currentY + trackHeight) {
        const pixelsPerTick = pixelsPerSecond / TICKS_PER_SECOND;
        
        for (const clip of track.clips || []) {
          const clipStartTime = clip.timeline_start_ticks / TICKS_PER_SECOND;
          const clipDuration = (clip.out_ticks - clip.in_ticks) / TICKS_PER_SECOND;
          const clipX = leftMargin + (clipStartTime * pixelsPerSecond) - scrollX;
          const clipWidth = clipDuration * pixelsPerSecond;
          
          if (x >= clipX && x <= clipX + clipWidth) {
            onClipClick(clip);
            return;
          }
        }
      }
      currentY += trackHeight + trackSpacing;
    }
  };

  const handleWheel = (e: React.WheelEvent<HTMLCanvasElement>) => {
    // Handle zoom gestures (trackpad pinch-to-zoom)
    // On macOS: Ctrl+scroll indicates pinch gesture (system default)
    const isZoomGesture = e.ctrlKey;
    
    if (isZoomGesture) {
      // Handle zoom
      e.preventDefault();
      if (!canvasRef.current) return;

      const canvas = canvasRef.current;
      const rect = canvas.getBoundingClientRect();
      const leftMargin = PIXELS_PER_REM;
      
      // Get mouse position relative to canvas
      const mouseX = e.clientX - rect.left;
      
      // Calculate focus time (time under cursor, or playhead if cursor is outside)
      let focusTime: number;
      if (mouseX >= leftMargin && mouseX <= canvas.width) {
        focusTime = (mouseX - leftMargin + scrollX) / pixelsPerSecond;
      } else {
        focusTime = playheadPosition / TICKS_PER_SECOND;
      }
      
      // Calculate zoom factor from wheel delta
      // Use exponential scaling for smooth zoom
      // For pinch gestures, use deltaY (primary scroll direction)
      const zoomFactor = Math.exp(-e.deltaY * ZOOM_SENSITIVITY);
      const oldScale = pixelsPerSecond;
      const newScale = Math.max(MIN_PIXELS_PER_SECOND, Math.min(MAX_PIXELS_PER_SECOND, oldScale * zoomFactor));
      
      // Anchor around focus point
      const focusX = focusTime * oldScale - scrollX;
      const newScrollX = focusTime * newScale - focusX;
      
      setPixelsPerSecond(newScale);
      setScrollX(Math.max(0, newScrollX)); // Prevent negative scroll
    } else if (Math.abs(e.deltaX) > Math.abs(e.deltaY)) {
      // Handle horizontal scrolling (sideways wheel events)
      e.preventDefault();
      const scrollSpeed = 0.5; // Adjust scroll sensitivity
      const newScrollX = scrollX + e.deltaX * scrollSpeed;
      setScrollX(Math.max(0, newScrollX)); // Prevent negative scroll
    }
    // For vertical scrolling without Ctrl, allow it to pass through (don't prevent default)
  };

  const handleMouseMove = (e: React.MouseEvent<HTMLCanvasElement>) => {
    if (!canvasRef.current) return;
    const canvas = canvasRef.current;
    const rect = canvas.getBoundingClientRect();
    const mouseX = e.clientX - rect.left;
    setHoverX(mouseX);
  };

  const handleMouseLeave = () => {
    setHoverX(null);
  };

  return (
    <div
      style={{
        backgroundColor: '#1e1e1e',
        borderTop: '1px solid #404040',
        height: '100%',
        overflow: 'auto',
        display: 'flex',
        flexDirection: 'column',
      }}
    >
      <canvas
        ref={canvasRef}
        width={1200}
        height={400}
        style={{
          display: 'block',
          cursor: 'pointer',
        }}
        onClick={handleCanvasClick}
        onWheel={handleWheel}
        onMouseMove={handleMouseMove}
        onMouseLeave={handleMouseLeave}
      />
    </div>
  );
}

