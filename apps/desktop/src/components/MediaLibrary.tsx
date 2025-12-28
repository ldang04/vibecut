import { useState, useEffect, useRef } from 'react';
import { useDaemon } from '../hooks/useDaemon';

interface ImportRawResponse {
  job_id: number;
  job_ids?: number[]; // For multiple file uploads
}

interface JobResponse {
  id: number;
  status: string;
  progress: number;
  job_type?: string;
}

interface MediaAsset {
  id: number;
  path: string;
  duration_ticks: number;
  width: number;
  height: number;
}

interface MediaLibraryProps {
  mode: 'raw' | 'references';
  onClipSelect: (clip: any) => void;
  onImportComplete?: () => void;
  onJobUpdate?: (job: JobResponse) => void;
  projectId?: number;
  onDragStart?: (asset: MediaAsset, event: React.MouseEvent) => void;
  onDragEnd?: () => void;
  externalDragAsset?: MediaAsset | null; // Added: external drag state from parent
}

// Helper to safely access electron API
const getElectronDialog = () => {
  if (typeof window !== 'undefined' && (window as any).electron && (window as any).electron.dialog) {
    return (window as any).electron.dialog;
  }
  return null;
};

// Thumbnail component that shows first frame of video
function ThumbnailVideo({ assetId, projectId, duration, formatDuration }: { assetId: number; projectId: number; duration: number; formatDuration: (ticks: number) => string }) {
  const [showPlaceholder, setShowPlaceholder] = useState(true);
  const [isLoading, setIsLoading] = useState(true);
  const [hasError, setHasError] = useState(false);
  const videoRef = useRef<HTMLVideoElement>(null);
  const retryTimeoutRef = useRef<NodeJS.Timeout | null>(null);

  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;

    // Reset state when assetId changes
    setShowPlaceholder(true);
    setIsLoading(true);
    setHasError(false);

    const handleLoadedData = () => {
      // Seek to first frame
      video.currentTime = 0.1;
    };

    const handleSeeked = () => {
      setShowPlaceholder(false);
      setIsLoading(false);
      setHasError(false);
      video.pause();
    };

    const handleError = (e: Event) => {
      setIsLoading(false);
      setHasError(true);
      setShowPlaceholder(true);
      
      // Retry after a delay (proxy might still be generating)
      if (retryTimeoutRef.current) {
        clearTimeout(retryTimeoutRef.current);
      }
      retryTimeoutRef.current = setTimeout(() => {
        if (video) {
          video.load(); // Reload the video element
        }
      }, 2000);
    };

    const handleCanPlay = () => {
      setIsLoading(false);
      setHasError(false);
    };

    video.addEventListener('loadeddata', handleLoadedData);
    video.addEventListener('seeked', handleSeeked);
    video.addEventListener('error', handleError);
    video.addEventListener('canplay', handleCanPlay);

    return () => {
      video.removeEventListener('loadeddata', handleLoadedData);
      video.removeEventListener('seeked', handleSeeked);
      video.removeEventListener('error', handleError);
      video.removeEventListener('canplay', handleCanPlay);
      if (retryTimeoutRef.current) {
        clearTimeout(retryTimeoutRef.current);
      }
    };
  }, [assetId]);

  return (
    <div
      style={{
        width: '100%',
        aspectRatio: '16/9',
        backgroundColor: '#2a2a2a',
        position: 'relative',
        overflow: 'hidden',
      }}
    >
      <video
        ref={videoRef}
        src={`http://127.0.0.1:7777/api/projects/${projectId}/media/${assetId}/proxy`}
        style={{
          width: '100%',
          height: '100%',
          objectFit: 'cover',
          display: showPlaceholder ? 'none' : 'block',
        }}
        preload="metadata"
        muted
        playsInline
      />
      {/* Fallback placeholder - shown when video hasn't loaded or is loading */}
      {showPlaceholder && (
        <div
          style={{
            position: 'absolute',
            top: 0,
            left: 0,
            width: '100%',
            height: '100%',
            display: 'flex',
            flexDirection: 'column',
            alignItems: 'center',
            justifyContent: 'center',
            backgroundColor: '#2a2a2a',
            pointerEvents: 'none',
          }}
        >
          {isLoading && !hasError && (
            <div style={{ fontSize: '12px', color: '#a0a0a0', marginTop: '8px' }}>
              Loading...
            </div>
          )}
          {hasError && (
            <div style={{ fontSize: '12px', color: '#a0a0a0', marginTop: '8px' }}>
              Generating proxy...
            </div>
          )}
          <div style={{ fontSize: '24px', color: '#505050' }}>▶</div>
        </div>
      )}
      <div
        style={{
          position: 'absolute',
          bottom: '4px',
          right: '4px',
          backgroundColor: 'rgba(0, 0, 0, 0.7)',
          color: '#e5e5e5',
          padding: '2px 6px',
          borderRadius: '4px',
          fontSize: '10px',
          pointerEvents: 'none',
        }}
      >
        {formatDuration(duration)}
      </div>
    </div>
  );
}

export function MediaLibrary({ mode, onClipSelect, onImportComplete, onJobUpdate, projectId = 1, onDragStart, onDragEnd, externalDragAsset }: MediaLibraryProps) {
  const [referenceAssets, setReferenceAssets] = useState<MediaAsset[]>([]); // Store reference assets separately
  const [referenceJobs, setReferenceJobs] = useState<Map<number, JobResponse>>(new Map());
  const [currentJobId, setCurrentJobId] = useState<number | null>(null);
  const [allJobIds, setAllJobIds] = useState<number[]>([]);
  const [activeJobs, setActiveJobs] = useState<Map<number, JobResponse>>(new Map());
  const [mediaAssets, setMediaAssets] = useState<MediaAsset[]>([]);
  const activeJobsRef = useRef<Map<number, JobResponse>>(new Map());
  const referenceJobsRef = useRef<Map<number, JobResponse>>(new Map());
  const importRaw = useDaemon<ImportRawResponse>(`/projects/${projectId}/import_raw`, { method: 'POST' });
  const importReference = useDaemon<ImportRawResponse>(`/projects/${projectId}/import_reference`, { method: 'POST' });
  const mediaAssetsData = useDaemon<MediaAsset[]>(`/projects/${projectId}/media`, { method: 'GET' });
  const referenceAssetsData = useDaemon<MediaAsset[]>(`/projects/${projectId}/references`, { method: 'GET' });
  const [hoveredAssetId, setHoveredAssetId] = useState<number | null>(null);
  const [isDragging, setIsDragging] = useState(false);
  const [dragAsset, setDragAsset] = useState<MediaAsset | null>(null);
  const [dragPosition, setDragPosition] = useState<{ x: number; y: number } | null>(null);
  const jobStatus = useDaemon<JobResponse>(
    currentJobId ? `/jobs/${currentJobId}` : '',
    { method: 'GET' }
  );
  const pollingIntervalRef = useRef<NodeJS.Timeout | null>(null);
  
  // Keep refs in sync with state
  useEffect(() => {
    activeJobsRef.current = activeJobs;
  }, [activeJobs]);
  
  useEffect(() => {
    referenceJobsRef.current = referenceJobs;
  }, [referenceJobs]);
  

  // Fetch media assets and clear state when projectId changes
  useEffect(() => {
    // Always clear existing media assets immediately when switching projects
    setMediaAssets([]);
    setReferenceAssets([]);
    setCurrentJobId(null);
    setAllJobIds([]);
    setActiveJobs(new Map());
    setReferenceJobs(new Map());
    
    if (projectId) {
      // Use setTimeout to ensure state is cleared before fetching
      // This prevents showing old data briefly
      const timeoutId = setTimeout(() => {
        mediaAssetsData.execute();
        referenceAssetsData.execute();
      }, 10);
      return () => clearTimeout(timeoutId);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [projectId]);

  // Update media assets when data changes (raw footage)
  useEffect(() => {
    // Only update if we have valid array data
    // This prevents showing stale data from previous projects
    if (mediaAssetsData.data && Array.isArray(mediaAssetsData.data)) {
      setMediaAssets(mediaAssetsData.data);
    } else if (mediaAssetsData.data === null && !mediaAssetsData.loading) {
      // Clear assets if data is explicitly null (empty project)
      setMediaAssets([]);
    } else if (!mediaAssetsData.loading && !mediaAssetsData.data) {
      // If loading is done but no data, ensure assets are cleared
      setMediaAssets([]);
    }
  }, [mediaAssetsData.data, mediaAssetsData.loading]);

  // Update reference assets when data changes (references from backend)
  useEffect(() => {
    // Only update if we have valid array data
    // This prevents showing stale data from previous projects
    if (referenceAssetsData.data && Array.isArray(referenceAssetsData.data)) {
      setReferenceAssets(referenceAssetsData.data);
    } else if (referenceAssetsData.data === null && !referenceAssetsData.loading) {
      // Clear assets if data is explicitly null (empty project)
      setReferenceAssets([]);
    } else if (!referenceAssetsData.loading && !referenceAssetsData.data) {
      // If loading is done but no data, ensure assets are cleared
      setReferenceAssets([]);
    }
  }, [referenceAssetsData.data, referenceAssetsData.loading]);

  const handleSelectReferenceFiles = async () => {
    const dialog = getElectronDialog();
    if (!dialog) {
      alert('File picker not available. Make sure you are running in Electron.');
      return;
    }

    try {
      const filePaths = await dialog.openFiles({
        multiSelect: true,
        title: 'Select Reference Video Files',
        filters: [
          {
            name: 'Video Files',
            extensions: ['mp4', 'mov', 'avi', 'mkv', 'webm', 'm4v', 'flv', 'wmv', 'mpg', 'mpeg', '3gp'],
          },
          { name: 'All Files', extensions: ['*'] },
        ],
      });

      if (filePaths && filePaths.length > 0) {
        // Send individual file paths to the API (same pattern as raw footage)
        console.log('Uploading reference files:', filePaths);
        const result = await importReference.execute({ 
          file_paths: filePaths,
        });
        if (result && result.job_id) {
          console.log('Reference upload initiated, job IDs:', result.job_ids || [result.job_id]);
          // If multiple jobs were created, track all of them
          if (result.job_ids && result.job_ids.length > 1) {
            // Store all job IDs
            setAllJobIds(result.job_ids);
            // Initialize all jobs in referenceJobs map
            const initialJobs = new Map<number, JobResponse>();
            result.job_ids.forEach(jobId => {
              const job = { id: jobId, status: 'Pending', progress: 0 };
              initialJobs.set(jobId, job);
              if (onJobUpdate) {
                onJobUpdate(job);
              }
            });
            setReferenceJobs(initialJobs);
            // Set the first job as current for polling
            setCurrentJobId(result.job_ids[0]);
            // Set a fallback refresh timer in case job completion detection fails
            const fileCount = result.job_ids.length;
            setTimeout(() => {
              console.log('Fallback: Refreshing reference assets after upload (fallback timer)');
              referenceAssetsData.execute().then(result => {
                if (result && Array.isArray(result)) {
                  setReferenceAssets([...result]);
                }
              });
            }, 3000 * fileCount); // Wait 3 seconds per file
          } else {
            // Single job (backward compatibility or folder mode)
            const jobId = result.job_id;
            setAllJobIds([jobId]);
            setCurrentJobId(jobId);
            const job = { id: jobId, status: 'Pending', progress: 0 };
            setReferenceJobs(new Map([[jobId, job]]));
            if (onJobUpdate) {
              onJobUpdate(job);
            }
            // Set a fallback refresh timer for single file
            setTimeout(() => {
              console.log('Fallback: Refreshing reference assets after upload (fallback timer)');
              referenceAssetsData.execute().then(result => {
                if (result && Array.isArray(result)) {
                  setReferenceAssets([...result]);
                }
              });
            }, 3000);
          }
          jobStatus.execute();
        } else if (importReference.error) {
          console.error('Reference upload failed:', importReference.error);
        }
      }
    } catch (error) {
      console.error('Error opening file picker:', error);
      alert('Failed to open file picker. Please check the console for details.');
    }
  };

  const handleImportRaw = async () => {
    const dialog = getElectronDialog();
    if (!dialog) {
      alert('File picker not available. Make sure you are running in Electron.');
      return;
    }

    try {
      const filePaths = await dialog.openFiles({
        multiSelect: true,
        title: 'Select Video Files',
        filters: [
          {
            name: 'Video Files',
            extensions: ['mp4', 'mov', 'avi', 'mkv', 'webm', 'm4v', 'flv', 'wmv', 'mpg', 'mpeg', '3gp'],
          },
          { name: 'All Files', extensions: ['*'] },
        ],
      });

      if (filePaths && filePaths.length > 0) {
        // Send individual file paths to the API
        console.log('Uploading files:', filePaths);
        const result = await importRaw.execute({ file_paths: filePaths });
        if (result && result.job_id) {
          console.log('Upload initiated, job IDs:', result.job_ids || [result.job_id]);
          // If multiple jobs were created, track all of them
          if (result.job_ids && result.job_ids.length > 1) {
            // Store all job IDs
            setAllJobIds(result.job_ids);
            // Initialize all jobs in activeJobs map
            const initialJobs = new Map<number, JobResponse>();
            result.job_ids.forEach(jobId => {
              const job = { id: jobId, status: 'Pending', progress: 0 };
              initialJobs.set(jobId, job);
              if (onJobUpdate) {
                onJobUpdate(job);
              }
            });
            setActiveJobs(initialJobs);
            // Set the first job as current for polling
            setCurrentJobId(result.job_ids[0]);
            // Set a fallback refresh timer in case job completion detection fails
            const fileCount = result.job_ids.length;
            setTimeout(() => {
              console.log('Fallback: Refreshing media assets after upload (fallback timer)');
              mediaAssetsData.execute().then(result => {
                if (result && Array.isArray(result)) {
                  setMediaAssets([...result]);
                }
              });
            }, 3000 * fileCount); // Wait 3 seconds per file
          } else {
            // Single job (backward compatibility or folder mode)
            const jobId = result.job_id;
            setAllJobIds([jobId]);
            setCurrentJobId(jobId);
            const job = { id: jobId, status: 'Pending', progress: 0 };
            setActiveJobs(new Map([[jobId, job]]));
            if (onJobUpdate) {
              onJobUpdate(job);
            }
            // Set a fallback refresh timer for single file
            setTimeout(() => {
              console.log('Fallback: Refreshing media assets after upload (fallback timer)');
              mediaAssetsData.execute().then(result => {
                if (result && Array.isArray(result)) {
                  setMediaAssets([...result]);
                }
              });
            }, 3000);
          }
          jobStatus.execute();
        } else if (importRaw.error) {
          console.error('Upload failed:', importRaw.error);
        }
      }
    } catch (error) {
      console.error('Error opening file picker:', error);
      alert('Failed to open file picker. Please check the console for details.');
    }
  };

  // Poll job status while running
  useEffect(() => {
    if (pollingIntervalRef.current) {
      clearInterval(pollingIntervalRef.current);
      pollingIntervalRef.current = null;
    }

    if (!currentJobId) return;
    
    jobStatus.execute();

    pollingIntervalRef.current = setInterval(() => {
      jobStatus.execute();
    }, 500);

    return () => {
      if (pollingIntervalRef.current) {
        clearInterval(pollingIntervalRef.current);
        pollingIntervalRef.current = null;
      }
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [currentJobId]);

  // Update active jobs
  useEffect(() => {
    console.log('Job status effect triggered. jobStatus.data:', jobStatus.data, 'currentJobId:', currentJobId);
    if (jobStatus.data && currentJobId) {
      const jobData = jobStatus.data;
      console.log('Processing job data:', jobData);
      // Status comes back as a JSON string (with quotes), so we need to parse it or compare against the quoted version
      const status = typeof jobData.status === 'string' && jobData.status.startsWith('"') 
        ? JSON.parse(jobData.status) 
        : jobData.status;
      const isCompleted = status === 'Completed' || status === 'Failed' || status === 'Cancelled';
      const completedJobId = currentJobId;
      
      if (isCompleted) {
        console.log('Job completed:', completedJobId, 'Status:', jobData.status);
        // Check if this is a reference job BEFORE removing it from the map
        const isReferenceJob = referenceJobsRef.current.has(completedJobId);
        
        // Compute remaining jobs BEFORE updating state (using refs for current state)
        const currentActiveJobs = activeJobsRef.current;
        const currentReferenceJobs = referenceJobsRef.current;
        const allJobsMap = new Map([...currentActiveJobs, ...currentReferenceJobs]);
        const remainingJobs = allJobIds.filter(jobId => {
          if (jobId === completedJobId) {
            return false; // This job just completed
          }
          const job = allJobsMap.get(jobId);
          if (!job) return false;
          // Parse status if it's a JSON string
          const jobStatus = typeof job.status === 'string' && job.status.startsWith('"') 
            ? JSON.parse(job.status) 
            : job.status;
          return jobStatus === 'Pending' || jobStatus === 'Running';
        });
        
        console.log('Remaining jobs:', remainingJobs.length, 'All job IDs:', allJobIds.length);
        
        // Remove completed job from appropriate map (activeJobs or referenceJobs)
        if (isReferenceJob) {
          setReferenceJobs((prev) => {
            const updated = new Map(prev);
            updated.delete(completedJobId);
            return updated;
          });
        } else {
          setActiveJobs((prev) => {
            const updated = new Map(prev);
            updated.delete(completedJobId);
            return updated;
          });
        }
        
        // Stop polling for this job
        if (pollingIntervalRef.current) {
          clearInterval(pollingIntervalRef.current);
          pollingIntervalRef.current = null;
        }
        
        // Handle next job or completion - OUTSIDE of setState callback
        if (remainingJobs.length > 0) {
          // Poll the next job
          console.log('Moving to next job:', remainingJobs[0]);
          setCurrentJobId(remainingJobs[0]);
        } else {
          // No more active jobs - all uploads complete
          console.log('All jobs complete! Status:', status);
          setCurrentJobId(null);
          
          if (status === 'Completed') {
            console.log('Job was completed successfully, triggering media assets refresh...');
            // isReferenceJob was already determined above, before removing from map
            
            // Refresh appropriate assets based on job type
            const refreshAssets = async () => {
              try {
                if (isReferenceJob) {
                  console.log('Refreshing reference assets after upload completion...');
                  const result = await referenceAssetsData.execute();
                  console.log('Reference assets refresh result:', result);
                  if (result && Array.isArray(result)) {
                    console.log(`Updating referenceAssets with ${result.length} assets`);
                    setReferenceAssets([...result]);
                  }
                } else {
                  console.log('Refreshing media assets after upload completion...');
                  const result = await mediaAssetsData.execute();
                  console.log('Media assets refresh result:', result);
                  if (result && Array.isArray(result)) {
                    console.log(`Updating mediaAssets with ${result.length} assets`);
                    setMediaAssets([...result]);
                  }
                }
              } catch (error) {
                console.error('Error refreshing assets:', error);
              }
            };
            
            // Try immediately (no delay) - this mimics manual refresh behavior
            refreshAssets();
            
            // Retry aggressively to catch the update as soon as possible
            setTimeout(() => refreshAssets(), 100);
            setTimeout(() => refreshAssets(), 300);
            setTimeout(() => refreshAssets(), 600);
            setTimeout(() => refreshAssets(), 1000);
            setTimeout(() => refreshAssets(), 2000);
            
            setAllJobIds([]);
            
            if (onImportComplete) {
              onImportComplete();
            }
          } else {
            console.log('Job did not complete successfully, status:', jobData.status);
          }
        }
      } else {
        // Update job status in activeJobs or referenceJobs depending on which map contains the job
        const isReferenceJob = referenceJobsRef.current.has(completedJobId);
        console.log('Updating job status. Job ID:', completedJobId, 'Is reference job:', isReferenceJob, 'Status:', status);
        if (isReferenceJob) {
          setReferenceJobs((prev) => {
            const updated = new Map(prev);
            updated.set(completedJobId, jobData);
            console.log('Updated referenceJobs map with job:', completedJobId, 'Status:', jobData.status);
            return updated;
          });
        } else {
          setActiveJobs((prev) => {
            const updated = new Map(prev);
            updated.set(completedJobId, jobData);
            console.log('Updated activeJobs map with job:', completedJobId, 'Status:', jobData.status);
            return updated;
          });
        }
      }
      
      if (onJobUpdate) {
        onJobUpdate(jobData);
      }
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [jobStatus.data, currentJobId, allJobIds]);


  const activeJobsArray = Array.from(activeJobs.values()).filter(
    job => {
      // Parse status if it's a JSON string (with quotes)
      const jobStatus = typeof job.status === 'string' && job.status.startsWith('"') 
        ? JSON.parse(job.status) 
        : job.status;
      return jobStatus === 'Pending' || jobStatus === 'Running';
    }
  );

  const activeReferenceJobsArray = Array.from(referenceJobs.values()).filter(
    job => {
      // Parse status if it's a JSON string (with quotes)
      const jobStatus = typeof job.status === 'string' && job.status.startsWith('"') 
        ? JSON.parse(job.status) 
        : job.status;
      return jobStatus === 'Pending' || jobStatus === 'Running';
    }
  );

  const hasRawFootage = mediaAssets.length > 0 || activeJobsArray.length > 0;
  const hasReferenceFootage = referenceAssets.length > 0 || activeReferenceJobsArray.length > 0;
  const hasFootage = mode === 'raw' ? hasRawFootage : hasReferenceFootage;
  const displayAssets = mode === 'raw' ? mediaAssets : [];
  const displayReferenceAssets = mode === 'references' ? referenceAssets : []; // Display reference assets (with thumbnails)

  const formatDuration = (ticks: number): string => {
    const seconds = Math.floor(ticks / 48000);
    const minutes = Math.floor(seconds / 60);
    const secs = seconds % 60;
    return `${minutes}:${secs.toString().padStart(2, '0')}`;
  };

  const getFileName = (path: string): string => {
    const parts = path.split(/[/\\]/);
    return parts[parts.length - 1];
  };

  const handleDeleteAsset = async (assetId: number, e: React.MouseEvent) => {
    e.stopPropagation(); // Prevent triggering the onClick on the parent div
    
    if (!confirm('Are you sure you want to delete this footage?')) {
      return;
    }

    try {
      const response = await fetch(
        `http://127.0.0.1:7777/api/projects/${projectId}/media/${assetId}`,
        { method: 'DELETE' }
      );

      if (response.ok) {
        // Remove from local state immediately
        if (mode === 'raw') {
          setMediaAssets((prev) => prev.filter((asset) => asset.id !== assetId));
        } else {
          setReferenceAssets((prev) => prev.filter((asset) => asset.id !== assetId));
        }
        // Refresh media assets from server
        mediaAssetsData.execute();
      } else {
        console.error('Failed to delete asset:', response.statusText);
        alert('Failed to delete footage');
      }
    } catch (error) {
      console.error('Error deleting asset:', error);
      alert('Error deleting footage');
    }
  };

  const handleImport = mode === 'raw' ? handleImportRaw : handleSelectReferenceFiles;

  // Clear drag state when external drag asset is cleared (drop happened)
  // Use a small delay to ensure the drop is processed first
  useEffect(() => {
    if (!externalDragAsset && (isDragging || dragAsset)) {
      // External drag was cleared, so clear our local drag state
      // Use setTimeout to ensure drop is processed first
      const timeoutId = setTimeout(() => {
        setIsDragging(false);
        setDragAsset(null);
        setDragPosition(null);
        // Reset cursor
        document.body.style.cursor = '';
      }, 50); // Small delay to allow drop to process
      
      return () => clearTimeout(timeoutId);
    }
  }, [externalDragAsset, isDragging, dragAsset]);

  // Handle global mouse events for drag
  useEffect(() => {
    const handleMouseUp = () => {
      if (isDragging) {
        // Clear immediately - Timeline's mouseup handler will process the drop first
        // Use a small delay to ensure Timeline processes the drop
        setTimeout(() => {
          setIsDragging(false);
          setDragAsset(null);
          setDragPosition(null);
          if (onDragEnd) {
            onDragEnd();
          }
        }, 0);
      }
    };

    const handleMouseMove = (e: MouseEvent) => {
      if (isDragging) {
        // Update cursor style globally while dragging
        document.body.style.cursor = 'grabbing';
        // Update drag position for preview
        setDragPosition({ x: e.clientX, y: e.clientY });
      }
    };

    if (isDragging) {
      document.addEventListener('mouseup', handleMouseUp);
      document.addEventListener('mousemove', handleMouseMove);
      return () => {
        document.removeEventListener('mouseup', handleMouseUp);
        document.removeEventListener('mousemove', handleMouseMove);
        document.body.style.cursor = '';
      };
    }
  }, [isDragging, onDragEnd]);

  return (
    <div
      style={{
        backgroundColor: '#252525',
        borderRight: '1px solid #404040',
        width: '100%',
        height: '100%',
        display: 'flex',
        flexDirection: 'column',
        overflow: 'hidden',
      }}
    >
      {/* Content Area */}
      <div style={{ display: 'flex', flexDirection: 'column', height: '100%', flex: 1, overflow: 'hidden' }}>
        {/* Import Button - always visible, large when no footage */}
        <div style={{ padding: hasFootage ? '1rem 1rem 0.75rem 1rem' : '2rem 1rem', flexShrink: 0 }}>
          <button
            onClick={handleImport}
            disabled={(importRaw.loading && mode === 'raw') || (importReference.loading && mode === 'references')}
            style={{
              width: '100%',
              padding: hasFootage ? '0.5rem' : '1rem',
              backgroundColor: ((importRaw.loading && mode === 'raw') || (importReference.loading && mode === 'references')) ? '#505050' : '#2a2a2a',
              color: '#e5e5e5',
              border: '1px solid #404040',
              borderRadius: '4px',
              fontSize: hasFootage ? '12px' : '14px',
              fontWeight: hasFootage ? 400 : 500,
              cursor: ((importRaw.loading && mode === 'raw') || (importReference.loading && mode === 'references')) ? 'not-allowed' : 'pointer',
              transition: 'background-color 0.2s',
              outline: 'none',
            }}
            onMouseDown={(e) => e.preventDefault()}
          >
            {((importRaw.loading && mode === 'raw') || (importReference.loading && mode === 'references')) ? 'Importing...' : 'Import Video Clips'}
          </button>

          {importRaw.error && mode === 'raw' && (
            <div style={{ marginTop: '0.5rem', color: '#ef4444', fontSize: '12px' }}>
              Error: {importRaw.error.message}
              {importRaw.error.message.includes('Failed to fetch') && (
                <div style={{ marginTop: '0.25rem', fontSize: '11px', color: '#a0a0a0' }}>
                  Make sure the daemon server is running on port 7777
                </div>
              )}
            </div>
          )}

          {importReference.error && mode === 'references' && (
            <div style={{ marginTop: '0.5rem', color: '#ef4444', fontSize: '12px' }}>
              Error: {importReference.error.message}
              {importReference.error.message.includes('Failed to fetch') && (
                <div style={{ marginTop: '0.25rem', fontSize: '11px', color: '#a0a0a0' }}>
                  Make sure the daemon server is running on port 7777
                </div>
              )}
              {importReference.error.message.includes('404') && (
                <div style={{ marginTop: '0.25rem', fontSize: '11px', color: '#a0a0a0' }}>
                  The /import_reference endpoint may not be implemented yet
                </div>
              )}
            </div>
          )}

        </div>

        {/* Scrollable media assets grid */}
        {hasFootage && (
          <div style={{ flex: 1, overflowY: 'auto', padding: '0 0.75rem 0.75rem 0.75rem' }}>
            {mode === 'raw' && displayAssets.length > 0 ? (
              <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '0.75rem' }}>
                {displayAssets.map((asset) => (
                  <div
                    key={asset.id}
                    onClick={() => {
                      if (!isDragging) {
                        onClipSelect(asset);
                      }
                    }}
                    onMouseDown={(e) => {
                      if (e.button === 0) { // Left mouse button
                        e.preventDefault(); // Prevent text selection
                        setIsDragging(true);
                        setDragAsset(asset);
                        if (onDragStart) {
                          onDragStart(asset, e);
                        }
                      }
                    }}
                    onMouseEnter={() => setHoveredAssetId(asset.id)}
                    onMouseLeave={() => setHoveredAssetId(null)}
                    style={{
                      borderRadius: '8px',
                      overflow: 'hidden',
                      backgroundColor: '#1e1e1e',
                      border: '1px solid #404040',
                      cursor: isDragging && dragAsset?.id === asset.id ? 'grabbing' : 'grab',
                      transition: 'border-color 0.2s',
                      outline: 'none',
                      position: 'relative',
                      userSelect: 'none',
                    }}
                  >
                    {/* Delete button - appears on hover */}
                    {hoveredAssetId === asset.id && (
                      <button
                        onClick={(e) => handleDeleteAsset(asset.id, e)}
                        style={{
                          position: 'absolute',
                          top: '-1px',
                          left: '-1px',
                          width: '20px',
                          height: '20px',
                          borderRadius: '50%',
                          backgroundColor: 'rgba(0, 0, 0, 0.8)',
                          border: '1px solid rgba(255, 255, 255, 0.3)',
                          color: '#e5e5e5',
                          display: 'flex',
                          alignItems: 'center',
                          justifyContent: 'center',
                          cursor: 'pointer',
                          fontSize: '12px',
                          lineHeight: '1',
                          zIndex: 10,
                          transition: 'background-color 0.2s, border-color 0.2s',
                          padding: 0,
                          margin: 0,
                        }}
                        onMouseEnter={(e) => {
                          e.currentTarget.style.backgroundColor = 'rgba(239, 68, 68, 0.9)';
                          e.currentTarget.style.borderColor = '#ef4444';
                        }}
                        onMouseLeave={(e) => {
                          e.currentTarget.style.backgroundColor = 'rgba(0, 0, 0, 0.8)';
                          e.currentTarget.style.borderColor = 'rgba(255, 255, 255, 0.3)';
                        }}
                        title="Delete footage"
                      >
                        ×
                      </button>
                    )}
                    {/* Thumbnail - using video element to show first frame */}
                    <ThumbnailVideo 
                      assetId={asset.id} 
                      projectId={projectId}
                      duration={asset.duration_ticks}
                      formatDuration={formatDuration}
                    />
                    <div style={{ padding: '0.5rem' }}>
                      <div
                        style={{
                          fontSize: '11px',
                          color: '#e5e5e5',
                          overflow: 'hidden',
                          textOverflow: 'ellipsis',
                          whiteSpace: 'nowrap',
                        }}
                        title={getFileName(asset.path)}
                      >
                        {getFileName(asset.path)}
                      </div>
                      <div style={{ fontSize: '10px', color: '#a0a0a0', marginTop: '2px' }}>
                        {asset.width}×{asset.height}
                      </div>
                    </div>
                  </div>
                ))}
              </div>
            ) : mode === 'references' && displayReferenceAssets.length > 0 ? (
              <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '0.75rem' }}>
                {displayReferenceAssets.map((asset) => (
                  <div
                    key={asset.id}
                    onClick={() => {
                      if (!isDragging) {
                        onClipSelect(asset);
                      }
                    }}
                    onMouseDown={(e) => {
                      if (e.button === 0) { // Left mouse button
                        e.preventDefault(); // Prevent text selection
                        setIsDragging(true);
                        setDragAsset(asset);
                        if (onDragStart) {
                          onDragStart(asset, e);
                        }
                      }
                    }}
                    onMouseEnter={() => setHoveredAssetId(asset.id)}
                    onMouseLeave={() => setHoveredAssetId(null)}
                    style={{
                      borderRadius: '8px',
                      overflow: 'hidden',
                      backgroundColor: '#1e1e1e',
                      border: '1px solid #404040',
                      cursor: isDragging && dragAsset?.id === asset.id ? 'grabbing' : 'grab',
                      transition: 'border-color 0.2s',
                      outline: 'none',
                      position: 'relative',
                      userSelect: 'none',
                    }}
                  >
                    {/* Delete button - appears on hover */}
                    {hoveredAssetId === asset.id && (
                      <button
                        onClick={(e) => handleDeleteAsset(asset.id, e)}
                        style={{
                          position: 'absolute',
                          top: '4px',
                          left: '4px',
                          width: '28px',
                          height: '28px',
                          borderRadius: '50%',
                          backgroundColor: 'rgba(0, 0, 0, 0.8)',
                          border: '1px solid rgba(255, 255, 255, 0.3)',
                          color: '#e5e5e5',
                          display: 'flex',
                          alignItems: 'center',
                          justifyContent: 'center',
                          cursor: 'pointer',
                          fontSize: '16px',
                          lineHeight: '1',
                          zIndex: 10,
                          transition: 'background-color 0.2s, border-color 0.2s',
                          padding: 0,
                          margin: 0,
                        }}
                        onMouseEnter={(e) => {
                          e.currentTarget.style.backgroundColor = 'rgba(239, 68, 68, 0.9)';
                          e.currentTarget.style.borderColor = '#ef4444';
                        }}
                        onMouseLeave={(e) => {
                          e.currentTarget.style.backgroundColor = 'rgba(0, 0, 0, 0.8)';
                          e.currentTarget.style.borderColor = 'rgba(255, 255, 255, 0.3)';
                        }}
                        title="Delete footage"
                      >
                        ×
                      </button>
                    )}
                    {/* Thumbnail - using video element to show first frame */}
                    <ThumbnailVideo
                      assetId={asset.id}
                      projectId={projectId}
                      duration={asset.duration_ticks}
                      formatDuration={formatDuration}
                    />
                    <div style={{ padding: '0.5rem' }}>
                      <div
                        style={{
                          fontSize: '11px',
                          color: '#e5e5e5',
                          overflow: 'hidden',
                          textOverflow: 'ellipsis',
                          whiteSpace: 'nowrap',
                        }}
                        title={getFileName(asset.path)}
                      >
                        {getFileName(asset.path)}
                      </div>
                      <div style={{ fontSize: '10px', color: '#a0a0a0', marginTop: '2px' }}>
                        {asset.width}×{asset.height}
                      </div>
                    </div>
                  </div>
                ))}
              </div>
            ) : null}
          </div>
        )}
      </div>

      {/* Drag Preview - follows cursor */}
      {isDragging && dragAsset && dragPosition && (
        <div
          style={{
            position: 'fixed',
            left: dragPosition.x - 80,
            top: dragPosition.y - 60,
            width: '160px',
            height: '90px',
            backgroundColor: '#1e1e1e',
            border: '2px solid #3b82f6',
            borderRadius: '8px',
            pointerEvents: 'none',
            zIndex: 10000,
            boxShadow: '0 4px 12px rgba(0, 0, 0, 0.5)',
            opacity: 0.9,
            transform: 'scale(0.8)',
          }}
        >
          <div
            style={{
              width: '100%',
              height: '100%',
              borderRadius: '6px',
              overflow: 'hidden',
              backgroundColor: '#2a2a2a',
            }}
          >
            <video
              src={`http://127.0.0.1:7777/api/projects/${projectId}/media/${dragAsset.id}/proxy`}
              style={{
                width: '100%',
                height: '100%',
                objectFit: 'cover',
              }}
              muted
              playsInline
              preload="metadata"
              onLoadedMetadata={(e) => {
                const video = e.target as HTMLVideoElement;
                video.currentTime = 0.1;
              }}
            />
          </div>
        </div>
      )}
    </div>
  );
}
