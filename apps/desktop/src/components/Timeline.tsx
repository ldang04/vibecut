import { useEffect, useRef, useState, useCallback } from 'react';

interface TimelineData {
  tracks: any[];
  captions: any[];
  music: any[];
}

interface MediaAsset {
  id: number;
  path: string;
  duration_ticks: number;
  width: number;
  height: number;
}

interface TextTemplate {
  id: string;
  name: 'Title' | 'Subtitle';
  defaultText: string;
  style: {
    fontSize: number;
    alignment: 'center' | 'bottom';
    position: 'center' | 'bottom';
  };
}

interface AudioAsset {
  id: number;
  path: string;
  duration_ticks: number;
}

interface TimelineProps {
  timeline: TimelineData | null;
  selectedClip: any | null;
  onClipClick: (clip: any, event?: React.MouseEvent) => void;
  playheadPosition?: number; // in ticks
  dragAsset?: MediaAsset | null;
  dragTextTemplate?: TextTemplate | null;
  dragAudioAsset?: AudioAsset | null;
  onClipInsert?: (assetId: number, positionTicks: number, trackId: number, intent?: 'primary' | 'layered', modifierKey?: boolean) => void;
  onTextClipInsert?: (template: TextTemplate, positionTicks: number, intent?: 'primary' | 'layered', trackId?: number) => void;
  onAudioClipInsert?: (audioAsset: AudioAsset, positionTicks: number) => void;
  onDragEnd?: () => void; // Callback to clear drag state in parent
  onHoverTimeChange?: (time: number) => void;
  onClipTrim?: (clipId: string, newInTicks: number, newOutTicks: number) => void;
  onClipSplit?: (clipId: string, positionTicks: number) => void;
  onPlayheadSet?: (positionTicks: number) => void;
  onClipReorder?: (clipId: string, newPositionTicks: number) => void;
  onConvertPrimaryToOverlay?: (clipId: string, positionTicks: number) => void;
  onConvertOverlayToPrimary?: (clipId: string, positionTicks: number) => void;
  onMoveClip?: (clipId: string, newPositionTicks: number) => void;
  activeTool?: 'pointer' | 'cut';
  projectId?: number; // Project ID for thumbnail API calls
  isPlaying?: boolean; // Whether timeline is currently playing (for auto-scroll)
}

const TICKS_PER_SECOND = 48000;
const MIN_PIXELS_PER_SECOND = 10;
const MAX_PIXELS_PER_SECOND = 500;
const ZOOM_SENSITIVITY = 0.01;
const PIXELS_PER_REM = 16; // 1rem = 16px

// Thumbnail cache - shared across all Timeline instances
const thumbnailCache = new Map<string, HTMLImageElement>();
const thumbnailLoadingPromises = new Map<string, Promise<HTMLImageElement | null>>();
const MAX_CACHE_SIZE = 200; // Maximum number of cached thumbnails

// Load thumbnail image with caching
async function loadThumbnail(assetId: number, timestampSec: number, projectId: number): Promise<HTMLImageElement | null> {
  const cacheKey = `${assetId}_${timestampSec}`;
  
  // Check cache first
  if (thumbnailCache.has(cacheKey)) {
    return thumbnailCache.get(cacheKey)!;
  }
  
  // Check if already loading
  if (thumbnailLoadingPromises.has(cacheKey)) {
    return thumbnailLoadingPromises.get(cacheKey)!;
  }
  
  // Start loading
  const loadPromise = (async (): Promise<HTMLImageElement | null> => {
    try {
      // Format timestamp as 4-digit string (e.g., "0000", "0100")
      const timestampStr = timestampSec.toString().padStart(4, '0');
      const url = `http://127.0.0.1:7777/api/projects/${projectId}/media/${assetId}/thumbnail/${timestampStr}`;
      
      const img = new Image();
      img.crossOrigin = 'anonymous';
      
      // Try to load thumbnail
      try {
        await new Promise<void>((resolve, reject) => {
          img.onload = () => resolve();
          img.onerror = () => reject(new Error('Failed to load thumbnail'));
          img.src = url;
        });
      } catch {
        // Thumbnail doesn't exist - try to generate thumbnails for this asset
        // Only try once per asset to avoid spamming the server
        const generateKey = `generate_${assetId}`;
        if (!thumbnailLoadingPromises.has(generateKey)) {
          const generatePromise = (async () => {
            try {
              const response = await fetch(`http://127.0.0.1:7777/api/projects/${projectId}/media/${assetId}/generate_thumbnails`, {
                method: 'POST',
              });
              if (response.ok) {
                // Thumbnails generated, wait a bit then retry
                await new Promise(resolve => setTimeout(resolve, 1500));
                // Retry loading the image
                const retryImg = new Image();
                retryImg.crossOrigin = 'anonymous';
                await new Promise<void>((resolveRetry, rejectRetry) => {
                  retryImg.onload = () => resolveRetry();
                  retryImg.onerror = () => rejectRetry(new Error('Failed to load thumbnail after generation'));
                  retryImg.src = url;
                });
                // Success - return the retry image
                return retryImg;
              }
            } catch {
              // Ignore generation errors
            }
            return null;
          })();
          thumbnailLoadingPromises.set(generateKey, generatePromise);
          const generatedImg = await generatePromise;
          if (generatedImg) {
            // Add to cache
            if (thumbnailCache.size >= MAX_CACHE_SIZE) {
              const firstKey = thumbnailCache.keys().next().value;
              if (firstKey) {
                thumbnailCache.delete(firstKey);
              }
            }
            thumbnailCache.set(cacheKey, generatedImg);
            thumbnailLoadingPromises.delete(cacheKey);
            thumbnailLoadingPromises.delete(generateKey);
            return generatedImg;
          }
        } else {
          // Already generating, wait for it
          const generatedImg = await thumbnailLoadingPromises.get(generateKey)!;
          if (generatedImg) {
            thumbnailCache.set(cacheKey, generatedImg);
            thumbnailLoadingPromises.delete(cacheKey);
            return generatedImg;
          }
        }
        // Failed to generate or load
        thumbnailLoadingPromises.delete(cacheKey);
        return null;
      }
      
      // Add to cache (with LRU eviction if needed)
      if (thumbnailCache.size >= MAX_CACHE_SIZE) {
        // Remove oldest entry (simple FIFO eviction)
        const firstKey = thumbnailCache.keys().next().value;
        if (firstKey) {
          thumbnailCache.delete(firstKey);
        }
      }
      thumbnailCache.set(cacheKey, img);
      thumbnailLoadingPromises.delete(cacheKey);
      return img;
    } catch {
      // Don't log warnings for missing thumbnails - they'll be generated on-demand
      thumbnailLoadingPromises.delete(cacheKey);
      return null;
    }
  })();
  
  thumbnailLoadingPromises.set(cacheKey, loadPromise);
  return loadPromise;
}

export function Timeline({ timeline, selectedClip, onClipClick, playheadPosition = 0, dragAsset, dragTextTemplate, dragAudioAsset, onClipInsert, onTextClipInsert, onAudioClipInsert, onDragEnd, onHoverTimeChange, onClipTrim, onClipSplit, onPlayheadSet, onClipReorder, onConvertPrimaryToOverlay, onConvertOverlayToPrimary, onMoveClip, activeTool = 'pointer', projectId = 1, isPlaying = false }: TimelineProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [pixelsPerSecond, setPixelsPerSecond] = useState(20); // Initial zoom level
  const [scrollX, setScrollX] = useState(0); // Horizontal scroll offset in pixels
  const [hoverX, setHoverX] = useState<number | null>(null); // Cursor X position for zoom anchoring
  const [hoverPlayheadTicks, setHoverPlayheadTicks] = useState<number | null>(null); // Hover playhead position in ticks
  const [dropPosition, setDropPosition] = useState<{ time: number; trackId: number; intent?: 'primary' | 'layered' } | null>(null);
  const [trimState, setTrimState] = useState<{ clipId: string; edge: 'left' | 'right'; startX: number; originalIn: number; originalOut: number; originalStart: number; assetId: number; assetDurationTicks: number } | null>(null);
  const [hoveredEdge, setHoveredEdge] = useState<{ clipId: string; edge: 'left' | 'right' } | null>(null);
  const [loadedThumbnails, setLoadedThumbnails] = useState<Map<string, HTMLImageElement>>(new Map());
  
  // Drag state for existing clips
  const [draggedClip, setDraggedClip] = useState<{ clip: any; startX: number; startY: number; offsetX: number; originalPosition: number } | null>(null);
  const [clipDropPosition, setClipDropPosition] = useState<{ time: number; intent: 'primary' | 'layered' } | null>(null);
  const [potentialDrag, setPotentialDrag] = useState<{ clip: any; startX: number; startY: number; offsetX: number; originalPosition: number } | null>(null);

  // Helper function to draw rounded rectangle
  const drawRoundedRect = useCallback((ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, r: number) => {
    if (w < 2 * r) r = w / 2;
    if (h < 2 * r) r = h / 2;
    ctx.beginPath();
    ctx.moveTo(x + r, y);
    ctx.arcTo(x + w, y, x + w, y + h, r);
    ctx.arcTo(x + w, y + h, x, y + h, r);
    ctx.arcTo(x, y + h, x, y, r);
    ctx.arcTo(x, y, x + w, y, r);
    ctx.closePath();
  }, []);

  // Calculate earliest next timestamp for placing clips
  const calculateEarliestNextTimestamp = useCallback((timeline: TimelineData | null): number => {
    if (!timeline || !timeline.tracks || timeline.tracks.length === 0) {
      return 0; // Start at 0s if no clips exist
    }

    // Find the latest end time across all tracks
    let latestEndTime = 0;
    timeline.tracks.forEach((track) => {
      if (track.clips && track.clips.length > 0) {
        track.clips.forEach((clip: any) => {
          const clipEndTime = (clip.timeline_start_ticks + (clip.out_ticks - clip.in_ticks)) / TICKS_PER_SECOND;
          if (clipEndTime > latestEndTime) {
            latestEndTime = clipEndTime;
          }
        });
      }
    });

    return latestEndTime;
  }, []);

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
    const trackSpacing = 10;
    let y = 50;

    if (!timeline.tracks) return;
    
    // Separate tracks by kind: Caption (text) tracks first, then Video, then Audio
    const captionTracks = timeline.tracks.filter(t => t.kind === 'Caption' || t.kind === 'caption');
    const videoTracks = timeline.tracks.filter(t => t.kind === 'Video' || t.kind === 'video' || !t.kind);
    const audioTracks = timeline.tracks.filter(t => t.kind === 'Audio' || t.kind === 'audio');
    
    // Separate video tracks into primary (track 1) and overlays (track_id > 1)
    const primaryVideoTrack = videoTracks.find(t => (t.id || 1) === 1);
    const overlayVideoTracks = videoTracks
      .filter(t => (t.id || 1) > 1 && (t.clips?.length || 0) > 0) // Only show overlay tracks that have clips
      .sort((a, b) => (a.id || 0) - (b.id || 0)); // Sort by ID ascending
    
    // Draw Caption tracks (text) - above video
    captionTracks.forEach((track) => {
      // Draw track background (no labels - tracks are implicit)
      ctx.fillStyle = '#252525';
      ctx.fillRect(leftMargin, y, canvas.width - leftMargin, trackHeight);

      // Draw track border
      ctx.strokeStyle = '#404040';
      ctx.strokeRect(leftMargin, y, canvas.width - leftMargin, trackHeight);

      // Draw clips
      track.clips?.forEach((clip: any) => {
        const clipId = clip.id || `${clip.asset_id}-${clip.timeline_start_ticks}`;
        
        // Skip drawing if this clip is being dragged
        if (draggedClip && (draggedClip.clip.id === clipId || 
            (draggedClip.clip.asset_id === clip.asset_id && 
             draggedClip.clip.timeline_start_ticks === clip.timeline_start_ticks))) {
          return;
        }
        
        const isSelected = selectedClip && 
          (selectedClip.id === clipId || 
           (selectedClip.id === clip.id) ||
           (selectedClip.asset_id === clip.asset_id && 
            selectedClip.timeline_start_ticks === clip.timeline_start_ticks));
        
        // Calculate clip position - handle trimming
        let clipStartTime = clip.timeline_start_ticks / TICKS_PER_SECOND;
        let clipDuration = (clip.out_ticks - clip.in_ticks) / TICKS_PER_SECOND;
        
        if (trimState && trimState.clipId === clipId) {
          // Show trim preview - allow both inward and outward trimming
          if (trimState.edge === 'left') {
            const newIn = (trimState.originalIn + (hoverX! - trimState.startX) / pixelsPerSecond * TICKS_PER_SECOND);
            // Allow extending outward (negative newIn) up to 0, or trimming inward up to originalOut - 1 second
            const constrainedIn = Math.max(0, Math.min(newIn, trimState.originalOut - TICKS_PER_SECOND));
            // Calculate timeline start adjustment (moves left when extending outward)
            const inAdjustment = constrainedIn - trimState.originalIn;
            clipStartTime = (trimState.originalStart + inAdjustment / TICKS_PER_SECOND) / TICKS_PER_SECOND;
            clipDuration = (trimState.originalOut - constrainedIn) / TICKS_PER_SECOND;
          } else {
            const newOut = (trimState.originalOut + (hoverX! - trimState.startX) / pixelsPerSecond * TICKS_PER_SECOND);
            // Allow extending outward up to asset duration, or trimming inward to originalIn + 1 second
            const constrainedOut = Math.max(trimState.originalIn + TICKS_PER_SECOND, Math.min(newOut, trimState.assetDurationTicks));
            clipDuration = (constrainedOut - trimState.originalIn) / TICKS_PER_SECOND;
          }
        }
        
        const x = leftMargin + (clipStartTime * pixelsPerSecond) - scrollX;
        const width = clipDuration * pixelsPerSecond;
        
        // Draw rounded rectangle for clip
        const clipY = y + 5;
        const clipHeight = trackHeight - 10;
        const radius = 6;
        
        // Draw text clip (purple background with text)
        ctx.save();
        drawRoundedRect(ctx, x, clipY, width, clipHeight, radius);
        ctx.clip();
        
        // Draw purple background for text clip
        ctx.fillStyle = isSelected ? '#a855f7' : '#9333ea';
        ctx.fillRect(x, clipY, width, clipHeight);
        
        ctx.restore();
        
        // Draw text content if available (prefer text_content from backend)
        const textContent = clip.text_content || clip.text || clip.defaultText || 'Add text';
        ctx.fillStyle = '#ffffff';
        ctx.font = '12px sans-serif';
        ctx.textAlign = 'center';
        ctx.textBaseline = 'middle';
        const maxTextWidth = width - 10;
        const textX = x + width / 2;
        const textY = y + trackHeight / 2;
        
        // Truncate text if too long
        let displayText = textContent;
        const metrics = ctx.measureText(displayText);
        if (metrics.width > maxTextWidth) {
          // Truncate with ellipsis
          while (ctx.measureText(displayText + '...').width > maxTextWidth && displayText.length > 0) {
            displayText = displayText.slice(0, -1);
          }
          displayText += '...';
        }
        ctx.fillText(displayText, textX, textY);

        // Draw border on top - yellow if selected, purple otherwise
        ctx.strokeStyle = isSelected ? '#fbbf24' : '#a855f7';
        ctx.lineWidth = isSelected ? 3 : 1;
        drawRoundedRect(ctx, x, clipY, width, clipHeight, radius);
        ctx.stroke();
        
        // Draw trim handles if pointer tool and clip is selected or hovered
        if (activeTool === 'pointer' && (isSelected || hoveredEdge?.clipId === clipId)) {
          const handleWidth = 5;
          // Left edge
          if (hoveredEdge && hoveredEdge.clipId === clipId && hoveredEdge.edge === 'left') {
            ctx.fillStyle = '#10b981';
            ctx.fillRect(x - handleWidth / 2, y + 5, handleWidth, trackHeight - 10);
          }
          // Right edge
          if (hoveredEdge && hoveredEdge.clipId === clipId && hoveredEdge.edge === 'right') {
            ctx.fillStyle = '#10b981';
            ctx.fillRect(x + width - handleWidth / 2, y + 5, handleWidth, trackHeight - 10);
          }
        }
      });

      y += trackHeight + trackSpacing;
    });
    
    // Helper function to draw a video track (used for both overlays and primary)
    const drawVideoTrack = (track: any, trackY: number) => {
      // Draw track background
      ctx.fillStyle = '#252525';
      ctx.fillRect(leftMargin, trackY, canvas.width - leftMargin, trackHeight);

      // Draw track border
      ctx.strokeStyle = '#404040';
      ctx.strokeRect(leftMargin, trackY, canvas.width - leftMargin, trackHeight);

      // Draw clips
      track.clips?.forEach((clip: any) => {
        const clipId = clip.id || `${clip.asset_id}-${clip.timeline_start_ticks}`;
        
        // Skip drawing if this clip is being dragged
        if (draggedClip && (draggedClip.clip.id === clipId || 
            (draggedClip.clip.asset_id === clip.asset_id && 
             draggedClip.clip.timeline_start_ticks === clip.timeline_start_ticks))) {
          return;
        }
        
        const isSelected = selectedClip && 
          (selectedClip.id === clipId || 
           (selectedClip.id === clip.id) ||
           (selectedClip.asset_id === clip.asset_id && 
            selectedClip.timeline_start_ticks === clip.timeline_start_ticks));
        
        // Calculate clip position - handle trimming
        let clipStartTime = clip.timeline_start_ticks / TICKS_PER_SECOND;
        let clipDuration = (clip.out_ticks - clip.in_ticks) / TICKS_PER_SECOND;
        
        if (trimState && trimState.clipId === clipId) {
          if (trimState.edge === 'left') {
            const newIn = (trimState.originalIn + (hoverX! - trimState.startX) / pixelsPerSecond * TICKS_PER_SECOND);
            const constrainedIn = Math.max(0, Math.min(newIn, trimState.originalOut - TICKS_PER_SECOND));
            const inAdjustment = constrainedIn - trimState.originalIn;
            clipStartTime = (trimState.originalStart + inAdjustment / TICKS_PER_SECOND) / TICKS_PER_SECOND;
            clipDuration = (trimState.originalOut - constrainedIn) / TICKS_PER_SECOND;
          } else {
            const newOut = (trimState.originalOut + (hoverX! - trimState.startX) / pixelsPerSecond * TICKS_PER_SECOND);
            const constrainedOut = Math.max(trimState.originalIn + TICKS_PER_SECOND, Math.min(newOut, trimState.assetDurationTicks));
            clipDuration = (constrainedOut - trimState.originalIn) / TICKS_PER_SECOND;
          }
        }
        
        const x = leftMargin + (clipStartTime * pixelsPerSecond) - scrollX;
        const width = clipDuration * pixelsPerSecond;
        
        const clipY = trackY + 5;
        const clipHeight = trackHeight - 10;
        const radius = 6;
        
        ctx.save();
        drawRoundedRect(ctx, x, clipY, width, clipHeight, radius);
        ctx.clip();
        
        let hasThumbnails = false;
        if (width >= 80) {
          const thumbSpacingPx = 80;
          const numThumbs = Math.ceil(width / thumbSpacingPx);
          
          if (numThumbs > 0) {
            const sourceStartTime = clip.in_ticks / TICKS_PER_SECOND;
            const sourceEndTime = clip.out_ticks / TICKS_PER_SECOND;
            const sourceDuration = sourceEndTime - sourceStartTime;
            
            for (let i = 0; i < numThumbs; i++) {
              const segmentStart = x + (i * thumbSpacingPx);
              const segmentEnd = i === numThumbs - 1 ? x + width : x + ((i + 1) * thumbSpacingPx);
              const segmentWidth = segmentEnd - segmentStart;
              
              if (segmentEnd >= leftMargin && segmentStart <= canvas.width && segmentWidth > 0) {
                const sampleTime = sourceStartTime + (i / Math.max(1, numThumbs - 1)) * sourceDuration;
                const timestampSec = Math.floor(sampleTime);
                const cacheKey = `${clip.asset_id}_${timestampSec}`;
                const thumbnail = thumbnailCache.get(cacheKey);
                
                if (thumbnail && thumbnail.complete && thumbnail.naturalWidth > 0) {
                  hasThumbnails = true;
                  const thumbAspect = thumbnail.width / thumbnail.height;
                  const targetAspect = segmentWidth / clipHeight;
                  
                  let drawWidth = segmentWidth;
                  let drawHeight = clipHeight;
                  let drawX = segmentStart;
                  let drawY = clipY;
                  
                  if (thumbAspect > targetAspect) {
                    drawWidth = clipHeight * thumbAspect;
                    drawX = segmentStart - (drawWidth - segmentWidth) / 2;
                  } else {
                    drawHeight = segmentWidth / thumbAspect;
                    drawY = clipY - (drawHeight - clipHeight) / 2;
                  }
                  
                  ctx.drawImage(thumbnail, drawX, drawY, drawWidth, drawHeight);
                } else {
                  loadThumbnail(clip.asset_id, timestampSec, projectId).then((img) => {
                    if (img && canvasRef.current) {
                      setLoadedThumbnails((prev) => {
                        const newMap = new Map(prev);
                        newMap.set(cacheKey, img);
                        return newMap;
                      });
                    }
                  });
                }
              }
            }
          }
        }
        
        if (!hasThumbnails) {
          ctx.fillStyle = isSelected ? '#10b981' : '#3b82f6';
          ctx.fillRect(x, clipY, width, clipHeight);
        }
        
        ctx.restore();

        ctx.strokeStyle = isSelected ? '#fbbf24' : '#2563eb';
        ctx.lineWidth = isSelected ? 3 : 1;
        drawRoundedRect(ctx, x, clipY, width, clipHeight, radius);
        ctx.stroke();
        
        if (activeTool === 'pointer' && (isSelected || hoveredEdge?.clipId === clipId)) {
          const handleWidth = 5;
          if (hoveredEdge && hoveredEdge.clipId === clipId && hoveredEdge.edge === 'left') {
            ctx.fillStyle = '#10b981';
            ctx.fillRect(x - handleWidth / 2, trackY + 5, handleWidth, trackHeight - 10);
          }
          if (hoveredEdge && hoveredEdge.clipId === clipId && hoveredEdge.edge === 'right') {
            ctx.fillStyle = '#10b981';
            ctx.fillRect(x + width - handleWidth / 2, trackY + 5, handleWidth, trackHeight - 10);
          }
        }
      });
    };
    
    // Calculate primary track Y position (will be set after overlays)
    const primaryTrackY = y + (overlayVideoTracks.length * (trackHeight + trackSpacing));
    
    // Draw overlay tracks first (above primary, stacking upward)
    // Render in reverse order so highest ID is closest to primary
    [...overlayVideoTracks].reverse().forEach((track) => {
      const trackIndex = overlayVideoTracks.indexOf(track);
      const trackY = primaryTrackY - (trackHeight + trackSpacing) * (overlayVideoTracks.length - trackIndex);
      drawVideoTrack(track, trackY);
    });
    
    // Draw primary video track
    if (primaryVideoTrack) {
      drawVideoTrack(primaryVideoTrack, primaryTrackY);
      y = primaryTrackY + trackHeight + trackSpacing;
    } else {
      // If no primary track exists, set y to where it would be
      y = primaryTrackY + trackHeight + trackSpacing;
    }

    // Draw Audio tracks - below video
    audioTracks.forEach((track) => {
      ctx.fillStyle = '#252525';
      ctx.fillRect(leftMargin, y, canvas.width - leftMargin, trackHeight);
      ctx.strokeStyle = '#404040';
      ctx.strokeRect(leftMargin, y, canvas.width - leftMargin, trackHeight);

      track.clips?.forEach((clip: any) => {
        const clipId = clip.id || `${clip.asset_id}-${clip.timeline_start_ticks}`;
        
        if (draggedClip && (draggedClip.clip.id === clipId || 
            (draggedClip.clip.asset_id === clip.asset_id && 
             draggedClip.clip.timeline_start_ticks === clip.timeline_start_ticks))) {
          return;
        }
        
        const isSelected = selectedClip && 
          (selectedClip.id === clipId || 
           (selectedClip.id === clip.id) ||
           (selectedClip.asset_id === clip.asset_id && 
            selectedClip.timeline_start_ticks === clip.timeline_start_ticks));
        
        let clipStartTime = clip.timeline_start_ticks / TICKS_PER_SECOND;
        let clipDuration = (clip.out_ticks - clip.in_ticks) / TICKS_PER_SECOND;
        
        if (trimState && trimState.clipId === clipId) {
          if (trimState.edge === 'left') {
            const newIn = (trimState.originalIn + (hoverX! - trimState.startX) / pixelsPerSecond * TICKS_PER_SECOND);
            const constrainedIn = Math.max(0, Math.min(newIn, trimState.originalOut - TICKS_PER_SECOND));
            const inAdjustment = constrainedIn - trimState.originalIn;
            clipStartTime = (trimState.originalStart + inAdjustment / TICKS_PER_SECOND) / TICKS_PER_SECOND;
            clipDuration = (trimState.originalOut - constrainedIn) / TICKS_PER_SECOND;
          } else {
            const newOut = (trimState.originalOut + (hoverX! - trimState.startX) / pixelsPerSecond * TICKS_PER_SECOND);
            const constrainedOut = Math.max(trimState.originalIn + TICKS_PER_SECOND, Math.min(newOut, trimState.assetDurationTicks));
            clipDuration = (constrainedOut - trimState.originalIn) / TICKS_PER_SECOND;
          }
        }
        
        const x = leftMargin + (clipStartTime * pixelsPerSecond) - scrollX;
        const width = clipDuration * pixelsPerSecond;
        const clipY = y + 5;
        const clipHeight = trackHeight - 10;
        const radius = 6;
        
        ctx.save();
        drawRoundedRect(ctx, x, clipY, width, clipHeight, radius);
        ctx.clip();
        
        ctx.fillStyle = isSelected ? '#10b981' : '#059669';
        ctx.fillRect(x, clipY, width, clipHeight);
        
        ctx.restore();
        ctx.fillStyle = '#ffffff';
        ctx.font = '12px sans-serif';
        ctx.textAlign = 'center';
        ctx.fillText('🎵', x + width / 2, y + trackHeight / 2 + 4);
        
        ctx.strokeStyle = isSelected ? '#fbbf24' : '#10b981';
        ctx.lineWidth = isSelected ? 3 : 1;
        drawRoundedRect(ctx, x, clipY, width, clipHeight, radius);
        ctx.stroke();
        
        if (activeTool === 'pointer' && (isSelected || hoveredEdge?.clipId === clipId)) {
          const handleWidth = 5;
          if (hoveredEdge && hoveredEdge.clipId === clipId && hoveredEdge.edge === 'left') {
            ctx.fillStyle = '#10b981';
            ctx.fillRect(x - handleWidth / 2, y + 5, handleWidth, trackHeight - 10);
          }
          if (hoveredEdge && hoveredEdge.clipId === clipId && hoveredEdge.edge === 'right') {
            ctx.fillStyle = '#10b981';
            ctx.fillRect(x + width - handleWidth / 2, y + 5, handleWidth, trackHeight - 10);
          }
        }
      });

      y += trackHeight + trackSpacing;
    });

    // Draw ghost preview when dragging text template
    if (dragTextTemplate && dropPosition) {
      const dropTime = dropPosition.time;
      const insertX = leftMargin + (dropTime * pixelsPerSecond) - scrollX;
      const defaultDuration = 3; // 3 seconds default for text clips
      const previewWidth = defaultDuration * pixelsPerSecond;
      const previewY = 50 + 5; // First caption track
      const previewHeight = trackHeight - 10;
      const radius = 6;
      
      if (insertX >= leftMargin && insertX <= canvas.width) {
        // Purple overlay indicator for text
        ctx.strokeStyle = '#a855f7';
        ctx.lineWidth = 2;
        ctx.setLineDash([5, 5]);
        ctx.beginPath();
        ctx.moveTo(insertX, 30);
        ctx.lineTo(insertX, Math.max(y, 50 + trackHeight));
        ctx.stroke();
        ctx.setLineDash([]);
        
        ctx.strokeStyle = '#a855f7';
        ctx.lineWidth = 2;
        ctx.setLineDash([8, 4]);
        drawRoundedRect(ctx, insertX, previewY, previewWidth, previewHeight, radius);
        ctx.stroke();
        ctx.setLineDash([]);
        
        ctx.fillStyle = 'rgba(168, 85, 247, 0.2)';
        drawRoundedRect(ctx, insertX, previewY, previewWidth, previewHeight, radius);
        ctx.fill();
      }
    }
    
    // Draw ghost preview when dragging audio asset
    if (dragAudioAsset && dropPosition) {
      const dropTime = dropPosition.time;
      const insertX = leftMargin + (dropTime * pixelsPerSecond) - scrollX;
      const previewWidth = Math.max(20, (dragAudioAsset.duration_ticks / TICKS_PER_SECOND) * pixelsPerSecond);
      // Find first audio track Y position (after video tracks)
      const numCaptionTracks = captionTracks.length;
      const numVideoTracks = videoTracks.length;
      const audioTrackY = 50 + (numCaptionTracks + numVideoTracks) * (trackHeight + trackSpacing);
      const previewY = audioTrackY + 5;
      const previewHeight = trackHeight - 10;
      const radius = 6;
      
      if (insertX >= leftMargin && insertX <= canvas.width) {
        // Green overlay indicator for audio
        ctx.strokeStyle = '#10b981';
        ctx.lineWidth = 2;
        ctx.setLineDash([5, 5]);
        ctx.beginPath();
        ctx.moveTo(insertX, 30);
        ctx.lineTo(insertX, Math.max(y, audioTrackY + trackHeight));
        ctx.stroke();
        ctx.setLineDash([]);
        
        ctx.strokeStyle = '#10b981';
        ctx.lineWidth = 2;
        ctx.setLineDash([8, 4]);
        drawRoundedRect(ctx, insertX, previewY, previewWidth, previewHeight, radius);
        ctx.stroke();
        ctx.setLineDash([]);
        
        ctx.fillStyle = 'rgba(16, 185, 129, 0.2)';
        drawRoundedRect(ctx, insertX, previewY, previewWidth, previewHeight, radius);
        ctx.fill();
      }
    }

    // Draw ghost preview when dragging video asset
    if (dragAsset && dropPosition) {
      const dropTime = dropPosition.time;
      const insertX = leftMargin + (dropTime * pixelsPerSecond) - scrollX;
      
      // Calculate track Y position
      let dropY = 50;
      if (timeline.tracks && timeline.tracks.length > 0) {
        // Find track index
        let trackIndex = 0;
        for (let i = 0; i < timeline.tracks.length; i++) {
          const track = timeline.tracks[i];
          if ((track.id || (i + 1)) === dropPosition.trackId) {
            trackIndex = i;
            break;
          }
        }
        dropY = 50 + trackIndex * (trackHeight + trackSpacing);
      }
      
      if (insertX >= leftMargin && insertX <= canvas.width && dropY >= 50) {
        const previewWidth = Math.max(20, (dragAsset.duration_ticks / TICKS_PER_SECOND) * pixelsPerSecond);
        const previewY = dropY + 5;
        const previewHeight = trackHeight - 10;
        const radius = 6;
        
        // Different visual feedback based on intent
        if (dropPosition.intent === 'layered') {
          // Layered insert - blue color, above primary track
          ctx.strokeStyle = '#3b82f6';
          ctx.lineWidth = 2;
          ctx.setLineDash([5, 5]);
          ctx.beginPath();
          ctx.moveTo(insertX, 30);
          ctx.lineTo(insertX, Math.max(y, dropY + trackHeight));
          ctx.stroke();
          ctx.setLineDash([]);
          
          ctx.strokeStyle = '#3b82f6';
          ctx.lineWidth = 2;
          ctx.setLineDash([8, 4]);
          drawRoundedRect(ctx, insertX, previewY, previewWidth, previewHeight, radius);
          ctx.stroke();
          ctx.setLineDash([]);
          
          ctx.fillStyle = 'rgba(59, 130, 246, 0.2)';
          drawRoundedRect(ctx, insertX, previewY, previewWidth, previewHeight, radius);
          ctx.fill();
          
          // Highlight underlying primary clip at drop time
          if (timeline && timeline.tracks) {
            const primaryTrack = timeline.tracks.find(t => (t.id || 1) === 1);
            if (primaryTrack && primaryTrack.clips) {
              const dropTime = dropPosition.time;
              const dropTimeTicks = dropTime * TICKS_PER_SECOND;
              
              // Find the primary clip at drop time
              for (const clip of primaryTrack.clips) {
                const clipStartTicks = clip.timeline_start_ticks;
                const clipDuration = clip.out_ticks - clip.in_ticks;
                const clipEndTicks = clipStartTicks + clipDuration;
                
                if (dropTimeTicks >= clipStartTicks && dropTimeTicks < clipEndTicks) {
                  // This is the underlying clip - highlight it
                  const clipStartTime = clipStartTicks / TICKS_PER_SECOND;
                  const clipDurationSec = clipDuration / TICKS_PER_SECOND;
                  const clipX = leftMargin + (clipStartTime * pixelsPerSecond) - scrollX;
                  const clipWidth = clipDurationSec * pixelsPerSecond;
                  const clipY = dropY + 5;
                  const clipHeight = trackHeight - 10;
                  
                  // Draw highlight overlay on primary clip
                  ctx.fillStyle = 'rgba(59, 130, 246, 0.15)';
                  drawRoundedRect(ctx, clipX, clipY, clipWidth, clipHeight, radius);
                  ctx.fill();
                  
                  // Draw connection line from overlay to primary clip
                  const overlayCenterY = previewY + previewHeight / 2;
                  const primaryCenterY = clipY + clipHeight / 2;
                  ctx.strokeStyle = '#3b82f6';
                  ctx.lineWidth = 1.5;
                  ctx.setLineDash([3, 3]);
                  ctx.beginPath();
                  ctx.moveTo(insertX, overlayCenterY);
                  ctx.lineTo(insertX, primaryCenterY);
                  ctx.stroke();
                  ctx.setLineDash([]);
                  
                  break;
                }
              }
            }
          }
        } else {
          // Primary insert - green color, show ripple preview
          ctx.strokeStyle = '#10b981';
          ctx.lineWidth = 2;
          ctx.setLineDash([5, 5]);
          ctx.beginPath();
          ctx.moveTo(insertX, 30);
          ctx.lineTo(insertX, Math.max(y, dropY + trackHeight));
          ctx.stroke();
          ctx.setLineDash([]);
          
          // Draw ghost preview
          ctx.strokeStyle = '#10b981';
          ctx.lineWidth = 2;
          ctx.setLineDash([8, 4]);
          drawRoundedRect(ctx, insertX, previewY, previewWidth, previewHeight, radius);
          ctx.stroke();
          ctx.setLineDash([]);
          
          ctx.fillStyle = 'rgba(16, 185, 129, 0.2)';
          drawRoundedRect(ctx, insertX, previewY, previewWidth, previewHeight, radius);
          ctx.fill();
          
          // Show ripple effect - preview shifted clips on primary track
          if (timeline.tracks && timeline.tracks.length > 0) {
            const primaryTrack = timeline.tracks.find(t => (t.id || 1) === 1);
            if (primaryTrack) {
              const rippleDuration = dragAsset.duration_ticks / TICKS_PER_SECOND;
              primaryTrack.clips?.forEach((clip: any) => {
                const clipStartTime = clip.timeline_start_ticks / TICKS_PER_SECOND;
                if (clipStartTime >= dropTime) {
                  // This clip will be shifted - show preview
                  const shiftedStartTime = clipStartTime + rippleDuration;
                  const shiftedX = leftMargin + (shiftedStartTime * pixelsPerSecond) - scrollX;
                  const clipDuration = (clip.out_ticks - clip.in_ticks) / TICKS_PER_SECOND;
                  const clipWidth = clipDuration * pixelsPerSecond;
                  
                  if (shiftedX + clipWidth >= leftMargin && shiftedX <= canvas.width) {
                    ctx.strokeStyle = 'rgba(16, 185, 129, 0.5)';
                    ctx.lineWidth = 1;
                    ctx.setLineDash([4, 4]);
                    drawRoundedRect(ctx, shiftedX, previewY, clipWidth, previewHeight, radius);
                    ctx.stroke();
                    ctx.setLineDash([]);
                  }
                }
              });
            }
          }
        }
      }
    }

    // Draw clip drag feedback
    if (draggedClip && clipDropPosition && timeline) {
      const clip = draggedClip.clip;
      const clipDuration = (clip.out_ticks - clip.in_ticks) / TICKS_PER_SECOND;
      const dropTime = clipDropPosition.time;
      const dropX = leftMargin + (dropTime * pixelsPerSecond) - scrollX;
      const trackHeight = 60;
      const trackSpacing = 10;
      
      // Calculate primary track Y position (accounting for caption tracks and existing overlay tracks)
      const captionTracks = timeline.tracks?.filter(t => t.kind === 'Caption' || t.kind === 'caption') || [];
      const existingOverlayTracks = timeline.tracks?.filter(t => 
        (t.kind === 'Video' || t.kind === 'video' || !t.kind) && 
        (t.id || 1) > 1 && 
        (t.clips?.length || 0) > 0
      ) || [];
      const primaryTrackY = 50 + (captionTracks.length * (trackHeight + trackSpacing)) + (existingOverlayTracks.length * (trackHeight + trackSpacing));
      
      const previewHeight = trackHeight - 10;
      const radius = 6;
      const previewWidth = clipDuration * pixelsPerSecond;
      
      if (dropX >= leftMargin && dropX <= canvas.width) {
        if (clipDropPosition.intent === 'primary') {
          // Primary reorder - show on primary track
          const previewY = primaryTrackY + 5;
          // Primary reorder - show insertion indicator and ripple preview
          // Draw insertion indicator (vertical line)
          ctx.strokeStyle = '#10b981';
          ctx.lineWidth = 2;
          ctx.setLineDash([5, 5]);
          ctx.beginPath();
          ctx.moveTo(dropX, 30);
          ctx.lineTo(dropX, Math.max(y, primaryTrackY + trackHeight));
          ctx.stroke();
          ctx.setLineDash([]);
          
          // Add track highlight on primary track
          ctx.fillStyle = 'rgba(16, 185, 129, 0.08)';
          ctx.fillRect(leftMargin, primaryTrackY, canvas.width - leftMargin, trackHeight);
          
          // Draw ghost preview of dragged clip (more prominent)
          ctx.strokeStyle = '#10b981';
          ctx.lineWidth = 3; // Increased from 2 for better visibility
          ctx.setLineDash([8, 4]);
          drawRoundedRect(ctx, dropX, previewY, previewWidth, previewHeight, radius);
          ctx.stroke();
          ctx.setLineDash([]);
          
          ctx.fillStyle = 'rgba(16, 185, 129, 0.25)'; // Increased opacity from 0.2
          drawRoundedRect(ctx, dropX, previewY, previewWidth, previewHeight, radius);
          ctx.fill();
          
          // Show ripple preview - preview shifted clips
          const primaryTrack = timeline.tracks?.find(t => (t.id || 1) === 1);
          if (primaryTrack) {
            primaryTrack.clips?.forEach((otherClip: any) => {
              const otherClipId = otherClip.id || `${otherClip.asset_id}-${otherClip.timeline_start_ticks}`;
              // Skip the dragged clip
              if (draggedClip.clip.id === otherClipId || 
                  (draggedClip.clip.asset_id === otherClip.asset_id && 
                   draggedClip.clip.timeline_start_ticks === otherClip.timeline_start_ticks)) {
                return;
              }
              
              const otherClipStartTime = otherClip.timeline_start_ticks / TICKS_PER_SECOND;
              const otherClipDuration = (otherClip.out_ticks - otherClip.in_ticks) / TICKS_PER_SECOND;
              
              // Calculate if this clip will be shifted
              const originalStart = draggedClip.originalPosition / TICKS_PER_SECOND;
              let shiftedStartTime = otherClipStartTime;
              
              // If clip is after original position, it shifts left
              if (otherClipStartTime > originalStart) {
                shiftedStartTime = otherClipStartTime - clipDuration;
              }
              
              // If clip is at/after drop position, it shifts right
              if (otherClipStartTime >= dropTime) {
                shiftedStartTime = otherClipStartTime + clipDuration;
              }
              
              // Only show preview if position changed
              if (Math.abs(shiftedStartTime - otherClipStartTime) > 0.01) {
                const shiftedX = leftMargin + (shiftedStartTime * pixelsPerSecond) - scrollX;
                const otherClipWidth = otherClipDuration * pixelsPerSecond;
                
                if (shiftedX + otherClipWidth >= leftMargin && shiftedX <= canvas.width) {
                  ctx.strokeStyle = 'rgba(16, 185, 129, 0.5)';
                  ctx.lineWidth = 1;
                  ctx.setLineDash([4, 4]);
                  drawRoundedRect(ctx, shiftedX, previewY, otherClipWidth, previewHeight, radius);
                  ctx.stroke();
                  ctx.setLineDash([]);
                }
              }
            });
          }
        } else {
          // Layered overlay - show preview above primary track
          // Calculate overlay track Y position (above primary, accounting for existing overlays)
          const overlayPreviewY = primaryTrackY - (trackHeight + trackSpacing);
          
          // Draw insertion line (vertical line at drop time)
          ctx.strokeStyle = '#3b82f6';
          ctx.lineWidth = 2;
          ctx.setLineDash([5, 5]);
          ctx.beginPath();
          ctx.moveTo(dropX, 30);
          ctx.lineTo(dropX, Math.max(y, primaryTrackY + trackHeight));
          ctx.stroke();
          ctx.setLineDash([]);
          
          // Draw overlay preview at the overlay track position (more prominent)
          const overlayClipY = overlayPreviewY + 5;
          
          // Add track highlight on overlay track
          ctx.fillStyle = 'rgba(59, 130, 246, 0.08)';
          ctx.fillRect(leftMargin, overlayPreviewY, canvas.width - leftMargin, trackHeight);
          
          ctx.strokeStyle = '#3b82f6';
          ctx.lineWidth = 3; // Increased from 2 for better visibility
          ctx.setLineDash([8, 4]);
          drawRoundedRect(ctx, dropX, overlayClipY, previewWidth, previewHeight, radius);
          ctx.stroke();
          ctx.setLineDash([]);
          
          ctx.fillStyle = 'rgba(59, 130, 246, 0.25)'; // Increased opacity from 0.2
          drawRoundedRect(ctx, dropX, overlayClipY, previewWidth, previewHeight, radius);
          ctx.fill();
          
          // Highlight underlying primary clip at drop time
          if (timeline && timeline.tracks) {
            const primaryTrack = timeline.tracks.find(t => (t.id || 1) === 1);
            if (primaryTrack && primaryTrack.clips) {
              const dropTime = clipDropPosition.time;
              const dropTimeTicks = dropTime * TICKS_PER_SECOND;
              
              // Find the primary clip at drop time
              for (const clip of primaryTrack.clips) {
                const clipStartTicks = clip.timeline_start_ticks;
                const clipDuration = clip.out_ticks - clip.in_ticks;
                const clipEndTicks = clipStartTicks + clipDuration;
                
                if (dropTimeTicks >= clipStartTicks && dropTimeTicks < clipEndTicks) {
                  // This is the underlying clip - highlight it
                  const clipStartTime = clipStartTicks / TICKS_PER_SECOND;
                  const clipDurationSec = clipDuration / TICKS_PER_SECOND;
                  const clipX = leftMargin + (clipStartTime * pixelsPerSecond) - scrollX;
                  const clipWidth = clipDurationSec * pixelsPerSecond;
                  const clipY = primaryTrackY + 5;
                  const clipHeight = trackHeight - 10;
                  
                  // Draw highlight overlay on primary clip
                  ctx.fillStyle = 'rgba(59, 130, 246, 0.15)';
                  drawRoundedRect(ctx, clipX, clipY, clipWidth, clipHeight, radius);
                  ctx.fill();
                  
                  // Draw connection line from overlay to primary clip (more prominent)
                  const overlayCenterY = overlayClipY + previewHeight / 2;
                  const primaryCenterY = clipY + clipHeight / 2;
                  ctx.strokeStyle = '#3b82f6';
                  ctx.lineWidth = 2.5; // Increased from 1.5 for better visibility
                  ctx.setLineDash([4, 4]); // Slightly longer dashes
                  ctx.beginPath();
                  ctx.moveTo(dropX, overlayCenterY);
                  ctx.lineTo(dropX, primaryCenterY);
                  ctx.stroke();
                  ctx.setLineDash([]);
                  
                  // Add track highlight on primary track
                  ctx.fillStyle = 'rgba(59, 130, 246, 0.08)';
                  ctx.fillRect(leftMargin, primaryTrackY, canvas.width - leftMargin, trackHeight);
                  
                  break;
                }
              }
            }
          }
        }
      }
    }

            // Draw hover playhead (yellow) - follows mouse
            if (hoverPlayheadTicks !== null) {
              const hoverPlayheadTime = hoverPlayheadTicks / TICKS_PER_SECOND;
              const hoverPlayheadX = leftMargin + (hoverPlayheadTime * pixelsPerSecond) - scrollX;
              if (hoverPlayheadX >= leftMargin && hoverPlayheadX <= canvas.width) {
                ctx.strokeStyle = '#fbbf24'; // Yellow
                ctx.lineWidth = 2;
                ctx.beginPath();
                ctx.moveTo(hoverPlayheadX, 0);
                ctx.lineTo(hoverPlayheadX, canvas.height);
                ctx.stroke();
              }
            }

            // Draw main playhead (red) - current playback position
            if (playheadPosition > 0) {
              const playheadTime = playheadPosition / TICKS_PER_SECOND;
              const playheadX = leftMargin + (playheadTime * pixelsPerSecond) - scrollX;
              if (playheadX >= leftMargin && playheadX <= canvas.width) {
                ctx.strokeStyle = '#ef4444'; // Red
                ctx.lineWidth = 2;
                ctx.beginPath();
                ctx.moveTo(playheadX, 0);
                ctx.lineTo(playheadX, canvas.height);
                ctx.stroke();
              }
            }
  }, [timeline, selectedClip, playheadPosition, hoverPlayheadTicks, pixelsPerSecond, scrollX, dragAsset, dropPosition, trimState, hoveredEdge, activeTool, hoverX, calculateEarliestNextTimestamp, drawRoundedRect, loadedThumbnails, projectId, draggedClip, clipDropPosition]);

  // Auto-scroll during playback to keep playhead visible (Final Cut behavior)
  useEffect(() => {
    if (!isPlaying || !canvasRef.current || playheadPosition === 0) return;

    const canvas = canvasRef.current;
    const leftMargin = 3 * PIXELS_PER_REM;
    const playheadTime = playheadPosition / TICKS_PER_SECOND;
    const playheadX = leftMargin + (playheadTime * pixelsPerSecond) - scrollX;
    
    // Define scroll margins (keep playhead within this area)
    const scrollMarginLeft = leftMargin + 100; // 100px from left edge
    const scrollMarginRight = canvas.width - 200; // 200px from right edge
    
    // Auto-scroll if playhead is approaching edges
    // Use requestAnimationFrame to avoid synchronous setState in effect
    if (playheadX < scrollMarginLeft) {
      // Playhead is too far left - scroll left
      requestAnimationFrame(() => {
        const newScrollX = Math.max(0, scrollX - (scrollMarginLeft - playheadX));
        setScrollX(newScrollX);
      });
    } else if (playheadX > scrollMarginRight) {
      // Playhead is too far right - scroll right
      requestAnimationFrame(() => {
        const newScrollX = scrollX + (playheadX - scrollMarginRight);
        setScrollX(newScrollX);
      });
    }
  }, [playheadPosition, isPlaying, pixelsPerSecond, scrollX]);

  const handleCanvasClick = (e: React.MouseEvent<HTMLCanvasElement>) => {
    if (!timeline || !canvasRef.current) return;

    const canvas = canvasRef.current;
    const rect = canvas.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top;
    const leftMargin = 3 * PIXELS_PER_REM;

    // Handle cut tool
    if (activeTool === 'cut') {
      const trackHeight = 60;
      const trackSpacing = 10;
      let currentY = 50;
      
      for (const track of timeline.tracks || []) {
        if (y >= currentY && y <= currentY + trackHeight) {
          for (const clip of track.clips || []) {
            const clipStartTime = clip.timeline_start_ticks / TICKS_PER_SECOND;
            const clipDuration = (clip.out_ticks - clip.in_ticks) / TICKS_PER_SECOND;
            const clipX = leftMargin + (clipStartTime * pixelsPerSecond) - scrollX;
            const clipWidth = clipDuration * pixelsPerSecond;
            
            if (x >= clipX && x <= clipX + clipWidth) {
              const clipId = clip.id || `${clip.asset_id}-${clip.timeline_start_ticks}`;
              const clickTime = (x - leftMargin + scrollX) / pixelsPerSecond;
              const positionTicks = Math.round(clickTime * TICKS_PER_SECOND);
              if (onClipSplit) {
                onClipSplit(clipId, positionTicks);
              }
              return;
            }
          }
        }
        currentY += trackHeight + trackSpacing;
      }
      return;
    }

    // Handle pointer tool - Final Cut behavior: clicking always moves playhead
    const trackHeight = 60;
    const trackSpacing = 10;
    let clickedClip: any = null;
    
    // Find clicked clip - must match rendering order: caption, overlay, primary, audio
    if (timeline.tracks) {
      // Separate tracks by kind to match rendering order
      const captionTracks = timeline.tracks.filter(t => t.kind === 'Caption' || t.kind === 'caption');
      const videoTracks = timeline.tracks.filter(t => t.kind === 'Video' || t.kind === 'video' || !t.kind);
      const audioTracks = timeline.tracks.filter(t => t.kind === 'Audio' || t.kind === 'audio');
      
      // Separate video tracks into primary and overlays
      // Match rendering: sort ascending, then reverse when iterating
      const primaryVideoTrack = videoTracks.find(t => (t.id || 1) === 1);
      const overlayVideoTracks = videoTracks
        .filter(t => (t.id || 1) > 1 && (t.clips?.length || 0) > 0)
        .sort((a, b) => (a.id || 0) - (b.id || 0)); // Sort ascending (matches rendering)
      
      let currentY = 50;
      
      // Check caption tracks first
      for (const track of captionTracks) {
        if (y >= currentY && y <= currentY + trackHeight) {
          for (const clip of track.clips || []) {
            const clipStartTime = clip.timeline_start_ticks / TICKS_PER_SECOND;
            const clipDuration = (clip.out_ticks - clip.in_ticks) / TICKS_PER_SECOND;
            const clipX = leftMargin + (clipStartTime * pixelsPerSecond) - scrollX;
            const clipWidth = clipDuration * pixelsPerSecond;
            
            if (x >= clipX && x <= clipX + clipWidth) {
              clickedClip = clip;
              break;
            }
          }
          if (clickedClip) break;
        }
        currentY += trackHeight + trackSpacing;
      }
      
      // Check overlay video tracks (top to bottom) - rendered ABOVE primary track
      // Match rendering: iterate in reverse order (highest ID closest to primary)
      if (!clickedClip) {
        const primaryTrackY = 50 + (captionTracks.length * (trackHeight + trackSpacing));
        // Iterate in reverse order to match rendering
        for (let i = overlayVideoTracks.length - 1; i >= 0; i--) {
          const track = overlayVideoTracks[i];
          const trackIndex = overlayVideoTracks.indexOf(track);
          // Match rendering calculation exactly
          const trackY = primaryTrackY - (trackHeight + trackSpacing) * (overlayVideoTracks.length - trackIndex);
          if (y >= trackY && y <= trackY + trackHeight) {
            for (const clip of track.clips || []) {
              const clipStartTime = clip.timeline_start_ticks / TICKS_PER_SECOND;
              const clipDuration = (clip.out_ticks - clip.in_ticks) / TICKS_PER_SECOND;
              const clipX = leftMargin + (clipStartTime * pixelsPerSecond) - scrollX;
              const clipWidth = clipDuration * pixelsPerSecond;
              
              if (x >= clipX && x <= clipX + clipWidth) {
                clickedClip = clip;
                break;
              }
            }
            if (clickedClip) break;
          }
        }
      }
      
      // Check primary video track
      if (!clickedClip && primaryVideoTrack) {
        const primaryTrackY = 50 + (captionTracks.length * (trackHeight + trackSpacing));
        if (y >= primaryTrackY && y <= primaryTrackY + trackHeight) {
          for (const clip of primaryVideoTrack.clips || []) {
            const clipStartTime = clip.timeline_start_ticks / TICKS_PER_SECOND;
            const clipDuration = (clip.out_ticks - clip.in_ticks) / TICKS_PER_SECOND;
            const clipX = leftMargin + (clipStartTime * pixelsPerSecond) - scrollX;
            const clipWidth = clipDuration * pixelsPerSecond;
            
            if (x >= clipX && x <= clipX + clipWidth) {
              clickedClip = clip;
              break;
            }
          }
        }
      }
      
      // Check audio tracks
      if (!clickedClip) {
        const primaryTrackY = 50 + (captionTracks.length * (trackHeight + trackSpacing));
        const videoTracksY = primaryTrackY + (primaryVideoTrack ? trackHeight + trackSpacing : 0) + (overlayVideoTracks.length * (trackHeight + trackSpacing));
        currentY = videoTracksY;
        for (const track of audioTracks) {
          if (y >= currentY && y <= currentY + trackHeight) {
            for (const clip of track.clips || []) {
              const clipStartTime = clip.timeline_start_ticks / TICKS_PER_SECOND;
              const clipDuration = (clip.out_ticks - clip.in_ticks) / TICKS_PER_SECOND;
              const clipX = leftMargin + (clipStartTime * pixelsPerSecond) - scrollX;
              const clipWidth = clipDuration * pixelsPerSecond;
              
              if (x >= clipX && x <= clipX + clipWidth) {
                clickedClip = clip;
                break;
              }
            }
            if (clickedClip) break;
          }
          currentY += trackHeight + trackSpacing;
        }
      }
    }
    
    // Always set playhead to clicked position (Final Cut behavior)
    if (onPlayheadSet) {
      const clickTime = (x - leftMargin + scrollX) / pixelsPerSecond;
      const positionTicks = Math.round(clickTime * TICKS_PER_SECOND);
      onPlayheadSet(positionTicks);
    }
    
    // If a clip was clicked, also select it
    if (clickedClip && onClipClick) {
      onClipClick(clickedClip, e);
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
    const mouseY = e.clientY - rect.top;
    setHoverX(mouseX);

    const leftMargin = 3 * PIXELS_PER_REM;
    const trackHeight = 60;
    const trackSpacing = 10;
    const edgeThreshold = 5; // pixels
    
    // Calculate hover time and update hover playhead
    if (mouseX >= leftMargin && mouseX <= canvas.width) {
      const hoverTime = (mouseX - leftMargin + scrollX) / pixelsPerSecond;
      const hoverTicks = Math.round(hoverTime * TICKS_PER_SECOND);
      setHoverPlayheadTicks(hoverTicks);
      if (onHoverTimeChange) {
        onHoverTimeChange(hoverTime);
      }
    } else {
      // Clear hover playhead when mouse leaves timeline area
      setHoverPlayheadTicks(null);
    }

    // Check if potential drag should become actual drag (requires intentional movement)
    const DRAG_THRESHOLD = 5; // pixels - minimum movement to start dragging (reduced for better responsiveness)
    if (potentialDrag && !draggedClip) {
      const moveDistance = Math.sqrt(
        Math.pow(mouseX - potentialDrag.startX, 2) + 
        Math.pow(mouseY - potentialDrag.startY, 2)
      );
      
      // Only start dragging if user has moved mouse significantly
      if (moveDistance >= DRAG_THRESHOLD) {
        setDraggedClip(potentialDrag);
        setPotentialDrag(null);
      }
    }
    
    // Handle clip drag
    if (draggedClip && timeline && onClipReorder) {
      const leftMargin = 3 * PIXELS_PER_REM;
      const trackHeight = 60;
      const trackSpacing = 10;
      
      // Calculate primary track Y position (accounting for caption tracks only)
      // Don't account for existing overlay tracks since we might be creating a new one
      const captionTracks = timeline.tracks?.filter(t => t.kind === 'Caption' || t.kind === 'caption') || [];
      const primaryTrackY = 50 + (captionTracks.length * (trackHeight + trackSpacing));
      const primaryTrackBottom = primaryTrackY + trackHeight;
      
      // Add buffer zone around primary track boundary for easier intent detection
      // Buffer zone is 15 pixels above and below the primary track
      const BUFFER_ZONE = 15;
      const primaryTrackTopWithBuffer = primaryTrackY - BUFFER_ZONE;
      const primaryTrackBottomWithBuffer = primaryTrackBottom + BUFFER_ZONE;
      
      // Detect intent based on absolute Y position with buffer zones (Final Cut style)
      // If mouseY is above primary track (including buffer), it's overlay intent
      // If mouseY is on or below primary track (including buffer), it's primary reorder intent
      let intent: 'primary' | 'layered';
      if (mouseY < primaryTrackTopWithBuffer) {
        intent = 'layered'; // Clearly above primary track
      } else if (mouseY > primaryTrackBottomWithBuffer) {
        intent = 'primary'; // Clearly below primary track
      } else {
        // In buffer zone - use center of primary track as decision point
        const primaryTrackCenter = primaryTrackY + (trackHeight / 2);
        intent = mouseY < primaryTrackCenter ? 'layered' : 'primary';
      }
      
      // Require minimum horizontal drag distance before showing preview (5 pixels for better responsiveness)
      const minDragDistance = 5 / pixelsPerSecond; // Convert to seconds
      const horizontalDelta = Math.abs(mouseX - draggedClip.startX);
      const horizontalDeltaTime = horizontalDelta / pixelsPerSecond;
      
      // Calculate drop time from mouse position
      let dropTime = (mouseX - leftMargin + scrollX) / pixelsPerSecond;
      dropTime = Math.max(0, dropTime);
      
      // For primary intent, only show preview if dragged to a position that would change order
      if (intent === 'primary' && horizontalDeltaTime >= minDragDistance) {
        const primaryTrack = timeline.tracks?.find(t => (t.id || 1) === 1);
        if (primaryTrack && primaryTrack.clips) {
          const draggedClipId = draggedClip.clip.id || `${draggedClip.clip.asset_id}-${draggedClip.clip.timeline_start_ticks}`;
          const originalStartTime = draggedClip.originalPosition / TICKS_PER_SECOND;
          
          // Find clips that would be affected by this drop position
          let foundValidDropPosition = false;
          let snapToPosition: number | null = null;
          
          // Check if drop position is to the left or right of another clip
          for (const otherClip of primaryTrack.clips) {
            const otherClipId = otherClip.id || `${otherClip.asset_id}-${otherClip.timeline_start_ticks}`;
            if (otherClipId === draggedClipId) continue;
            
            const otherClipStartTime = otherClip.timeline_start_ticks / TICKS_PER_SECOND;
            const otherClipEndTime = otherClipStartTime + (otherClip.out_ticks - otherClip.in_ticks) / TICKS_PER_SECOND;
            
            // Check if drop position is near the left edge of another clip (within 30% of clip width or 0.5s, whichever is smaller)
            const otherClipWidth = otherClipEndTime - otherClipStartTime;
            const snapThreshold = Math.min(otherClipWidth * 0.3, 0.5);
            
            if (Math.abs(dropTime - otherClipStartTime) < snapThreshold) {
              // Snap to left of this clip
              snapToPosition = otherClipStartTime;
              foundValidDropPosition = true;
              break;
            }
            
            // Check if drop position is near the right edge of another clip
            if (Math.abs(dropTime - otherClipEndTime) < snapThreshold) {
              // Snap to right of this clip
              snapToPosition = otherClipEndTime;
              foundValidDropPosition = true;
              break;
            }
            
            // Check if drop position is between clips (in a gap)
            if (dropTime > otherClipEndTime) {
              // Check if there's a next clip
              const nextClip = primaryTrack.clips.find((c: any) => {
                const cId = c.id || `${c.asset_id}-${c.timeline_start_ticks}`;
                return cId !== draggedClipId && cId !== otherClipId && 
                       c.timeline_start_ticks / TICKS_PER_SECOND > otherClipEndTime;
              });
              
              if (nextClip) {
                const nextClipStartTime = nextClip.timeline_start_ticks / TICKS_PER_SECOND;
                if (dropTime < nextClipStartTime) {
                  // Drop position is in a gap between clips
                  snapToPosition = otherClipEndTime;
                  foundValidDropPosition = true;
                  break;
                }
              } else {
                // No next clip, drop at end
                snapToPosition = otherClipEndTime;
                foundValidDropPosition = true;
                break;
              }
            }
          }
          
          // Also check if dragging to the very beginning (before first clip)
          if (!foundValidDropPosition && primaryTrack.clips.length > 0) {
            const firstClip = primaryTrack.clips.reduce((first: any, clip: any) => {
              const firstTime = first.timeline_start_ticks / TICKS_PER_SECOND;
              const clipTime = clip.timeline_start_ticks / TICKS_PER_SECOND;
              return clipTime < firstTime ? clip : first;
            });
            const firstClipStartTime = firstClip.timeline_start_ticks / TICKS_PER_SECOND;
            
            if (dropTime < firstClipStartTime - 0.1) {
              // Dragging to before first clip
              snapToPosition = 0;
              foundValidDropPosition = true;
            }
          }
          
          // Only show preview if we found a valid drop position that would change order
          if (foundValidDropPosition && snapToPosition !== null) {
            // Check if this position would actually change the clip's order
            const wouldChangeOrder = Math.abs(snapToPosition - originalStartTime) > 0.01;
            
            if (wouldChangeOrder) {
              // Find end of timeline for clamping
              let timelineEnd = 0;
              primaryTrack.clips.forEach((clip: any) => {
                const clipEndTime = (clip.timeline_start_ticks + (clip.out_ticks - clip.in_ticks)) / TICKS_PER_SECOND;
                if (clipEndTime > timelineEnd) {
                  timelineEnd = clipEndTime;
                }
              });
              
              const clampedPosition = Math.min(snapToPosition, timelineEnd);
              setClipDropPosition({ time: clampedPosition, intent });
            } else {
              // Position wouldn't change order, don't show preview
              setClipDropPosition(null);
            }
          } else {
            // No valid drop position found, don't show preview
            setClipDropPosition(null);
          }
        } else {
          setClipDropPosition(null);
        }
      } else if (intent === 'layered') {
        // For layered intent, always show preview when dragging above primary track
        // Don't require minimum drag distance - allow immediate preview
        // Use the drop time directly (time where mouse is horizontally)
        setClipDropPosition({ time: dropTime, intent });
      } else {
        // Not enough drag distance or invalid intent, don't show preview
        setClipDropPosition(null);
      }
      return;
    }

    // Handle trim drag
    if (trimState) {
      // Trim is in progress, just update hoverX for visual feedback
      return;
    }

    // Handle edge detection for trimming (only in pointer tool mode)
    if (activeTool === 'pointer' && timeline && !dragAsset && !dragTextTemplate && !dragAudioAsset && !draggedClip && !potentialDrag) {
      let currentY = 50;
      let foundEdge = false;
      
      for (const track of timeline.tracks || []) {
        if (mouseY >= currentY && mouseY <= currentY + trackHeight) {
          for (const clip of track.clips || []) {
            const clipId = clip.id || `${clip.asset_id}-${clip.timeline_start_ticks}`;
            const clipStartTime = clip.timeline_start_ticks / TICKS_PER_SECOND;
            const clipDuration = (clip.out_ticks - clip.in_ticks) / TICKS_PER_SECOND;
            const clipX = leftMargin + (clipStartTime * pixelsPerSecond) - scrollX;
            const clipWidth = clipDuration * pixelsPerSecond;
            
            if (mouseX >= clipX && mouseX <= clipX + clipWidth) {
              // Check if near left edge
              if (Math.abs(mouseX - clipX) < edgeThreshold) {
                setHoveredEdge({ clipId, edge: 'left' });
                foundEdge = true;
                break;
              }
              // Check if near right edge
              if (Math.abs(mouseX - (clipX + clipWidth)) < edgeThreshold) {
                setHoveredEdge({ clipId, edge: 'right' });
                foundEdge = true;
                break;
              }
            }
          }
          if (foundEdge) break;
        }
        currentY += trackHeight + trackSpacing;
      }
      
      if (!foundEdge) {
        setHoveredEdge(null);
      }
    }

    // Handle drag drop position calculation
    if (dragAsset) {
      // Define primary storyline Y bounds (track 1)
      const primaryTrackY = 50;
      const primaryTrackBottom = primaryTrackY + trackHeight;
      
      // Detect drop intent based on Y position
      let intent: 'primary' | 'layered' = 'primary';
      if (mouseY < primaryTrackY) {
        // Dropping above primary track = layered intent
        intent = 'layered';
      } else if (mouseY >= primaryTrackY && mouseY <= primaryTrackBottom) {
        // Dropping on primary track = primary intent
        intent = 'primary';
      } else {
        // Dropping below primary track = still primary (append to end)
        intent = 'primary';
      }
      
      // Calculate drop time - snap to earliest next timestamp for primary, or use hover time for layered
      let dropTime: number;
      if (intent === 'primary') {
        dropTime = calculateEarliestNextTimestamp(timeline);
      } else {
        // For layered, use the hover time (or earliest next if no hover)
        const hoverTime = (mouseX - leftMargin + scrollX) / pixelsPerSecond;
        dropTime = Math.max(0, hoverTime);
      }
      
      // For primary intent, always use track 1
      // For layered intent, find appropriate overlay track
      let targetTrackId = 1;
      if (intent === 'layered') {
        // Find the highest track ID and use next one for overlay
        if (timeline && timeline.tracks && timeline.tracks.length > 0) {
          const maxTrackId = Math.max(...timeline.tracks.map(t => t.id || 1));
          targetTrackId = maxTrackId + 1;
        } else {
          targetTrackId = 2; // First overlay track
        }
      }

      const newDropPosition = { time: dropTime, trackId: targetTrackId, intent };
      setDropPosition(newDropPosition);
    } else if (dragTextTemplate) {
      // Text templates can be dropped on primary timeline OR above it
      const hoverTime = (mouseX - leftMargin + scrollX) / pixelsPerSecond;
      const dropTime = Math.max(0, hoverTime);
      
      // Define primary storyline Y bounds (track 1)
      const primaryTrackY = 50;
      const primaryTrackBottom = primaryTrackY + trackHeight;
      
      // Detect drop intent based on Y position
      let intent: 'primary' | 'layered' = 'primary';
      let targetTrackId = 1; // Default to primary track
      
      if (mouseY < primaryTrackY) {
        // Dropping above primary track = layered intent (caption track above)
        intent = 'layered';
        // Find or create caption track (use track ID 2 for first caption track, or next available)
        if (timeline && timeline.tracks && timeline.tracks.length > 0) {
          const captionTracks = timeline.tracks.filter(t => t.kind === 'Caption' || t.kind === 'caption');
          if (captionTracks.length > 0) {
            targetTrackId = Math.max(...captionTracks.map(t => t.id || 2));
          } else {
            targetTrackId = 2; // First caption track
          }
        } else {
          targetTrackId = 2; // First caption track
        }
      } else if (mouseY >= primaryTrackY && mouseY <= primaryTrackBottom) {
        // Dropping on primary track = primary intent (insert into primary timeline)
        intent = 'primary';
        targetTrackId = 1;
      } else {
        // Dropping below primary track = still primary (append to end)
        intent = 'primary';
        targetTrackId = 1;
      }
      
      setDropPosition({ time: dropTime, trackId: targetTrackId, intent });
    } else if (dragAudioAsset) {
      // Audio assets always overlay (no ripple)
      const hoverTime = (mouseX - leftMargin + scrollX) / pixelsPerSecond;
      const dropTime = Math.max(0, hoverTime);
      // Find or create audio track (use track ID after video tracks)
      let targetTrackId = 10; // Start audio tracks at ID 10
      if (timeline && timeline.tracks && timeline.tracks.length > 0) {
        const audioTracks = timeline.tracks.filter(t => t.kind === 'Audio' || t.kind === 'audio');
        if (audioTracks.length > 0) {
          targetTrackId = Math.max(...audioTracks.map(t => t.id || 10));
        } else {
          targetTrackId = 10; // First audio track
        }
      }
      setDropPosition({ time: dropTime, trackId: targetTrackId, intent: 'layered' });
    } else {
      // Clear drop position when not dragging
      if (dropPosition) {
        setDropPosition(null);
      }
    }
  };

  const handleMouseLeave = () => {
    setHoverX(null);
    setHoverPlayheadTicks(null);
    setDropPosition(null);
    // Don't clear draggedClip on mouse leave - allow drag to continue
  };

  const handleMouseDown = (e: React.MouseEvent<HTMLCanvasElement>) => {
    if (!canvasRef.current || !timeline || activeTool !== 'pointer') return;
    
    const canvas = canvasRef.current;
    const rect = canvas.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top;
    const leftMargin = 3 * PIXELS_PER_REM;
    const trackHeight = 60;
    const trackSpacing = 10;
    const edgeThreshold = 5; // pixels
    
    // Check for trim operation first (edge hover)
    if (hoveredEdge && onClipTrim) {
      for (const track of timeline.tracks || []) {
        for (const clip of track.clips || []) {
          const clipId = clip.id || `${clip.asset_id}-${clip.timeline_start_ticks}`;
          if (clipId === hoveredEdge.clipId) {
            // Fetch asset duration to know how far we can extend outward
            fetch(`http://127.0.0.1:7777/api/projects/${projectId}/media`)
              .then(res => res.json())
              .then((assets: any[]) => {
                const asset = assets.find((a: any) => a.id === clip.asset_id);
                const assetDurationTicks = asset?.duration_ticks || clip.out_ticks; // Fallback to current out if not found
                setTrimState({
                  clipId,
                  edge: hoveredEdge.edge,
                  startX: x,
                  originalIn: clip.in_ticks,
                  originalOut: clip.out_ticks,
                  originalStart: clip.timeline_start_ticks,
                  assetId: clip.asset_id,
                  assetDurationTicks,
                });
              })
              .catch(() => {
                // On error, use current out_ticks as max (can't extend outward)
                setTrimState({
                  clipId,
                  edge: hoveredEdge.edge,
                  startX: x,
                  originalIn: clip.in_ticks,
                  originalOut: clip.out_ticks,
                  originalStart: clip.timeline_start_ticks,
                  assetId: clip.asset_id,
                  assetDurationTicks: clip.out_ticks,
                });
              });
            return;
          }
        }
      }
    }
    
    // Check for potential clip drag (click on clip body, not edge)
    // Don't start dragging immediately - wait for intentional movement
    // Must match rendering order: caption, overlay, primary, audio
    if (!hoveredEdge && (onClipReorder || onConvertPrimaryToOverlay || onConvertOverlayToPrimary || onMoveClip)) {
      // Separate tracks by kind to match rendering order
      const captionTracks = timeline.tracks?.filter(t => t.kind === 'Caption' || t.kind === 'caption') || [];
      const videoTracks = timeline.tracks?.filter(t => t.kind === 'Video' || t.kind === 'video' || !t.kind) || [];
      const audioTracks = timeline.tracks?.filter(t => t.kind === 'Audio' || t.kind === 'audio') || [];
      
      // Separate video tracks into primary and overlays
      // Match rendering: sort ascending, then reverse when iterating
      const primaryVideoTrack = videoTracks.find(t => (t.id || 1) === 1);
      const overlayVideoTracks = videoTracks
        .filter(t => (t.id || 1) > 1 && (t.clips?.length || 0) > 0)
        .sort((a, b) => (a.id || 0) - (b.id || 0)); // Sort ascending (matches rendering)
      
      let foundClip = false;
      let currentY = 50;
      
      // Check caption tracks first
      for (const track of captionTracks) {
        if (y >= currentY && y <= currentY + trackHeight) {
          for (const clip of track.clips || []) {
            const clipStartTime = clip.timeline_start_ticks / TICKS_PER_SECOND;
            const clipDuration = (clip.out_ticks - clip.in_ticks) / TICKS_PER_SECOND;
            const clipX = leftMargin + (clipStartTime * pixelsPerSecond) - scrollX;
            const clipWidth = clipDuration * pixelsPerSecond;
            
            // Check if click is on clip body (not near edges)
            if (x >= clipX + edgeThreshold && x <= clipX + clipWidth - edgeThreshold && 
                x >= clipX && x <= clipX + clipWidth) {
              const offsetX = x - clipX;
              setPotentialDrag({
                clip,
                startX: x,
                startY: y,
                offsetX,
                originalPosition: clip.timeline_start_ticks,
              });
              foundClip = true;
              break;
            }
          }
          if (foundClip) break;
        }
        currentY += trackHeight + trackSpacing;
      }
      
      // Check overlay video tracks (top to bottom) - rendered ABOVE primary track
      // Match rendering: iterate in reverse order (highest ID closest to primary)
      if (!foundClip) {
        const primaryTrackY = 50 + (captionTracks.length * (trackHeight + trackSpacing));
        // Iterate in reverse order to match rendering
        for (let i = overlayVideoTracks.length - 1; i >= 0; i--) {
          const track = overlayVideoTracks[i];
          const trackIndex = overlayVideoTracks.indexOf(track);
          // Match rendering calculation exactly
          const trackY = primaryTrackY - (trackHeight + trackSpacing) * (overlayVideoTracks.length - trackIndex);
          if (y >= trackY && y <= trackY + trackHeight) {
            for (const clip of track.clips || []) {
              const clipStartTime = clip.timeline_start_ticks / TICKS_PER_SECOND;
              const clipDuration = (clip.out_ticks - clip.in_ticks) / TICKS_PER_SECOND;
              const clipX = leftMargin + (clipStartTime * pixelsPerSecond) - scrollX;
              const clipWidth = clipDuration * pixelsPerSecond;
              
              // Check if click is on clip body - allow dragging from anywhere on clip (no edge threshold for video)
              if (x >= clipX && x <= clipX + clipWidth) {
                const offsetX = x - clipX;
                setPotentialDrag({
                  clip,
                  startX: x,
                  startY: y,
                  offsetX,
                  originalPosition: clip.timeline_start_ticks,
                });
                foundClip = true;
                break;
              }
            }
            if (foundClip) break;
          }
        }
      }
      
      // Check primary video track
      if (!foundClip && primaryVideoTrack) {
        const primaryTrackY = 50 + (captionTracks.length * (trackHeight + trackSpacing));
        if (y >= primaryTrackY && y <= primaryTrackY + trackHeight) {
          for (const clip of primaryVideoTrack.clips || []) {
            const clipStartTime = clip.timeline_start_ticks / TICKS_PER_SECOND;
            const clipDuration = (clip.out_ticks - clip.in_ticks) / TICKS_PER_SECOND;
            const clipX = leftMargin + (clipStartTime * pixelsPerSecond) - scrollX;
            const clipWidth = clipDuration * pixelsPerSecond;
            
            // Check if click is on clip body (not near edges) - allow dragging from anywhere on clip
            if (x >= clipX && x <= clipX + clipWidth) {
              const offsetX = x - clipX;
              setPotentialDrag({
                clip,
                startX: x,
                startY: y,
                offsetX,
                originalPosition: clip.timeline_start_ticks,
              });
              foundClip = true;
              break;
            }
          }
        }
      }
      
      // Check audio tracks
      if (!foundClip) {
        const primaryTrackY = 50 + (captionTracks.length * (trackHeight + trackSpacing));
        const videoTracksY = primaryTrackY + (primaryVideoTrack ? trackHeight + trackSpacing : 0) + (overlayVideoTracks.length * (trackHeight + trackSpacing));
        currentY = videoTracksY;
        for (const track of audioTracks) {
          if (y >= currentY && y <= currentY + trackHeight) {
            for (const clip of track.clips || []) {
              const clipStartTime = clip.timeline_start_ticks / TICKS_PER_SECOND;
              const clipDuration = (clip.out_ticks - clip.in_ticks) / TICKS_PER_SECOND;
              const clipX = leftMargin + (clipStartTime * pixelsPerSecond) - scrollX;
              const clipWidth = clipDuration * pixelsPerSecond;
              
              // Check if click is on clip body (not near edges)
              if (x >= clipX + edgeThreshold && x <= clipX + clipWidth - edgeThreshold && 
                  x >= clipX && x <= clipX + clipWidth) {
                const offsetX = x - clipX;
                setPotentialDrag({
                  clip,
                  startX: x,
                  startY: y,
                  offsetX,
                  originalPosition: clip.timeline_start_ticks,
                });
                foundClip = true;
                break;
              }
            }
            if (foundClip) break;
          }
          currentY += trackHeight + trackSpacing;
        }
      }
    }
  };

  // Global mouseup handler for drag drop (handles case where mouse leaves canvas)
  useEffect(() => {
    const handleGlobalMouseUp = (e: MouseEvent) => {
      // Handle clip reorder drop or primary → overlay conversion
      if (draggedClip && clipDropPosition) {
        const clipId = draggedClip.clip.id || `${draggedClip.clip.asset_id}-${draggedClip.clip.timeline_start_ticks}`;
        const positionTicks = Math.round(clipDropPosition.time * TICKS_PER_SECOND);
        
        // Check if this is a primary clip or overlay clip
        // Check both track_id property and if clip exists in primary track
        const primaryTrack = timeline?.tracks?.find(t => (t.id || 1) === 1);
        const isPrimaryClip = (draggedClip.clip.track_id === 1) || 
                             (primaryTrack?.clips?.some((c: any) => c.id === clipId) ?? false);
        const isOverlayClip = !isPrimaryClip && (draggedClip.clip.track_id && draggedClip.clip.track_id > 1);
        
        if (clipDropPosition.intent === 'layered') {
          console.log('Global mouseup: Layered intent detected', { clipId, positionTicks, isPrimaryClip, hasConvertHandler: !!onConvertPrimaryToOverlay, hasMoveHandler: !!onMoveClip });
          if (isPrimaryClip && onConvertPrimaryToOverlay) {
            // Convert primary clip to overlay
            console.log('Global mouseup: Calling onConvertPrimaryToOverlay');
            onConvertPrimaryToOverlay(clipId, positionTicks);
          } else if (onMoveClip) {
            // Move overlay clip to new position
            console.log('Global mouseup: Calling onMoveClip');
            onMoveClip(clipId, positionTicks);
          } else {
            console.warn('Global mouseup: No handler available for layered intent');
          }
        } else if (clipDropPosition.intent === 'primary') {
          if (isOverlayClip && onConvertOverlayToPrimary) {
            // Convert overlay clip to primary
            console.log('Global mouseup: Calling onConvertOverlayToPrimary');
            onConvertOverlayToPrimary(clipId, positionTicks);
          } else if (isPrimaryClip && positionTicks !== draggedClip.originalPosition && onClipReorder) {
            // Regular primary reorder
          onClipReorder(clipId, positionTicks);
          }
        }
        
        setDraggedClip(null);
        setClipDropPosition(null);
        setPotentialDrag(null);
        return;
      }
      
      // Clear potential drag if mouse is released without dragging
      if (potentialDrag) {
        setPotentialDrag(null);
      }
      
      // Handle text template drop
      if (dragTextTemplate && dropPosition && onTextClipInsert) {
        if (canvasRef.current) {
          const rect = canvasRef.current.getBoundingClientRect();
          const mouseX = e.clientX - rect.left;
          const mouseY = e.clientY - rect.top;
          
          if (mouseX >= 0 && mouseX <= rect.width && mouseY >= 0 && mouseY <= rect.height) {
            e.preventDefault();
            e.stopPropagation();
            const positionTicks = Math.round(dropPosition.time * TICKS_PER_SECOND);
            onTextClipInsert(dragTextTemplate, positionTicks, dropPosition.intent, dropPosition.trackId);
            setDropPosition(null);
            // Clear drag state immediately
            if (onDragEnd) {
              onDragEnd();
            }
          }
        }
        return;
      }
      
      // Handle audio asset drop
      if (dragAudioAsset && dropPosition && onAudioClipInsert) {
        if (canvasRef.current) {
          const rect = canvasRef.current.getBoundingClientRect();
          const mouseX = e.clientX - rect.left;
          const mouseY = e.clientY - rect.top;
          
          if (mouseX >= 0 && mouseX <= rect.width && mouseY >= 0 && mouseY <= rect.height) {
            e.preventDefault();
            e.stopPropagation();
            const positionTicks = Math.round(dropPosition.time * TICKS_PER_SECOND);
            onAudioClipInsert(dragAudioAsset, positionTicks);
            setDropPosition(null);
            // Clear drag state immediately
            if (onDragEnd) {
              onDragEnd();
            }
          }
        }
        return;
      }
      
      // Clear drag state if mouse is released without a valid drop
      if ((dragTextTemplate || dragAudioAsset) && !dropPosition && onDragEnd) {
        onDragEnd();
        setDropPosition(null);
      }
      
      // Only handle if we're dragging and have a drop position
      if (dragAsset && dropPosition && onClipInsert) {
        // Check if mouse is over the canvas
        if (canvasRef.current) {
          const rect = canvasRef.current.getBoundingClientRect();
          const mouseX = e.clientX - rect.left;
          const mouseY = e.clientY - rect.top;
          
          // Only process if mouse is within canvas bounds
          if (mouseX >= 0 && mouseX <= rect.width && mouseY >= 0 && mouseY <= rect.height) {
            e.preventDefault();
            e.stopPropagation();
            // Use the drop position that was calculated (includes intent detection)
            const positionTicks = Math.round(dropPosition.time * TICKS_PER_SECOND);
            const modifierKey = e.shiftKey || e.altKey; // Shift or Alt for overwrite mode
            onClipInsert(dragAsset.id, positionTicks, dropPosition.trackId, dropPosition.intent, modifierKey);
            setDropPosition(null);
          }
        }
      }
    };

    if (dragAsset || draggedClip || dragTextTemplate || dragAudioAsset) {
      document.addEventListener('mouseup', handleGlobalMouseUp);
      return () => {
        document.removeEventListener('mouseup', handleGlobalMouseUp);
      };
    }
  }, [dragAsset, dropPosition, onClipInsert, draggedClip, clipDropPosition, potentialDrag, onClipReorder, onConvertPrimaryToOverlay, onConvertOverlayToPrimary, onMoveClip, timeline, calculateEarliestNextTimestamp, dragTextTemplate, dragAudioAsset, onTextClipInsert, onAudioClipInsert, onDragEnd]);

  const handleMouseUp = (e: React.MouseEvent<HTMLCanvasElement>) => {
    // Handle clip reorder drop or primary → overlay conversion
    if (draggedClip && clipDropPosition) {
      e.preventDefault();
      e.stopPropagation();
      const clipId = draggedClip.clip.id || `${draggedClip.clip.asset_id}-${draggedClip.clip.timeline_start_ticks}`;
      const positionTicks = Math.round(clipDropPosition.time * TICKS_PER_SECOND);
      
      // Check if this is a primary clip or overlay clip
      // Check both track_id property and if clip exists in primary track
      const primaryTrack = timeline?.tracks?.find(t => (t.id || 1) === 1);
      const isPrimaryClip = (draggedClip.clip.track_id === 1) || 
                           (primaryTrack?.clips?.some((c: any) => c.id === clipId) ?? false);
      const isOverlayClip = !isPrimaryClip && (draggedClip.clip.track_id && draggedClip.clip.track_id > 1);
      
      if (clipDropPosition.intent === 'layered') {
        console.log('Layered intent detected', { clipId, positionTicks, isPrimaryClip, hasConvertHandler: !!onConvertPrimaryToOverlay, hasMoveHandler: !!onMoveClip });
        if (isPrimaryClip && onConvertPrimaryToOverlay) {
          // Convert primary clip to overlay
          console.log('Calling onConvertPrimaryToOverlay');
          onConvertPrimaryToOverlay(clipId, positionTicks);
        } else if (onMoveClip) {
          // Move overlay clip to new position
          console.log('Calling onMoveClip');
          onMoveClip(clipId, positionTicks);
        } else {
          console.warn('No handler available for layered intent');
        }
      } else if (clipDropPosition.intent === 'primary') {
        if (isOverlayClip && onConvertOverlayToPrimary) {
          // Convert overlay clip to primary
          console.log('Calling onConvertOverlayToPrimary');
          onConvertOverlayToPrimary(clipId, positionTicks);
        } else if (isPrimaryClip && positionTicks !== draggedClip.originalPosition && onClipReorder) {
          // Regular primary reorder
        onClipReorder(clipId, positionTicks);
        }
      }
      
      setDraggedClip(null);
      setClipDropPosition(null);
      setPotentialDrag(null);
      return;
    }
    
    // Clear potential drag if mouse is released without dragging
    if (potentialDrag) {
      setPotentialDrag(null);
    }

    // Handle text template drop
    if (dragTextTemplate && dropPosition && onTextClipInsert) {
      e.preventDefault();
      e.stopPropagation();
      const positionTicks = Math.round(dropPosition.time * TICKS_PER_SECOND);
      onTextClipInsert(dragTextTemplate, positionTicks, dropPosition.intent, dropPosition.trackId);
      setDropPosition(null);
      // Clear drag state immediately
      if (onDragEnd) {
        onDragEnd();
      }
      return;
    }
    
    // Handle audio asset drop
    if (dragAudioAsset && dropPosition && onAudioClipInsert) {
      e.preventDefault();
      e.stopPropagation();
      const positionTicks = Math.round(dropPosition.time * TICKS_PER_SECOND);
      onAudioClipInsert(dragAudioAsset, positionTicks);
      setDropPosition(null);
      return;
    }

    // Handle drag drop
    if (dragAsset && dropPosition && onClipInsert) {
      e.preventDefault();
      e.stopPropagation();
      console.log('Timeline handleMouseUp: drop detected', { dragAsset, dropPosition });
      // Use the drop position that was calculated (includes overlap detection)
      const positionTicks = Math.round(dropPosition.time * TICKS_PER_SECOND);
      const modifierKey = e.shiftKey || e.altKey; // Shift or Alt for overwrite mode
      console.log('Calling onClipInsert with:', { assetId: dragAsset.id, positionTicks, trackId: dropPosition.trackId, intent: dropPosition.intent, modifierKey });
      onClipInsert(dragAsset.id, positionTicks, dropPosition.trackId, dropPosition.intent, modifierKey);
      setDropPosition(null);
      return;
    }

    // Handle trim completion
    if (trimState && onClipTrim && canvasRef.current) {
      const canvas = canvasRef.current;
      const currentX = e.clientX - canvas.getBoundingClientRect().left;
      const deltaX = currentX - trimState.startX;
      const deltaTicks = (deltaX / pixelsPerSecond) * TICKS_PER_SECOND;
      
      let newIn = trimState.originalIn;
      let newOut = trimState.originalOut;
      
      if (trimState.edge === 'left') {
        // Allow extending outward (negative delta) up to 0, or trimming inward
        newIn = Math.max(0, Math.min(trimState.originalIn + deltaTicks, trimState.originalOut - TICKS_PER_SECOND));
      } else {
        // Allow extending outward up to asset duration, or trimming inward
        newOut = Math.max(trimState.originalIn + TICKS_PER_SECOND, Math.min(trimState.originalOut + deltaTicks, trimState.assetDurationTicks));
      }
      
      onClipTrim(trimState.clipId, newIn, newOut);
      setTrimState(null);
    }
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
        onClick={handleCanvasClick}
        onWheel={handleWheel}
        onMouseMove={handleMouseMove}
        onMouseLeave={handleMouseLeave}
        onMouseDown={handleMouseDown}
        onMouseUp={handleMouseUp}
        style={{
          display: 'block',
          cursor: dragAsset ? 'copy' : 
                  trimState ? 'ew-resize' :
                  hoveredEdge ? 'ew-resize' :
                  activeTool === 'cut' ? 'crosshair' : 'pointer',
          pointerEvents: 'auto',
        }}
      />
    </div>
  );
}

