import React, { useState, useRef, useEffect, useCallback } from 'react';
import { useOrchestrator } from '../hooks/useOrchestrator';
import { useOrchestratorEvents } from '../hooks/useOrchestratorEvents';

interface OrchestratorPanelProps {
  projectId: number;
}

interface Message {
  role: 'user' | 'assistant';
  content: string;
  isStreaming?: boolean;
}

export const OrchestratorPanel: React.FC<OrchestratorPanelProps> = ({ projectId }) => {
  const [messages, setMessages] = useState<Message[]>([]);
  const [userInput, setUserInput] = useState('');
  const [proposal, setProposal] = useState<{ candidate_segments: Array<{ segment_id: number; duration_sec: number }>; narrative_structure?: string } | null>(null);
  const [editPlan, setEditPlan] = useState<Record<string, unknown> | null>(null);
  const [suggestions, setSuggestions] = useState<Array<{ label: string; action: string; confirm_token?: string | null }>>([]);
  const [questions, setQuestions] = useState<string[]>([]);
  const [currentMode, setCurrentMode] = useState<'talk' | 'busy' | 'act'>('talk');
  const [inputFocused, setInputFocused] = useState(false);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const streamingTimeoutRef = useRef<NodeJS.Timeout | null>(null);
  
  const { propose, generatePlan, applyPlan, getMessages, isProposing, isGenerating, isApplying } = useOrchestrator(projectId);

  // Subscribe to orchestrator events for proactive messages
  useOrchestratorEvents(projectId, async (event) => {
    if (event.type === 'AnalysisComplete' && event.project_id === projectId) {
      // Fetch new messages from the database when analysis completes
      // The agent has already generated a proactive message
      try {
        const newMessages = await getMessages();
        // Filter to only new messages (not already in state)
        const existingContents = new Set(messages.map(m => m.content));
        const messagesToAdd = newMessages
          .filter((msg: any) => !existingContents.has(msg.content))
          .map((msg: any) => ({
            role: msg.role as 'user' | 'assistant',
            content: msg.content,
            isStreaming: false,
          }));
        
        if (messagesToAdd.length > 0) {
          setMessages(prev => [...prev, ...messagesToAdd]);
        }
      } catch (error) {
        console.error('Error fetching messages after analysis complete:', error);
      }
    }
  });

  // Auto-resize textarea
  useEffect(() => {
    if (textareaRef.current) {
      textareaRef.current.style.height = 'auto';
      textareaRef.current.style.height = `${Math.min(textareaRef.current.scrollHeight, 120)}px`;
    }
  }, [userInput]);

  // Cleanup streaming timeout on unmount
  useEffect(() => {
    return () => {
      if (streamingTimeoutRef.current) {
        clearTimeout(streamingTimeoutRef.current);
      }
    };
  }, []);

  // Stream text character by character
  const streamMessage = useCallback((messageIndex: number, fullText: string, speed: number = 20) => {
    let currentIndex = 0;
    
    const stream = () => {
      if (currentIndex < fullText.length) {
        const partialText = fullText.slice(0, currentIndex + 1);
        setMessages(prev => {
          const updated = [...prev];
          if (updated[messageIndex]) {
            updated[messageIndex] = {
              ...updated[messageIndex],
              content: partialText,
              isStreaming: currentIndex < fullText.length - 1,
            };
          }
          return updated;
        });
        currentIndex++;
        streamingTimeoutRef.current = setTimeout(stream, speed);
      } else {
        // Mark as complete
        setMessages(prev => {
          const updated = [...prev];
          if (updated[messageIndex]) {
            updated[messageIndex] = {
              ...updated[messageIndex],
              isStreaming: false,
            };
          }
          return updated;
        });
      }
    };
    
    stream();
  }, []);

  const handleSendMessage = async () => {
    if (!userInput.trim()) return;

    const userInputLower = userInput.trim().toLowerCase();
    
    // Check if user is confirming an action
    const isConfirmation = ['yes', 'yeah', 'yep', 'sure', 'ok', 'okay', 'go ahead', 'do it', 'generate', 'create', 'show', 'review'].includes(userInputLower);
    
    // Check last assistant message to see what they're confirming
    const lastAssistantMessage = messages.filter(m => m.role === 'assistant').pop();
    const lastMessageContent = lastAssistantMessage?.content.toLowerCase() || '';
    
    // If user says "yes" and last message was about showing/reviewing segments, automatically propose
    if (isConfirmation && (lastMessageContent.includes('show') || lastMessageContent.includes('review') || lastMessageContent.includes('segments') || lastMessageContent.includes('moments') || lastMessageContent.includes('check out'))) {
      setUserInput('');
      // Automatically trigger propose to show segments
      try {
        const result = await propose({
          user_intent: "show me the segments",
          filters: undefined,
          context: undefined,
        });
        
        const messageIndex = messages.length + 1;
        setMessages(prev => [...prev, {
          role: 'assistant',
          content: '',
          isStreaming: true,
        }]);
        
        setTimeout(() => {
          streamMessage(messageIndex, result.message, 15);
        }, 50);
        
        setCurrentMode(result.mode);
        setSuggestions([]);
        setQuestions([]);
        
        if (result.mode === 'act' && result.data) {
          setProposal(result.data);
        } else {
          setProposal(null);
        }
      } catch (error) {
        console.error('Error proposing edit:', error);
      }
      return;
    }
    
    // If user says "yes" and last message was about generating a plan, trigger generate plan
    if (isConfirmation && proposal && (lastMessageContent.includes('generate') || lastMessageContent.includes('edit plan') || lastMessageContent.includes('plan'))) {
      setUserInput('');
      handleGeneratePlan();
      return;
    }
    
    // If user says "yes" and last message was about applying plan, trigger apply plan
    if (isConfirmation && editPlan && (lastMessageContent.includes('apply') || lastMessageContent.includes('timeline'))) {
      setUserInput('');
      handleApplyPlan();
      return;
    }

    const userMessage = { role: 'user' as const, content: userInput };
    const messageToSend = userInput;
    setMessages(prev => [...prev, userMessage]);
    setUserInput('');

    try {
      const result = await propose({
        user_intent: messageToSend,
        filters: undefined,
        context: undefined,
      });

      // Add message placeholder and start streaming
      const messageIndex = messages.length + 1; // +1 for user message we just added
      setMessages(prev => [...prev, {
        role: 'assistant',
        content: '',
        isStreaming: true,
      }]);
      
      // Start streaming the message
      setTimeout(() => {
        streamMessage(messageIndex, result.message, 15); // 15ms per character for smooth streaming
      }, 50);

      // Handle mode-specific UI - only store proposal data, no buttons
      setCurrentMode(result.mode);
      setSuggestions([]); // No buttons - everything through chat
      setQuestions([]);
      
      if (result.mode === 'act' && result.data) {
        setProposal(result.data);
      } else {
        setProposal(null);
      }
    } catch (error) {
      console.error('Error proposing edit:', error);
      let errorMessage = "Something went wrong — let me try again.";
      
      if (error instanceof Error) {
        const msg = error.message.toLowerCase();
        
        if (msg.includes('failed to fetch') || msg.includes('networkerror')) {
          errorMessage = "I can't connect to the server right now. Make sure the daemon is running and try again.";
        } else if (msg.includes('500') || msg.includes('internal server error')) {
          errorMessage = "The ML service isn't running. Start it with: cd ml/service && source .venv/bin/activate && uvicorn main:app --host 127.0.0.1 --port 8001";
        } else {
          errorMessage = `Hmm, I ran into an issue: ${error.message}. Want to try again?`;
        }
      }
      
      // Stream error message too
      const errorMessageIndex = messages.length + 1;
      setMessages(prev => [...prev, {
        role: 'assistant',
        content: '',
        isStreaming: true,
      }]);
      
      setTimeout(() => {
        streamMessage(errorMessageIndex, errorMessage, 15);
      }, 50);
    }
  };

  const handleGeneratePlan = async () => {
    if (!proposal || !proposal.candidate_segments) return;

    try {
      // Convert proposal to beats format
      const beats = proposal.candidate_segments.slice(0, 10).map((seg, idx: number) => ({
        beat_id: `beat_${idx}`,
        segment_ids: [seg.segment_id],
        target_sec: seg.duration_sec,
      }));

      const result = await generatePlan({
        beats,
        constraints: {
          target_length: null,
          vibe: null,
          captions_on: false,
          music_on: false,
        },
        style_profile_id: null,
        narrative_structure: proposal.narrative_structure || '',
      });

      if (result.data) {
        setEditPlan(result.data.edit_plan);
        if (result.message) {
          const planMessageIndex = messages.length;
          setMessages(prev => [...prev, {
            role: 'assistant',
            content: '',
            isStreaming: true,
          }]);
          
          setTimeout(() => {
            streamMessage(planMessageIndex, result.message, 15);
          }, 50);
        }
      }
    } catch (error) {
      console.error('Error generating plan:', error);
      const errorPlanIndex = messages.length;
      setMessages(prev => [...prev, {
        role: 'assistant',
        content: '',
        isStreaming: true,
      }]);
      
      setTimeout(() => {
        streamMessage(errorPlanIndex, "Hmm, I couldn't generate the plan. Want to try again?", 15);
      }, 50);
    }
  };

  const handleApplyPlan = async (confirmToken?: string) => {
    if (!editPlan) return;

    try {
      const result = await applyPlan({ edit_plan: editPlan }, confirmToken);
      
      if (result.mode === 'talk' && result.message.includes('replace')) {
        // Confirmation needed - stream message and show suggestions
        const confirmMessageIndex = messages.length;
        setMessages(prev => [...prev, {
          role: 'assistant',
          content: '',
          isStreaming: true,
        }]);
        setSuggestions(result.suggestions || []);
        
        setTimeout(() => {
          streamMessage(confirmMessageIndex, result.message, 15);
        }, 50);
        return;
      }
      
      // Plan applied successfully
      if (result.message) {
        const applyMessageIndex = messages.length;
        setMessages(prev => [...prev, {
          role: 'assistant',
          content: '',
          isStreaming: true,
        }]);
        
        setTimeout(() => {
          streamMessage(applyMessageIndex, result.message, 15);
        }, 50);
      }
      setProposal(null);
      setEditPlan(null);
      setSuggestions([]);
    } catch (error) {
      console.error('Error applying plan:', error);
      const errorApplyIndex = messages.length;
      setMessages(prev => [...prev, {
        role: 'assistant',
        content: '',
        isStreaming: true,
      }]);
      
      setTimeout(() => {
        streamMessage(errorApplyIndex, "Hmm, I couldn't apply the plan. Want to try again?", 15);
      }, 50);
    }
  };

  return (
    <>
      <style>{`
        @keyframes blink {
          0%, 50% { opacity: 1; }
          51%, 100% { opacity: 0; }
        }
        @keyframes flicker {
          0%, 100% { opacity: 1; }
          25% { opacity: 0.6; }
          50% { opacity: 0.8; }
          75% { opacity: 0.5; }
        }
      `}</style>
      <div style={{
        width: '100%',
        height: '100%',
        display: 'flex',
        flexDirection: 'column',
        backgroundColor: '#1a1a1a',
        color: '#e5e5e5',
      }}>

      <div style={{
        flex: 1,
        overflowY: 'auto',
        padding: '0.75rem',
      }}>
        {messages.map((msg, idx) => (
          <div key={idx} style={{
            marginBottom: '0.75rem',
            padding: '0.625rem',
            backgroundColor: msg.role === 'user' ? '#2a2a2a' : '#1e1e1e',
            borderRadius: '4px',
            border: '1px solid #404040',
          }}>
            <div style={{ 
              fontWeight: 600, 
              marginBottom: '0.25rem',
              fontSize: '12px',
              color: msg.role === 'user' ? '#3b82f6' : '#10b981',
            }}>
              {msg.role === 'user' ? 'You' : 'AI'}
            </div>
            <div style={{ 
              fontSize: '13px',
              color: '#e5e5e5',
              lineHeight: '1.4',
            }}>
              {msg.content}
              {msg.isStreaming && (
                <span style={{
                  display: 'inline-block',
                  width: '2px',
                  height: '14px',
                  backgroundColor: '#10b981',
                  marginLeft: '2px',
                  animation: 'blink 1s infinite',
                }} />
              )}
            </div>
          </div>
        ))}

        {/* Show loading indicator while processing */}
        {(isProposing || isGenerating || isApplying) && (
          <div style={{
            marginTop: '0.75rem',
            padding: '0.75rem',
            backgroundColor: '#1e1e1e',
            borderRadius: '4px',
            border: '1px solid #404040',
            textAlign: 'center',
          }}>
            <div style={{ 
              fontSize: '13px',
              color: '#10b981',
              animation: 'flicker 1.5s ease-in-out infinite',
            }}>
              Planning next moves
            </div>
          </div>
        )}

      </div>

      <div style={{
        padding: '0.75rem',
        borderTop: '1px solid #404040',
        backgroundColor: '#1a1a1a',
      }}>
        <div style={{
          display: 'flex',
          alignItems: 'flex-end',
          gap: '0.5rem',
          backgroundColor: '#1e1e1e',
          border: `1px solid ${inputFocused ? '#3b82f6' : '#404040'}`,
          borderRadius: '8px',
          padding: '0.5rem',
          transition: 'border-color 0.2s',
        }}>
          <textarea
            ref={textareaRef}
            value={userInput}
            onChange={(e) => setUserInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && !e.shiftKey) {
                e.preventDefault();
                handleSendMessage();
              }
            }}
            onFocus={() => setInputFocused(true)}
            onBlur={() => setInputFocused(false)}
            placeholder="Orchestrate your video"
            rows={1}
            style={{
              flex: 1,
              minHeight: '24px',
              maxHeight: '120px',
              padding: '0.375rem 0.5rem',
              backgroundColor: 'transparent',
              border: 'none',
              borderRadius: '4px',
              fontSize: '13px',
              color: '#e5e5e5',
              fontFamily: 'inherit',
              outline: 'none',
              resize: 'none',
              lineHeight: '1.5',
              overflowY: 'auto',
            }}
          />
          <button
            onClick={handleSendMessage}
            disabled={isProposing || !userInput.trim()}
            style={{
              width: isProposing ? '20px' : '20px',
              height: '20px',
              padding: 0,
              backgroundColor: '#ffffff',
              color: '#666666',
              border: 'none',
              borderRadius: isProposing ? '4px' : '50%',
              cursor: (isProposing || !userInput.trim()) ? 'not-allowed' : 'pointer',
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              flexShrink: 0,
              opacity: (isProposing || !userInput.trim()) ? 0.5 : 1,
              transition: 'all 0.2s',
            }}
            onMouseEnter={(e) => {
              if (!isProposing && userInput.trim()) {
                e.currentTarget.style.opacity = '0.8';
              }
            }}
            onMouseLeave={(e) => {
              if (!isProposing && userInput.trim()) {
                e.currentTarget.style.opacity = '1';
              }
            }}
          >
            {isProposing ? (
              <div style={{
                width: '10px',
                height: '10px',
                backgroundColor: '#666666',
                borderRadius: '2px',
              }} />
            ) : (
              <svg
                width="12"
                height="12"
                viewBox="0 0 14 14"
                fill="none"
                xmlns="http://www.w3.org/2000/svg"
              >
                <path
                  d="M7 2L7 12M2 7L7 2L12 7"
                  stroke="#666666"
                  strokeWidth="1.5"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                />
              </svg>
            )}
          </button>
        </div>
      </div>
    </div>
    </>
  );
};

