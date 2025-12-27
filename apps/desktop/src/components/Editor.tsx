import { useState, useEffect } from 'react';
import { Toolbar } from './Toolbar';
import { MediaLibrary } from './MediaLibrary';
import { MediaSidebar } from './MediaSidebar';
import { Viewer } from './Viewer';
import { Timeline } from './Timeline';
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
  const [isGenerating, setIsGenerating] = useState(false);
  const [isExporting, setIsExporting] = useState(false);
  const [hasActiveUploadJobs, setHasActiveUploadJobs] = useState(false);
  const [activeJobIds, setActiveJobIds] = useState<Set<number>>(new Set());
  const [mediaTab, setMediaTab] = useState<'raw' | 'references'>('raw');
  const [isSidebarCollapsed, setIsSidebarCollapsed] = useState(false);

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
    // Clear active upload jobs when switching projects
    setActiveJobIds(new Set());
    setHasActiveUploadJobs(false);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [projectId]);

  useEffect(() => {
    if (timelineData.data?.timeline) {
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
    // Play clip from timeline
    setSelectedClip(clip);
    setVideoSrc(`http://127.0.0.1:7777/api/projects/${projectId}/media/${clip.asset_id}/proxy`);
    setVideoStartTime(clip.in_ticks / 48000);
    setVideoEndTime(clip.out_ticks / 48000);
    setPlayheadPosition(clip.timeline_start_ticks);
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
    // Update playhead position based on current time
    // This is simplified - would need to calculate based on clip positions
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
        />
      </div>

      {/* Viewer - center */}
      <div style={{ gridColumn: '3', gridRow: '2', overflow: 'hidden' }}>
        <Viewer
          videoSrc={videoSrc}
          startTime={videoStartTime}
          endTime={videoEndTime}
          currentTime={currentTime}
          onTimeUpdate={handleTimeUpdate}
        />
      </div>

      {/* Timeline - bottom, spans full width */}
      <div style={{ gridColumn: '1 / -1', gridRow: '3', overflow: 'hidden' }}>
        <Timeline
          timeline={timeline}
          selectedClip={selectedClip}
          onClipClick={handleTimelineClipClick}
          playheadPosition={playheadPosition}
        />
      </div>
    </div>
  );
}

