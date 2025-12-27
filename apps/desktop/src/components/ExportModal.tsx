import { useState } from 'react';
import { useDaemon } from '../hooks/useDaemon';

interface ExportResponse {
  job_id: number;
}

export function ExportModal({ projectId, onClose }: { projectId: number; onClose: () => void }) {
  const [outPath, setOutPath] = useState('');
  const [preset, setPreset] = useState('mp4');
  const exportApi = useDaemon<ExportResponse>(
    `/projects/${projectId}/export`,
    { method: 'POST' }
  );

  const handleExport = async () => {
    if (!outPath.trim()) {
      alert('Please enter an output path');
      return;
    }

    await exportApi.execute({
      preset,
      out_path: outPath,
    });

    if (exportApi.data) {
      alert(`Export started! Job ID: ${exportApi.data.job_id}`);
      onClose();
    }
  };

  return (
    <div
      style={{
        position: 'fixed',
        top: 0,
        left: 0,
        right: 0,
        bottom: 0,
        backgroundColor: 'rgba(0, 0, 0, 0.5)',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        zIndex: 1000,
      }}
      onClick={onClose}
    >
      <div
        style={{
          backgroundColor: 'white',
          padding: '2rem',
          borderRadius: '0.5rem',
          maxWidth: '500px',
          width: '90%',
        }}
        onClick={(e) => e.stopPropagation()}
      >
        <h2 style={{ marginTop: 0 }}>Export Project</h2>

        <div style={{ marginBottom: '1rem' }}>
          <label style={{ display: 'block', marginBottom: '0.5rem', fontWeight: 500 }}>
            Output Path:
          </label>
          <input
            type="text"
            value={outPath}
            onChange={(e) => setOutPath(e.target.value)}
            placeholder="/path/to/output.mp4"
            style={{
              width: '100%',
              padding: '0.5rem',
              border: '1px solid #ccc',
              borderRadius: '0.25rem',
            }}
          />
        </div>

        <div style={{ marginBottom: '1rem' }}>
          <label style={{ display: 'block', marginBottom: '0.5rem', fontWeight: 500 }}>
            Preset:
          </label>
          <select
            value={preset}
            onChange={(e) => setPreset(e.target.value)}
            style={{
              width: '100%',
              padding: '0.5rem',
              border: '1px solid #ccc',
              borderRadius: '0.25rem',
            }}
          >
            <option value="mp4">MP4</option>
            <option value="mov">MOV</option>
          </select>
        </div>

        <div style={{ display: 'flex', gap: '0.5rem', justifyContent: 'flex-end' }}>
          <button
            onClick={onClose}
            style={{
              padding: '0.5rem 1rem',
              backgroundColor: '#6b7280',
              color: 'white',
              border: 'none',
              borderRadius: '0.25rem',
              cursor: 'pointer',
            }}
          >
            Cancel
          </button>
          <button
            onClick={handleExport}
            disabled={exportApi.loading}
            style={{
              padding: '0.5rem 1rem',
              backgroundColor: '#3b82f6',
              color: 'white',
              border: 'none',
              borderRadius: '0.25rem',
              cursor: exportApi.loading ? 'not-allowed' : 'pointer',
            }}
          >
            {exportApi.loading ? 'Exporting...' : 'Export'}
          </button>
        </div>

        {exportApi.error && (
          <div style={{ marginTop: '1rem', color: '#ef4444' }}>
            Error: {exportApi.error.message}
          </div>
        )}
      </div>
    </div>
  );
}
