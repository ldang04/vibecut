import { useState } from 'react';

interface MediaSidebarProps {
  selectedTab: 'raw' | 'references';
  onTabChange: (tab: 'raw' | 'references') => void;
  onCollapseChange?: (collapsed: boolean) => void;
}

export function MediaSidebar({ selectedTab, onTabChange, onCollapseChange }: MediaSidebarProps) {
  const [isCollapsed, setIsCollapsed] = useState(false);

  const handleCollapse = (collapsed: boolean) => {
    setIsCollapsed(collapsed);
    if (onCollapseChange) {
      onCollapseChange(collapsed);
    }
  };

  return (
    <div
      style={{
        width: '100%',
        backgroundColor: '#1e1e1e',
        borderRight: '1px solid #404040',
        display: 'flex',
        flexDirection: 'column',
        height: '100%',
      }}
    >
      <div
        style={{
          padding: '0.5rem 0',
          display: 'flex',
          justifyContent: 'flex-start',
          alignItems: 'center',
          borderBottom: isCollapsed ? 'none' : '1px solid #404040',
        }}
      >
        <button
          onClick={(e) => {
            e.stopPropagation();
            handleCollapse(!isCollapsed);
          }}
          onMouseDown={(e) => e.preventDefault()}
          style={{
            background: 'none',
            border: 'none',
            color: '#a0a0a0',
            cursor: 'pointer',
            fontSize: '12px',
            padding: '0.25rem 0.5rem',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            outline: 'none',
          }}
          onFocus={(e) => e.target.blur()}
          title={isCollapsed ? "Expand Sidebar" : "Collapse Sidebar"}
        >
          {isCollapsed ? '▶' : '◀'}
        </button>
      </div>

      {!isCollapsed && (
        <div style={{ display: 'flex', flexDirection: 'column' }}>
          <button
            onClick={() => onTabChange('raw')}
            onMouseDown={(e) => e.preventDefault()}
            style={{
              backgroundColor: selectedTab === 'raw' ? '#2a2a2a' : 'transparent',
              border: 'none',
              borderBottom: '1px solid #404040',
              color: selectedTab === 'raw' ? '#e5e5e5' : '#a0a0a0',
              cursor: 'pointer',
              fontSize: '13px',
              fontWeight: selectedTab === 'raw' ? 600 : 400,
              padding: '0.75rem',
              textAlign: 'left',
              transition: 'background-color 0.2s',
              outline: 'none',
            }}
          >
            Raw Footage
          </button>

          <button
            onClick={() => onTabChange('references')}
            onMouseDown={(e) => e.preventDefault()}
            style={{
              backgroundColor: selectedTab === 'references' ? '#2a2a2a' : 'transparent',
              border: 'none',
              color: selectedTab === 'references' ? '#e5e5e5' : '#a0a0a0',
              cursor: 'pointer',
              fontSize: '13px',
              fontWeight: selectedTab === 'references' ? 600 : 400,
              padding: '0.75rem',
              textAlign: 'left',
              transition: 'background-color 0.2s',
              outline: 'none',
            }}
          >
            References
          </button>
        </div>
      )}
    </div>
  );
}

