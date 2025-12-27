# VibeCut ML Service

Python FastAPI service for transcription, vision analysis, and embeddings.

## Setup

1. Create a virtual environment:
```bash
python -m venv .venv
```

2. Activate the virtual environment:
```bash
# On macOS/Linux:
source .venv/bin/activate

# On Windows:
.venv\Scripts\activate
```

3. Install dependencies:
```bash
pip install -r requirements.txt
```

## Running

Start the service:
```bash
uvicorn main:app --host 127.0.0.1 --port 8001
```

Or run directly:
```bash
python main.py
```

The service will be available at `http://127.0.0.1:8001`

## Health Check

```bash
curl http://127.0.0.1:8001/health
```
