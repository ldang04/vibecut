import { useState, useEffect, useRef } from 'react';

const API_BASE = 'http://127.0.0.1:7777/api';

interface JobEvent {
  type: 'JobCompleted' | 'JobFailed' | 'AnalysisComplete';
  job_id?: number;
  job_type?: string;
  asset_id?: number;
  project_id?: number;
  readiness?: string;
  error?: string;
}

export function useOrchestratorEvents(
  projectId: number,
  onEvent?: (event: JobEvent) => void
) {
  const [isConnected, setIsConnected] = useState(false);
  const [error, setError] = useState<Error | null>(null);
  const eventSourceRef = useRef<EventSource | null>(null);
  const reconnectTimeoutRef = useRef<NodeJS.Timeout | null>(null);
  const reconnectAttempts = useRef(0);
  const maxReconnectAttempts = 10;
  const baseReconnectDelay = 1000; // 1 second

  useEffect(() => {
    const connect = () => {
      // Close existing connection if any
      if (eventSourceRef.current) {
        eventSourceRef.current.close();
      }

      const url = `${API_BASE}/projects/${projectId}/orchestrator/events`;
      const eventSource = new EventSource(url);

      eventSource.onopen = () => {
        setIsConnected(true);
        setError(null);
        reconnectAttempts.current = 0;
        console.log('[SSE] Connected to orchestrator events');
      };

      eventSource.onmessage = (e) => {
        try {
          const event: JobEvent = JSON.parse(e.data);
          
          // Handle different event types
          if (event.type === 'AnalysisComplete' && event.project_id === projectId) {
            console.log('[SSE] Analysis complete for asset', event.asset_id);
          } else if (event.type === 'JobCompleted' || event.type === 'JobFailed') {
            console.log('[SSE] Job event:', event.type, event.job_id);
          }

          if (onEvent) {
            onEvent(event);
          }
        } catch (err) {
          console.error('[SSE] Error parsing event:', err);
        }
      };

      eventSource.onerror = (err) => {
        console.error('[SSE] EventSource error:', err);
        setIsConnected(false);
        eventSource.close();

        // Exponential backoff reconnection
        if (reconnectAttempts.current < maxReconnectAttempts) {
          const delay = baseReconnectDelay * Math.pow(2, reconnectAttempts.current);
          reconnectAttempts.current += 1;
          
          reconnectTimeoutRef.current = setTimeout(() => {
            console.log(`[SSE] Reconnecting (attempt ${reconnectAttempts.current})...`);
            connect();
          }, delay);
        } else {
          setError(new Error('Failed to connect to orchestrator events after multiple attempts'));
        }
      };

      eventSourceRef.current = eventSource;
    };

    connect();

    return () => {
      if (reconnectTimeoutRef.current) {
        clearTimeout(reconnectTimeoutRef.current);
      }
      if (eventSourceRef.current) {
        eventSourceRef.current.close();
      }
    };
  }, [projectId, onEvent]);

  return { isConnected, error };
}

