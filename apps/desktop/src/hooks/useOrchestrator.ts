import { useState, useCallback } from 'react';

const API_BASE = 'http://127.0.0.1:7777/api';

interface ProposeRequest {
  user_intent: string;
  filters?: any;
  context?: any;
}

interface AgentResponse<T> {
  mode: 'talk' | 'busy' | 'act';
  message: string;
  suggestions: string[];
  questions: string[];
  data?: T;
  debug?: any;
}

interface ProposeData {
  candidate_segments: any[];
  narrative_structure?: string;
}

interface PlanData {
  edit_plan: any;
}

interface ApplyData {
  timeline: any;
}

type ProposeResponse = AgentResponse<ProposeData>;
type PlanResponse = AgentResponse<PlanData>;
type ApplyResponse = AgentResponse<ApplyData>;

interface PlanRequest {
  beats: any[];
  constraints: {
    target_length: number | null;
    vibe: string | null;
    captions_on: boolean;
    music_on: boolean;
  };
  style_profile_id: number | null;
  narrative_structure: string;
}

interface ApplyRequest {
  edit_plan: any;
  confirm_token?: string;
}

export function usePropose(projectId: number) {
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<Error | null>(null);

  const propose = useCallback(async (request: ProposeRequest): Promise<ProposeResponse> => {
    setIsLoading(true);
    setError(null);
    try {
      const response = await fetch(`${API_BASE}/projects/${projectId}/orchestrator/propose`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify(request),
      });

      if (!response.ok) {
        let errorMessage = `HTTP error! status: ${response.status}`;
        try {
          const errorData = await response.json();
          if (errorData && typeof errorData === 'object' && 'error' in errorData) {
            errorMessage = String(errorData.error);
          } else if (errorData && typeof errorData === 'object' && 'message' in errorData) {
            errorMessage = String(errorData.message);
          }
        } catch {
          // If response is not JSON, use status text
          errorMessage = response.statusText || errorMessage;
        }
        throw new Error(errorMessage);
      }

      const data = await response.json();
      return data;
    } catch (err) {
      const error = err instanceof Error ? err : new Error('Unknown error');
      setError(error);
      throw error;
    } finally {
      setIsLoading(false);
    }
  }, [projectId]);

  return { propose, isLoading, error };
}

export function useGeneratePlan(projectId: number) {
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<Error | null>(null);

  const generatePlan = useCallback(async (request: PlanRequest): Promise<PlanResponse> => {
    setIsLoading(true);
    setError(null);
    try {
      const response = await fetch(`${API_BASE}/projects/${projectId}/orchestrator/plan`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify(request),
      });

      if (!response.ok) {
        throw new Error(`HTTP error! status: ${response.status}`);
      }

      const data = await response.json();
      return data;
    } catch (err) {
      const error = err instanceof Error ? err : new Error('Unknown error');
      setError(error);
      throw error;
    } finally {
      setIsLoading(false);
    }
  }, [projectId]);

  return { generatePlan, isLoading, error };
}

export function useApplyPlan(projectId: number) {
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<Error | null>(null);

  const applyPlan = useCallback(async (request: ApplyRequest, confirmToken?: string): Promise<ApplyResponse> => {
    setIsLoading(true);
    setError(null);
    try {
      let url = `${API_BASE}/projects/${projectId}/orchestrator/apply`;
      if (confirmToken) {
        url += `?confirm=${encodeURIComponent(confirmToken)}`;
      }
      
      const response = await fetch(url, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify(request),
      });

      if (!response.ok) {
        throw new Error(`HTTP error! status: ${response.status}`);
      }

      const data = await response.json();
      return data;
    } catch (err) {
      const error = err instanceof Error ? err : new Error('Unknown error');
      setError(error);
      throw error;
    } finally {
      setIsLoading(false);
    }
  }, [projectId]);

  return { applyPlan, isLoading, error };
}

