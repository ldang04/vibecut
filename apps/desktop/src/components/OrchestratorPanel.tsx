import React, { useState, useRef, useEffect, useCallback } from 'react';
import { usePropose, useGeneratePlan, useApplyPlan } from '../hooks/useOrchestrator';

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
  const [suggestions, setSuggestions] = useState<string[]>([]);
  const [questions, setQuestions] = useState<string[]>([]);
  const [currentMode, setCurrentMode] = useState<'talk' | 'busy' | 'act'>('talk');
  const [showProgress, setShowProgress] = useState(false);
  const [inputFocused, setInputFocused] = useState(false);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const streamingTimeoutRef = useRef<NodeJS.Timeout | null>(null);
  
  const { propose, isLoading: isProposing } = usePropose(projectId);
  const { generatePlan, isLoading: isGenerating } = useGeneratePlan(projectId);
  const { applyPlan, isLoading: isApplying } = useApplyPlan(projectId);

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

      // Handle mode-specific UI
      setCurrentMode(result.mode);
      
      if (result.mode === 'busy') {
        // Show progress indicator
        setShowProgress(true);
        setSuggestions([]);
        setQuestions([]);
        setProposal(null);
      } else if (result.mode === 'talk') {
        // Show suggestions as buttons and questions
        setShowProgress(false);
        setSuggestions(result.suggestions || []);
        setQuestions(result.questions || []);
        setProposal(null);
      } else if (result.mode === 'act' && result.data) {
        // Show proposal with "Generate Plan" button
        setShowProgress(false);
        setSuggestions(result.suggestions || []);
        setQuestions([]);
        setProposal(result.data);
      } else {
        setShowProgress(false);
        setSuggestions([]);
        setQuestions([]);
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

        {/* Show progress indicator for BUSY mode */}
        {showProgress && currentMode === 'busy' && (
          <div style={{
            marginTop: '0.75rem',
            padding: '0.75rem',
            backgroundColor: '#1e1e1e',
            borderRadius: '4px',
            border: '1px solid #404040',
          }}>
            <div style={{ 
              fontSize: '12px',
              color: '#999',
              marginBottom: '0.5rem',
            }}>
              Analyzing...
            </div>
            <div style={{
              width: '100%',
              height: '4px',
              backgroundColor: '#404040',
              borderRadius: '2px',
              overflow: 'hidden',
            }}>
              <div style={{
                width: '60%',
                height: '100%',
                backgroundColor: '#3b82f6',
                animation: 'pulse 1.5s ease-in-out infinite',
              }} />
            </div>
          </div>
        )}

        {/* Show suggestions as buttons */}
        {suggestions.length > 0 && (
          <div style={{
            marginTop: '0.75rem',
            display: 'flex',
            flexDirection: 'column',
            gap: '0.5rem',
          }}>
            {suggestions.map((suggestion, idx) => (
              <button
                key={idx}
                onClick={() => {
                  if (suggestion === 'Import clips') {
                    // Trigger import action (would need to be implemented)
                    console.log('Import clips clicked');
                  } else if (suggestion === 'Analyze clips') {
                    // Trigger analysis (would need to be implemented)
                    console.log('Analyze clips clicked');
                  } else if (suggestion === 'Generate Plan') {
                    handleGeneratePlan();
                  } else if (suggestion === 'Apply Plan') {
                    handleApplyPlan(undefined);
                  } else if (suggestion === 'Overwrite timeline') {
                    handleApplyPlan('overwrite');
                  } else if (suggestion === 'Create new version') {
                    handleApplyPlan('new_version');
                  } else if (suggestion === 'Cancel') {
                    setSuggestions([]);
                    setEditPlan(null);
                  } else {
                    // Generic suggestion click
                    setUserInput(suggestion);
                  }
                }}
                style={{
                  width: '100%',
                  padding: '0.5rem',
                  backgroundColor: '#2a2a2a',
                  color: '#e5e5e5',
                  border: '1px solid #404040',
                  borderRadius: '4px',
                  cursor: 'pointer',
                  fontSize: '13px',
                  textAlign: 'left',
                  transition: 'background-color 0.2s',
                }}
                onMouseEnter={(e) => {
                  e.currentTarget.style.backgroundColor = '#353535';
                }}
                onMouseLeave={(e) => {
                  e.currentTarget.style.backgroundColor = '#2a2a2a';
                }}
              >
                {suggestion}
              </button>
            ))}
          </div>
        )}

        {/* Show questions */}
        {questions.length > 0 && (
          <div style={{
            marginTop: '0.75rem',
            padding: '0.75rem',
            backgroundColor: '#1e1e1e',
            borderRadius: '4px',
            border: '1px solid #404040',
          }}>
            <div style={{ 
              fontSize: '12px',
              color: '#999',
              marginBottom: '0.5rem',
            }}>
              Questions:
            </div>
            {questions.map((question, idx) => (
              <div key={idx} style={{
                fontSize: '12px',
                color: '#ccc',
                marginBottom: '0.25rem',
                paddingLeft: '0.5rem',
              }}>
                • {question}
              </div>
            ))}
          </div>
        )}

        {/* Show proposal for ACT mode */}
        {proposal && proposal.candidate_segments && (
          <div style={{
            marginTop: '0.75rem',
            padding: '0.75rem',
            backgroundColor: '#1e1e1e',
            borderRadius: '4px',
            border: '1px solid #404040',
          }}>
            <div style={{ 
              fontWeight: 600, 
              marginBottom: '0.5rem',
              fontSize: '13px',
              color: '#e5e5e5',
            }}>
              Proposal
            </div>
            <div style={{ 
              fontSize: '12px', 
              marginBottom: '0.75rem',
              color: '#999',
            }}>
              Found {proposal.candidate_segments.length} candidate segments
            </div>
            {!editPlan && (
              <button
                onClick={handleGeneratePlan}
                disabled={isGenerating}
                style={{
                  width: '100%',
                  padding: '0.5rem',
                  backgroundColor: isGenerating ? '#505050' : '#3b82f6',
                  color: '#ffffff',
                  border: 'none',
                  borderRadius: '4px',
                  cursor: isGenerating ? 'not-allowed' : 'pointer',
                  fontSize: '13px',
                  fontWeight: 500,
                  transition: 'background-color 0.2s',
                }}
                onMouseEnter={(e) => {
                  if (!isGenerating) {
                    e.currentTarget.style.backgroundColor = '#2563eb';
                  }
                }}
                onMouseLeave={(e) => {
                  if (!isGenerating) {
                    e.currentTarget.style.backgroundColor = '#3b82f6';
                  }
                }}
              >
                {isGenerating ? 'Generating...' : 'Generate Plan'}
              </button>
            )}
          </div>
        )}

        {editPlan && (
          <div style={{
            marginTop: '0.75rem',
            padding: '0.75rem',
            backgroundColor: '#1e1e1e',
            borderRadius: '4px',
            border: '1px solid #404040',
          }}>
            <div style={{ 
              fontWeight: 600, 
              marginBottom: '0.75rem',
              fontSize: '13px',
              color: '#e5e5e5',
            }}>
              Edit Plan Ready
            </div>
            <button
              onClick={() => handleApplyPlan(undefined)}
              disabled={isApplying}
              style={{
                width: '100%',
                padding: '0.5rem',
                backgroundColor: isApplying ? '#505050' : '#10b981',
                color: '#ffffff',
                border: 'none',
                borderRadius: '4px',
                cursor: isApplying ? 'not-allowed' : 'pointer',
                fontSize: '13px',
                fontWeight: 500,
                transition: 'background-color 0.2s',
              }}
              onMouseEnter={(e) => {
                if (!isApplying) {
                  e.currentTarget.style.backgroundColor = '#059669';
                }
              }}
              onMouseLeave={(e) => {
                if (!isApplying) {
                  e.currentTarget.style.backgroundColor = '#10b981';
                }
              }}
            >
              {isApplying ? 'Applying...' : 'Apply Plan'}
            </button>
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

