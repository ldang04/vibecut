import { useState } from 'react';
import { useDaemon } from '../hooks/useDaemon';

interface CreateProjectResponse {
  id: number;
}

interface CreateProjectModalProps {
  onClose: () => void;
  onCreated: (projectId: number, projectName: string) => void;
}

export function CreateProjectModal({ onClose, onCreated }: CreateProjectModalProps) {
  const [projectName, setProjectName] = useState('');
  const [error, setError] = useState<string | null>(null);
  const createProject = useDaemon<CreateProjectResponse>('/projects', { method: 'POST' });

  const handleCreate = async () => {
    if (!projectName.trim()) {
      setError('Project name cannot be empty');
      return;
    }

    setError(null);
    const result = await createProject.execute({
      name: projectName.trim(),
      cache_dir: '.cache',
    });

    if (result && result.id) {
      onCreated(result.id, projectName.trim());
      onClose();
    } else {
      setError('Failed to create project');
    }
  };

  const handleKeyPress = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter') {
      handleCreate();
    } else if (e.key === 'Escape') {
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
        backgroundColor: 'rgba(0, 0, 0, 0.7)',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        zIndex: 1000,
      }}
      onClick={(e) => {
        if (e.target === e.currentTarget) {
          onClose();
        }
      }}
    >
      <div
        style={{
          backgroundColor: '#2a2a2a',
          border: '1px solid #404040',
          borderRadius: '8px',
          padding: '1.5rem',
          minWidth: '400px',
          maxWidth: '500px',
        }}
        onClick={(e) => e.stopPropagation()}
      >
        <h2 style={{ marginTop: 0, marginBottom: '1rem', color: '#e5e5e5' }}>
          Create New Project
        </h2>

        <div style={{ marginBottom: '1rem' }}>
          <label
            style={{
              display: 'block',
              marginBottom: '0.5rem',
              color: '#e5e5e5',
              fontSize: '0.875rem',
            }}
          >
            Project Name
          </label>
          <input
            type="text"
            value={projectName}
            onChange={(e) => {
              setProjectName(e.target.value);
              setError(null);
            }}
            onKeyDown={handleKeyPress}
            placeholder="Enter project name"
            autoFocus
            style={{
              width: '100%',
              padding: '0.5rem',
              backgroundColor: '#1a1a1a',
              border: '1px solid #404040',
              borderRadius: '4px',
              color: '#e5e5e5',
              fontSize: '0.875rem',
              boxSizing: 'border-box',
            }}
          />
        </div>

        {error && (
          <div
            style={{
              marginBottom: '1rem',
              padding: '0.5rem',
              backgroundColor: 'rgba(239, 68, 68, 0.1)',
              border: '1px solid #ef4444',
              borderRadius: '4px',
              color: '#ef4444',
              fontSize: '0.875rem',
            }}
          >
            {error}
          </div>
        )}

        {createProject.error && (
          <div
            style={{
              marginBottom: '1rem',
              padding: '0.5rem',
              backgroundColor: 'rgba(239, 68, 68, 0.1)',
              border: '1px solid #ef4444',
              borderRadius: '4px',
              color: '#ef4444',
              fontSize: '0.875rem',
            }}
          >
            {createProject.error.message}
          </div>
        )}

        <div
          style={{
            display: 'flex',
            gap: '0.5rem',
            justifyContent: 'flex-end',
          }}
        >
          <button
            onClick={onClose}
            disabled={createProject.loading}
            style={{
              padding: '0.5rem 1rem',
              backgroundColor: '#2a2a2a',
              color: '#e5e5e5',
              border: '1px solid #404040',
              borderRadius: '4px',
              cursor: createProject.loading ? 'not-allowed' : 'pointer',
              fontSize: '0.875rem',
            }}
          >
            Cancel
          </button>
          <button
            onClick={handleCreate}
            disabled={createProject.loading || !projectName.trim()}
            style={{
              padding: '0.5rem 1rem',
              backgroundColor: createProject.loading || !projectName.trim() ? '#505050' : '#10b981',
              color: 'white',
              border: 'none',
              borderRadius: '4px',
              cursor: createProject.loading || !projectName.trim() ? 'not-allowed' : 'pointer',
              fontSize: '0.875rem',
            }}
          >
            {createProject.loading ? 'Creating...' : 'Create'}
          </button>
        </div>
      </div>
    </div>
  );
}

