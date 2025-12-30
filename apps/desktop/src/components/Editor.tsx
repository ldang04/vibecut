import { useState, useEffect, useCallback, useRef } from 'react';
import { Toolbar } from './Toolbar';
import { MediaLibrary } from './MediaLibrary';
import { MediaSidebar } from './MediaSidebar';
import { Viewer } from './Viewer';
import { Timeline } from './Timeline';
import { TimelineToolbar } from './TimelineToolbar';
import { TextEditorPanel } from './TextEditorPanel';
import { OrchestratorPanel } from './OrchestratorPanel';

type Tool = 'pointer' | 'cut';
import { useDaemon } from '../hooks/useDaemon';

interface TimelineData {
  tracks: any[];
  captions: any[];
  music: any[];
}

interface TimelineResponse {
  timeline: TimelineData;
}

interface GenerateResponse {
  job_id: number;
}

interface ExportResponse {
  job_id: number;
}

interface Project {
  id: number;
  name: string;
  created_at: string;
  cache_dir: string;
  style_profile_id?: number;
}

interface EditorProps {
  projectId: number;
  currentProjectName: string;
  projects: Project[];
  onProjectSelect: (projectId: number) => void;
  onCreateProject: () => void;
}

export function Editor({ projectId, currentProjectName, projects, onProjectSelect, onCreateProject }: EditorProps) {
  const [timeline, setTimeline] = useState<TimelineData | null>(null);
  const [selectedClip, setSelectedClip] = useState<any | null>(null);
  const [videoSrc, setVideoSrc] = useState<string>('');
  const [videoStartTime, setVideoStartTime] = useState<number | undefined>();
  const [videoEndTime, setVideoEndTime] = useState<number | undefined>();
  const [currentTime, setCurrentTime] = useState<number>(0);
  const [playheadPosition, setPlayheadPosition] = useState<number>(0);
  const playheadPositionRef = useRef<number>(0); // Ref to track current playhead position (always up-to-date)
  const [isGenerating, setIsGenerating] = useState(false);
  const [isExporting, setIsExporting] = useState(false);
  const [hasActiveUploadJobs, setHasActiveUploadJobs] = useState(false);
  const [hasActiveAnalysisJobs, setHasActiveAnalysisJobs] = useState(false);
  const [activeJobIds, setActiveJobIds] = useState<Set<number>>(new Set());
  const [activeAnalysisJobIds, setActiveAnalysisJobIds] = useState<Set<number>>(new Set());
  const [mediaTab, setMediaTab] = useState<'raw' | 'references' | 'text' | 'audio'>('raw');
  const [isSidebarCollapsed, setIsSidebarCollapsed] = useState(false);
  const [activeTool, setActiveTool] = useState<Tool>('pointer');
  const [dragAsset, setDragAsset] = useState<any | null>(null);
  const [dragTextTemplate, setDragTextTemplate] = useState<any | null>(null);
  const [dragAudioAsset, setDragAudioAsset] = useState<any | null>(null);
  const [hoverTime, setHoverTime] = useState<number>(0);
  const [isTimelinePlaying, setIsTimelinePlaying] = useState(false);
  const [currentPlayingClip, setCurrentPlayingClip] = useState<any | null>(null);
  const transitioningRef = useRef(false); // Guard to prevent duplicate transitions
  const [hoverSourceTime, setHoverSourceTime] = useState<number | undefined>(undefined);
  const [hoverVideoSrc, setHoverVideoSrc] = useState<string>('');
  const [timelineHistory, setTimelineHistory] = useState<TimelineData[]>([]); // History stack for undo
  const [redoHistory, setRedoHistory] = useState<TimelineData[]>([]); // History stack for redo
  const [editingTextClip, setEditingTextClip] = useState<any | null>(null);
  // Show orchestrator by default in dev, or when VIBECUT_ORCHESTRATOR env var is set
  const showOrchestrator = import.meta.env.VITE_VIBECUT_ORCHESTRATOR === 'true' || import.meta.env.DEV;
  
  // Resizable panel widths
  const [libraryWidth, setLibraryWidth] = useState<number>(300);
  const [orchestratorWidth, setOrchestratorWidth] = useState<number>(200);
  const [resizingPanel, setResizingPanel] = useState<'library' | 'orchestrator' | null>(null);
  const resizeStartRef = useRef<{ panel: 'library' | 'orchestrator'; startX: number; startWidth: number } | null>(null);
  const applyOperations = useDaemon<any>(`/projects/${projectId}/timeline/apply`, { method: 'POST' });

  const timelineData = useDaemon<TimelineResponse>(
    `/projects/${projectId}/timeline`,
    { method: 'GET' }
  );
  const generate = useDaemon<GenerateResponse>(
    `/projects/${projectId}/generate`,
    { method: 'POST' }
  );
  const exportApi = useDaemon<ExportResponse>(
    `/projects/${projectId}/export`,
    { method: 'POST' }
  );

  // Load timeline on mount or when project changes
  useEffect(() => {
    timelineData.execute();
    // Reset selected clip and video when switching projects
    setSelectedClip(null);
    setVideoSrc('');
    setVideoStartTime(undefined);
    setVideoEndTime(undefined);
    setCurrentTime(0);
    setPlayheadPosition(0);
    playheadPositionRef.current = 0;
    // Clear active upload and analysis jobs when switching projects
    setActiveJobIds(new Set());
    setActiveAnalysisJobIds(new Set());
    setHasActiveUploadJobs(false);
    setHasActiveAnalysisJobs(false);
    // Clear history when switching projects
    setTimelineHistory([]);
    setRedoHistory([]);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [projectId]);

  useEffect(() => {
    // Only update timeline from timelineData if we don't have a timeline yet
    // or if timelineData was explicitly refreshed (not from automatic polling)
    if (timelineData.data?.timeline) {
      // Only update if timeline is null or if the data is newer
      // For now, always update to ensure we have the latest from server
      setTimeline(timelineData.data.timeline);
    }
  }, [timelineData.data]);

  const handleGenerate = async () => {
    setIsGenerating(true);
    const result = await generate.execute({});
    setIsGenerating(false);
    
    if (result) {
      // Refresh timeline
      timelineData.execute();
    }
  };

  const handleExport = async () => {
    setIsExporting(true);
    const result = await exportApi.execute({
      out_path: `./exports/project_${projectId}_${Date.now()}.mp4`,
    });
    setIsExporting(false);
    
    if (result) {
      // Export job started
    }
  };

  const handleLibraryClipSelect = (asset: any) => {
    // Preview raw clip from library
    setSelectedClip(asset);
    setVideoSrc(`http://127.0.0.1:7777/api/projects/${projectId}/media/${asset.id}/proxy`);
    setVideoStartTime(undefined);
    setVideoEndTime(undefined);
    setCurrentTime(0);
  };

  const handleTimelineClipClick = (clip: any) => {
    // Check if this is a text clip - can be identified by:
    // 1. Being on a Caption track
    // 2. Having text_content or text_template property
    // 3. Having asset_id === 0 (placeholder for text clips)
    const track = timeline?.tracks?.find(t => 
      t.clips?.some((c: any) => 
        (c.id === clip.id) || 
        (c.asset_id === clip.asset_id && c.timeline_start_ticks === clip.timeline_start_ticks) ||
        (clip.id && c.id === clip.id)
      )
    );
    
    const isTextClip = 
      (track && (track.kind === 'Caption' || track.kind === 'caption')) ||
      (clip.text_content !== undefined) ||
      (clip.text_template !== undefined) ||
      (clip.asset_id === 0 || clip.asset_id === null);
    
    if (isTextClip) {
      // Single click on text clip opens editor
      console.log('Opening text editor for clip:', clip);
      setEditingTextClip(clip);
      return;
    }
    
    // Stop any ongoing playback when clicking a clip
    if (isTimelinePlaying) {
      stopTimelinePlayback();
    }
    // Play clip from timeline (only if it has an asset_id - video clips)
    if (clip.asset_id) {
    const clipPlayheadTicks = clip.timeline_start_ticks;
    setSelectedClip(clip);
    setVideoSrc(`http://127.0.0.1:7777/api/projects/${projectId}/media/${clip.asset_id}/proxy`);
    setVideoStartTime(clip.in_ticks / 48000);
    setVideoEndTime(clip.out_ticks / 48000);
    setPlayheadPosition(clipPlayheadTicks);
    playheadPositionRef.current = clipPlayheadTicks; // Update ref immediately
    setCurrentTime(clip.in_ticks / 48000);
    } else {
      // Text or audio clip - just select it
      setSelectedClip(clip);
    }
  };
  
  const handleTextClipSave = async (clipId: string, text: string) => {
    // Update text clip content - use text_content field to match backend
    const operation = {
      type: 'UpdateClipText',
      clip_id: clipId,
      text_content: text || 'Add text', // Default to "Add text" if empty
    };

    try {
      const result = await applyOperations.execute({ operations: [operation] });
      if (result && result.timeline) {
        setTimeline(result.timeline);
      }
      timelineData.execute();
    } catch (error) {
      console.error('Error updating text clip:', error);
      // Fallback: refresh timeline
      timelineData.execute();
    }
  };

  const handleImportComplete = () => {
    // Refresh timeline if needed
    // Could trigger a timeline refresh here
  };

  const handleJobUpdate = (job: any) => {
    // Handle job updates from MediaLibrary
    // Track upload jobs and analysis jobs separately
    const status = typeof job.status === 'string' && job.status.startsWith('"') 
      ? JSON.parse(job.status) 
      : job.status;
    
    // Parse job type - it comes as a JSON string from the API
    // The API serializes the enum as a JSON string, so job.job_type is already a string like "TranscribeAsset"
    // But it might be double-encoded, so we need to handle both cases
    let jobType: string | null = null;
    if (job.job_type) {
      if (typeof job.job_type === 'string') {
        // Try to parse if it looks like a JSON string (starts and ends with quotes)
        if (job.job_type.startsWith('"') && job.job_type.endsWith('"')) {
          try {
            jobType = JSON.parse(job.job_type);
          } catch (e) {
            // If parsing fails, try removing outer quotes manually
            jobType = job.job_type.slice(1, -1);
          }
        } else {
          // Already a plain string
          jobType = job.job_type;
        }
      }
    }
    
    // Debug logging to help diagnose
    console.log('[Editor] Job update:', { 
      id: job.id, 
      job_type: jobType, 
      status, 
      raw_job_type: job.job_type,
      has_job_type: !!job.job_type,
      job_type_type: typeof job.job_type
    });
    
    // Determine if this is an upload job or analysis job
    const isUploadJob = jobType === 'ImportRaw' || jobType === 'ImportReference' || jobType === 'ImportAudio';
    const isAnalysisJob = jobType === 'TranscribeAsset' || 
                          jobType === 'AnalyzeVisionAsset' || 
                          jobType === 'BuildSegments' ||
                          jobType === 'EnrichSegmentsFromTranscript' ||
                          jobType === 'EnrichSegmentsFromVision' ||
                          jobType === 'ComputeSegmentMetadata' ||
                          jobType === 'EmbedSegments';
    
    console.log('[Editor] Job classification:', { 
      id: job.id, 
      jobType, 
      isUploadJob, 
      isAnalysisJob, 
      status 
    });
    
    if (status === 'Pending' || status === 'Running') {
      if (isUploadJob) {
        console.log('[Editor] Adding upload job:', job.id);
        setActiveJobIds((prev) => {
          const newSet = new Set(prev);
          newSet.add(job.id);
          return newSet;
        });
      }
      if (isAnalysisJob) {
        console.log('[Editor] Adding analysis job:', job.id, 'type:', jobType);
        setActiveAnalysisJobIds((prev) => {
          const newSet = new Set(prev);
          newSet.add(job.id);
          console.log('[Editor] Analysis job IDs after add:', Array.from(newSet));
          return newSet;
        });
      }
    } else if (status === 'Completed' || status === 'Failed' || status === 'Cancelled') {
      if (isUploadJob) {
        console.log('[Editor] Removing upload job:', job.id);
        setActiveJobIds((prev) => {
          const newSet = new Set(prev);
          newSet.delete(job.id);
          return newSet;
        });
      }
      if (isAnalysisJob) {
        console.log('[Editor] Removing analysis job:', job.id);
        setActiveAnalysisJobIds((prev) => {
          const newSet = new Set(prev);
          newSet.delete(job.id);
          return newSet;
        });
      }
    }
  };

  // Update hasActiveUploadJobs based on whether there are any active upload jobs
  useEffect(() => {
    setHasActiveUploadJobs(activeJobIds.size > 0);
    console.log('[Editor] Active upload jobs:', activeJobIds.size);
  }, [activeJobIds]);

  // Update hasActiveAnalysisJobs based on whether there are any active analysis jobs
  useEffect(() => {
    setHasActiveAnalysisJobs(activeAnalysisJobIds.size > 0);
    console.log('[Editor] Active analysis jobs:', activeAnalysisJobIds.size, 'isAnalyzing:', activeAnalysisJobIds.size > 0);
  }, [activeAnalysisJobIds]);

  // Handle resize mouse move
  useEffect(() => {
    if (!resizingPanel || !resizeStartRef.current) return;
    
    const handleMouseMove = (e: MouseEvent) => {
      if (!resizeStartRef.current) return;
      
      const deltaX = e.clientX - resizeStartRef.current.startX;
      const minWidth = 80;
      const maxWidth = window.innerWidth * 0.5;
      
      if (resizeStartRef.current.panel === 'library') {
        const newWidth = Math.max(minWidth, Math.min(maxWidth, resizeStartRef.current.startWidth + deltaX));
        setLibraryWidth(newWidth);
      } else if (resizeStartRef.current.panel === 'orchestrator') {
        // For orchestrator, we resize from the left edge, so deltaX should be negative
        const newWidth = Math.max(minWidth, Math.min(maxWidth, resizeStartRef.current.startWidth - deltaX));
        setOrchestratorWidth(newWidth);
      }
    };
    
    const handleMouseUp = () => {
      setResizingPanel(null);
      resizeStartRef.current = null;
    };
    
    window.addEventListener('mousemove', handleMouseMove);
    window.addEventListener('mouseup', handleMouseUp);
    
    return () => {
      window.removeEventListener('mousemove', handleMouseMove);
      window.removeEventListener('mouseup', handleMouseUp);
    };
  }, [resizingPanel, libraryWidth, orchestratorWidth]);
  
  const handleResizeStart = (panel: 'library' | 'orchestrator', e: React.MouseEvent) => {
    e.preventDefault();
    setResizingPanel(panel);
    resizeStartRef.current = {
      panel,
      startX: e.clientX,
      startWidth: panel === 'library' ? libraryWidth : orchestratorWidth,
    };
  };

  // Resolve which clip should be playing at a given timestamp using vertical priority
  // Topmost *video* clip that spans the timestamp wins (Final Cut Pro model).
  // Text clips (asset_id === 0) are overlays only and must NOT drive video playback.
  const resolveClipAtTimestamp = useCallback((timeline: TimelineData, ticks: number): any | null => {
    if (!timeline || !timeline.tracks) return null;
    
    // Get all video tracks
    const videoTracks = timeline.tracks.filter((t: any) => 
      t.kind === 'Video' || t.kind === 'video' || !t.kind
    );
    
    // Separate overlay tracks (id > 1) and primary track (id = 1)
    const overlayTracks = videoTracks
      .filter((t: any) => (t.id || 1) > 1)
      .sort((a: any, b: any) => (b.id || 0) - (a.id || 0)); // Sort descending (topmost first)
    const primaryTrack = videoTracks.find((t: any) => (t.id || 1) === 1);
    
    // Check tracks in order: overlay tracks (top to bottom), then primary track
    const tracksToCheck = [...overlayTracks];
    if (primaryTrack) {
      tracksToCheck.push(primaryTrack);
    }
    
    // For each track from top to bottom, find a clip that spans this timestamp
    for (const track of tracksToCheck) {
      if (!track.clips) continue;
      
      for (const clip of track.clips) {
         // Skip text/caption-only clips (no real video asset)
         if (!clip.asset_id || clip.asset_id === 0) {
           continue;
         }
         
        const clipStartTicks = clip.timeline_start_ticks;
        const clipDuration = clip.out_ticks - clip.in_ticks;
        const clipEndTicks = clipStartTicks + clipDuration;
        
        // Check if timestamp is within this clip's range
        if (ticks >= clipStartTicks && ticks < clipEndTicks) {
          // Found a clip that spans this timestamp - return it (topmost wins)
          return { ...clip, track };
        }
      }
    }
    
    // No clip found at this timestamp
    return null;
  }, []);

  const handleTimeUpdate = useCallback((time: number) => {
    setCurrentTime(time);
    // Update playhead position based on current playing clip
    if (currentPlayingClip && isTimelinePlaying && timeline) {
      const clipStartTicks = currentPlayingClip.timeline_start_ticks;
      const clipInSeconds = currentPlayingClip.in_ticks / 48000;
      // Calculate timeline position: clip start + (current video time - clip in point)
      const timelineTime = (clipStartTicks / 48000) + (time - clipInSeconds);
      const newPlayheadTicks = Math.round(timelineTime * 48000);
      setPlayheadPosition(newPlayheadTicks);
      playheadPositionRef.current = newPlayheadTicks; // Update ref immediately
      
      // Use vertical priority resolution to determine what clip should be playing at this timestamp
      // This handles overlay start/end transitions seamlessly (Final Cut Pro model)
      const resolvedClip = resolveClipAtTimestamp(timeline, newPlayheadTicks);
      
      // Check if resolved clip differs from current clip (transition needed)
      if (resolvedClip && !transitioningRef.current) {
        const currentClipId = currentPlayingClip.id || `${currentPlayingClip.asset_id}-${currentPlayingClip.timeline_start_ticks}`;
        const resolvedClipId = resolvedClip.id || `${resolvedClip.asset_id}-${resolvedClip.timeline_start_ticks}`;
        
        if (currentClipId !== resolvedClipId) {
          // Clip resolution changed - transition to new clip
          transitioningRef.current = true; // Set guard to prevent duplicate transitions
          
          // Calculate the source time within the resolved clip
          const resolvedClipStartTicks = resolvedClip.timeline_start_ticks;
          const playheadOffset = newPlayheadTicks - resolvedClipStartTicks; // Offset from clip start in timeline
          // Ensure offset is within clip duration
          const clipDuration = resolvedClip.out_ticks - resolvedClip.in_ticks;
          const clampedOffset = Math.max(0, Math.min(playheadOffset, clipDuration));
          // Calculate source time: clip's in_ticks + offset from clip start
          const sourceTimeTicks = resolvedClip.in_ticks + clampedOffset;
          // Clamp to clip's valid range
          const clampedSourceTimeTicks = Math.max(
            resolvedClip.in_ticks,
            Math.min(sourceTimeTicks, resolvedClip.out_ticks - 1)
          );
          const sourceTime = clampedSourceTimeTicks / 48000;
          
          // Transition to resolved clip
          setCurrentPlayingClip(resolvedClip);
          setSelectedClip(resolvedClip);
          setVideoSrc(`http://127.0.0.1:7777/api/projects/${projectId}/media/${resolvedClip.asset_id}/proxy`);
          setVideoStartTime(resolvedClip.in_ticks / 48000);
          setVideoEndTime(resolvedClip.out_ticks / 48000);
          setCurrentTime(sourceTime);
          
          // Reset transition guard after transition completes
          setTimeout(() => {
            transitioningRef.current = false;
          }, 300);
        }
      } else if (!resolvedClip && !transitioningRef.current) {
          // No clip at this timestamp - check if we should continue or stop
          // Find next clip after current position
          const allClips: any[] = [];
          timeline.tracks?.forEach((track: any) => {
            track.clips?.forEach((clip: any) => {
              allClips.push({ ...clip, track });
            });
          });
          allClips.sort((a, b) => a.timeline_start_ticks - b.timeline_start_ticks);
          
          const nextClip = allClips.find((clip) => clip.timeline_start_ticks > newPlayheadTicks);
          if (nextClip) {
            // There's a clip coming up, wait for it
            // Don't stop playback - just continue (clip will resolve when we reach it)
        } else {
          // No more clips, stop playback
          setIsTimelinePlaying(false);
          setCurrentPlayingClip(null);
        }
      }
    }
  }, [currentPlayingClip, isTimelinePlaying, timeline, projectId, resolveClipAtTimestamp]);

  // Calculate source time for timeline hover position
  useEffect(() => {
    if (!timeline || hoverTime === 0 || isTimelinePlaying) {
      // Don't update hover preview when playing or no hover
      setHoverSourceTime(undefined);
      setHoverVideoSrc('');
      return;
    }

    // Use vertical priority resolution to find clip at hover time
    const hoverTicks = hoverTime * 48000;
    const hoverClip = resolveClipAtTimestamp(timeline, hoverTicks);

    if (hoverClip) {
      // Calculate source time within the clip
      // offsetFromClipStart is the offset from the clip's timeline start position (in timeline ticks)
      const offsetFromClipStart = hoverTicks - hoverClip.timeline_start_ticks;
      // Ensure offset is non-negative and within clip duration
      const clipDuration = hoverClip.out_ticks - hoverClip.in_ticks;
      const clampedOffset = Math.max(0, Math.min(offsetFromClipStart, clipDuration));
      // Convert offset to source ticks (assuming 1x speed - timeline ticks = source ticks)
      // Then add to the clip's in_ticks to get the absolute source time
      const sourceTimeTicks = hoverClip.in_ticks + clampedOffset;
      // Clamp to clip's valid range to ensure we don't go outside the clip's boundaries
      const clampedSourceTimeTicks = Math.max(
        hoverClip.in_ticks,
        Math.min(sourceTimeTicks, hoverClip.out_ticks - 1)
      );
      const sourceTime = clampedSourceTimeTicks / 48000;
      
      setHoverSourceTime(sourceTime);
      setHoverVideoSrc(`http://127.0.0.1:7777/api/projects/${projectId}/media/${hoverClip.asset_id}/proxy`);
    } else {
      setHoverSourceTime(undefined);
      setHoverVideoSrc('');
    }
  }, [hoverTime, timeline, isTimelinePlaying, projectId, resolveClipAtTimestamp]);

  // Handle video ended - fallback transition (should rarely be needed since handleTimeUpdate handles transitions)
  const handleVideoEnded = () => {
    // This is a fallback - transitions should be handled by handleTimeUpdate before video ends
    // But if we get here, it means handleTimeUpdate didn't catch it, so handle it now
    if (!timeline || !currentPlayingClip || transitioningRef.current) return;

    transitioningRef.current = true;

    // Find all clips sorted by timeline position
    const allClips: any[] = [];
    timeline.tracks?.forEach((track: any) => {
      track.clips?.forEach((clip: any) => {
        allClips.push({ ...clip, track });
      });
    });
    allClips.sort((a, b) => a.timeline_start_ticks - b.timeline_start_ticks);

    // Find current clip index
    const currentIndex = allClips.findIndex(
      (clip) => clip.id === currentPlayingClip.id || 
      (clip.asset_id === currentPlayingClip.asset_id && 
       clip.timeline_start_ticks === currentPlayingClip.timeline_start_ticks)
    );

    if (currentIndex >= 0 && currentIndex < allClips.length - 1) {
      // Play next clip
      const nextClip = allClips[currentIndex + 1];
      setCurrentPlayingClip(nextClip);
      setVideoSrc(`http://127.0.0.1:7777/api/projects/${projectId}/media/${nextClip.asset_id}/proxy`);
      setVideoStartTime(nextClip.in_ticks / 48000);
      setVideoEndTime(nextClip.out_ticks / 48000);
      const nextPlayheadTicks = nextClip.timeline_start_ticks;
      setPlayheadPosition(nextPlayheadTicks);
      playheadPositionRef.current = nextPlayheadTicks; // Update ref immediately
      setCurrentTime(nextClip.in_ticks / 48000);
      
      // Reset transition guard
      setTimeout(() => {
        transitioningRef.current = false;
      }, 200);
    } else {
      // No more clips, stop playback
      setIsTimelinePlaying(false);
      setCurrentPlayingClip(null);
      transitioningRef.current = false;
    }
  };

  // Start timeline playback from playhead position
  const startTimelinePlayback = useCallback(() => {
    if (!timeline) return;

    // Use ref to get the most up-to-date playhead position (avoids stale closure values)
    const playheadTicks = playheadPositionRef.current || 0;

    // Use vertical priority resolution to find the clip at playhead position
    // This ensures topmost clip wins (Final Cut Pro model)
    let clipToPlay = resolveClipAtTimestamp(timeline, playheadTicks);

    // If no clip at playhead position, find the first clip after playhead
    if (!clipToPlay) {
      // Get all clips sorted by timeline position for fallback
    const allClips: any[] = [];
    timeline.tracks?.forEach((track: any) => {
      track.clips?.forEach((clip: any) => {
        allClips.push({ ...clip, track });
      });
    });
    allClips.sort((a, b) => a.timeline_start_ticks - b.timeline_start_ticks);

      clipToPlay = allClips.find(
        (clip) => clip.timeline_start_ticks >= playheadTicks
      );

    // If no clip after playhead, use first clip
    if (!clipToPlay && allClips.length > 0) {
      clipToPlay = allClips[0];
      }
    }

    if (clipToPlay) {
      setIsTimelinePlaying(true);
      setCurrentPlayingClip(clipToPlay);
      setSelectedClip(clipToPlay);
      setVideoSrc(`http://127.0.0.1:7777/api/projects/${projectId}/media/${clipToPlay.asset_id}/proxy`);
      
      // Calculate the video start time based on playhead position
      const clipStart = clipToPlay.timeline_start_ticks;
      const clipDuration = clipToPlay.out_ticks - clipToPlay.in_ticks;
      
      // If playhead is within this clip, calculate the offset
      let videoStartTime: number;
      if (playheadTicks >= clipStart && playheadTicks < clipStart + clipDuration) {
        // Playhead is within this clip - calculate the source time
        const playheadOffset = playheadTicks - clipStart; // Offset from clip start in timeline
        // Ensure offset is within clip duration
        const clampedOffset = Math.max(0, Math.min(playheadOffset, clipDuration));
        // Calculate source time: clip's in_ticks + offset from clip start
        const sourceTimeTicks = clipToPlay.in_ticks + clampedOffset;
        // Clamp to clip's valid range
        const clampedSourceTimeTicks = Math.max(
          clipToPlay.in_ticks,
          Math.min(sourceTimeTicks, clipToPlay.out_ticks - 1)
        );
        videoStartTime = clampedSourceTimeTicks / 48000;
      } else {
        // Playhead is before this clip - start from clip's in point
        videoStartTime = clipToPlay.in_ticks / 48000;
      }
      
      setVideoStartTime(videoStartTime);
      setVideoEndTime(clipToPlay.out_ticks / 48000);
      // Don't reset playhead position - keep it at the current position
      // setPlayheadPosition(clipToPlay.timeline_start_ticks);
      setCurrentTime(videoStartTime);
    }
  }, [timeline, projectId, resolveClipAtTimestamp]);

  // Stop timeline playback
  const stopTimelinePlayback = useCallback(() => {
    setIsTimelinePlaying(false);
    setCurrentPlayingClip(null);
    // Note: Video will be paused by VideoPlayer when isPlaying becomes false
  }, []);

  // Calculate total duration from timeline
  const calculateTotalDuration = (): number => {
    if (!timeline || !timeline.tracks) return 0;
    let maxEnd = 0;
    for (const track of timeline.tracks) {
      for (const clip of track.clips || []) {
        const clipEnd = clip.timeline_start_ticks + (clip.out_ticks - clip.in_ticks);
        if (clipEnd > maxEnd) {
          maxEnd = clipEnd;
        }
      }
    }
    return maxEnd / 48000; // Convert ticks to seconds
  };

  const totalDuration = calculateTotalDuration();

  // Helper function to save current timeline state to history
  const saveToHistory = useCallback((currentTimeline: TimelineData | null) => {
    if (currentTimeline) {
      // Deep clone the timeline to save to history
      const timelineCopy = JSON.parse(JSON.stringify(currentTimeline));
      setTimelineHistory((prev) => [...prev, timelineCopy]);
      // Clear redo history when a new action is performed
      setRedoHistory([]);
    }
  }, []);

  // Helper function to rebuild timeline from state
  const rebuildTimelineFromState = useCallback(async (targetTimeline: TimelineData) => {
    try {
      // First, clear the timeline
      const clearOp = { type: 'ClearTimeline' };
      await applyOperations.execute({ operations: [clearOp] });
      
      // Then, rebuild the timeline by inserting all clips from target state
      if (targetTimeline.tracks) {
        const rebuildOps: any[] = [];
        
        // Collect all clips from all tracks, sorted by position
        const allClips: Array<{ clip: any; trackId: number }> = [];
        targetTimeline.tracks.forEach((track: any) => {
          const trackId = track.id || 1;
          if (track.clips) {
            track.clips.forEach((clip: any) => {
              allClips.push({ clip, trackId });
            });
          }
        });
        
        // Sort by timeline position
        allClips.sort((a, b) => a.clip.timeline_start_ticks - b.clip.timeline_start_ticks);
        
        // Create insert operations for each clip
        // Note: We'll use RippleInsertClip for primary track, InsertLayeredClip for others
        for (const { clip, trackId } of allClips) {
          if (trackId === 1) {
            rebuildOps.push({
              type: 'RippleInsertClip',
              asset_id: clip.asset_id,
              position_ticks: clip.timeline_start_ticks,
              duration_ticks: clip.out_ticks - clip.in_ticks,
            });
          } else {
            rebuildOps.push({
              type: 'InsertLayeredClip',
              asset_id: clip.asset_id,
              position_ticks: clip.timeline_start_ticks,
              duration_ticks: clip.out_ticks - clip.in_ticks,
              base_track_id: 1,
            });
          }
        }
        
        // Apply all rebuild operations
        if (rebuildOps.length > 0) {
          const rebuildResult = await applyOperations.execute({ operations: rebuildOps });
          if (rebuildResult && rebuildResult.timeline) {
            setTimeline(rebuildResult.timeline);
          }
        }
      }
      
      // Refresh from server to ensure consistency
      setTimeout(() => {
        timelineData.execute();
      }, 100);
    } catch (error) {
      console.error('Error rebuilding timeline:', error);
      // Refresh from server to try to sync
      setTimeout(() => {
        timelineData.execute();
      }, 100);
    }
  }, [applyOperations, timelineData]);

  // Handle undo - restore previous timeline state
  const handleUndo = useCallback(async () => {
    if (timelineHistory.length === 0) {
      return; // Nothing to undo
    }

    // Get the previous timeline state from history
    const previousTimeline = timelineHistory[timelineHistory.length - 1];
    
    if (!previousTimeline) return;
    
    // Stop playback if active
    if (isTimelinePlaying) {
      stopTimelinePlayback();
    }
    
    // Save current timeline state to redo history before undoing
    if (timeline) {
      const currentTimelineCopy = JSON.parse(JSON.stringify(timeline));
      setRedoHistory((prev) => [...prev, currentTimelineCopy]);
    }
    
    // Restore timeline state locally first
    setTimeline(previousTimeline);
    
    // Remove from history (we've used this state)
    setTimelineHistory((prev) => prev.slice(0, -1));
    
    // Rebuild timeline on server
    await rebuildTimelineFromState(previousTimeline);
  }, [timelineHistory, timeline, isTimelinePlaying, stopTimelinePlayback, rebuildTimelineFromState]);

  // Handle redo - restore next timeline state
  const handleRedo = useCallback(async () => {
    if (redoHistory.length === 0) {
      return; // Nothing to redo
    }

    // Get the next timeline state from redo history
    const nextTimeline = redoHistory[redoHistory.length - 1];
    
    if (!nextTimeline) return;
    
    // Stop playback if active
    if (isTimelinePlaying) {
      stopTimelinePlayback();
    }
    
    // Save current timeline state to undo history before redoing
    if (timeline) {
      const currentTimelineCopy = JSON.parse(JSON.stringify(timeline));
      setTimelineHistory((prev) => [...prev, currentTimelineCopy]);
    }
    
    // Restore timeline state locally first
    setTimeline(nextTimeline);
    
    // Remove from redo history (we've used this state)
    setRedoHistory((prev) => prev.slice(0, -1));
    
    // Rebuild timeline on server
    await rebuildTimelineFromState(nextTimeline);
  }, [redoHistory, timeline, isTimelinePlaying, stopTimelinePlayback, rebuildTimelineFromState]);

  // Handle text clip insertion
  const handleTextClipInsert = async (template: any, positionTicks: number, intent?: 'primary' | 'layered', trackId?: number) => {
    // Clear drag state immediately to prevent repeated insertions
    setDragTextTemplate(null);
    handleDragEnd();
    
    // Default duration for text clips: 3 seconds
    const defaultDurationTicks = 3 * 48000;
    
    let operation: any;
    
    if (intent === 'primary') {
      // Insert as primary clip on the primary timeline (track 1)
      operation = {
        type: 'RippleInsertClip',
        asset_id: 0, // Placeholder - text clips don't have asset_id
        position_ticks: positionTicks,
        duration_ticks: defaultDurationTicks,
        text_content: 'Add text', // Default text
        text_template: template.name,
        text_color: '#ffffff', // White text by default
        background_color: 'transparent', // Transparent background
        background_alpha: 0, // Fully transparent background
      };
    } else {
      // Insert as overlay clip above primary timeline
      // Find or create a caption track
      let captionTrackId = 2; // Start caption tracks at ID 2
      if (trackId) {
        captionTrackId = trackId;
      } else if (timeline && timeline.tracks) {
        const captionTracks = timeline.tracks.filter(t => t.kind === 'Caption' || t.kind === 'caption');
        if (captionTracks.length > 0) {
          captionTrackId = Math.max(...captionTracks.map(t => t.id || 2));
        }
      }
      
      // Create a text clip using InsertLayeredClip operation
      operation = {
        type: 'InsertLayeredClip',
        asset_id: 0, // Placeholder - text clips don't have asset_id
        position_ticks: positionTicks,
        duration_ticks: defaultDurationTicks,
        base_track_id: 1, // Layer above primary track
        track_id: captionTrackId,
        text_content: 'Add text', // Default text
        text_template: template.name,
        text_color: '#ffffff', // White text by default
        background_color: 'transparent', // Transparent background
        background_alpha: 0, // Fully transparent background
      };
    }

    try {
      const result = await applyOperations.execute({ operations: [operation] });
      if (result) {
      timelineData.execute();
      }
    } catch (error) {
      console.error('Error inserting text clip:', error);
    }
  };

  // Handle audio clip insertion
  const handleAudioClipInsert = async (audioAsset: any, positionTicks: number) => {
    // Clear drag state immediately to prevent repeated insertions
    setDragAudioAsset(null);
    handleDragEnd();
    
    // Find or create an audio track
    let audioTrackId = 10; // Start audio tracks at ID 10
    if (timeline && timeline.tracks) {
      const audioTracks = timeline.tracks.filter(t => t.kind === 'Audio' || t.kind === 'audio');
      if (audioTracks.length > 0) {
        audioTrackId = Math.max(...audioTracks.map(t => t.id || 10));
      }
    }
    
    // Create an audio clip using InsertLayeredClip operation
    const operation = {
      type: 'InsertLayeredClip',
      asset_id: audioAsset.id,
      position_ticks: positionTicks,
      duration_ticks: audioAsset.duration_ticks,
      base_track_id: 1, // Layer below primary track
      track_id: audioTrackId,
    };

    try {
      const result = await applyOperations.execute({ operations: [operation] });
      if (result) {
        timelineData.execute();
      }
    } catch (error) {
      console.error('Error inserting audio clip:', error);
    }
  };

  // Handle clip insertion
  const handleClipInsert = async (assetId: number, positionTicks: number, trackId: number, intent?: 'primary' | 'layered', modifierKey?: boolean) => {
    console.log('handleClipInsert called:', { assetId, positionTicks, trackId });
    
    // Save current timeline state to history before making changes
    saveToHistory(timeline);
    
    // Fetch asset duration
    const mediaAssets = await fetch(`http://127.0.0.1:7777/api/projects/${projectId}/media`)
      .then(res => res.json())
      .catch(() => []);
    
    const asset = mediaAssets.find((a: any) => a.id === assetId);
    if (!asset) {
      console.error('Asset not found');
      // Clear drag asset even on error
      setDragAsset(null);
      handleDragEnd();
      return;
    }

    // Determine operation type based on intent and modifier key
    let operation: any;
    if (intent === 'layered') {
      // Layered insert - always use InsertLayeredClip
      operation = {
        type: 'InsertLayeredClip',
        asset_id: assetId,
        position_ticks: positionTicks,
        duration_ticks: asset.duration_ticks,
        base_track_id: 1, // Layer above primary track (track 1)
      };
    } else if (modifierKey) {
      // Overwrite mode (Shift/Alt + primary intent)
      operation = {
        type: 'OverwriteClip',
        asset_id: assetId,
        position_ticks: positionTicks,
        duration_ticks: asset.duration_ticks,
      };
    } else {
      // Primary intent - use RippleInsertClip (default Final Cut behavior)
      operation = {
        type: 'RippleInsertClip',
        asset_id: assetId,
        position_ticks: positionTicks,
        duration_ticks: asset.duration_ticks,
      };
    }

    console.log('Sending operation:', operation);
    try {
      const result = await applyOperations.execute({ operations: [operation] });
      console.log('Operation result:', result);
      console.log('Operation result type:', typeof result);
      console.log('Operation result keys:', result ? Object.keys(result) : 'null');
    
      // Clear drag asset after operation completes (success or failure)
      // Use setTimeout to ensure state updates happen after the current event loop
      setTimeout(() => {
        setDragAsset(null);
        handleDragEnd();
      }, 0);
      
      if (result) {
      // The API returns TimelineResponse { timeline: {...} }
      // useDaemon returns the response object directly, so result should be { timeline: {...} }
      console.log('Full result object:', JSON.stringify(result, null, 2));
      
      let timelineToSet: TimelineData | null = null;
      
      // The result should be { timeline: {...} }
      if (result && typeof result === 'object' && 'timeline' in result) {
        const timelineValue = (result as any).timeline;
        console.log('Extracted timeline value:', timelineValue);
        console.log('Timeline value type:', typeof timelineValue);
        console.log('Timeline value keys:', timelineValue ? Object.keys(timelineValue) : 'null');
        
        // Check if timeline has the expected structure
        if (timelineValue && typeof timelineValue === 'object') {
          // Ensure it has tracks array (even if empty)
          timelineToSet = {
            tracks: timelineValue.tracks || [],
            captions: timelineValue.captions || [],
            music: timelineValue.music || [],
            ...timelineValue, // Spread to include settings and other properties
          };
          console.log('Processed timeline to set:', timelineToSet);
          console.log('Tracks in processed timeline:', timelineToSet.tracks);
          console.log('Number of tracks:', timelineToSet.tracks?.length || 0);
        }
      }
      
      if (timelineToSet) {
        console.log('Setting timeline state with tracks:', timelineToSet.tracks?.length || 0);
        setTimeline(timelineToSet);
      } else {
        console.error('Could not extract valid timeline from result. Result structure:', result);
        // Fallback: refresh from server immediately
        const refreshed = await timelineData.execute();
        if (refreshed && (refreshed as any).timeline) {
          setTimeline((refreshed as any).timeline);
        }
      }
      
      // Refresh from server to ensure consistency (with a small delay to let state update)
      setTimeout(() => {
        timelineData.execute();
      }, 100);
      // If timeline was playing, restart from current position
      if (isTimelinePlaying) {
        stopTimelinePlayback();
        // Use setTimeout to ensure timeline state is updated first
        setTimeout(() => {
          startTimelinePlayback();
        }, 200);
      }
    } else {
      // Operation failed - error should be logged by useDaemon
      console.error('Operation failed - result is null');
      // Clear drag asset even on error
      setTimeout(() => {
        setDragAsset(null);
        handleDragEnd();
      }, 0);
    }
    } catch (error) {
      console.error('Error in handleClipInsert:', error);
      // Clear drag asset even on error
      setTimeout(() => {
        setDragAsset(null);
        handleDragEnd();
      }, 0);
    }
  };

  // Handle clip trim
  const handleClipTrim = async (clipId: string, newInTicks: number, newOutTicks: number) => {
    // Save current timeline state to history before making changes
    saveToHistory(timeline);
    
    const operation = {
      type: 'TrimClip',
      clip_id: clipId,
      new_in_ticks: newInTicks,
      new_out_ticks: newOutTicks,
    };

    const result = await applyOperations.execute({ operations: [operation] });
    if (result) {
      timelineData.execute();
    }
  };

  // Handle clip split
  const handleClipSplit = async (clipId: string, positionTicks: number) => {
    // Save current timeline state to history before making changes
    saveToHistory(timeline);
    
    const operation = {
      type: 'SplitClip',
      clip_id: clipId,
      position_ticks: positionTicks,
    };

    const result = await applyOperations.execute({ operations: [operation] });
    if (result && result.timeline) {
      setTimeline(result.timeline);
      timelineData.execute();
    } else {
      timelineData.execute();
    }
  };

  // Handle clip delete
  const handleClipDelete = async (clipId: string) => {
    // Save current timeline state to history before making changes
    saveToHistory(timeline);
    
    const operation = {
      type: 'DeleteClip',
      clip_id: clipId,
    };

    const result = await applyOperations.execute({ operations: [operation] });
    if (result && result.timeline) {
      setTimeline(result.timeline);
      setSelectedClip(null);
      if (isTimelinePlaying && currentPlayingClip?.id === clipId) {
        stopTimelinePlayback();
      }
      timelineData.execute();
    } else {
      setSelectedClip(null);
      timelineData.execute();
    }
  };

  // Handle clip reorder (magnetic timeline reordering)
  const handleClipReorder = async (clipId: string, newPositionTicks: number) => {
    // Save current timeline state to history before making changes
    saveToHistory(timeline);
    
    const operation = {
      type: 'ReorderClip',
      clip_id: clipId,
      new_position_ticks: newPositionTicks,
    };

    const result = await applyOperations.execute({ operations: [operation] });
    if (result && result.timeline) {
      setTimeline(result.timeline);
      timelineData.execute();
      
      // If timeline was playing, restart from current position
      if (isTimelinePlaying) {
        stopTimelinePlayback();
        setTimeout(() => {
          startTimelinePlayback();
        }, 200);
      }
    } else {
      timelineData.execute();
    }
  };

  // Handle moving a clip (used for overlay clips)
  const handleMoveClip = async (clipId: string, newPositionTicks: number) => {
    const operation = {
      type: 'MoveClip',
      clip_id: clipId,
      new_position_ticks: newPositionTicks,
    };

    const result = await applyOperations.execute({ operations: [operation] });
    if (result && result.timeline) {
      setTimeline(result.timeline);
      timelineData.execute();
      
      // If timeline was playing, restart from current position
      if (isTimelinePlaying) {
        stopTimelinePlayback();
        setTimeout(() => {
          startTimelinePlayback();
        }, 200);
      }
    } else {
      timelineData.execute();
    }
  };

  // Handle primary clip → overlay conversion
  const handleConvertPrimaryToOverlay = async (clipId: string, positionTicks: number) => {
    console.log('handleConvertPrimaryToOverlay called', { clipId, positionTicks });
    
    // Save current timeline state to history before making changes
    saveToHistory(timeline);
    
    const operation = {
      type: 'ConvertPrimaryToOverlay',
      clip_id: clipId,
      position_ticks: positionTicks,
    };

    console.log('Sending ConvertPrimaryToOverlay operation', operation);
    try {
      const result = await applyOperations.execute({ operations: [operation] });
      console.log('ConvertPrimaryToOverlay result', result);
      console.log('Timeline tracks:', result?.timeline?.tracks);
      if (result && result.timeline) {
        console.log('Setting timeline with tracks:', result.timeline.tracks);
        console.log('Track details:', result.timeline.tracks.map((t: any) => ({ id: t.id, kind: t.kind, clips: t.clips?.length || 0 })));
        setTimeline(result.timeline);
        timelineData.execute();
        
        // If timeline was playing, restart from current position
        if (isTimelinePlaying) {
          stopTimelinePlayback();
          setTimeout(() => {
            startTimelinePlayback();
          }, 200);
        }
      } else {
        console.error('ConvertPrimaryToOverlay: No timeline in result', result);
        timelineData.execute();
      }
    } catch (error) {
      console.error('Error converting primary clip to overlay:', error);
      timelineData.execute();
    }
  };

  // Handle overlay clip → primary conversion
  const handleConvertOverlayToPrimary = async (clipId: string, positionTicks: number) => {
    console.log('handleConvertOverlayToPrimary called', { clipId, positionTicks });
    
    // Save current timeline state to history before making changes
    saveToHistory(timeline);
    
    const operation = {
      type: 'ConvertOverlayToPrimary',
      clip_id: clipId,
      position_ticks: positionTicks,
    };

    console.log('Sending ConvertOverlayToPrimary operation', operation);
    try {
      const result = await applyOperations.execute({ operations: [operation] });
      console.log('ConvertOverlayToPrimary result', result);
      if (result && result.timeline) {
        setTimeline(result.timeline);
        timelineData.execute();
        
        // If timeline was playing, restart from current position
        if (isTimelinePlaying) {
          stopTimelinePlayback();
          setTimeout(() => {
            startTimelinePlayback();
          }, 200);
        }
      } else {
        console.error('ConvertOverlayToPrimary: No timeline in result', result);
        timelineData.execute();
      }
    } catch (error) {
      console.error('Error converting overlay clip to primary:', error);
      timelineData.execute();
    }
  };

  // Handle clear timeline
  const handleClearTimeline = async () => {
    if (!window.confirm('Are you sure you want to clear the entire timeline? This cannot be undone.')) {
      return;
    }

    // Save current timeline state to history before making changes
    saveToHistory(timeline);

    const operation = {
      type: 'ClearTimeline',
    };

    const result = await applyOperations.execute({ operations: [operation] });
    if (result && result.timeline) {
      setTimeline(result.timeline);
      setSelectedClip(null);
      stopTimelinePlayback();
      timelineData.execute();
    } else {
      timelineData.execute();
    }
  };

  // Keyboard handler for spacebar (play/pause timeline), delete, and clear timeline
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      // Only handle if the event target is not an input field
      const target = e.target as HTMLElement;
      const isInputField = target.tagName === 'INPUT' || 
                          target.tagName === 'TEXTAREA' || 
                          target.tagName === 'SELECT' ||
                          target.isContentEditable;
      
      if (isInputField) {
        return;
      }

      // Undo shortcut: 'z' (Cmd/Ctrl+Z for standard undo, or just 'z' as requested)
      if ((e.key === 'z' || e.key === 'Z') && !e.shiftKey) {
        // Check if Cmd/Ctrl is pressed (standard undo) or just 'z' (as user requested)
        if (e.metaKey || e.ctrlKey || (!e.metaKey && !e.ctrlKey)) {
          e.preventDefault();
          handleUndo();
          return;
        }
      }

      // Redo shortcut: Cmd+Shift+Z or Ctrl+Shift+Z
      if ((e.key === 'z' || e.key === 'Z') && (e.metaKey || e.ctrlKey) && e.shiftKey) {
        e.preventDefault();
        handleRedo();
        return;
      }

      // Tool shortcuts: 'a' for pointer, 'b' for cut
      if (e.key === 'a' || e.key === 'A') {
        e.preventDefault();
        setActiveTool('pointer');
        return;
      }
      
      if (e.key === 'b' || e.key === 'B') {
        e.preventDefault();
        setActiveTool('cut');
        return;
      }

      // Spacebar: Toggle timeline playback (start from playhead position)
      if (e.code === 'Space') {
        e.preventDefault();
        if (isTimelinePlaying) {
          stopTimelinePlayback();
        } else {
          startTimelinePlayback();
        }
        return;
      }

      // Clear timeline: Cmd/Ctrl + Shift + Delete
      if ((e.key === 'Delete' || e.key === 'Backspace') && (e.metaKey || e.ctrlKey) && e.shiftKey) {
        e.preventDefault();
        handleClearTimeline();
        return;
      }

      // Delete selected clip: Delete or Backspace
      if ((e.key === 'Delete' || e.key === 'Backspace') && selectedClip) {
        // Check if selectedClip has an id (it's a ClipInstance from timeline)
        if (selectedClip.id) {
          handleClipDelete(selectedClip.id);
        } else if (selectedClip.asset_id) {
          // It's a MediaAsset, can't delete from timeline (only from library)
          // For now, do nothing or handle differently
          return;
        }
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [selectedClip, handleClearTimeline, isTimelinePlaying, startTimelinePlayback, stopTimelinePlayback, setActiveTool, handleUndo, handleRedo]);

  // Handle drag start from library
  const handleDragStart = (asset: any, event: React.MouseEvent) => {
    // Check if it's a text template, audio asset, or video asset
    if (asset && typeof asset === 'object') {
      if ('name' in asset && (asset.name === 'Title' || asset.name === 'Subtitle')) {
        // It's a text template
        setDragTextTemplate(asset);
        setDragAsset(null);
        setDragAudioAsset(null);
      } else if ('duration_ticks' in asset && !('width' in asset)) {
        // It's an audio asset (has duration_ticks but no width/height)
        setDragAudioAsset(asset);
        setDragAsset(null);
        setDragTextTemplate(null);
      } else {
        // It's a video asset
    setDragAsset(asset);
        setDragTextTemplate(null);
        setDragAudioAsset(null);
      }
    }
  };

  const handleDragEnd = () => {
    setDragAsset(null);
    setDragTextTemplate(null);
    setDragAudioAsset(null);
  };

  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        height: '100vh',
        width: '100vw',
        backgroundColor: '#1a1a1a',
        color: '#e5e5e5',
      }}
    >
      {/* Toolbar - spans full width */}
      <div style={{ height: '40px', flexShrink: 0 }}>
        <Toolbar
          onGenerate={handleGenerate}
          onExport={handleExport}
          isGenerating={isGenerating}
          isExporting={isExporting}
          isUploading={hasActiveUploadJobs}
          isAnalyzing={hasActiveAnalysisJobs}
          currentProjectId={projectId}
          currentProjectName={currentProjectName}
          projects={projects}
          onProjectSelect={onProjectSelect}
          onCreateProject={onCreateProject}
        />
      </div>

      {/* Resizable panels row */}
      <div style={{ display: 'flex', flex: 1, overflow: 'hidden', minHeight: 0 }}>
        {/* Media Sidebar - leftmost (fixed width) */}
        <div style={{ width: isSidebarCollapsed ? '40px' : '120px', overflow: 'hidden', flexShrink: 0, display: 'flex', flexDirection: 'column' }}>
          <MediaSidebar 
            selectedTab={mediaTab} 
            onTabChange={setMediaTab}
            onCollapseChange={setIsSidebarCollapsed}
          />
        </div>

        {/* Media Library */}
        <div style={{ width: `${libraryWidth}px`, overflow: 'hidden', flexShrink: 0, display: 'flex', flexDirection: 'column' }}>
          <MediaLibrary
            mode={mediaTab}
            onClipSelect={handleLibraryClipSelect}
            onImportComplete={handleImportComplete}
            onJobUpdate={handleJobUpdate}
            projectId={projectId}
            onDragStart={handleDragStart}
            onDragEnd={handleDragEnd}
            externalDragAsset={dragAsset}
          />
        </div>
        
        {/* Resize handle between library and viewer */}
        <div
          onMouseDown={(e) => handleResizeStart('library', e)}
          style={{
            width: '4px',
            cursor: 'col-resize',
            backgroundColor: resizingPanel === 'library' ? '#3b82f6' : '#404040',
            flexShrink: 0,
            userSelect: 'none',
          }}
        />

        {/* Viewer - center (takes remaining space) */}
        <div style={{ flex: 1, overflow: 'hidden', position: 'relative', display: 'flex', minWidth: 0 }}>
          <div style={{ flex: 1, overflow: 'hidden' }}>
            <Viewer
              videoSrc={hoverVideoSrc || videoSrc}
              startTime={hoverSourceTime !== undefined ? hoverSourceTime : videoStartTime}
              endTime={videoEndTime}
              currentTime={currentTime}
              onTimeUpdate={handleTimeUpdate}
              onEnded={isTimelinePlaying ? handleVideoEnded : undefined}
              isPlaying={isTimelinePlaying}
              timelineHoverTime={hoverTime}
              onPlayPause={(playing) => {
                // When user clicks play/pause button in video player, sync with timeline playback
                if (playing && !isTimelinePlaying) {
                  startTimelinePlayback();
                } else if (!playing && isTimelinePlaying) {
                  stopTimelinePlayback();
                }
              }}
            />
          </div>
          {/* Text Editor Panel or Orchestrator Panel - to the right of playback */}
          {editingTextClip ? (
            <div style={{ width: '200px', borderLeft: '1px solid #404040', backgroundColor: '#1a1a1a', display: 'flex', flexDirection: 'column', flexShrink: 0 }}>
              <TextEditorPanel
                clip={editingTextClip}
                onSave={handleTextClipSave}
                onClose={() => setEditingTextClip(null)}
              />
            </div>
          ) : showOrchestrator ? (
            <>
              {/* Resize handle for orchestrator panel */}
              <div
                onMouseDown={(e) => handleResizeStart('orchestrator', e)}
                style={{
                  width: '4px',
                  cursor: 'col-resize',
                  backgroundColor: resizingPanel === 'orchestrator' ? '#3b82f6' : '#404040',
                  flexShrink: 0,
                  userSelect: 'none',
                }}
              />
              <div style={{ width: `${orchestratorWidth}px`, overflow: 'hidden', flexShrink: 0, display: 'flex', flexDirection: 'column' }}>
                <OrchestratorPanel projectId={projectId} />
              </div>
            </>
          ) : null}
        </div>
      </div>

      {/* Timeline Toolbar - above timeline */}
      <div style={{ height: '300px', flexShrink: 0, overflow: 'hidden', display: 'flex', flexDirection: 'column' }}>
        <TimelineToolbar
          activeTool={activeTool}
          onToolChange={setActiveTool}
          playheadTime={playheadPosition / 48000}
          totalDuration={totalDuration}
        />
        <div style={{ flex: 1, overflow: 'hidden' }}>
          <Timeline
            timeline={timeline}
            selectedClip={selectedClip}
            onClipClick={handleTimelineClipClick}
            playheadPosition={playheadPosition}
            dragAsset={dragAsset}
            dragTextTemplate={dragTextTemplate}
            dragAudioAsset={dragAudioAsset}
            onClipInsert={handleClipInsert}
            onTextClipInsert={handleTextClipInsert}
            onAudioClipInsert={handleAudioClipInsert}
            onDragEnd={handleDragEnd}
            onHoverTimeChange={setHoverTime}
            onClipTrim={handleClipTrim}
            onClipSplit={handleClipSplit}
            onPlayheadSet={(ticks) => {
              // Stop any ongoing playback when playhead is manually set
              if (isTimelinePlaying) {
                stopTimelinePlayback();
              }
              setPlayheadPosition(ticks);
              playheadPositionRef.current = ticks; // Update ref immediately
            }}
            onClipReorder={handleClipReorder}
            onConvertPrimaryToOverlay={handleConvertPrimaryToOverlay}
            onConvertOverlayToPrimary={handleConvertOverlayToPrimary}
            onMoveClip={handleMoveClip}
            activeTool={activeTool}
            projectId={projectId}
          />
        </div>
      </div>
    </div>
  );
}

