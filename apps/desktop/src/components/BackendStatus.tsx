import { useState, useEffect } from 'react';

interface HealthResponse {
  ok: boolean;
  version: string;
}

export function BackendStatus() {
  const [status, setStatus] = useState<'checking' | 'online' | 'offline'>('checking');
  const [version, setVersion] = useState<string>('');
  const [error, setError] = useState<string>('');

  useEffect(() => {
    const checkHealth = async () => {
      try {
        const response = await fetch('http://127.0.0.1:7777/health');
        if (!response.ok) {
          throw new Error(`HTTP error! status: ${response.status}`);
        }
        const data: HealthResponse = await response.json();
        setStatus('online');
        setVersion(data.version);
        setError('');
      } catch (err) {
        setStatus('offline');
        setVersion('');
        setError(err instanceof Error ? err.message : 'Unknown error');
      }
    };

    // Check immediately
    checkHealth();

    // Check every 5 seconds
    const interval = setInterval(checkHealth, 5000);

    return () => clearInterval(interval);
  }, []);

  const getStatusColor = () => {
    switch (status) {
      case 'online':
        return '#10b981'; // green
      case 'offline':
        return '#ef4444'; // red
      case 'checking':
        return '#f59e0b'; // amber
      default:
        return '#6b7280'; // gray
    }
  };

  const getStatusText = () => {
    switch (status) {
      case 'online':
        return 'Online';
      case 'offline':
        return 'Offline';
      case 'checking':
        return 'Checking...';
      default:
        return 'Unknown';
    }
  };

  return (
      <div style={{ display: 'flex', alignItems: 'center', gap: '0.5rem' }}>
        <div
          style={{
          width: '8px',
          height: '8px',
            borderRadius: '50%',
            backgroundColor: getStatusColor(),
            transition: 'background-color 0.2s',
          }}
        />
      <span style={{ fontSize: '12px', color: status === 'online' ? '#10b981' : '#ef4444' }}>
        {getStatusText()}
      </span>
        {version && (
        <span style={{ color: '#a0a0a0', fontSize: '11px' }}>
          v{version}
          </span>
      )}
    </div>
  );
}
