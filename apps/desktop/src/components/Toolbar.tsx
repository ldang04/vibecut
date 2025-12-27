import { useState, useRef, useEffect } from 'react';
import { BackendStatus } from './BackendStatus';

interface Project {
  id: number;
  name: string;
  created_at: string;
  cache_dir: string;
  style_profile_id?: number;
}

interface ToolbarProps {
  onGenerate: () => void;
  onExport: () => void;
  isGenerating?: boolean;
  isExporting?: boolean;
  isUploading?: boolean;
  currentProjectId?: number;
  currentProjectName?: string;
  projects?: Project[];
  onProjectSelect: (projectId: number) => void;
  onCreateProject: () => void;
}

export function Toolbar({
  onGenerate,
  onExport,
  isGenerating = false,
  isExporting = false,
  isUploading = false,
  currentProjectId,
  currentProjectName = 'My First Project',
  projects = [],
  onProjectSelect,
  onCreateProject,
}: ToolbarProps) {
  const [isDropdownOpen, setIsDropdownOpen] = useState(false);
  const dropdownRef = useRef<HTMLDivElement>(null);

  // Close dropdown when clicking outside
  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (dropdownRef.current && !dropdownRef.current.contains(event.target as Node)) {
        setIsDropdownOpen(false);
      }
    };

    if (isDropdownOpen) {
      document.addEventListener('mousedown', handleClickOutside);
      return () => {
        document.removeEventListener('mousedown', handleClickOutside);
      };
    }
  }, [isDropdownOpen]);

  const otherProjects = projects.filter((p) => p.id !== currentProjectId);
  return (
    <div
      style={{
        backgroundColor: '#2a2a2a',
        borderBottom: '1px solid #404040',
        height: '40px',
        display: 'flex',
        alignItems: 'center',
        padding: '0 1rem',
        gap: '0.5rem',
      }}
    >
      {/* Upload progress indicator on left */}
      {isUploading && (
        <div style={{ display: 'flex', alignItems: 'center', gap: '0.5rem' }}>
          <div
            style={{
              width: '16px',
              height: '16px',
              border: '2px solid #404040',
              borderTop: '2px solid #3b82f6',
              borderRadius: '50%',
              animation: 'spin 1s linear infinite',
            }}
          />
          <span style={{ fontSize: '0.875rem', color: '#e5e5e5' }}>Uploading...</span>
        </div>
      )}

      <div style={{ flex: 1 }} />

      {/* Project Selector - Center */}
      <div
        ref={dropdownRef}
        style={{
          position: 'relative',
          display: 'flex',
          alignItems: 'center',
        }}
      >
        <div
          onClick={() => setIsDropdownOpen(!isDropdownOpen)}
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: '0.5rem',
            cursor: 'pointer',
            padding: '0.25rem 0.5rem',
            borderRadius: '4px',
            transition: 'background-color 0.2s',
          }}
          onMouseEnter={(e) => {
            e.currentTarget.style.backgroundColor = 'rgba(255, 255, 255, 0.1)';
          }}
          onMouseLeave={(e) => {
            e.currentTarget.style.backgroundColor = 'transparent';
          }}
        >
          <span style={{ fontSize: '0.875rem', color: '#e5e5e5', fontWeight: 500 }}>
            {currentProjectName}
          </span>
          <span style={{ fontSize: '0.75rem', color: '#a0a0a0' }}>
            {isDropdownOpen ? '▼' : '▶'}
          </span>
        </div>

        {/* Dropdown Menu */}
        {isDropdownOpen && (
          <div
            style={{
              position: 'absolute',
              top: '100%',
              left: '50%',
              transform: 'translateX(-50%)',
              marginTop: '0.25rem',
              backgroundColor: '#2a2a2a',
              border: '1px solid #404040',
              borderRadius: '4px',
              minWidth: '200px',
              maxWidth: '300px',
              maxHeight: '300px',
              overflowY: 'auto',
              zIndex: 1000,
              boxShadow: '0 4px 6px rgba(0, 0, 0, 0.3)',
            }}
          >
            {/* Other Projects */}
            {otherProjects.length > 0 && (
              <>
                {otherProjects.map((project) => (
                  <div
                    key={project.id}
                    onClick={() => {
                      onProjectSelect(project.id);
                      setIsDropdownOpen(false);
                    }}
                    style={{
                      padding: '0.5rem 1rem',
                      cursor: 'pointer',
                      fontSize: '0.875rem',
                      color: '#e5e5e5',
                      transition: 'background-color 0.2s',
                    }}
                    onMouseEnter={(e) => {
                      e.currentTarget.style.backgroundColor = 'rgba(255, 255, 255, 0.1)';
                    }}
                    onMouseLeave={(e) => {
                      e.currentTarget.style.backgroundColor = 'transparent';
                    }}
                  >
                    {project.name}
                  </div>
                ))}
                {/* Divider */}
                <div
                  style={{
                    height: '1px',
                    backgroundColor: '#404040',
                    margin: '0.25rem 0',
                  }}
                />
              </>
            )}

            {/* Create New Project */}
            <div
              onClick={() => {
                onCreateProject();
                setIsDropdownOpen(false);
              }}
              style={{
                padding: '0.5rem 1rem',
                cursor: 'pointer',
                fontSize: '0.875rem',
                color: '#10b981',
                transition: 'background-color 0.2s',
              }}
              onMouseEnter={(e) => {
                e.currentTarget.style.backgroundColor = 'rgba(16, 185, 129, 0.1)';
              }}
              onMouseLeave={(e) => {
                e.currentTarget.style.backgroundColor = 'transparent';
              }}
            >
              Create new project
            </div>
          </div>
        )}
      </div>

      <div style={{ flex: 1 }} />

      <button
        onClick={onGenerate}
        disabled={isGenerating}
        style={{
          backgroundColor: isGenerating ? '#505050' : '#10b981',
          color: 'white',
          border: 'none',
          padding: '0.375rem 0.75rem',
          fontSize: '0.875rem',
          borderRadius: '4px',
          cursor: isGenerating ? 'not-allowed' : 'pointer',
        }}
      >
        {isGenerating ? 'Generating...' : 'Generate'}
      </button>

      <button
        onClick={onExport}
        disabled={isExporting}
        style={{
          backgroundColor: isExporting ? '#505050' : '#2a2a2a',
          color: '#e5e5e5',
          border: '1px solid #404040',
          padding: '0.375rem 0.75rem',
          fontSize: '0.875rem',
          borderRadius: '4px',
          cursor: isExporting ? 'not-allowed' : 'pointer',
        }}
      >
        {isExporting ? 'Exporting...' : 'Export'}
      </button>

      <BackendStatus />
      <style>{`
        @keyframes spin {
          0% { transform: rotate(0deg); }
          100% { transform: rotate(360deg); }
        }
      `}</style>
    </div>
  );
}
