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

interface TimelineProps {
  timeline: TimelineData | null;
  selectedClip: any | null;
  onClipClick: (clip: any) => void;
  playheadPosition?: number; // in ticks
  dragAsset?: MediaAsset | null;
  onClipInsert?: (assetId: number, positionTicks: number, trackId: number, intent?: 'primary' | 'layered', modifierKey?: boolean) => void;
  onHoverTimeChange?: (time: number) => void;
  onClipTrim?: (clipId: string, newInTicks: number, newOutTicks: number) => void;
  onClipSplit?: (clipId: string, positionTicks: number) => void;
  onPlayheadSet?: (positionTicks: number) => void;
  onClipReorder?: (clipId: string, newPositionTicks: number) => void;
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

export function Timeline({ timeline, selectedClip, onClipClick, playheadPosition = 0, dragAsset, onClipInsert, onHoverTimeChange, onClipTrim, onClipSplit, onPlayheadSet, onClipReorder, activeTool = 'pointer', projectId = 1, isPlaying = false }: TimelineProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [pixelsPerSecond, setPixelsPerSecond] = useState(20); // Initial zoom level
  const [scrollX, setScrollX] = useState(0); // Horizontal scroll offset in pixels
  const [hoverX, setHoverX] = useState<number | null>(null); // Cursor X position for zoom anchoring
  const [hoverPlayheadTicks, setHoverPlayheadTicks] = useState<number | null>(null); // Hover playhead position in ticks
  const [dropPosition, setDropPosition] = useState<{ time: number; trackId: number; intent?: 'primary' | 'layered' } | null>(null);
  const [trimState, setTrimState] = useState<{ clipId: string; edge: 'left' | 'right'; startX: number; originalIn: number; originalOut: number; originalStart: number } | null>(null);
  const [hoveredEdge, setHoveredEdge] = useState<{ clipId: string; edge: 'left' | 'right' } | null>(null);
  const [loadedThumbnails, setLoadedThumbnails] = useState<Map<string, HTMLImageElement>>(new Map());
  
  // Drag state for existing clips
  const [draggedClip, setDraggedClip] = useState<{ clip: any; startX: number; startY: number; offsetX: number; originalPosition: number } | null>(null);
  const [clipDropPosition, setClipDropPosition] = useState<{ time: number; intent: 'primary' | 'layered' } | null>(null);

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
    
    timeline.tracks.forEach((track) => {
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
          // Show trim preview
          if (trimState.edge === 'left') {
            const newIn = (trimState.originalIn + (hoverX! - trimState.startX) / pixelsPerSecond * TICKS_PER_SECOND);
            const constrainedIn = Math.max(0, Math.min(newIn, trimState.originalOut - TICKS_PER_SECOND)); // Min 1 second
            clipStartTime = (trimState.originalStart + (constrainedIn - trimState.originalIn) / TICKS_PER_SECOND) / TICKS_PER_SECOND;
            clipDuration = (trimState.originalOut - constrainedIn) / TICKS_PER_SECOND;
          } else {
            const newOut = (trimState.originalOut + (hoverX! - trimState.startX) / pixelsPerSecond * TICKS_PER_SECOND);
            const constrainedOut = Math.min(newOut, trimState.originalOut + 1000 * TICKS_PER_SECOND); // Max asset duration
            clipDuration = (constrainedOut - trimState.originalIn) / TICKS_PER_SECOND;
          }
        }
        
        const x = leftMargin + (clipStartTime * pixelsPerSecond) - scrollX;
        const width = clipDuration * pixelsPerSecond;
        
        // Draw rounded rectangle for clip
        const clipY = y + 5;
        const clipHeight = trackHeight - 10;
        const radius = 6;
        
        ctx.fillStyle = isSelected ? '#10b981' : '#3b82f6';
        drawRoundedRect(ctx, x, clipY, width, clipHeight, radius);
        ctx.fill();

        // Clip border - yellow if selected
        ctx.strokeStyle = isSelected ? '#fbbf24' : '#2563eb';
        ctx.lineWidth = isSelected ? 3 : 1;
        drawRoundedRect(ctx, x, clipY, width, clipHeight, radius);
        ctx.stroke();
        
        // Draw thumbnails as filmstrip
        if (width >= 80) { // Only show thumbnails if clip is wide enough
          const thumbSpacingPx = 80; // Spacing between thumbnails
          const numThumbs = Math.floor(width / thumbSpacingPx);
          
          if (numThumbs > 0) {
            // Calculate source time range for this clip
            const sourceStartTime = clip.in_ticks / TICKS_PER_SECOND;
            const sourceEndTime = clip.out_ticks / TICKS_PER_SECOND;
            const sourceDuration = sourceEndTime - sourceStartTime;
            
            // Sample thumbnails evenly across the clip
            for (let i = 0; i < numThumbs; i++) {
              const thumbX = x + (i * thumbSpacingPx);
              
              // Only draw if thumbnail is visible in viewport
              if (thumbX + thumbSpacingPx >= leftMargin && thumbX <= canvas.width) {
                // Calculate source time for this thumbnail
                const sampleTime = sourceStartTime + (i / numThumbs) * sourceDuration;
                const timestampSec = Math.floor(sampleTime);
                
                // Try to get thumbnail from cache
                const cacheKey = `${clip.asset_id}_${timestampSec}`;
                const thumbnail = thumbnailCache.get(cacheKey);
                
                if (thumbnail && thumbnail.complete && thumbnail.naturalWidth > 0) {
                  // Draw thumbnail
                  const thumbWidth = thumbSpacingPx;
                  const thumbHeight = clipHeight;
                  
                  // Calculate aspect ratio to maintain
                  const thumbAspect = thumbnail.width / thumbnail.height;
                  const targetAspect = thumbWidth / thumbHeight;
                  
                  let drawWidth = thumbWidth;
                  let drawHeight = thumbHeight;
                  let drawX = thumbX;
                  let drawY = clipY;
                  
                  if (thumbAspect > targetAspect) {
                    // Thumbnail is wider - fit to height
                    drawWidth = thumbHeight * thumbAspect;
                    drawX = thumbX + (thumbWidth - drawWidth) / 2;
                  } else {
                    // Thumbnail is taller - fit to width
                    drawHeight = thumbWidth / thumbAspect;
                    drawY = clipY + (thumbHeight - drawHeight) / 2;
                  }
                  
                  // Clip to rounded rectangle bounds
                  ctx.save();
                  drawRoundedRect(ctx, x, clipY, width, clipHeight, radius);
                  ctx.clip();
                  
                  ctx.drawImage(thumbnail, drawX, drawY, drawWidth, drawHeight);
                  ctx.restore();
                } else {
                  // Thumbnail not loaded yet - load it asynchronously
                  loadThumbnail(clip.asset_id, timestampSec, projectId).then((img) => {
                    if (img && canvasRef.current) {
                      // Trigger re-render to draw the loaded thumbnail
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

    // Draw ghost preview when dragging
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
      const primaryTrackY = 50;
      const previewY = primaryTrackY + 5;
      const previewHeight = trackHeight - 10;
      const radius = 6;
      const previewWidth = clipDuration * pixelsPerSecond;
      
      if (dropX >= leftMargin && dropX <= canvas.width) {
        if (clipDropPosition.intent === 'primary') {
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
          
          // Draw ghost preview of dragged clip
          ctx.strokeStyle = '#10b981';
          ctx.lineWidth = 2;
          ctx.setLineDash([8, 4]);
          drawRoundedRect(ctx, dropX, previewY, previewWidth, previewHeight, radius);
          ctx.stroke();
          ctx.setLineDash([]);
          
          ctx.fillStyle = 'rgba(16, 185, 129, 0.2)';
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
          // Layered overlay - show different visual feedback
          ctx.strokeStyle = '#3b82f6';
          ctx.lineWidth = 2;
          ctx.setLineDash([5, 5]);
          ctx.beginPath();
          ctx.moveTo(dropX, 30);
          ctx.lineTo(dropX, Math.max(y, primaryTrackY + trackHeight));
          ctx.stroke();
          ctx.setLineDash([]);
          
          ctx.strokeStyle = '#3b82f6';
          ctx.lineWidth = 2;
          ctx.setLineDash([8, 4]);
          drawRoundedRect(ctx, dropX, previewY, previewWidth, previewHeight, radius);
          ctx.stroke();
          ctx.setLineDash([]);
          
          ctx.fillStyle = 'rgba(59, 130, 246, 0.2)';
          drawRoundedRect(ctx, dropX, previewY, previewWidth, previewHeight, radius);
          ctx.fill();
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
    let currentY = 50;
    let clickedClip: any = null;
    
    // Find clicked clip if any
    if (timeline.tracks) {
      for (const track of timeline.tracks) {
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
        }
        currentY += trackHeight + trackSpacing;
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
      onClipClick(clickedClip);
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

    // Handle clip drag
    if (draggedClip && timeline && onClipReorder) {
      const leftMargin = 3 * PIXELS_PER_REM;
      const primaryTrackY = 50;
      const trackHeight = 60;
      const primaryTrackBottom = primaryTrackY + trackHeight;
      
      // Detect intent based on vertical movement
      const verticalDelta = Math.abs(mouseY - draggedClip.startY);
      const horizontalDelta = Math.abs(mouseX - draggedClip.startX);
      const intent: 'primary' | 'layered' = (verticalDelta > 20 && verticalDelta > horizontalDelta) ? 'layered' : 'primary';
      
      // Calculate drop time from mouse position
      let dropTime = (mouseX - leftMargin + scrollX) / pixelsPerSecond;
      dropTime = Math.max(0, dropTime);
      
      // Clamp to valid bounds (0 to end of timeline)
      if (intent === 'primary') {
        // Find end of timeline
        let timelineEnd = 0;
        if (timeline.tracks && timeline.tracks.length > 0) {
          const primaryTrack = timeline.tracks.find(t => (t.id || 1) === 1);
          if (primaryTrack && primaryTrack.clips) {
            primaryTrack.clips.forEach((clip: any) => {
              const clipEndTime = (clip.timeline_start_ticks + (clip.out_ticks - clip.in_ticks)) / TICKS_PER_SECOND;
              if (clipEndTime > timelineEnd) {
                timelineEnd = clipEndTime;
              }
            });
          }
        }
        dropTime = Math.min(dropTime, timelineEnd);
      }
      
      setClipDropPosition({ time: dropTime, intent });
      return;
    }

    // Handle trim drag
    if (trimState) {
      // Trim is in progress, just update hoverX for visual feedback
      return;
    }

    // Handle edge detection for trimming (only in pointer tool mode)
    if (activeTool === 'pointer' && timeline && !dragAsset && !draggedClip) {
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
            setTrimState({
              clipId,
              edge: hoveredEdge.edge,
              startX: x,
              originalIn: clip.in_ticks,
              originalOut: clip.out_ticks,
              originalStart: clip.timeline_start_ticks,
            });
            return;
          }
        }
      }
    }
    
    // Check for clip drag (click on clip body, not edge)
    if (!hoveredEdge && onClipReorder) {
      let currentY = 50;
      for (const track of timeline.tracks || []) {
        if (y >= currentY && y <= currentY + trackHeight) {
          for (const clip of track.clips || []) {
            const clipStartTime = clip.timeline_start_ticks / TICKS_PER_SECOND;
            const clipDuration = (clip.out_ticks - clip.in_ticks) / TICKS_PER_SECOND;
            const clipX = leftMargin + (clipStartTime * pixelsPerSecond) - scrollX;
            const clipWidth = clipDuration * pixelsPerSecond;
            
            // Check if click is on clip body (not near edges)
            if (x >= clipX + edgeThreshold && x <= clipX + clipWidth - edgeThreshold && 
                x >= clipX && x <= clipX + clipWidth) {
              // Start drag
              const offsetX = x - clipX;
              setDraggedClip({
                clip,
                startX: x,
                startY: y,
                offsetX,
                originalPosition: clip.timeline_start_ticks,
              });
              return;
            }
          }
        }
        currentY += trackHeight + trackSpacing;
      }
    }
  };

  // Global mouseup handler for drag drop (handles case where mouse leaves canvas)
  useEffect(() => {
    const handleGlobalMouseUp = (e: MouseEvent) => {
      // Handle clip reorder drop
      if (draggedClip && clipDropPosition && onClipReorder) {
        const clipId = draggedClip.clip.id || `${draggedClip.clip.asset_id}-${draggedClip.clip.timeline_start_ticks}`;
        const positionTicks = Math.round(clipDropPosition.time * TICKS_PER_SECOND);
        
        // Only reorder if position changed and intent is primary
        if (clipDropPosition.intent === 'primary' && positionTicks !== draggedClip.originalPosition) {
          onClipReorder(clipId, positionTicks);
        }
        
        setDraggedClip(null);
        setClipDropPosition(null);
        return;
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

    if (dragAsset || draggedClip) {
      document.addEventListener('mouseup', handleGlobalMouseUp);
      return () => {
        document.removeEventListener('mouseup', handleGlobalMouseUp);
      };
    }
  }, [dragAsset, dropPosition, onClipInsert, draggedClip, clipDropPosition, onClipReorder, timeline, calculateEarliestNextTimestamp]);

  const handleMouseUp = (e: React.MouseEvent<HTMLCanvasElement>) => {
    // Handle clip reorder drop
    if (draggedClip && clipDropPosition && onClipReorder) {
      e.preventDefault();
      e.stopPropagation();
      const clipId = draggedClip.clip.id || `${draggedClip.clip.asset_id}-${draggedClip.clip.timeline_start_ticks}`;
      const positionTicks = Math.round(clipDropPosition.time * TICKS_PER_SECOND);
      
      // Only reorder if position changed and intent is primary
      if (clipDropPosition.intent === 'primary' && positionTicks !== draggedClip.originalPosition) {
        onClipReorder(clipId, positionTicks);
      }
      
      setDraggedClip(null);
      setClipDropPosition(null);
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
        newIn = Math.max(0, Math.min(trimState.originalIn + deltaTicks, trimState.originalOut - TICKS_PER_SECOND));
      } else {
        newOut = Math.max(trimState.originalIn + TICKS_PER_SECOND, trimState.originalOut + deltaTicks);
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

