# VibeCut Architecture Documentation

This document provides a comprehensive overview of how VibeCut works, covering all components from frontend to backend, data flow, algorithms, and system interactions. This is intended as context for LLMs working on the codebase.

## Table of Contents

1. [System Overview](#system-overview)
2. [Architecture Components](#architecture-components)
3. [Database Schema](#database-schema)
4. [Job Processing System](#job-processing-system)
5. [Embedding System](#embedding-system)
6. [Orchestrator System](#orchestrator-system)
7. [Timeline Engine](#timeline-engine)
8. [API Endpoints](#api-endpoints)
9. [Data Flow](#data-flow)
10. [Frontend Architecture](#frontend-architecture)
11. [Key Algorithms](#key-algorithms)

---

## System Overview

VibeCut is a local-only AI video editor that creates professional video edits from raw footage using AI analysis and style profiles. The system consists of three main components:

1. **Electron Desktop App** (`apps/desktop`): React + TypeScript UI
2. **Rust Daemon** (`crates/daemon`): HTTP server handling project management, job orchestration, FFmpeg operations
3. **Python ML Service** (`ml/service`): FastAPI service for transcription, vision analysis, and embeddings

### Communication Flow

```
Electron App (port 5173) 
    ↓ HTTP
Rust Daemon (port 7777)
    ↓ HTTP
Python ML Service (port 8001)
```

The Electron app communicates with the Rust daemon via HTTP. The Rust daemon communicates with the Python ML service via HTTP. All communication is local-only (127.0.0.1).

---

## Architecture Components

### 1. Electron Desktop App (`apps/desktop`)

**Technology Stack:**
- React 18 with TypeScript
- Vite 7.3.0 (requires Node.js 20.19+ or 22.12+)
- Electron for desktop windowing
- Custom hooks for daemon communication

**Key Components:**

#### `Editor.tsx` (Main Component)
- **Purpose**: Orchestrates all sub-components and manages global state
- **State Management**:
  - Timeline data (tracks, clips, captions, music)
  - Selected clip and video playback state
  - Playhead position and current time
  - Active jobs (upload and analysis)
  - Timeline history for undo/redo
  - UI state (tools, sidebar collapse, media tabs)
- **Key Features**:
  - Timeline playback with automatic clip transitions
  - Drag-and-drop from media library to timeline
  - Undo/redo support via history stack
  - Job status polling and UI updates
  - Project switching with state cleanup

#### `Timeline.tsx`
- **Purpose**: Renders the timeline UI with tracks and clips
- **Features**:
  - Visual representation of timeline with tracks (video, audio, caption)
  - Clip rendering with in/out points
  - Playhead visualization
  - Click-to-select and drag-to-move clips
  - Time ruler with tick marks
  - Zoom controls

#### `MediaLibrary.tsx`
- **Purpose**: Displays media assets (raw footage, references, text templates, audio)
- **Features**:
  - Tabbed interface (raw, references, text, audio)
  - Thumbnail generation and display
  - Drag-and-drop to timeline
  - Asset metadata display
  - Import functionality

#### `OrchestratorPanel.tsx`
- **Purpose**: AI assistant chat interface for conversational editing
- **Features**:
  - Chat UI with message history
  - Streaming text display (character-by-character)
  - Mode-aware UI (TALK, BUSY, ACT)
  - Suggestion buttons and questions
  - Proposal display and plan generation
  - Plan application with confirmation

#### `Viewer.tsx`
- **Purpose**: Video playback component
- **Features**:
  - HTML5 video element
  - Play/pause controls
  - Time display
  - Source time range clipping
  - Automatic clip transitions during timeline playback

#### `Toolbar.tsx`
- **Purpose**: Top toolbar with project info and global actions
- **Features**:
  - Project name and selector
  - Upload/Analyzing status indicators
  - Export button
  - Create project button

**Hooks:**

#### `useDaemon.ts`
- **Purpose**: Generic hook for making HTTP requests to the daemon
- **Features**:
  - Automatic loading/error state management
  - GET/POST/PUT/DELETE support
  - JSON request/response handling
  - Error handling with 404 suppression
  - Endpoint change detection and state reset

#### `useOrchestrator.ts`
- **Purpose**: Specialized hooks for orchestrator API endpoints
- **Hooks**:
  - `usePropose`: Propose candidate segments
  - `useGeneratePlan`: Generate EditPlan from beats
  - `useApplyPlan`: Apply EditPlan to timeline

**Electron Integration:**

#### `main/electron.ts`
- **Purpose**: Electron main process
- **Features**:
  - Window creation and management
  - Auto-spawns Rust daemon on startup
  - IPC handlers for window controls
  - DevTools in development mode

#### `main/preload.ts`
- **Purpose**: Preload script for secure IPC
- **Features**:
  - Exposes safe IPC methods to renderer
  - Type-safe Electron API access

### 2. Rust Daemon (`crates/daemon`)

**Technology Stack:**
- Rust with Tokio async runtime
- Axum web framework
- SQLite database (rusqlite)
- reqwest for HTTP client
- FFmpeg wrapper for video operations

**Key Modules:**

#### `main.rs`
- **Purpose**: Entry point and server initialization
- **Initialization**:
  1. Sets up tracing/logging
  2. Creates SQLite database at `.cache/vibecut.db`
  3. Initializes `Database` and `JobManager`
  4. Spawns `JobProcessor` in background task
  5. Sets up Axum router with CORS
  6. Binds to `127.0.0.1:7777`

#### `db/mod.rs`
- **Purpose**: Database abstraction layer
- **Features**:
  - SQLite connection management with mutex
  - Schema initialization and migrations
  - CRUD operations for all entities
  - Helper functions for segment time calculations
  - Prerequisite checking for job gating

#### `jobs/mod.rs`
- **Purpose**: Job type definitions and job manager
- **Job Types**:
  - `ImportRaw`: Import raw footage
  - `TranscribeAsset`: Transcribe audio
  - `AnalyzeVisionAsset`: Analyze video frames
  - `BuildSegments`: Create segments from video
  - `EnrichSegmentsFromTranscript`: Enrich segments with transcript data
  - `EnrichSegmentsFromVision`: Enrich segments with vision data
  - `ComputeSegmentMetadata`: Compute summary, keywords, quality
  - `EmbedSegments`: Generate embeddings
  - `GenerateProxy`: Generate proxy video
  - `Export`: Export final video

#### `jobs/processor.rs`
- **Purpose**: Background job processor
- **Features**:
  - Polls for pending jobs every 1-2 seconds
  - Checks prerequisites before running jobs
  - Routes jobs to appropriate handlers
  - Updates job status (Pending → Running → Completed/Failed)
  - Handles errors gracefully

#### `jobs/build_segments.rs`
- **Purpose**: Create segments from video assets
- **Algorithm**:
  1. Uses FFmpeg to probe video duration
  2. Creates segments based on fixed chunking (4-6 seconds) or shot boundaries
  3. Stores segments with `src_in_ticks` and `src_out_ticks`
  4. Marks job as completed

#### `jobs/transcribe.rs`
- **Purpose**: Transcribe audio using ML service
- **Flow**:
  1. Calls ML service `/transcribe` endpoint
  2. Stores transcript JSON in `asset_transcripts` table
  3. Updates `transcript_ready_at` timestamp
  4. Queues `EnrichSegmentsFromTranscript` job

#### `jobs/vision.rs`
- **Purpose**: Analyze video frames using ML service
- **Flow**:
  1. Calls ML service `/vision/analyze` endpoint
  2. Stores vision JSON in `asset_vision` table
  3. Updates `vision_ready_at` timestamp
  4. Queues `EnrichSegmentsFromVision` job

#### `jobs/enrichment.rs`
- **Purpose**: Enrich segments with transcript and vision data
- **Flow**:
  - `EnrichSegmentsFromTranscript`: Aligns transcript segments to time windows
  - `EnrichSegmentsFromVision`: Attaches vision tags and subject detection to segments

#### `jobs/metadata.rs`
- **Purpose**: Compute segment metadata (summary, keywords, quality)
- **Algorithm**:
  - Generates deterministic summaries from transcript or vision tags
  - Extracts keywords from transcript
  - Computes quality scores (blur, shake, exposure)

#### `jobs/embeddings.rs`
- **Purpose**: Generate embeddings for segments
- **Flow**:
  1. For each segment:
     - Constructs structured semantic text (`spoken: ...`, `summary: ...`, `keywords: ...`)
     - Calls ML service `/embeddings/text` (384-dim, all-MiniLM-L6-v2)
     - Calls ML service `/embeddings/vision` (512-dim, CLIP ViT-B-32)
     - Computes fusion embedding (weighted combination: 0.6 text + 0.4 vision)
     - Stores all three embeddings in database (idempotent)
  2. Updates `embeddings_ready_at` timestamp

#### `embeddings/mod.rs`
- **Purpose**: Embedding similarity search
- **Features**:
  - Cosine similarity computation
  - Filtering by project, raw vs reference
  - Support for text, vision, and fusion embeddings
  - Dimension mismatch handling

#### `api/orchestrator.rs`
- **Purpose**: AI orchestrator endpoints
- **Endpoints**:
  - `POST /projects/:id/orchestrator/propose`: Propose candidate segments
  - `POST /projects/:id/orchestrator/plan`: Generate EditPlan
  - `POST /projects/:id/orchestrator/apply`: Apply EditPlan to timeline

**Agent Modes:**
- `TalkConfirm`: Destructive action needs confirmation
- `TalkImport`: No media assets
- `TalkAnalyze`: No segments
- `TalkClarify`: Ambiguous intent
- `Busy`: Jobs running or coverage incomplete
- `Act`: Ready to execute

**Precondition Checking:**
- Counts media assets, segments, embeddings
- Computes embedding coverage (segments with embeddings / total segments)
- Checks for running jobs
- Determines mode based on state

#### `llm/mod.rs`
- **Purpose**: Wrapper for ML service LLM endpoints
- **Functions**:
  - `embed_text`: Generate text embedding
  - `reason_narrative`: Call `/orchestrator/reason` for narrative reasoning
  - `generate_edit_plan`: Call `/orchestrator/generate_plan` for EditPlan

#### `api/media.rs`
- **Purpose**: Media asset management
- **Endpoints**:
  - `POST /projects/:id/import_raw`: Import raw footage
  - `POST /projects/:id/import_reference`: Import reference footage
  - `GET /projects/:id/media/:asset_id/proxy`: Stream proxy video
  - `GET /projects/:id/media/:asset_id/thumbnail`: Get thumbnail

#### `api/timeline.rs`
- **Purpose**: Timeline management
- **Endpoints**:
  - `GET /projects/:id/timeline`: Get timeline JSON
  - `POST /projects/:id/timeline/apply`: Apply timeline operations

#### `api/projects.rs`
- **Purpose**: Project management
- **Endpoints**:
  - `POST /api/projects`: Create project
  - `GET /api/projects/:id`: Get project

#### `api/jobs.rs`
- **Purpose**: Job status and control
- **Endpoints**:
  - `GET /api/jobs/:id`: Get job status
  - `POST /api/jobs/:id/cancel`: Cancel job

### 3. Python ML Service (`ml/service`)

**Technology Stack:**
- FastAPI
- sentence-transformers (all-MiniLM-L6-v2)
- open-clip-torch (CLIP ViT-B-32)
- faster-whisper (Whisper base)
- OpenCV (cv2) for video processing
- Pillow (PIL) for image processing
- PyTorch for model inference

**Endpoints:**

#### `/health`
- Returns service health status

#### `/transcribe`
- **Input**: `{ "mediaPath": "/path/to/video" }`
- **Output**: `{ "segments": [...] }` with start, end, text, words
- **Model**: Whisper base (CPU, int8)
- **Features**: Word-level timestamps

#### `/vision/analyze`
- **Input**: `{ "mediaPath": "/path/to/video" }`
- **Output**: `{ "shots": [...], "faces": [...], "scenes": [...] }`
- **Features**: Shot detection, face detection, scene classification

#### `/embeddings/text`
- **Input**: `{ "text": "spoken: ...\nsummary: ...\nkeywords: ..." }`
- **Output**: `{ "embedding": [384 floats] }`
- **Model**: sentence-transformers all-MiniLM-L6-v2
- **Features**: L2 normalization for cosine similarity

#### `/embeddings/vision`
- **Input**: `{ "media_path": "...", "start_time": 0.0, "end_time": 5.0 }`
- **Output**: `{ "embedding": [512 floats] }`
- **Model**: CLIP ViT-B-32 (OpenAI pretrained)
- **Algorithm**:
  1. Extracts keyframe (middle frame of segment)
  2. Preprocesses with CLIP transforms
  3. Encodes with CLIP vision encoder
  4. Returns normalized embedding

#### `/embeddings/semantic` (DEPRECATED)
- Delegates to `/embeddings/text` for backward compatibility

#### `/orchestrator/reason`
- **Input**: `{ "segments": [...], "style_profile": {...}, "timeline_context": {...} }`
- **Output**: `{ "explanation": "", "questions": [], "narrative_structure": "linear" }`
- **Note**: Returns structured data only; daemon generates user-facing messages

#### `/orchestrator/generate_plan`
- **Input**: `{ "narrative_structure": "...", "beats": [...], "constraints": {...}, "style_profile_id": ... }`
- **Output**: `{ "primary_segments": [...], "overlays": [...], "trims": [...], "titles": [...], "audio_events": [...] }`
- **Algorithm**:
  - Converts beats to primary segment insertions
  - Adds caption overlays if `captions_on` is true
  - Returns structured EditPlan

#### `/style/profile_from_references`
- **Input**: `{ "referenceVideoPaths": [...] }`
- **Output**: `{ "pacing": {...}, "caption_templates": [...], "music": {...}, "structure": {...} }`
- **Note**: Currently returns placeholder data

---

## Database Schema

The database is SQLite located at `.cache/vibecut.db`. All timestamps are stored as RFC3339 strings.

### Core Tables

#### `projects`
- `id` (INTEGER PRIMARY KEY)
- `name` (TEXT NOT NULL)
- `created_at` (TEXT NOT NULL)
- `cache_dir` (TEXT NOT NULL)
- `style_profile_id` (INTEGER, FOREIGN KEY)

#### `media_assets`
- `id` (INTEGER PRIMARY KEY)
- `project_id` (INTEGER NOT NULL, FOREIGN KEY)
- `path` (TEXT NOT NULL)
- `checksum` (TEXT)
- `duration_ticks` (INTEGER NOT NULL)
- `fps_num`, `fps_den` (INTEGER NOT NULL)
- `width`, `height` (INTEGER NOT NULL)
- `has_audio` (INTEGER NOT NULL)
- `is_reference` (INTEGER NOT NULL DEFAULT 0)
- `thumbnail_dir` (TEXT)
- `segments_built_at` (TEXT)
- `transcript_ready_at` (TEXT)
- `vision_ready_at` (TEXT)
- `metadata_ready_at` (TEXT)
- `embeddings_ready_at` (TEXT)
- UNIQUE(project_id, path)

**Analysis State Tracking:**
The timestamps (`segments_built_at`, `transcript_ready_at`, etc.) are used for job gating. Jobs check these timestamps to determine if prerequisites are met.

#### `segments`
- `id` (INTEGER PRIMARY KEY)
- `media_asset_id` (INTEGER NOT NULL, FOREIGN KEY)
- `project_id` (INTEGER NOT NULL, FOREIGN KEY)
- `start_ticks` (INTEGER NOT NULL) - Legacy, use `src_in_ticks`
- `end_ticks` (INTEGER NOT NULL) - Legacy, use `src_out_ticks`
- `src_in_ticks` (INTEGER) - Source in point (stable after creation)
- `src_out_ticks` (INTEGER) - Source out point (stable after creation)
- `segment_kind` (TEXT) - e.g., "shot", "scene"
- `summary_text` (TEXT) - Deterministic summary
- `keywords_json` (TEXT) - JSON array of keywords
- `quality_json` (TEXT) - Quality scores (blur, shake, exposure)
- `subject_json` (TEXT) - Subject presence intervals
- `scene_json` (TEXT) - Scene tags (indoor/outdoor, day/night)
- `capture_time` (TEXT) - Original recording time
- `transcript` (TEXT) - Full transcript text
- `speaker` (TEXT) - Speaker identification
- `scores_json` (TEXT) - Additional scores
- `tags_json` (TEXT) - Additional tags

**Segment Immutability:**
- `id`, `media_asset_id`, `src_in_ticks`, `src_out_ticks` are stable after creation
- Metadata fields (`summary_text`, `keywords_json`, etc.) can be enriched incrementally

#### `embeddings`
- `id` (INTEGER PRIMARY KEY)
- `segment_id` (INTEGER NOT NULL, FOREIGN KEY)
- `embedding_type` (TEXT NOT NULL) - "text", "vision", or "fusion"
- `model_name` (TEXT NOT NULL) - e.g., "all-MiniLM-L6-v2", "clip-vit-b-32"
- `model_version` (TEXT) - e.g., "1"
- `vector_blob` (BLOB NOT NULL) - Embedding vector as f32 bytes (little-endian)
- `semantic_text` (TEXT) - Original text used for embedding
- UNIQUE(segment_id, embedding_type, model_name)

**Embedding Storage:**
- Vectors are stored as BLOB: each f32 is 4 bytes, little-endian
- Example: 384-dim text embedding = 1536 bytes

#### `style_profiles`
- `id` (INTEGER PRIMARY KEY)
- `name` (TEXT NOT NULL)
- `project_id` (INTEGER, FOREIGN KEY)
- `reference_asset_ids_json` (TEXT) - JSON array of asset IDs
- `json_blob` (TEXT NOT NULL) - Full style profile JSON
- `created_at` (TEXT NOT NULL)

#### `timeline_projects`
- `id` (INTEGER PRIMARY KEY)
- `project_id` (INTEGER NOT NULL, FOREIGN KEY)
- `json_blob` (TEXT NOT NULL) - Timeline JSON
- `created_at` (TEXT NOT NULL)
- `updated_at` (TEXT NOT NULL)

#### `jobs`
- `id` (INTEGER PRIMARY KEY)
- `type` (TEXT NOT NULL) - JSON-serialized JobType enum
- `status` (TEXT NOT NULL) - JSON-serialized JobStatus enum
- `progress` (REAL NOT NULL) - 0.0 to 1.0
- `payload_json` (TEXT) - JSON payload
- `created_at` (TEXT NOT NULL)
- `updated_at` (TEXT NOT NULL)

**Job Status Values:**
- `"Pending"`: Waiting to run
- `"Running"`: Currently executing
- `"Completed"`: Finished successfully
- `"Failed"`: Error occurred
- `"Cancelled"`: User cancelled

#### `asset_transcripts`
- `asset_id` (INTEGER PRIMARY KEY, FOREIGN KEY)
- `transcript_json` (TEXT NOT NULL) - Full transcript JSON from ML service

#### `asset_vision`
- `asset_id` (INTEGER PRIMARY KEY, FOREIGN KEY)
- `vision_json` (TEXT NOT NULL) - Full vision analysis JSON from ML service

#### `orchestrator_messages`
- `id` (INTEGER PRIMARY KEY)
- `project_id` (INTEGER NOT NULL, FOREIGN KEY)
- `role` (TEXT NOT NULL) - "user" or "assistant"
- `content` (TEXT NOT NULL)
- `created_at` (TEXT NOT NULL)

#### `orchestrator_proposals`
- `id` (INTEGER PRIMARY KEY)
- `project_id` (INTEGER NOT NULL, FOREIGN KEY)
- `proposal_json` (TEXT NOT NULL)
- `created_at` (TEXT NOT NULL)

#### `orchestrator_applies`
- `id` (INTEGER PRIMARY KEY)
- `project_id` (INTEGER NOT NULL, FOREIGN KEY)
- `edit_plan_json` (TEXT NOT NULL)
- `created_at` (TEXT NOT NULL)

#### `proxies`
- `id` (INTEGER PRIMARY KEY)
- `media_asset_id` (INTEGER NOT NULL, FOREIGN KEY)
- `path` (TEXT NOT NULL)
- `codec` (TEXT NOT NULL)
- `width`, `height` (INTEGER NOT NULL)

#### `edit_logs`
- `id` (INTEGER PRIMARY KEY)
- `project_id` (INTEGER NOT NULL, FOREIGN KEY)
- `diff_json` (TEXT NOT NULL)
- `created_at` (TEXT NOT NULL)

---

## Job Processing System

### Job Lifecycle

1. **Creation**: Job is created with status `Pending` and stored in database
2. **Prerequisite Check**: `JobProcessor` checks if prerequisites are met
3. **Execution**: Job status updated to `Running`, handler function called
4. **Completion**: Job status updated to `Completed` or `Failed`

### Job Gating

Jobs are gated by analysis state timestamps on `media_assets`:

- `BuildSegments`: No prerequisites (runs immediately)
- `TranscribeAsset`: No prerequisites (runs immediately)
- `AnalyzeVisionAsset`: No prerequisites (runs immediately)
- `EnrichSegmentsFromTranscript`: Requires `segments_built_at` AND `transcript_ready_at`
- `EnrichSegmentsFromVision`: Requires `segments_built_at` AND `vision_ready_at`
- `ComputeSegmentMetadata`: Requires `segments_built_at`
- `EmbedSegments`: Requires `metadata_ready_at`

### Job Processing Flow

```
ImportRaw
  ↓
Queue: TranscribeAsset, AnalyzeVisionAsset, BuildSegments
  ↓
When BuildSegments completes → segments_built_at set
  ↓
When TranscribeAsset completes → transcript_ready_at set → Queue EnrichSegmentsFromTranscript
When AnalyzeVisionAsset completes → vision_ready_at set → Queue EnrichSegmentsFromVision
  ↓
When both enrichments complete → Queue ComputeSegmentMetadata
  ↓
When ComputeSegmentMetadata completes → metadata_ready_at set → Queue EmbedSegments
  ↓
When EmbedSegments completes → embeddings_ready_at set
```

### Idempotency

- Embedding jobs check for existing embeddings before generating (UNIQUE constraint prevents duplicates)
- Other jobs are not idempotent (will re-run if queued again)

---

## Embedding System

### Embedding Types

1. **Text Embedding** (Semantic)
   - **Model**: sentence-transformers all-MiniLM-L6-v2
   - **Dimensions**: 384
   - **Input**: Structured text (`spoken: ...`, `summary: ...`, `keywords: ...`)
   - **Output**: Normalized 384-dim vector
   - **Use Case**: Narrative reasoning, topic matching, intent matching

2. **Vision Embedding** (Aesthetic)
   - **Model**: CLIP ViT-B-32 (OpenAI pretrained)
   - **Dimensions**: 512
   - **Input**: Keyframe (middle frame of segment)
   - **Output**: Normalized 512-dim vector
   - **Use Case**: Visual similarity, style matching, shot similarity

3. **Fusion Embedding** (Multimodal)
   - **Algorithm**: Weighted combination of normalized text and vision embeddings
   - **Weights**: 0.6 text + 0.4 vision
   - **Dimensions**: min(text_dim, vision_dim) = 384
   - **Use Case**: Combined semantic + aesthetic matching

### Embedding Generation

**Text Embedding:**
1. Construct structured text from segment metadata
2. Call ML service `/embeddings/text`
3. Store as BLOB (f32 array, little-endian)

**Vision Embedding:**
1. Extract keyframe (middle frame) from segment time range
2. Call ML service `/embeddings/vision` with media_path and time range
3. Store as BLOB

**Fusion Embedding:**
1. Load text and vision embeddings from database
2. Normalize both
3. Trim to minimum dimension
4. Weighted combination: `fusion = normalize(0.6 * text + 0.4 * vision)`
5. Store as BLOB

### Similarity Search

**Algorithm:**
1. Load all embeddings of specified type and model
2. Compute cosine similarity: `cos(θ) = (A · B) / (||A|| * ||B||)`
3. Sort by similarity (descending)
4. Return top N results

**Filtering:**
- By project: `WHERE s.project_id = ?`
- Raw vs reference: `WHERE (m.is_reference IS NULL OR m.is_reference = 0)`
- Reference only: `WHERE m.is_reference = 1`

---

## Orchestrator System

### Overview

The orchestrator is an AI assistant that helps users create video edits through conversational interaction. It operates in three layers:

1. **Retrieval**: Fast, local embedding similarity search
2. **Narrative Reasoning**: LLM-based reasoning (structured outputs only)
3. **EditPlan Synthesis**: LLM-based EditPlan generation

### Agent Modes

The orchestrator has three main modes:

1. **TALK**: Missing inputs, ambiguous intent, or needs confirmation
   - `TalkConfirm`: Destructive action needs confirmation
   - `TalkImport`: No media assets
   - `TalkAnalyze`: No segments
   - `TalkClarify`: Ambiguous intent

2. **BUSY**: Preconditions in-progress
   - Jobs running
   - Embedding coverage < 80%

3. **ACT**: Ready to execute
   - Preconditions met
   - Clear intent
   - Safe action

### Mode Determination Logic

```rust
if destructive && !confirmed → TalkConfirm
if media_assets_count == 0 → TalkImport
if segments_count == 0 → TalkAnalyze
if jobs_running_count > 0 || embedding_coverage < 0.8 → Busy
if intent_ambiguous → TalkClarify
else → Act
```

### Precondition Checking

**ProjectState:**
- `media_assets_count`: Count of raw media assets
- `segments_count`: Total segments
- `segments_with_text_embeddings`: Segments with text embeddings
- `segments_with_vision_embeddings`: Segments with vision embeddings
- `embedding_coverage`: `segments_with_text_embeddings / segments_count`
- `jobs_running_count`: Count of running/pending analysis jobs
- `jobs_failed_count`: Count of failed jobs

**Embedding Coverage Calculation:**
```rust
embedding_coverage = segments_with_text_embeddings / segments_count
// Must filter by model_name: 'all-MiniLM-L6-v2'
```

### Propose Endpoint

**Flow:**
1. Check preconditions → determine mode
2. If TALK or BUSY: Return friendly message with suggestions
3. If ACT:
   - Embed user intent text
   - Perform similarity search (text or fusion embeddings)
   - Filter by project, raw segments only
   - Call ML service `/orchestrator/reason` for narrative reasoning
   - Return candidate segments with narrative structure

**Response Format:**
```json
{
  "mode": "act",
  "message": "I found 12 good moments...",
  "suggestions": ["Generate Plan", "Show all"],
  "questions": [],
  "data": {
    "candidate_segments": [...],
    "narrative_structure": "linear"
  }
}
```

### Plan Endpoint

**Flow:**
1. Check preconditions
2. Convert beats to JSON
3. Call ML service `/orchestrator/generate_plan`
4. Return EditPlan

**EditPlan Structure:**
```json
{
  "primary_segments": [
    {
      "operation": "insert",
      "segment_id": 123,
      "trim_in_offset_ticks": 0,
      "trim_out_offset_ticks": 0,
      "target_duration_sec": 5.0
    }
  ],
  "overlays": [...],
  "trims": [...],
  "titles": [...],
  "audio_events": [...]
}
```

### Apply Endpoint

**Flow:**
1. Check if applying is destructive (timeline has existing clips)
2. If destructive and no `confirm_token`: Return TALK mode with confirmation
3. Otherwise:
   - Parse EditPlan
   - For each primary segment:
     - Get segment from database
     - Apply trim offsets
     - Create clip on timeline
     - Accumulate timeline position
   - Store updated timeline
   - Return success

**Destructive Action Handling:**
- If timeline has clips and no `confirm_token`: Return `TalkConfirm` mode
- User clicks "Replace timeline" → Frontend calls `/apply?confirm=overwrite`
- Daemon verifies token and proceeds

---

## Timeline Engine

### Timeline Structure

**Timeline JSON:**
```json
{
  "settings": {
    "fps": 30.0,
    "resolution": { "width": 1920, "height": 1080 },
    "sample_rate": 48000,
    "ticks_per_second": 48000
  },
  "tracks": [
    {
      "id": 0,
      "kind": "Video",
      "clips": [
        {
          "id": "uuid",
          "asset_id": 123,
          "in_ticks": 0,
          "out_ticks": 240000,
          "timeline_start_ticks": 0,
          "speed": 1.0,
          "track_id": 0
        }
      ]
    }
  ],
  "captions": [...],
  "music": [...],
  "markers": [...]
}
```

### Time System

**Ticks:**
- Canonical time unit: 48,000 ticks per second
- All time values stored in ticks
- Conversion: `seconds = ticks / 48000`

**Segment Time:**
- `src_in_ticks`: Source in point (stable)
- `src_out_ticks`: Source out point (stable)
- `start_ticks`: Legacy field (use `src_in_ticks`)
- `end_ticks`: Legacy field (use `src_out_ticks`)

**Timeline Time:**
- `timeline_start_ticks`: Position on timeline
- `in_ticks`, `out_ticks`: Source range used in clip

### Timeline Operations

**Apply EditPlan:**
1. Parse EditPlan JSON
2. For each primary segment:
   - Get segment from database
   - Calculate final in/out: `final_in = src_in + trim_in`, `final_out = src_out - trim_out`
   - Create clip with `timeline_start_ticks = current_time`
   - Update `current_time += (final_out - final_in)`
3. Store timeline JSON

**Undo/Redo:**
- Frontend maintains history stack
- Each operation pushes to history
- Undo pops from history, redo pushes to redo stack

---

## API Endpoints

### Daemon API (port 7777)

#### Health
- `GET /health` → `{ "ok": true, "version": "0.1.0" }`

#### Projects
- `POST /api/projects` → Create project
- `GET /api/projects/:id` → Get project

#### Media
- `POST /api/projects/:id/import_raw` → Import raw footage
- `POST /api/projects/:id/import_reference` → Import reference footage
- `GET /api/projects/:id/media/:asset_id/proxy` → Stream proxy video
- `GET /api/projects/:id/media/:asset_id/thumbnail` → Get thumbnail

#### Orchestrator
- `POST /api/projects/:id/orchestrator/propose` → Propose candidate segments
- `POST /api/projects/:id/orchestrator/plan` → Generate EditPlan
- `POST /api/projects/:id/orchestrator/apply` → Apply EditPlan

#### Timeline
- `GET /api/projects/:id/timeline` → Get timeline
- `POST /api/projects/:id/timeline/apply` → Apply timeline operations

#### Jobs
- `GET /api/jobs/:id` → Get job status
- `POST /api/jobs/:id/cancel` → Cancel job

#### Export
- `POST /api/projects/:id/export` → Export video

### ML Service API (port 8001)

#### Health
- `GET /health` → `{ "ok": true, "version": "0.1.0" }`

#### Transcription
- `POST /transcribe` → Transcribe audio

#### Vision
- `POST /vision/analyze` → Analyze video frames

#### Embeddings
- `POST /embeddings/text` → Generate text embedding
- `POST /embeddings/vision` → Generate vision embedding
- `POST /embeddings/semantic` → (DEPRECATED) Delegates to text

#### Orchestrator
- `POST /orchestrator/reason` → Narrative reasoning
- `POST /orchestrator/generate_plan` → Generate EditPlan

#### Style
- `POST /style/profile_from_references` → Build style profile

---

## Data Flow

### Import Flow

```
User uploads video
  ↓
POST /api/projects/:id/import_raw
  ↓
Daemon: Create media_asset record
  ↓
Queue jobs: TranscribeAsset, AnalyzeVisionAsset, BuildSegments
  ↓
JobProcessor picks up jobs
  ↓
TranscribeAsset → ML service /transcribe → Store transcript → Queue EnrichSegmentsFromTranscript
AnalyzeVisionAsset → ML service /vision/analyze → Store vision → Queue EnrichSegmentsFromVision
BuildSegments → Create segments → Queue EnrichSegmentsFromTranscript, EnrichSegmentsFromVision
  ↓
EnrichSegmentsFromTranscript → Align transcript to segments
EnrichSegmentsFromVision → Attach vision tags to segments
  ↓
Queue ComputeSegmentMetadata
  ↓
ComputeSegmentMetadata → Generate summary, keywords, quality
  ↓
Queue EmbedSegments
  ↓
EmbedSegments → Generate text, vision, fusion embeddings → Store in database
```

### Orchestrator Flow

```
User sends message: "Create a morning routine video"
  ↓
POST /api/projects/:id/orchestrator/propose
  ↓
Daemon: Check preconditions
  ↓
If TALK/BUSY: Return friendly message
If ACT:
  ↓
Embed user intent: "Create a morning routine video"
  ↓
Similarity search (text embeddings, raw segments only)
  ↓
Filter by project, quality, unused
  ↓
Call ML service /orchestrator/reason
  ↓
Return candidate segments + narrative structure
  ↓
User clicks "Generate Plan"
  ↓
POST /api/projects/:id/orchestrator/plan
  ↓
Call ML service /orchestrator/generate_plan
  ↓
Return EditPlan
  ↓
User clicks "Apply Plan"
  ↓
POST /api/projects/:id/orchestrator/apply
  ↓
Parse EditPlan, create clips on timeline
  ↓
Store timeline, return success
```

### Timeline Playback Flow

```
User clicks play
  ↓
Editor.tsx: setIsTimelinePlaying(true)
  ↓
Find clip at current playhead position
  ↓
Set videoSrc to proxy URL
Set videoStartTime, videoEndTime from clip
  ↓
HTML5 video plays
  ↓
onTimeUpdate: Update playhead position
  ↓
When clip ends: Find next clip, transition
```

---

## Frontend Architecture

### Component Hierarchy

```
App.tsx
  ↓
Project.tsx (or Library.tsx)
  ↓
Editor.tsx
  ├── Toolbar.tsx
  ├── MediaLibrary.tsx
  ├── MediaSidebar.tsx
  ├── Viewer.tsx
  ├── Timeline.tsx
  │   └── TimelineToolbar.tsx
  ├── TextEditorPanel.tsx
  └── OrchestratorPanel.tsx
```

### State Management

**Global State (Editor.tsx):**
- Timeline data
- Selected clip
- Playback state
- Job status
- UI state (tools, sidebar, tabs)

**Local State:**
- Each component manages its own UI state
- Data fetched via `useDaemon` hook

### Communication Pattern

**Data Fetching:**
```typescript
const timelineData = useDaemon<TimelineResponse>(
  `/projects/${projectId}/timeline`,
  { method: 'GET' }
);

// Execute manually
timelineData.execute();

// Access data
if (timelineData.data) {
  // Use timelineData.data.timeline
}
```

**Job Polling:**
```typescript
useEffect(() => {
  const interval = setInterval(() => {
    // Poll job status
    jobStatus.execute();
  }, 1000);
  return () => clearInterval(interval);
}, []);
```

### Streaming Text

**Implementation:**
```typescript
const streamMessage = (messageIndex: number, fullText: string, speed: number = 20) => {
  let index = 0;
  const stream = () => {
    if (index < fullText.length) {
      setMessages(prev => {
        const updated = [...prev];
        updated[messageIndex] = {
          ...updated[messageIndex],
          content: fullText.slice(0, index + 1),
          isStreaming: true,
        };
        return updated;
      });
      index++;
      streamingTimeoutRef.current = setTimeout(stream, speed);
    } else {
      setMessages(prev => {
        const updated = [...prev];
        updated[messageIndex] = {
          ...updated[messageIndex],
          isStreaming: false,
        };
        return updated;
      });
    }
  };
  stream();
};
```

---

## Key Algorithms

### Segment Creation

**Fixed Chunking:**
1. Get video duration from FFmpeg probe
2. Divide into 4-6 second chunks
3. Create segments with `src_in_ticks` and `src_out_ticks`

**Shot Boundary Detection:**
- (Future: Use vision analysis to detect shot boundaries)

### Embedding Similarity

**Cosine Similarity:**
```rust
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    
    dot_product / (norm_a * norm_b)
}
```

### Fusion Embedding

**Algorithm:**
1. Normalize text embedding: `text_norm = text / ||text||`
2. Normalize vision embedding: `vision_norm = vision / ||vision||`
3. Trim to minimum dimension
4. Weighted combination: `fusion = 0.6 * text_norm + 0.4 * vision_norm`
5. Renormalize: `fusion = fusion / ||fusion||`

### Job Prerequisite Checking

**Algorithm:**
```rust
fn check_job_prerequisites(db: &Database, job_type: &JobType, asset_id: i64) -> Result<bool> {
    match job_type {
        JobType::EnrichSegmentsFromTranscript => {
            // Check: segments_built_at IS NOT NULL AND transcript_ready_at IS NOT NULL
            db.check_asset_prerequisites(asset_id, &["segments_built", "transcript_ready"])
        }
        JobType::EmbedSegments => {
            // Check: metadata_ready_at IS NOT NULL
            db.check_asset_prerequisites(asset_id, &["metadata_ready"])
        }
        // ...
    }
}
```

---

## Key Invariants

1. **Segments are immutable units**: `segment_id`, `src_in_ticks`, `src_out_ticks` never change after creation
2. **Timeline clips reference segments**: Clips always reference `segment_id` + in/out ranges
3. **Embeddings are never sent to LLM**: LLM only sees human-readable summaries and metadata
4. **LLM outputs structure, not actions**: EditPlan is deterministic, not direct timeline mutations
5. **Timeline remains authoritative**: All edits go through timeline operations
6. **Job gating prevents thrash**: Jobs check prerequisites before running
7. **Idempotent embeddings**: Embedding jobs check for existing embeddings before generating

---

## Error Handling

### Daemon Errors

- **Database errors**: Return 500 with error message
- **ML service errors**: Return 500, log error
- **Job failures**: Mark job as `Failed`, log error

### Frontend Errors

- **Network errors**: Display in UI, retry on user action
- **404 errors**: Suppressed (expected for missing resources)
- **500 errors**: Display error message, suggest retry

### Agent Errors

- **Never sound like system errors**: Use friendly, action-forward messages
- **Template**: "I couldn't do X because Y. If you do Z, I can help."
- **Example**: "I couldn't generate an edit yet because there aren't any analyzed clips. Upload some footage and I'll take care of the rest."

---

## Performance Considerations

1. **Embedding storage**: BLOB storage for efficient vector storage
2. **Similarity search**: In-memory computation (future: use vector index)
3. **Job processing**: Sequential processing (future: parallel processing)
4. **Frontend polling**: 1-second intervals for job status
5. **Proxy videos**: Generated on-demand for playback

---

## Future Improvements

1. **Vector indexing**: Use ANN index (e.g., HNSW) for faster similarity search
2. **Parallel job processing**: Process multiple jobs concurrently
3. **Shot boundary detection**: Use vision analysis for better segment boundaries
4. **LLM integration**: Use OpenAI GPT-4 for narrative reasoning and EditPlan generation
5. **Style profile extraction**: Analyze reference videos for actual style patterns
6. **Real-time preview**: Stream timeline preview during editing

---

## Development Workflow

1. **Start ML service**: `cd ml/service && source .venv/bin/activate && uvicorn main:app --host 127.0.0.1 --port 8001`
2. **Start Electron app**: `cd apps/desktop && pnpm run electron:dev` (auto-spawns daemon)
3. **Build daemon manually**: `cargo build --bin daemon`
4. **Run daemon manually**: `cargo run --bin daemon`

---

## Conclusion

This document provides a comprehensive overview of VibeCut's architecture. The system is designed for local-only operation with a clear separation of concerns: frontend (React/Electron), backend (Rust daemon), and ML service (Python/FastAPI). The orchestrator system provides an AI assistant interface for conversational video editing, while the job processing system ensures reliable, gated analysis of video assets.

