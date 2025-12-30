
export type Tool = 'pointer' | 'cut';

interface TimelineToolbarProps {
  activeTool: Tool;
  onToolChange: (tool: Tool) => void;
  playheadTime: number; // in seconds - playhead is authoritative, not hover time
  totalDuration: number; // in seconds
}

function formatTimecode(seconds: number): string {
  const mins = Math.floor(seconds / 60);
  const secs = Math.floor(seconds % 60);
  return `${mins.toString().padStart(2, '0')}:${secs.toString().padStart(2, '0')}`;
}

export function TimelineToolbar({
  activeTool,
  onToolChange,
  playheadTime,
  totalDuration,
}: TimelineToolbarProps) {
  return (
    <div
      style={{
        backgroundColor: '#2a2a2a',
        borderBottom: '1px solid #404040',
        height: '40px',
        display: 'flex',
        alignItems: 'center',
        padding: '0 1rem',
        position: 'relative',
      }}
    >
      {/* Left spacer */}
      <div style={{ flex: 1 }} />

      {/* Centered Timecode Display */}
      <div
        style={{
          position: 'absolute',
          left: '50%',
          transform: 'translateX(-50%)',
          fontSize: '0.875rem',
          color: '#e5e5e5',
          fontFamily: 'monospace',
          display: 'flex',
          alignItems: 'center',
          gap: '0.5rem',
        }}
      >
        <span>{formatTimecode(playheadTime)}</span>
        <span style={{ color: '#666' }}>/</span>
        <span>{formatTimecode(totalDuration)}</span>
      </div>

      {/* Right spacer */}
      <div style={{ flex: 1 }} />

      {/* Tool Selector - Right side */}
      <div style={{ display: 'flex', gap: '0.5rem' }}>
        <button
          onClick={() => onToolChange('pointer')}
          style={{
            backgroundColor: activeTool === 'pointer' ? '#3b82f6' : '#1e1e1e',
            color: '#e5e5e5',
            border: '1px solid #404040',
            padding: '0.375rem 0.75rem',
            fontSize: '0.875rem',
            borderRadius: '4px',
            cursor: 'pointer',
            display: 'flex',
            alignItems: 'center',
            gap: '0.25rem',
            transition: 'background-color 0.2s',
          }}
          title="Pointer Tool"
        >
          <span>↖</span>
        </button>
        <button
          onClick={() => onToolChange('cut')}
          style={{
            backgroundColor: activeTool === 'cut' ? '#3b82f6' : '#1e1e1e',
            color: '#e5e5e5',
            border: '1px solid #404040',
            padding: '0.375rem 0.75rem',
            fontSize: '0.875rem',
            borderRadius: '4px',
            cursor: 'pointer',
            display: 'flex',
            alignItems: 'center',
            gap: '0.25rem',
            transition: 'background-color 0.2s',
          }}
          title="Cut Tool"
        >
          <span>✂</span>
        </button>
      </div>
    </div>
  );
}

