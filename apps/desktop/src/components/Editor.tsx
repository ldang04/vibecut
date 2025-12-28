import { useState, useEffect, useCallback, useRef } from 'react';
import { Toolbar } from './Toolbar';
import { MediaLibrary } from './MediaLibrary';
import { MediaSidebar } from './MediaSidebar';
import { Viewer } from './Viewer';
import { Timeline } from './Timeline';
import { TimelineToolbar } from './TimelineToolbar';

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
  const [activeJobIds, setActiveJobIds] = useState<Set<number>>(new Set());
  const [mediaTab, setMediaTab] = useState<'raw' | 'references'>('raw');
  const [isSidebarCollapsed, setIsSidebarCollapsed] = useState(false);
  const [activeTool, setActiveTool] = useState<Tool>('pointer');
  const [dragAsset, setDragAsset] = useState<any | null>(null);
  const [hoverTime, setHoverTime] = useState<number>(0);
  const [isTimelinePlaying, setIsTimelinePlaying] = useState(false);
  const [currentPlayingClip, setCurrentPlayingClip] = useState<any | null>(null);
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
    // Clear active upload jobs when switching projects
    setActiveJobIds(new Set());
    setHasActiveUploadJobs(false);
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
    // Stop any ongoing playback when clicking a clip
    if (isTimelinePlaying) {
      stopTimelinePlayback();
    }
    // Play clip from timeline
    const clipPlayheadTicks = clip.timeline_start_ticks;
    setSelectedClip(clip);
    setVideoSrc(`http://127.0.0.1:7777/api/projects/${projectId}/media/${clip.asset_id}/proxy`);
    setVideoStartTime(clip.in_ticks / 48000);
    setVideoEndTime(clip.out_ticks / 48000);
    setPlayheadPosition(clipPlayheadTicks);
    playheadPositionRef.current = clipPlayheadTicks; // Update ref immediately
    setCurrentTime(clip.in_ticks / 48000);
  };

  const handleImportComplete = () => {
    // Refresh timeline if needed
    // Could trigger a timeline refresh here
  };

  const handleJobUpdate = (job: any) => {
    // Handle job updates from MediaLibrary
    // Track all active jobs to know when uploads are truly complete
    setActiveJobIds((prev) => {
      const newSet = new Set(prev);
      
      // Parse status if it's a JSON string
      const status = typeof job.status === 'string' && job.status.startsWith('"') 
        ? JSON.parse(job.status) 
        : job.status;
      
      if (status === 'Pending' || status === 'Running') {
        // Add job to active set
        newSet.add(job.id);
        return newSet;
      } else if (status === 'Completed' || status === 'Failed' || status === 'Cancelled') {
        // Remove job from active set
        newSet.delete(job.id);
        return newSet;
      }
      
      return newSet;
    });
  };

  // Update hasActiveUploadJobs based on whether there are any active jobs
  useEffect(() => {
    setHasActiveUploadJobs(activeJobIds.size > 0);
  }, [activeJobIds]);

  // Update hasActiveUploadJobs based on whether there are any active jobs
  useEffect(() => {
    setHasActiveUploadJobs(activeJobIds.size > 0);
  }, [activeJobIds]);

  const handleTimeUpdate = (time: number) => {
    setCurrentTime(time);
    // Update playhead position based on current playing clip
    if (currentPlayingClip && isTimelinePlaying) {
      const clipStartTicks = currentPlayingClip.timeline_start_ticks;
      const clipInSeconds = currentPlayingClip.in_ticks / 48000;
      // Calculate timeline position: clip start + (current video time - clip in point)
      const timelineTime = (clipStartTicks / 48000) + (time - clipInSeconds);
      const newPlayheadTicks = Math.round(timelineTime * 48000);
      setPlayheadPosition(newPlayheadTicks);
      playheadPositionRef.current = newPlayheadTicks; // Update ref immediately
      
      // Check if we've reached the end of the current clip
      const clipEndSeconds = currentPlayingClip.out_ticks / 48000;
      if (time >= clipEndSeconds) {
        // Find next clip
        const allClips: any[] = [];
        timeline?.tracks?.forEach((track: any) => {
          track.clips?.forEach((clip: any) => {
            allClips.push({ ...clip, track });
          });
        });
        allClips.sort((a, b) => a.timeline_start_ticks - b.timeline_start_ticks);
        
        const currentIndex = allClips.findIndex((clip) => clip.id === currentPlayingClip.id);
        if (currentIndex >= 0 && currentIndex < allClips.length - 1) {
          // Play next clip
          const nextClip = allClips[currentIndex + 1];
          setCurrentPlayingClip(nextClip);
          setSelectedClip(nextClip);
          setVideoSrc(`http://127.0.0.1:7777/api/projects/${projectId}/media/${nextClip.asset_id}/proxy`);
          setVideoStartTime(nextClip.in_ticks / 48000);
          setVideoEndTime(nextClip.out_ticks / 48000);
          const nextPlayheadTicks = nextClip.timeline_start_ticks;
          setPlayheadPosition(nextPlayheadTicks);
          playheadPositionRef.current = nextPlayheadTicks; // Update ref immediately
          setCurrentTime(nextClip.in_ticks / 48000);
        } else {
          // No more clips, stop playback
          setIsTimelinePlaying(false);
          setCurrentPlayingClip(null);
        }
      }
    }
  };

  // Handle video ended - transition to next clip
  const handleVideoEnded = () => {
    if (!timeline || !currentPlayingClip) return;

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
    } else {
      // No more clips, stop playback
      setIsTimelinePlaying(false);
      setCurrentPlayingClip(null);
    }
  };

  // Start timeline playback from playhead position
  const startTimelinePlayback = useCallback(() => {
    if (!timeline) return;

    // Use ref to get the most up-to-date playhead position (avoids stale closure values)
    const playheadTicks = playheadPositionRef.current || 0;

    // Find all clips sorted by timeline position
    const allClips: any[] = [];
    timeline.tracks?.forEach((track: any) => {
      track.clips?.forEach((clip: any) => {
        allClips.push({ ...clip, track });
      });
    });
    allClips.sort((a, b) => a.timeline_start_ticks - b.timeline_start_ticks);

    // Find the clip that contains the playhead position
    let clipToPlay = allClips.find((clip) => {
      const clipStart = clip.timeline_start_ticks;
      const clipDuration = clip.out_ticks - clip.in_ticks;
      const clipEnd = clipStart + clipDuration;
      return playheadTicks >= clipStart && playheadTicks < clipEnd;
    });

    // If playhead is not in any clip, find the first clip after playhead
    if (!clipToPlay) {
      clipToPlay = allClips.find(
        (clip) => clip.timeline_start_ticks >= playheadTicks
      );
    }

    // If no clip after playhead, use first clip
    if (!clipToPlay && allClips.length > 0) {
      clipToPlay = allClips[0];
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
        videoStartTime = (clipToPlay.in_ticks + playheadOffset) / 48000;
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
  }, [timeline, projectId]);

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

  // Handle clip insertion
  const handleClipInsert = async (assetId: number, positionTicks: number, trackId: number, intent?: 'primary' | 'layered', modifierKey?: boolean) => {
    console.log('handleClipInsert called:', { assetId, positionTicks, trackId });
    
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

  // Handle clear timeline
  const handleClearTimeline = async () => {
    if (!window.confirm('Are you sure you want to clear the entire timeline? This cannot be undone.')) {
      return;
    }

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
  }, [selectedClip, handleClearTimeline, isTimelinePlaying, startTimelinePlayback, stopTimelinePlayback]);

  // Handle drag start from library
  const handleDragStart = (asset: any) => {
    setDragAsset(asset);
  };

  const handleDragEnd = () => {
    setDragAsset(null);
  };

  return (
    <div
      style={{
        display: 'grid',
        gridTemplateRows: '40px 1fr 300px',
        gridTemplateColumns: isSidebarCollapsed ? '40px calc(300px + 80px) 1fr' : '120px 300px 1fr',
        height: '100vh',
        width: '100vw',
        backgroundColor: '#1a1a1a',
        color: '#e5e5e5',
      }}
    >
      {/* Toolbar - spans full width */}
      <div style={{ gridColumn: '1 / -1', gridRow: '1' }}>
        <Toolbar
          onGenerate={handleGenerate}
          onExport={handleExport}
          isGenerating={isGenerating}
          isExporting={isExporting}
          isUploading={hasActiveUploadJobs}
          currentProjectId={projectId}
          currentProjectName={currentProjectName}
          projects={projects}
          onProjectSelect={onProjectSelect}
          onCreateProject={onCreateProject}
        />
      </div>

      {/* Media Sidebar - leftmost */}
      <div style={{ gridColumn: '1', gridRow: '2', overflow: 'hidden' }}>
        <MediaSidebar 
          selectedTab={mediaTab} 
          onTabChange={setMediaTab}
          onCollapseChange={setIsSidebarCollapsed}
        />
      </div>

      {/* Media Library - left of center */}
      <div style={{ gridColumn: '2', gridRow: '2', overflow: 'hidden' }}>
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

      {/* Viewer - center */}
      <div style={{ gridColumn: '3', gridRow: '2', overflow: 'hidden', position: 'relative' }}>
        <Viewer
          videoSrc={videoSrc}
          startTime={videoStartTime}
          endTime={videoEndTime}
          currentTime={currentTime}
          onTimeUpdate={handleTimeUpdate}
          onEnded={isTimelinePlaying ? handleVideoEnded : undefined}
          isPlaying={isTimelinePlaying}
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

      {/* Timeline Toolbar - above timeline */}
      <div style={{ gridColumn: '1 / -1', gridRow: '3', overflow: 'hidden', display: 'flex', flexDirection: 'column' }}>
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
            onClipInsert={handleClipInsert}
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
            activeTool={activeTool}
            projectId={projectId}
          />
        </div>
      </div>
    </div>
  );
}

