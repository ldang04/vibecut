import { useState, useCallback, useEffect, useRef } from 'react';

const DAEMON_BASE_URL = 'http://127.0.0.1:7777/api';

export interface UseDaemonResult<T> {
  data: T | null;
  loading: boolean;
  error: Error | null;
  execute: (...args: any[]) => Promise<T | null>;
}

export function useDaemon<T = any>(
  endpoint: string,
  options?: {
    method?: 'GET' | 'POST' | 'PUT' | 'DELETE';
    immediate?: boolean;
  }
): UseDaemonResult<T> {
  const [data, setData] = useState<T | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<Error | null>(null);
  const prevEndpointRef = useRef<string>(endpoint);

  // Reset data when endpoint changes (e.g., when projectId changes)
  useEffect(() => {
    if (prevEndpointRef.current !== endpoint) {
      setData(null);
      setError(null);
      prevEndpointRef.current = endpoint;
    }
  }, [endpoint]);

  const execute = useCallback(
    async (body?: any): Promise<T | null> => {
      setLoading(true);
      setError(null);

      try {
        const fetchOptions: RequestInit = {
          method: options?.method || 'GET',
          headers: {
            'Content-Type': 'application/json',
          },
        };

        if (body && (options?.method === 'POST' || options?.method === 'PUT')) {
          fetchOptions.body = JSON.stringify(body);
        }

        const url = `${DAEMON_BASE_URL}${endpoint}`;
        // Only log POST/PUT requests to reduce console spam
        if (options?.method === 'POST' || options?.method === 'PUT') {
          console.log('Making request to:', url, 'with body:', body);
        }
        
        const response = await fetch(url, fetchOptions);

        if (!response.ok) {
          // Try to get error message from response body
          let errorMessage = `HTTP error! status: ${response.status}`;
          try {
            const errorBody = await response.text();
            if (errorBody) {
              errorMessage += ` - ${errorBody}`;
            }
          } catch {
            // Ignore if we can't read the error body
          }
          // Only log non-404 errors to reduce spam
          // 404s are expected for missing resources and will be handled by the caller
          if (response.status !== 404) {
            console.error('Request failed:', errorMessage);
          }
          throw new Error(errorMessage);
        }

        const result = await response.json();
        setData(result);
        return result;
      } catch (err) {
        // Only log non-404 errors to reduce spam
        // 404s are expected for missing resources (like timelines that don't exist yet)
        const is404Error = err instanceof Error && err.message.includes('404');
        if (!is404Error) {
          console.error('Fetch error:', err);
        }
        const error = err instanceof Error ? err : new Error(`Network error: ${err instanceof Error ? err.message : 'Unknown error'}`);
        // Don't set error state for 404s - they're expected for missing resources
        if (!is404Error) {
          setError(error);
        }
        setData(null);
        return null;
      } finally {
        setLoading(false);
      }
    },
    [endpoint, options?.method]
  );

  return { data, loading, error, execute };
}
