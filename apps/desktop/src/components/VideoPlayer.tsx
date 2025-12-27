import { useRef, useEffect, useState, useCallback } from 'react';

interface VideoPlayerProps {
  src: string;
  startTime?: number; // in seconds
  endTime?: number; // in seconds
  onTimeUpdate?: (currentTime: number) => void;
  onEnded?: () => void;
  onHoverTime?: (hoverTime: number | null) => void; // Callback for hover timestamp preview
}

export function VideoPlayer({ src, startTime, endTime, onTimeUpdate, onEnded, onHoverTime }: VideoPlayerProps) {
  const videoRef = useRef<HTMLVideoElement>(null);
  const progressBarRef = useRef<HTMLDivElement>(null);
  const [isPlaying, setIsPlaying] = useState(false);
  const [duration, setDuration] = useState(0);
  const [currentTime, setCurrentTime] = useState(0);
  const [hoverTime, setHoverTime] = useState<number | null>(null);

  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;

    // Set start time if provided
    if (startTime !== undefined) {
      video.currentTime = startTime;
    }

    const handleTimeUpdate = () => {
      const current = video.currentTime;
      setCurrentTime(current);
      if (onTimeUpdate) {
        onTimeUpdate(current);
      }
      // Check if we've reached the end time
      if (endTime !== undefined && current >= endTime) {
        video.pause();
        setIsPlaying(false);
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
  }, [src, startTime, endTime, onTimeUpdate, onEnded]);

  const togglePlayPause = useCallback(() => {
    const video = videoRef.current;
    if (!video) return;

    if (video.paused) {
      video.play();
    } else {
      video.pause();
    }
  }, []);

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

  // Keyboard shortcut: spacebar to toggle play/pause
  useEffect(() => {
    const handleKeyPress = (e: KeyboardEvent) => {
      // Only handle spacebar if not typing in an input/textarea/select field
      const target = e.target as HTMLElement;
      const isInputField = target.tagName === 'INPUT' || 
                          target.tagName === 'TEXTAREA' || 
                          target.tagName === 'SELECT' ||
                          target.isContentEditable;
      
      if (e.code === 'Space' && !isInputField) {
        e.preventDefault();
        togglePlayPause();
      }
    };

    window.addEventListener('keydown', handleKeyPress);
    return () => {
      window.removeEventListener('keydown', handleKeyPress);
    };
  }, [togglePlayPause]);

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

