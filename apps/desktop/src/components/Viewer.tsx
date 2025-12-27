import { useState } from 'react';
import { VideoPlayer } from './VideoPlayer';

interface ViewerProps {
  videoSrc: string;
  startTime?: number; // in seconds
  endTime?: number; // in seconds
  currentTime?: number; // in seconds
  onTimeUpdate?: (time: number) => void;
}

export function Viewer({ videoSrc, startTime, endTime, currentTime, onTimeUpdate }: ViewerProps) {
  const [hoverTime, setHoverTime] = useState<number | null>(null);
  const formatTimecode = (seconds: number): string => {
    const mins = Math.floor(seconds / 60);
    const secs = Math.floor(seconds % 60);
    const frames = Math.floor((seconds % 1) * 30); // Assuming 30fps
    return `${mins.toString().padStart(2, '0')}:${secs.toString().padStart(2, '0')}:${frames.toString().padStart(2, '0')}`;
  };

  return (
    <div
      style={{
        backgroundColor: '#000000',
        width: '100%',
        height: '100%',
        display: 'flex',
        flexDirection: 'column',
        position: 'relative',
        overflow: 'hidden',
      }}
    >
      {videoSrc ? (
        <>
          <VideoPlayer
            src={videoSrc}
            startTime={startTime}
            endTime={endTime}
            onTimeUpdate={onTimeUpdate}
            onHoverTime={setHoverTime}
          />
          {(currentTime !== undefined || hoverTime !== null) && (
            <div
              style={{
                position: 'absolute',
                bottom: '20px',
                left: '20px',
                backgroundColor: 'rgba(0, 0, 0, 0.7)',
                color: '#e5e5e5',
                padding: '0.5rem 1rem',
                borderRadius: '4px',
                fontFamily: 'monospace',
                fontSize: '14px',
              }}
            >
              {formatTimecode(hoverTime !== null ? hoverTime : (currentTime || 0))}
            </div>
          )}
        </>
      ) : (
        <div
          style={{
            color: '#505050',
            fontSize: '14px',
            textAlign: 'center',
          }}
        >
          No clip selected
        </div>
      )}
    </div>
  );
}

