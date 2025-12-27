import { useState, useEffect, useRef } from 'react';
import { useDaemon } from '../hooks/useDaemon';

interface ImportRawResponse {
  job_id: number;
}

interface JobResponse {
  id: number;
  status: string;
  progress: number;
}

interface GenerateResponse {
  job_id: number;
}

interface LibraryProps {
  onGenerateSuccess?: () => void;
}

export function Library({ onGenerateSuccess }: LibraryProps) {
  const [folderPath, setFolderPath] = useState('');
  const [currentJobId, setCurrentJobId] = useState<number | null>(null);
  const importRaw = useDaemon<ImportRawResponse>('/projects/1/import_raw', { method: 'POST' });
  const generate = useDaemon<GenerateResponse>('/projects/1/generate', { method: 'POST' });
  const jobStatus = useDaemon<JobResponse>(
    currentJobId ? `/jobs/${currentJobId}` : '',
    { method: 'GET' }
  );
  const pollingIntervalRef = useRef<NodeJS.Timeout | null>(null);

  const handleImport = async () => {
    if (!folderPath.trim()) {
      alert('Please enter a folder path');
      return;
    }

    const result = await importRaw.execute({ folder_path: folderPath });
    if (result && result.job_id) {
      setCurrentJobId(result.job_id);
      // Start polling job status
      jobStatus.execute();
    }
  };

  // Poll job status while running
  useEffect(() => {
    // Clear any existing polling
    if (pollingIntervalRef.current) {
      clearInterval(pollingIntervalRef.current);
      pollingIntervalRef.current = null;
    }

    if (!currentJobId) return;
    
    // Initial fetch
    jobStatus.execute();

    // Start polling
    pollingIntervalRef.current = setInterval(() => {
      jobStatus.execute();
    }, 500);

    return () => {
      if (pollingIntervalRef.current) {
        clearInterval(pollingIntervalRef.current);
        pollingIntervalRef.current = null;
      }
    };
  }, [currentJobId]);

  // Stop polling when job completes
  useEffect(() => {
    if (jobStatus.data) {
      const status = jobStatus.data.status;
      if (status === 'Completed' || status === 'Failed' || status === 'Cancelled') {
        if (pollingIntervalRef.current) {
          clearInterval(pollingIntervalRef.current);
          pollingIntervalRef.current = null;
        }
      }
    }
  }, [jobStatus.data]);

  const handleGenerate = async () => {
    const result = await generate.execute({});
    if (result) {
      // On success, navigate to project view
      if (onGenerateSuccess) {
        onGenerateSuccess();
      }
    }
  };

  return (
    <div style={{ padding: '2rem', maxWidth: '800px', margin: '0 auto' }}>
      <h1>Import Raw Footage</h1>
      
      <div style={{ marginBottom: '1rem' }}>
        <label style={{ display: 'block', marginBottom: '0.5rem', fontWeight: 500 }}>
          Folder Path:
        </label>
        <input
          type="text"
          value={folderPath}
          onChange={(e) => setFolderPath(e.target.value)}
          placeholder="/path/to/video/folder"
          style={{
            width: '100%',
            padding: '0.5rem',
            border: '1px solid #ccc',
            borderRadius: '0.25rem',
          }}
        />
      </div>

      <button
        onClick={handleImport}
        disabled={importRaw.loading}
        style={{
          padding: '0.5rem 1rem',
          backgroundColor: '#3b82f6',
          color: 'white',
          border: 'none',
          borderRadius: '0.25rem',
          cursor: importRaw.loading ? 'not-allowed' : 'pointer',
        }}
      >
        {importRaw.loading ? 'Importing...' : 'Import Raw Footage'}
      </button>

      {importRaw.error && (
        <div style={{ marginTop: '1rem', color: '#ef4444' }}>
          Error: {importRaw.error.message}
        </div>
      )}

      {currentJobId && (
        <div style={{ marginTop: '2rem', padding: '1rem', border: '1px solid #e5e7eb', borderRadius: '0.5rem' }}>
          <h2>Import Progress</h2>
          <div>Job ID: {currentJobId}</div>
          {jobStatus.loading && !jobStatus.data && (
            <div style={{ marginTop: '0.5rem', color: '#6b7280' }}>Checking status...</div>
          )}
          {jobStatus.data && (
            <>
              <div style={{ marginTop: '0.5rem' }}>
                Status: <strong>{jobStatus.data.status}</strong>
              </div>
              <div style={{ marginTop: '0.5rem' }}>
                Progress: <strong>{(jobStatus.data.progress * 100).toFixed(1)}%</strong>
              </div>
              <div style={{ marginTop: '0.5rem', width: '100%', height: '20px', backgroundColor: '#e5e7eb', borderRadius: '0.25rem', overflow: 'hidden' }}>
                <div
                  style={{
                    width: `${jobStatus.data.progress * 100}%`,
                    height: '100%',
                    backgroundColor: jobStatus.data.status === 'Failed' ? '#ef4444' : '#3b82f6',
                    transition: 'width 0.3s',
                  }}
                />
              </div>
              {jobStatus.data.status === 'Failed' && (
                <div style={{ marginTop: '0.5rem', color: '#ef4444' }}>
                  Job failed. Please check the daemon logs for details.
                </div>
              )}
            </>
          )}
          {jobStatus.error && (
            <div style={{ marginTop: '0.5rem', color: '#ef4444' }}>
              Error checking job status: {jobStatus.error.message}
            </div>
          )}
        </div>
      )}

      <div style={{ marginTop: '2rem', padding: '1rem', border: '1px solid #e5e7eb', borderRadius: '0.5rem' }}>
        <h2>Generate Rough Cut</h2>
        <p style={{ marginBottom: '1rem', color: '#6b7280' }}>
          Generate an AI-powered rough cut from imported footage
        </p>
        <button
          onClick={handleGenerate}
          disabled={generate.loading}
          style={{
            padding: '0.5rem 1rem',
            backgroundColor: '#10b981',
            color: 'white',
            border: 'none',
            borderRadius: '0.25rem',
            cursor: generate.loading ? 'not-allowed' : 'pointer',
          }}
        >
          {generate.loading ? 'Generating...' : 'Generate Rough Cut'}
        </button>
        {generate.error && (
          <div style={{ marginTop: '1rem', color: '#ef4444' }}>
            Error: {generate.error.message}
          </div>
        )}
      </div>
    </div>
  );
}
