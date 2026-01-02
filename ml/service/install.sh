#!/bin/bash
# Installation script that skips av (PyAV) if it fails to build

set -e

cd "$(dirname "$0")"
source .venv/bin/activate

echo "Installing core dependencies..."
pip install fastapi==0.115.0
pip install "uvicorn[standard]==0.32.0"
pip install "opencv-python>=4.10.0.82"
pip install "numpy>=2.0.0"
pip install pytesseract==0.3.13
pip install "scipy>=1.13.0"

echo "Installing faster-whisper (this may pull in av, but we'll skip it if it fails)..."
pip install faster-whisper==1.0.3 || {
    echo "Warning: faster-whisper installation had issues, trying without av dependency..."
    # Try installing faster-whisper's direct dependencies manually
    pip install "faster-whisper==1.0.3" --no-deps || true
    pip install ctranslate2 huggingface-hub || true
}

echo "Installation complete! If av failed to build, that's okay - it's not used by the ML service."



