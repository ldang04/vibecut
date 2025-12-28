import { useRef, useEffect, useState, useCallback } from 'react';

interface VideoPlayerProps {
  src: string;
  startTime?: number; // in seconds
  endTime?: number; // in seconds
  onTimeUpdate?: (currentTime: number) => void;
  onEnded?: () => void;
  onHoverTime?: (hoverTime: number | null) => void; // Callback for hover timestamp preview
  autoPlay?: boolean;
  isPlaying?: boolean; // External control of playback state
  onPlayPause?: (isPlaying: boolean) => void; // Callback when play/pause is toggled
}

export function VideoPlayer({ src, startTime, endTime, onTimeUpdate, onEnded, onHoverTime, autoPlay, isPlaying: externalIsPlaying, onPlayPause }: VideoPlayerProps) {
  const videoRef = useRef<HTMLVideoElement>(null);
  const progressBarRef = useRef<HTMLDivElement>(null);
  const [isPlaying, setIsPlaying] = useState(false);
  const [duration, setDuration] = useState(0);
  const [currentTime, setCurrentTime] = useState(0);
  const [hoverTime, setHoverTime] = useState<number | null>(null);

  // Track the last src and startTime to detect changes
  const lastSrcRef = useRef<string>('');
  const lastStartTimeRef = useRef<number | undefined>(undefined);
  
  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;

    // Check if src or startTime changed
    const srcChanged = lastSrcRef.current !== src;
    const startTimeChanged = lastStartTimeRef.current !== startTime;
    
    if (srcChanged) {
      lastSrcRef.current = src;
    }
    
    // Update video currentTime when startTime changes (playhead moved) or src changes
    if (startTime !== undefined && (srcChanged || startTimeChanged)) {
      lastStartTimeRef.current = startTime;
      video.currentTime = startTime;
    }

    // Sync with external playback state (Final Cut behavior - timeline controls video)
    // Only start playback if we're not already at the correct time
    if (externalIsPlaying !== undefined) {
      if (externalIsPlaying && video.paused) {
        // Ensure video is at the correct startTime before playing
        if (startTime !== undefined && Math.abs(video.currentTime - startTime) > 0.1) {
          video.currentTime = startTime;
        }
        video.play().catch(() => {
          // Auto-play may be blocked by browser, ignore error
        });
      } else if (!externalIsPlaying && !video.paused) {
        video.pause();
      }
    } else if (autoPlay && video.paused) {
      // Fallback to autoPlay if externalIsPlaying not provided
      if (startTime !== undefined && Math.abs(video.currentTime - startTime) > 0.1) {
        video.currentTime = startTime;
      }
      video.play().catch(() => {
        // Auto-play may be blocked by browser, ignore error
      });
    }

    const handleTimeUpdate = () => {
      const current = video.currentTime;
      setCurrentTime(current);
      if (onTimeUpdate) {
        onTimeUpdate(current);
      }
      // Check if we've reached the end time
      if (endTime !== undefined && current >= endTime) {
        // Clamp to endTime to prevent going past it
        if (video.currentTime > endTime) {
          video.currentTime = endTime;
        }
        video.pause();
        setIsPlaying(false);
        // Don't reset currentTime here - let onEnded handle it
        if (onEnded) {
          onEnded();
        }
      }
    };

    const handleLoadedMetadata = () => {
      setDuration(video.duration || 0);
    };

    const handlePlay = () => setIsPlaying(true);
    const handlePause = () => setIsPlaying(false);

    video.addEventListener('timeupdate', handleTimeUpdate);
    video.addEventListener('loadedmetadata', handleLoadedMetadata);
    video.addEventListener('play', handlePlay);
    video.addEventListener('pause', handlePause);

    return () => {
      video.removeEventListener('timeupdate', handleTimeUpdate);
      video.removeEventListener('loadedmetadata', handleLoadedMetadata);
      video.removeEventListener('play', handlePlay);
      video.removeEventListener('pause', handlePause);
    };
  }, [src, startTime, endTime, onTimeUpdate, onEnded, autoPlay, externalIsPlaying]);

  const togglePlayPause = useCallback(() => {
    const video = videoRef.current;
    if (!video) return;

    // If video is at or past endTime, don't restart - let onEnded handle it
    if (endTime !== undefined && video.currentTime >= endTime) {
      if (onEnded) {
        onEnded();
      }
      return;
    }

    // Toggle playback and notify parent (Final Cut behavior - button controls timeline)
    if (video.paused) {
      video.play();
      if (onPlayPause) {
        onPlayPause(true);
      }
    } else {
      video.pause();
      if (onPlayPause) {
        onPlayPause(false);
      }
    }
  }, [endTime, onEnded, onPlayPause]);

  const seekTo = (time: number) => {
    const video = videoRef.current;
    if (!video) return;
    video.currentTime = Math.max(0, Math.min(time, video.duration || 0));
  };

  // Handle progress bar hover
  const handleProgressBarMouseMove = (e: React.MouseEvent<HTMLDivElement>) => {
    const progressBar = progressBarRef.current;
    if (!progressBar || !duration) return;

    const rect = progressBar.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const progress = Math.max(0, Math.min(1, x / rect.width));
    const time = progress * duration;
    setHoverTime(time);
    if (onHoverTime) {
      onHoverTime(time);
    }
  };

  const handleProgressBarMouseLeave = () => {
    setHoverTime(null);
    if (onHoverTime) {
      onHoverTime(null);
    }
  };

  const handleProgressBarClick = (e: React.MouseEvent<HTMLDivElement>) => {
    const progressBar = progressBarRef.current;
    if (!progressBar || !duration) return;

    const rect = progressBar.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const progress = Math.max(0, Math.min(1, x / rect.width));
    const time = progress * duration;
    seekTo(time);
  };

  // Note: Spacebar handling is now in Editor.tsx to control timeline playback
  // This component no longer handles spacebar to avoid conflicts

  // Expose seekTo via ref if needed
  useEffect(() => {
    if (videoRef.current) {
      (videoRef.current as any).seekTo = seekTo;
    }
  }, []);

  const progress = duration > 0 ? (currentTime / duration) * 100 : 0;

  return (
    <div 
      style={{ 
        position: 'relative', 
        width: '100%', 
        height: '100%', 
        display: 'flex', 
        alignItems: 'center', 
        justifyContent: 'center', 
        overflow: 'hidden' 
      }}
    >
      <video
        ref={videoRef}
        src={src}
        style={{ 
          width: '100%',
          height: '100%',
          objectFit: 'contain',
        }}
      />
      
      {/* Custom Controls Overlay */}
      <div
        style={{
          position: 'absolute',
          bottom: 0,
          left: 0,
          right: 0,
          height: '80px',
          background: 'linear-gradient(to top, rgba(0, 0, 0, 0.7) 0%, rgba(0, 0, 0, 0.4) 50%, transparent 100%)',
          display: 'flex',
          flexDirection: 'column',
          alignItems: 'center',
          justifyContent: 'flex-end',
          paddingBottom: '12px',
          pointerEvents: 'none',
        }}
      >
        {/* Play/Pause Button - Centered at bottom */}
        <button
          onClick={togglePlayPause}
          style={{
            width: '48px',
            height: '48px',
            borderRadius: '50%',
            backgroundColor: 'rgba(0, 0, 0, 0.7)',
            border: '2px solid rgba(255, 255, 255, 0.8)',
            color: '#e5e5e5',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            cursor: 'pointer',
            fontSize: '20px',
            pointerEvents: 'auto',
            transition: 'background-color 0.2s, transform 0.1s',
          }}
          onMouseEnter={(e) => {
            e.currentTarget.style.backgroundColor = 'rgba(0, 0, 0, 0.9)';
            e.currentTarget.style.transform = 'scale(1.1)';
          }}
          onMouseLeave={(e) => {
            e.currentTarget.style.backgroundColor = 'rgba(0, 0, 0, 0.7)';
            e.currentTarget.style.transform = 'scale(1)';
          }}
        >
          {isPlaying ? '⏸' : '▶'}
        </button>

        {/* Progress Bar */}
        <div
          ref={progressBarRef}
          onClick={handleProgressBarClick}
          onMouseMove={handleProgressBarMouseMove}
          onMouseLeave={handleProgressBarMouseLeave}
          style={{
            width: '100%',
            height: '8px',
            backgroundColor: 'rgba(255, 255, 255, 0.2)',
            borderRadius: '4px',
            marginTop: '12px',
            cursor: 'pointer',
            position: 'relative',
            pointerEvents: 'auto',
          }}
        >
          {/* Progress Fill */}
          <div
            style={{
              width: `${progress}%`,
              height: '100%',
              backgroundColor: '#3b82f6',
              borderRadius: '4px',
              transition: 'width 0.1s linear',
            }}
          />
          {/* Hover Preview Indicator */}
          {hoverTime !== null && duration > 0 && (
            <div
              style={{
                position: 'absolute',
                left: `${(hoverTime / duration) * 100}%`,
                top: '50%',
                transform: 'translate(-50%, -50%)',
                width: '12px',
                height: '12px',
                borderRadius: '50%',
                backgroundColor: '#e5e5e5',
                border: '2px solid #3b82f6',
                pointerEvents: 'none',
              }}
            />
          )}
        </div>
      </div>
    </div>
  );
}

