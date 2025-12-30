import { useState, useEffect } from 'react';

interface TextEditorPanelProps {
  clip: any | null;
  onSave: (clipId: string, text: string) => void;
  onClose: () => void;
}

export function TextEditorPanel({ clip, onSave, onClose }: TextEditorPanelProps) {
  const [text, setText] = useState('Add text');

  useEffect(() => {
    if (clip) {
      // Get text from clip (could be in text, defaultText, or text_content field)
      // Default to "Add text" if no text is found
      const clipText = clip.text || clip.defaultText || clip.text_content || '';
      setText(clipText || 'Add text');
    }
  }, [clip]);

  if (!clip) return null;

  const handleSave = () => {
    const clipId = clip.id || `${clip.asset_id}-${clip.timeline_start_ticks}`;
    onSave(clipId, text);
    onClose();
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Escape') {
      onClose();
    } else if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
      handleSave();
    }
  };

  return (
    <div
      style={{
        width: '100%',
        height: '100%',
        display: 'flex',
        flexDirection: 'column',
        backgroundColor: '#2a2a2a',
        padding: '1rem',
        overflow: 'auto',
      }}
    >
      <div style={{ marginBottom: '1rem', display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
        <h3 style={{ margin: 0, color: '#e5e5e5', fontSize: '16px', fontWeight: 600 }}>
          Edit Text
        </h3>
        <button
          onClick={onClose}
          style={{
            padding: '0.25rem 0.5rem',
            backgroundColor: 'transparent',
            border: 'none',
            color: '#999',
            cursor: 'pointer',
            fontSize: '20px',
            lineHeight: 1,
          }}
          title="Close"
        >
          ×
        </button>
      </div>

      <div style={{ marginBottom: '1rem', display: 'flex', flexDirection: 'column' }}>
        <label style={{ marginBottom: '0.5rem', color: '#999', fontSize: '14px' }}>
          Text Content
        </label>
        <input
          type="text"
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={handleKeyDown}
          onKeyPress={(e) => {
            // Save on Enter key
            if (e.key === 'Enter') {
              e.preventDefault();
              handleSave();
            }
          }}
          placeholder="Add text"
          style={{
            width: '100%',
            padding: '0.5rem',
            backgroundColor: '#1e1e1e',
            border: '1px solid #404040',
            borderRadius: '4px',
            color: '#e5e5e5',
            fontSize: '14px',
            fontFamily: 'inherit',
            outline: 'none',
          }}
          autoFocus
        />
      </div>

      <div style={{ display: 'flex', justifyContent: 'flex-end', gap: '0.5rem', paddingTop: '1rem', borderTop: '1px solid #404040' }}>
        <button
          onClick={onClose}
          style={{
            padding: '0.5rem 1rem',
            backgroundColor: '#1e1e1e',
            border: '1px solid #404040',
            borderRadius: '4px',
            color: '#e5e5e5',
            cursor: 'pointer',
            fontSize: '14px',
          }}
        >
          Cancel
        </button>
        <button
          onClick={handleSave}
          style={{
            padding: '0.5rem 1rem',
            backgroundColor: '#3b82f6',
            border: '1px solid #3b82f6',
            borderRadius: '4px',
            color: '#ffffff',
            cursor: 'pointer',
            fontSize: '14px',
            fontWeight: 500,
          }}
        >
          Save
        </button>
      </div>
    </div>
  );
}

