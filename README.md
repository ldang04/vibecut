# VibeCut

A local-only AI video editor that creates professional video edits from raw footage using AI analysis and style profiles.

## Architecture

VibeCut consists of three main components:

- **Electron Desktop App** (`apps/desktop`): React + TypeScript UI, communicates with daemon via HTTP
- **Rust Daemon** (`crates/daemon`): HTTP server (Axum) handling project management, job orchestration, FFmpeg operations
- **Python ML Service** (`ml/service`): FastAPI service for transcription, vision analysis, and embeddings

## Prerequisites

- **Rust** (latest stable)
- **Node.js** 20.19+ or 22.12+ (specified in `.nvmrc` - **Node.js 21.x is NOT supported**)
- **Python** 3.8+
- **FFmpeg** (system dependency, must be in PATH)
- **pnpm** (recommended) or npm

> **Important**: Vite 7.3.0 requires Node.js 20.19+ or 22.12+. Node.js 21.x will cause `crypto.hash is not a function` errors.

## Quick Start

### 1. First-Time Setup

```bash
# Build Rust daemon
cargo build --bin daemon

# Set up ML service
cd ml/service
python -m venv .venv
source .venv/bin/activate  # On macOS/Linux: .venv\Scripts\activate on Windows
pip install -r requirements.txt
cd ../..

# Install Electron app dependencies
cd apps/desktop
pnpm install
cd ../..
```

### 2. Running the App

You need **2 terminals**:

#### Terminal 1: ML Service
```bash
cd ml/service
source .venv/bin/activate  # macOS/Linux
# .venv\Scripts\activate   # Windows
uvicorn main:app --host 127.0.0.1 --port 8001
```

#### Terminal 2: Electron App
```bash
cd apps/desktop
pnpm run electron:dev
```

The `electron:dev` script will:
- Start Vite dev server (port 5173)
- Auto-spawn Rust daemon (port 7777)
- Launch Electron window

### 3. Verify Everything is Running

- **Daemon Health**: `curl http://127.0.0.1:7777/health`
- **ML Service Health**: `curl http://127.0.0.1:8001/health`

## Project Structure

```
vibecut/
├── apps/
│   └── desktop/          # Electron + React app
├── crates/
│   ├── daemon/           # Rust HTTP daemon
│   └── engine/           # Timeline engine (operations, compiler, render)
├── ml/
│   └── service/          # Python FastAPI ML service
└── shared/
    └── schemas/          # JSON schemas for IPC
```

## Development

### Building

```bash
# Build daemon
cargo build --bin daemon

# Build Electron app (production)
cd apps/desktop
pnpm run electron:build
```

### Running Daemon Manually (Optional)

If you want to run the daemon separately for debugging:

```bash
cargo run --bin daemon
```

### Database

The daemon uses SQLite at `.cache/vibecut.db`. The database is created automatically on first run.

## API Endpoints

### Daemon (port 7777)

- `GET /health` - Health check
- `POST /api/projects` - Create project
- `GET /api/projects/:id` - Get project
- `POST /api/projects/:id/import_raw` - Import raw footage
- `POST /api/projects/:id/import_reference` - Import style reference
- `POST /api/projects/:id/generate` - Generate edit plan
- `GET /api/projects/:id/timeline` - Get timeline
- `POST /api/projects/:id/timeline/apply` - Apply timeline operations
- `POST /api/projects/:id/export` - Export final video
- `GET /api/jobs/:id` - Get job status
- `POST /api/jobs/:id/cancel` - Cancel job

### ML Service (port 8001)

- `GET /health` - Health check
- `POST /transcribe` - Transcribe audio
- `POST /vision/analyze` - Analyze video frames
- `POST /style/profile_from_references` - Build style profile

## Troubleshooting

- **Node.js version errors**: 
  - If you see `crypto.hash is not a function` or "Vite requires Node.js version 20.19+", you're using an unsupported Node version
  - **Fix**: Switch to the correct Node version using `nvm use` (this will automatically use the version from `.nvmrc`)
  - Verify with `node --version` (should show v20.19.x or v22.12+)
  - If you don't have the correct version installed: `nvm install 20.19` then `nvm use`
  
- **Daemon won't start**: Make sure it's built with `cargo build --bin daemon`
- **ML service errors**: Ensure Python dependencies are installed and virtual environment is activated
- **FFmpeg errors**: Verify FFmpeg is installed and in your PATH (`ffmpeg -version`)
- **Port conflicts**: Check if ports 7777, 8001, or 5173 are already in use

