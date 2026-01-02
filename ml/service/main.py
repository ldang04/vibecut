from fastapi import FastAPI, HTTPException
from pydantic import BaseModel
from typing import List, Optional, Dict
import os
import json
from pathlib import Path

# Load environment variables from .env file if it exists
try:
    from dotenv import load_dotenv
    import sys
    
    # Try to load .env from project root (two levels up) or current directory
    env_path = Path(__file__).parent.parent.parent / '.env'
    if not env_path.exists():
        env_path = Path(__file__).parent / '.env'
    
    if env_path.exists():
        load_dotenv(env_path, override=True)
        # Use sys.stderr to ensure it shows up in uvicorn logs
        print(f"[INFO] Loaded .env from {env_path}", file=sys.stderr, flush=True)
        # Verify OPENAI_API_KEY is loaded
        if os.getenv('OPENAI_API_KEY'):
            api_key_preview = os.getenv('OPENAI_API_KEY')[:20] + '...' if len(os.getenv('OPENAI_API_KEY', '')) > 20 else '***'
            print(f"[INFO] OPENAI_API_KEY loaded: {api_key_preview}", file=sys.stderr, flush=True)
        else:
            print(f"[WARNING] .env file found but OPENAI_API_KEY not set", file=sys.stderr, flush=True)
    else:
        # Also try loading from current directory
        load_dotenv(override=True)
        if os.getenv('OPENAI_API_KEY'):
            print(f"[INFO] Loaded .env from current directory", file=sys.stderr, flush=True)
        else:
            print(f"[WARNING] No .env file found and OPENAI_API_KEY not set in environment", file=sys.stderr, flush=True)
except ImportError:
    # python-dotenv not installed, skip .env loading
    print("[WARNING] python-dotenv not installed, .env file will not be loaded", file=sys.stderr, flush=True)
except Exception as e:
    print(f"[ERROR] Failed to load .env file: {e}", file=sys.stderr, flush=True)

app = FastAPI(title="VibeCut ML Service", version="0.1.0")


class HealthResponse(BaseModel):
    ok: bool
    version: str


class WordTimestamp(BaseModel):
    start: float
    end: float
    word: str


class TranscriptSegment(BaseModel):
    start: float
    end: float
    text: str
    words: Optional[List[WordTimestamp]] = None


class TranscribeRequest(BaseModel):
    mediaPath: str


class TranscribeResponse(BaseModel):
    segments: List[TranscriptSegment]


@app.get("/health", response_model=HealthResponse)
async def health():
    return HealthResponse(ok=True, version="0.1.0")


@app.post("/transcribe", response_model=TranscribeResponse)
async def transcribe(request: TranscribeRequest) -> TranscribeResponse:
    """
    Transcribe audio/video file using faster-whisper.
    
    Args:
        request: Contains mediaPath (local file path)
    
    Returns:
        TranscribeResponse with segments containing start, end, text, and optional word timestamps
    """
    media_path = request.mediaPath
    
    # Validate path is local (no network paths)
    if not os.path.isabs(media_path):
        raise HTTPException(status_code=400, detail="Path must be absolute")
    
    if not os.path.exists(media_path):
        raise HTTPException(status_code=404, detail=f"File not found: {media_path}")
    
    try:
        from faster_whisper import WhisperModel
        
        # Initialize Whisper model (small model for V1, can be configured later)
        # This will download the model on first run if not present
        model = WhisperModel("base", device="cpu", compute_type="int8")
        
        # Transcribe
        segments, info = model.transcribe(media_path, beam_size=5, word_timestamps=True)
        
        result_segments = []
        for segment in segments:
            words = []
            if segment.words:
                for word in segment.words:
                    words.append(WordTimestamp(
                        start=word.start,
                        end=word.end,
                        word=word.word
                    ))
            
            result_segments.append(TranscriptSegment(
                start=segment.start,
                end=segment.end,
                text=segment.text.strip(),
                words=words if words else None
            ))
        
        return TranscribeResponse(segments=result_segments)
        
    except ImportError:
        raise HTTPException(
            status_code=500,
            detail="faster-whisper not installed. Run: pip install faster-whisper"
        )
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"Transcription failed: {str(e)}")


class FaceBbox(BaseModel):
    x: float
    y: float
    width: float
    height: float


class VisionSegment(BaseModel):
    start: float
    end: float
    has_face: bool
    face_bbox: Optional[FaceBbox] = None
    blur_score: float
    motion_score: float
    tags: List[str]


class VisionAnalyzeRequest(BaseModel):
    mediaPath: str


class VisionAnalyzeResponse(BaseModel):
    segments: List[VisionSegment]


@app.post("/vision/analyze", response_model=VisionAnalyzeResponse)
async def analyze_vision(request: VisionAnalyzeRequest) -> VisionAnalyzeResponse:
    """
    Analyze video for faces, blur, motion, and basic scene tags.
    
    Args:
        request: Contains mediaPath (local file path)
    
    Returns:
        VisionAnalyzeResponse with segments containing vision analysis results
    """
    media_path = request.mediaPath
    
    # Validate path is local
    if not os.path.isabs(media_path):
        raise HTTPException(status_code=400, detail="Path must be absolute")
    
    if not os.path.exists(media_path):
        raise HTTPException(status_code=404, detail=f"File not found: {media_path}")
    
    try:
        import cv2
        import numpy as np
        
        # Open video
        cap = cv2.VideoCapture(media_path)
        if not cap.isOpened():
            raise HTTPException(status_code=500, detail="Failed to open video file")
        
        fps = cap.get(cv2.CAP_PROP_FPS)
        frame_count = int(cap.get(cv2.CAP_PROP_FRAME_COUNT))
        duration = frame_count / fps if fps > 0 else 0
        
        # Load face cascade
        face_cascade = cv2.CascadeClassifier(cv2.data.haarcascades + 'haarcascade_frontalface_default.xml')
        
        segments = []
        frame_idx = 0
        prev_gray = None
        sample_rate = max(1, int(fps / 2))  # Sample every 0.5 seconds
        
        while True:
            ret, frame = cap.read()
            if not ret:
                break
            
            if frame_idx % sample_rate != 0:
                frame_idx += 1
                continue
            
            # Convert to grayscale for processing
            gray = cv2.cvtColor(frame, cv2.COLOR_BGR2GRAY)
            
            # Face detection
            faces = face_cascade.detectMultiScale(gray, 1.1, 4)
            has_face = len(faces) > 0
            face_bbox = None
            if has_face and len(faces) > 0:
                # Use the first detected face
                x, y, w, h = faces[0]
                face_bbox = FaceBbox(x=float(x), y=float(y), width=float(w), height=float(h))
            
            # Blur detection (Laplacian variance)
            laplacian_var = cv2.Laplacian(gray, cv2.CV_64F).var()
            blur_score = float(laplacian_var)  # Higher = less blur
            
            # Motion estimation (frame difference)
            motion_score = 0.0
            if prev_gray is not None:
                diff = cv2.absdiff(gray, prev_gray)
                motion_score = float(np.mean(diff))
            prev_gray = gray.copy()
            
            # Basic scene tags (heuristics)
            tags = []
            # Brightness-based day/night detection
            mean_brightness = np.mean(gray)
            if mean_brightness > 127:
                tags.append("day")
            else:
                tags.append("night")
            
            # Simple indoor/outdoor heuristic (can be improved)
            # For now, use edge density as proxy
            edges = cv2.Canny(gray, 50, 150)
            edge_density = np.sum(edges > 0) / (frame.shape[0] * frame.shape[1])
            if edge_density > 0.1:
                tags.append("outdoors")
            else:
                tags.append("indoors")
            
            # Calculate time for this frame
            timestamp = frame_idx / fps if fps > 0 else 0.0
            segment_duration = sample_rate / fps if fps > 0 else 0.5
            
            segments.append(VisionSegment(
                start=timestamp,
                end=timestamp + segment_duration,
                has_face=has_face,
                face_bbox=face_bbox,
                blur_score=blur_score,
                motion_score=motion_score,
                tags=tags
            ))
            
            frame_idx += 1
        
        cap.release()
        
        return VisionAnalyzeResponse(segments=segments)
        
    except ImportError:
        raise HTTPException(
            status_code=500,
            detail="opencv-python not installed. Run: pip install opencv-python numpy"
        )
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"Vision analysis failed: {str(e)}")


class EmbeddingRequest(BaseModel):
    text: str


class EmbeddingResponse(BaseModel):
    embedding: List[float]


# Global model cache (singleton pattern)
_text_model = None

def get_text_model():
    """Get or load the sentence-transformers model (singleton pattern)"""
    global _text_model
    if _text_model is None:
        try:
            from sentence_transformers import SentenceTransformer
            _text_model = SentenceTransformer('all-MiniLM-L6-v2')
        except ImportError:
            raise HTTPException(
                status_code=500,
                detail="sentence-transformers not installed. Run: pip install sentence-transformers"
            )
    return _text_model


@app.post("/embeddings/text", response_model=EmbeddingResponse)
async def embeddings_text(request: EmbeddingRequest) -> EmbeddingResponse:
    """
    Generate text embedding using sentence-transformers all-MiniLM-L6-v2.
    Returns 384-dimensional vector.
    
    Args:
        request: Contains text to embed (can be structured or plain text)
    
    Returns:
        EmbeddingResponse with embedding vector
    """
    try:
        model = get_text_model()
        # Encode text with normalization (L2 normalization for cosine similarity)
        embedding = model.encode(request.text, normalize_embeddings=True)
        return EmbeddingResponse(embedding=embedding.tolist())
        
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"Text embedding generation failed: {str(e)}")


# Keep old endpoint for backward compatibility during transition
@app.post("/embeddings/semantic", response_model=EmbeddingResponse)
async def embeddings_semantic(request: EmbeddingRequest) -> EmbeddingResponse:
    """
    DEPRECATED: Use /embeddings/text instead.
    This endpoint is kept for backward compatibility.
    """
    # Delegate to new text endpoint
    return await embeddings_text(request)


class VisionEmbeddingRequest(BaseModel):
    media_path: str
    start_time: float  # Start time in seconds
    end_time: float    # End time in seconds


# Global vision model cache (singleton pattern)
_vision_model = None
_vision_preprocess = None

def get_vision_model():
    """Get or load the CLIP model (singleton pattern)"""
    global _vision_model, _vision_preprocess
    if _vision_model is None:
        try:
            import open_clip
            import torch
            model, _, preprocess = open_clip.create_model_and_transforms(
                'ViT-B-32', pretrained='openai'
            )
            model.eval()
            _vision_model = model
            _vision_preprocess = preprocess
        except ImportError:
            raise HTTPException(
                status_code=500,
                detail="open-clip-torch not installed. Run: pip install open-clip-torch torch"
            )
    return _vision_model, _vision_preprocess


@app.post("/embeddings/vision", response_model=EmbeddingResponse)
async def embeddings_vision(request: VisionEmbeddingRequest) -> EmbeddingResponse:
    """
    Generate vision embedding from keyframe using CLIP ViT-B-32.
    Returns 512-dimensional vector.
    
    Args:
        request: Contains media_path and time range for segment
    
    Returns:
        EmbeddingResponse with embedding vector
    """
    try:
        import cv2
        import torch
        from PIL import Image
        import numpy as np
        
        # Validate path
        if not os.path.exists(request.media_path):
            raise HTTPException(status_code=404, detail=f"File not found: {request.media_path}")
        
        # Extract keyframe (middle frame of segment)
        cap = cv2.VideoCapture(request.media_path)
        if not cap.isOpened():
            raise HTTPException(status_code=500, detail="Failed to open video file")
        
        fps = cap.get(cv2.CAP_PROP_FPS)
        if fps <= 0:
            cap.release()
            raise HTTPException(status_code=500, detail="Invalid video FPS")
        
        # Calculate frame number for middle of segment
        middle_time = (request.start_time + request.end_time) / 2.0
        frame_number = int(middle_time * fps)
        
        cap.set(cv2.CAP_PROP_POS_FRAMES, frame_number)
        ret, frame = cap.read()
        cap.release()
        
        if not ret:
            raise HTTPException(status_code=500, detail="Failed to extract frame")
        
        # Convert BGR to RGB for PIL
        frame_rgb = cv2.cvtColor(frame, cv2.COLOR_BGR2RGB)
        pil_image = Image.fromarray(frame_rgb)
        
        # Get model and preprocess
        model, preprocess = get_vision_model()
        
        # Preprocess image
        image_tensor = preprocess(pil_image).unsqueeze(0)
        
        # Generate embedding
        with torch.no_grad():
            image_features = model.encode_image(image_tensor)
            # Normalize for cosine similarity
            image_features = image_features / image_features.norm(dim=-1, keepdim=True)
            embedding = image_features.squeeze(0).cpu().numpy().tolist()
        
        return EmbeddingResponse(embedding=embedding)
        
    except ImportError as e:
        raise HTTPException(
            status_code=500,
            detail=f"Required library not installed: {str(e)}"
        )
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"Vision embedding generation failed: {str(e)}")


class ProfileFromReferencesRequest(BaseModel):
    referenceVideoPaths: List[str]


class StyleProfileResponse(BaseModel):
    pacing: dict
    caption_templates: List[dict]
    music: dict
    structure: dict


class OrchestratorReasonRequest(BaseModel):
    segments: List[dict]
    style_profile: Optional[dict] = None
    timeline_context: Optional[dict] = None


class OrchestratorReasonResponse(BaseModel):
    explanation: str
    questions: List[str]
    narrative_structure: Optional[str] = None


@app.post("/orchestrator/reason", response_model=OrchestratorReasonResponse)
async def orchestrator_reason(request: OrchestratorReasonRequest) -> OrchestratorReasonResponse:
    """
    Generate structured narrative reasoning. Returns analysis outputs only.
    Daemon translates these into user-facing messages.
    
    Args:
        request: Contains segments, optional style_profile, and optional timeline_context
    
    Returns:
        OrchestratorReasonResponse with structured data only (empty explanation/questions - daemon generates these)
    """
    try:
        # Extract segment information
        segments = request.segments
        num_segments = len(segments)
        
        if num_segments == 0:
            return OrchestratorReasonResponse(
                explanation="",  # Empty - daemon will generate message
                questions=[],     # Empty - daemon will generate
                narrative_structure=None
            )
        
        # Return structured analysis only
        # In a real implementation, this would use an LLM to generate structured reasoning
        # For now, return basic structure
        
        return OrchestratorReasonResponse(
            explanation="",  # Daemon generates friendly copy
            questions=[],     # Daemon generates questions
            narrative_structure="linear"  # Structured data
        )
        
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"Orchestrator reasoning failed: {str(e)}")


class GeneratePlanRequest(BaseModel):
    narrative_structure: str
    beats: List[Dict]
    constraints: Dict
    style_profile_id: Optional[int] = None


class GenerateResponseRequest(BaseModel):
    conversation_history: List[dict]
    project_state: dict
    context: dict
    event_type: str


class GenerateResponseResponse(BaseModel):
    message: str
    suggestions: List[dict]  # [{"label": str, "action": str, "confirm_token": Optional[str]}]
    questions: List[str]
    # Note: next_action and confidence removed - control flow is deterministic


@app.post("/orchestrator/generate_response", response_model=GenerateResponseResponse)
async def generate_response(request: GenerateResponseRequest) -> GenerateResponseResponse:
    """
    Generate intelligent, contextual agent response using LLM.
    
    Args:
        request: Contains conversation_history, project_state, context, event_type
    
    Returns:
        GenerateResponseResponse with LLM-generated message, suggestions, and questions
    """
    try:
        from openai import OpenAI
        
        # Initialize OpenAI client (use environment variable OPENAI_API_KEY)
        api_key = os.getenv('OPENAI_API_KEY')
        if not api_key:
            raise HTTPException(
                status_code=500,
                detail="OPENAI_API_KEY not set. Please set it in your .env file or environment variables."
            )
        client = OpenAI(api_key=api_key)
        
        # Build system prompt
        system_prompt = """You are a helpful, intelligent video editing assistant for VibeCut. You help users create video edits by understanding their creative vision and taking action.

CRITICAL: Read the conversation history carefully. Acknowledge what the user actually said. Reference their specific requests. Be conversational and natural, not robotic.

Your personality:
- Friendly, helpful, and conversational
- Acknowledge the user's specific requests (e.g., "I want to make a vlog of me getting ready" → acknowledge this)
- Show understanding of context (what they asked for, what's been done)
- Be proactive but not pushy
- Vary your responses - don't repeat the same phrases

When responding:
1. Acknowledge what the user said (e.g., "Got it - a vlog of you getting ready!")
2. ACTUALLY DO what you say you'll do - don't just talk about doing it
3. If you say "I'll show you segments" - the system will automatically show them, so describe what you found, not what you'll do
4. Be specific and natural - avoid generic phrases like "candidate segments ready"
5. When segments are found, describe them: "I found 4 moments: [describe each one briefly]"
6. Don't say "I'll pull them up" or "let me show you" - just describe what's there

CRITICAL: If the user asks "what's the plan?" or "describe the plan" or similar:
- Create a NARRATIVE description of the video plot/story, not just technical details
- Use the segment descriptions provided in the context to tell the story
- Describe the flow: "The video starts with [first moment], then cuts to [second moment], followed by [third moment]..."
- Be specific about what's happening in each moment (e.g., "forest pan shot" → "a panning shot through the forest")
- Make it sound like a story, not a technical spec
- Example: "The video opens with a panning shot through the forest, then cuts to a person walking their dog along a path, followed by a close-up of the dog..."

IMPORTANT: When segments are found and returned to you:
- The system has ALREADY retrieved them - you don't need to say "I'll show you"
- ONLY describe segments using the exact descriptions provided in the context
- DO NOT make up or invent descriptions - if a segment says "video segment" or is generic, acknowledge that
- If segment descriptions are generic/placeholder, say: "I found X segments, but they need more analysis for detailed descriptions"
- Only use specific, detailed descriptions that are actually provided - never hallucinate content
- Don't say "I'll pull them up" or "let me show you" - they're already shown, just describe what's actually there

Available actions (suggest these when appropriate):
- "generate_plan": User wants to create an edit plan
- "apply_plan": User wants to apply a plan to the timeline
- "import_clips": User needs to import video clips
- "analyze_clips": Start analyzing clips
- "overwrite_timeline": Overwrite existing timeline
- "create_new_version": Create new timeline version
- "broaden_search": Expand search for segments
- "show_all_moments": Show all available segments
- "show_progress": Show analysis progress
- "cancel": Cancel current operation

Return JSON with: message (natural, conversational text), suggestions (array of {label, action, confirm_token}), questions (if you need clarification).
"""
        
        # Build user prompt with context - make it conversational
        user_prompt_parts = []
        
        # Include conversation history first so LLM understands context
        if request.conversation_history:
            user_prompt_parts.append("Conversation so far:")
            for msg in request.conversation_history[-10:]:  # Last 10 messages
                role = msg.get("role", "unknown")
                content = msg.get("content", "")
                if role == "user":
                    user_prompt_parts.append(f"User: {content}")
                elif role == "assistant":
                    user_prompt_parts.append(f"Assistant: {content}")
        
        # Add current context
        user_prompt_parts.append(f"\nCurrent situation:")
        user_prompt_parts.append(f"- Event: {request.event_type}")
        
        if request.context and request.context.get("user_intent"):
            user_prompt_parts.append(f"- User's latest request: {request.context['user_intent']}")
        
        user_prompt_parts.append(f"- Project has {request.project_state.get('segments_count', 0)} segments analyzed")
        if request.context and request.context.get("candidate_count"):
            candidate_count = request.context.get("candidate_count", 0)
            user_prompt_parts.append(f"- Found {candidate_count} relevant segments for the edit")
            
            # Include segment descriptions if available
            if request.context.get("segment_descriptions"):
                segment_descriptions = request.context.get("segment_descriptions", [])
                if segment_descriptions:
                    user_prompt_parts.append(f"- Segment descriptions (in order):")
                    generic_count = 0
                    for i, desc in enumerate(segment_descriptions[:10], 1):  # First 10
                        if desc.lower() in ["video segment", "video", "segment"] or len(desc.strip()) < 5:
                            generic_count += 1
                            user_prompt_parts.append(f"  {i}. [generic/placeholder - no detailed description available]")
                        else:
                            user_prompt_parts.append(f"  {i}. {desc}")
                    
                    if generic_count > 0:
                        user_prompt_parts.append(f"  CRITICAL: {generic_count} segments have generic descriptions. DO NOT make up descriptions!")
                        user_prompt_parts.append(f"  Only describe segments that have real, specific descriptions above.")
                        user_prompt_parts.append(f"  For generic segments, say something like: 'I found {candidate_count} segments, but some need more analysis to get detailed descriptions.'")
                    else:
                        user_prompt_parts.append(f"  CRITICAL: Describe ONLY these segments using the exact descriptions above!")
                        user_prompt_parts.append(f"  Say 'I found {candidate_count} moments: [describe each using the descriptions above]'")
                        user_prompt_parts.append(f"  DO NOT make up descriptions - only use what's provided above!")
                    user_prompt_parts.append(f"  Don't say 'I'll show you' - they're already shown! Just describe what you found.")
            else:
                user_prompt_parts.append(f"  WARNING: No segment descriptions available. DO NOT make up descriptions!")
                user_prompt_parts.append(f"  Say something like: 'I found {candidate_count} segments, but I need more analysis to describe them in detail.'")
        
        if request.context and request.context.get("goal"):
            goal_obj = request.context.get("goal", {})
            if isinstance(goal_obj, dict):
                user_prompt_parts.append(f"- Active goal: {goal_obj.get('intent', '')} (status: {goal_obj.get('status', '')})")
        
        # Include edit plan details if available
        if request.context and request.context.get("edit_plan"):
            plan_obj = request.context.get("edit_plan", {})
            if isinstance(plan_obj, dict) and plan_obj.get("has_plan"):
                user_prompt_parts.append(f"- Edit plan exists: {plan_obj.get('plan_summary', 'plan ready')}")
                
                # Include segment descriptions for narrative description
                segment_descriptions = plan_obj.get("segment_descriptions", [])
                if segment_descriptions:
                    user_prompt_parts.append(f"- Segment descriptions in order:")
                    for i, desc in enumerate(segment_descriptions, 1):
                        user_prompt_parts.append(f"  {i}. {desc}")
                    user_prompt_parts.append(f"  CRITICAL: If user asks 'what's the plan?' or 'describe the plan', create a narrative description of the video plot using these segment descriptions!")
                    user_prompt_parts.append(f"  Example: 'The video starts with [first segment], then cuts to [second segment], followed by [third segment]...'")
                    user_prompt_parts.append(f"  Be specific about what's happening in each moment, not just technical details!")
        
        user_prompt_parts.append("\nGenerate a natural, conversational response that acknowledges what the user said and explains what's happening or what you're doing.")
        
        user_prompt = "\n".join(user_prompt_parts)
        
        # Call OpenAI API
        try:
            response = client.chat.completions.create(
                model="gpt-4o-mini",  # Use cheaper model for responses
                messages=[
                    {"role": "system", "content": system_prompt},
                    {"role": "user", "content": user_prompt}
                ],
                response_format={"type": "json_object"},
                temperature=0.9,  # Higher temperature for more natural, varied responses
            )
        except Exception as api_error:
            # Check if it's an authentication error
            error_str = str(api_error).lower()
            if "api key" in error_str or "authentication" in error_str or "unauthorized" in error_str:
                print(f"[ERROR] OpenAI API key not set or invalid. Please set OPENAI_API_KEY environment variable.")
                raise HTTPException(
                    status_code=500,
                    detail="OpenAI API key not configured. Please set OPENAI_API_KEY environment variable."
                )
            else:
                raise
        
        # Parse response
        response_text = response.choices[0].message.content
        if not response_text:
            raise HTTPException(status_code=500, detail="OpenAI returned empty response")
        
        response_json = json.loads(response_text)
        
        # Extract and validate suggestions
        suggestions_raw = response_json.get("suggestions", [])
        suggestions = []
        for sug in suggestions_raw:
            if isinstance(sug, dict) and "label" in sug and "action" in sug:
                suggestions.append({
                    "label": sug["label"],
                    "action": sug["action"],
                    "confirm_token": sug.get("confirm_token"),
                })
        
        return GenerateResponseResponse(
            message=response_json.get("message", "I'm here to help!"),
            suggestions=suggestions,
            questions=response_json.get("questions", []),
        )
        
    except ImportError as e:
        # OpenAI library not installed - log and raise
        print(f"[ERROR] OpenAI library not available: {e}")
        raise HTTPException(
            status_code=500, 
            detail="OpenAI library not installed. Please install with: pip install openai"
        )
    except Exception as e:
        # Log the full error for debugging
        import traceback
        print(f"[ERROR] LLM response generation failed: {e}")
        print(traceback.format_exc())
        raise HTTPException(status_code=500, detail=f"Response generation failed: {str(e)}")


class ParseIntentRequest(BaseModel):
    user_message: str
    conversation_history: Optional[List[dict]] = None


class ParseIntentResponse(BaseModel):
    user_intent: str
    target_length_sec: Optional[int] = None
    vibe: Optional[str] = None
    constraints: Optional[dict] = None
    clarifying_questions: List[str] = []


@app.post("/orchestrator/parse_intent", response_model=ParseIntentResponse)
async def parse_intent(request: ParseIntentRequest) -> ParseIntentResponse:
    """
    Parse natural language user message into structured intent.
    
    Args:
        request: Contains user_message and optional conversation_history
    
    Returns:
        ParseIntentResponse with structured intent fields
    """
    try:
        from openai import OpenAI
        
        api_key = os.getenv('OPENAI_API_KEY')
        if not api_key:
            raise HTTPException(
                status_code=500,
                detail="OPENAI_API_KEY not set. Please set it in your .env file or environment variables."
            )
        client = OpenAI(api_key=api_key)
        
        system_prompt = """You are a video editing assistant. Parse the user's natural language message into structured intent.

Extract:
- user_intent: A clear, concise description of what the user wants
- target_length_sec: Target video length in seconds (if mentioned)
- vibe: Editing style/vibe (e.g., "cinematic", "casual", "fast-paced", "cozy")
- constraints: Object with:
  - unused_only: true if user wants only unused clips
  - selected_clips: array of clip IDs if user specified specific clips
  - quality_threshold: minimum quality if mentioned
- clarifying_questions: Array of questions to ask if intent is ambiguous

Return JSON with: user_intent, target_length_sec (optional), vibe (optional), constraints (optional), clarifying_questions (array)."""
        
        user_prompt = f"User message: {request.user_message}"
        if request.conversation_history:
            user_prompt += "\n\nRecent conversation:"
            for msg in request.conversation_history[-5:]:
                role = msg.get("role", "unknown")
                content = msg.get("content", "")
                user_prompt += f"\n{role}: {content}"
        
        response = client.chat.completions.create(
            model="gpt-4o-mini",
            messages=[
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": user_prompt}
            ],
            response_format={"type": "json_object"},
            temperature=0.3,  # Lower temperature for more deterministic parsing
        )
        
        response_text = response.choices[0].message.content
        response_json = json.loads(response_text)
        
        return ParseIntentResponse(
            user_intent=response_json.get("user_intent", request.user_message),
            target_length_sec=response_json.get("target_length_sec"),
            vibe=response_json.get("vibe"),
            constraints=response_json.get("constraints"),
            clarifying_questions=response_json.get("clarifying_questions", []),
        )
        
    except ImportError:
        # Fallback: return basic intent
        return ParseIntentResponse(
            user_intent=request.user_message,
            target_length_sec=None,
            vibe=None,
            constraints=None,
            clarifying_questions=[],
        )
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"Intent parsing failed: {str(e)}")


@app.post("/orchestrator/generate_plan")
async def generate_plan(request: GeneratePlanRequest) -> Dict:
    """
    Generate EditPlan from beats and constraints.
    Returns structured EditPlan with primary_segments, overlays, trims, titles, audio_events.
    
    Args:
        request: Contains narrative_structure, beats (list of beat objects with segment_ids),
                 constraints (target_length, vibe, captions_on, music_on), optional style_profile_id
    
    Returns:
        EditPlan as JSON dict with:
        - primary_segments: List of segment operations (insert, trim)
        - overlays: List of overlay operations
        - trims: List of trim operations
        - titles: List of title operations
        - audio_events: List of audio operations
    """
    try:
        # For v1, generate a simple EditPlan based on beats
        # Each beat becomes a primary segment insertion
        primary_segments = []
        for beat in request.beats:
            segment_ids = beat.get("segment_ids", [])
            target_sec = beat.get("target_sec")
            
            for segment_id in segment_ids:
                primary_segments.append({
                    "operation": "insert",
                    "segment_id": segment_id,
                    "timeline_start_ticks": None,  # Will be computed by daemon based on accumulation
                    "trim_in_offset_ticks": 0,  # No trim at start
                    "trim_out_offset_ticks": 0,  # No trim at end
                    "target_duration_sec": target_sec,
                })
        
        # Generate basic EditPlan structure
        edit_plan = {
            "primary_segments": primary_segments,
            "overlays": [],
            "trims": [],
            "titles": [],
            "audio_events": [],
        }
        
        # Add captions if requested
        if request.constraints.get("captions_on", False):
            # For each primary segment, add a caption overlay
            for segment in primary_segments:
                edit_plan["overlays"].append({
                    "type": "caption",
                    "segment_id": segment["segment_id"],
                    "text": "",  # Will be filled from segment transcript
                    "start_ticks": None,  # Aligned to segment
                    "duration_ticks": None,  # Matches segment duration
                })
        
        return edit_plan
        
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"EditPlan generation failed: {str(e)}")


@app.post("/style/profile_from_references", response_model=StyleProfileResponse)
async def profile_from_references(request: ProfileFromReferencesRequest) -> StyleProfileResponse:
    """
    Analyze reference videos to extract editing style profile.
    
    Args:
        request: Contains list of reference video paths
    
    Returns:
        StyleProfileResponse with extracted style patterns
    """
    # Placeholder implementation - would analyze videos for:
    # - Cut detection and shot length distributions
    # - Caption OCR and layout detection
    # - Music detection and ducking patterns
    # - A-roll/B-roll ratio
    
    return StyleProfileResponse(
        pacing={
            "intro_clip_length_target": 3.0,
            "body_clip_length_target": 5.0,
            "outro_clip_length_target": 4.0,
            "silence_tolerance": 0.5,
            "cut_aggressiveness": 0.7,
        },
        caption_templates=[{
            "placement": {"x": 0.5, "y": 0.9, "safe_area": True},
            "font_family": "Arial",
            "font_weight": "bold",
            "font_size": 48,
            "stroke": True,
            "shadow": True,
        }],
        music={
            "ducking_profile": {"duck_amount": 0.5, "fade_in": 0.2, "fade_out": 0.2},
            "loudness_curve": [],
            "bpm_tendencies": [],
        },
        structure={
            "a_roll_b_roll_ratio": 0.6,
            "intro_duration_target": 10.0,
            "outro_duration_target": 5.0,
        },
    )


if __name__ == "__main__":
    import uvicorn
    uvicorn.run(app, host="127.0.0.1", port=8001)
