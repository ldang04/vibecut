#!/usr/bin/env python3
"""Test script to verify orchestrator endpoints work correctly"""

import requests
import json

ML_SERVICE_URL = "http://127.0.0.1:8001"
DAEMON_URL = "http://127.0.0.1:7777/api"

def test_ml_service_health():
    """Test ML service health endpoint"""
    print("Testing ML service health...")
    try:
        response = requests.get(f"{ML_SERVICE_URL}/health", timeout=2)
        if response.status_code == 200:
            print(f"✓ ML service is running: {response.json()}")
            return True
        else:
            print(f"✗ ML service returned status {response.status_code}")
            return False
    except requests.exceptions.ConnectionError:
        print("✗ ML service is not running (connection refused)")
        return False
    except Exception as e:
        print(f"✗ Error: {e}")
        return False

def test_embeddings_endpoint():
    """Test embeddings endpoint"""
    print("\nTesting embeddings endpoint...")
    try:
        response = requests.post(
            f"{ML_SERVICE_URL}/embeddings/semantic",
            json={"text": "test embedding"},
            timeout=5
        )
        if response.status_code == 200:
            data = response.json()
            if "embedding" in data and len(data["embedding"]) == 1536:
                print(f"✓ Embeddings endpoint works (returned {len(data['embedding'])} dimensions)")
                return True
            else:
                print(f"✗ Invalid embedding response: {data}")
                return False
        else:
            print(f"✗ Embeddings endpoint returned status {response.status_code}: {response.text}")
            return False
    except requests.exceptions.ConnectionError:
        print("✗ ML service is not running")
        return False
    except Exception as e:
        print(f"✗ Error: {e}")
        return False

def test_orchestrator_reason_endpoint():
    """Test orchestrator reason endpoint"""
    print("\nTesting orchestrator reason endpoint...")
    try:
        test_segments = [
            {
                "segment_id": 1,
                "summary_text": "A person walking in a park",
                "capture_time": "2024-01-01T10:00:00",
                "duration_sec": 5.0
            },
            {
                "segment_id": 2,
                "summary_text": "Sunset over the ocean",
                "capture_time": "2024-01-01T18:00:00",
                "duration_sec": 3.0
            }
        ]
        
        response = requests.post(
            f"{ML_SERVICE_URL}/orchestrator/reason",
            json={
                "segments": test_segments,
                "style_profile": None,
                "timeline_context": None
            },
            timeout=5
        )
        if response.status_code == 200:
            data = response.json()
            if "explanation" in data and "questions" in data:
                print(f"✓ Orchestrator reason endpoint works")
                print(f"  Explanation: {data['explanation'][:100]}...")
                print(f"  Questions: {data['questions']}")
                return True
            else:
                print(f"✗ Invalid response structure: {data}")
                return False
        else:
            print(f"✗ Orchestrator reason endpoint returned status {response.status_code}: {response.text}")
            return False
    except requests.exceptions.ConnectionError:
        print("✗ ML service is not running")
        return False
    except Exception as e:
        print(f"✗ Error: {e}")
        return False

def test_empty_segments():
    """Test orchestrator with empty segments"""
    print("\nTesting orchestrator with empty segments...")
    try:
        response = requests.post(
            f"{ML_SERVICE_URL}/orchestrator/reason",
            json={
                "segments": [],
                "style_profile": None,
                "timeline_context": None
            },
            timeout=5
        )
        if response.status_code == 200:
            data = response.json()
            if "explanation" in data:
                print(f"✓ Empty segments handled correctly")
                print(f"  Explanation: {data['explanation']}")
                return True
            else:
                print(f"✗ Invalid response: {data}")
                return False
        else:
            print(f"✗ Endpoint returned status {response.status_code}: {response.text}")
            return False
    except Exception as e:
        print(f"✗ Error: {e}")
        return False

if __name__ == "__main__":
    print("=" * 60)
    print("Orchestrator Sanity Check")
    print("=" * 60)
    
    results = []
    results.append(("ML Service Health", test_ml_service_health()))
    results.append(("Embeddings Endpoint", test_embeddings_endpoint()))
    results.append(("Orchestrator Reason", test_orchestrator_reason_endpoint()))
    results.append(("Empty Segments", test_empty_segments()))
    
    print("\n" + "=" * 60)
    print("Summary:")
    print("=" * 60)
    for name, result in results:
        status = "✓ PASS" if result else "✗ FAIL"
        print(f"{status} - {name}")
    
    all_passed = all(result for _, result in results)
    if all_passed:
        print("\n✓ All tests passed!")
    else:
        print("\n✗ Some tests failed. Make sure the ML service is running:")
        print("  cd ml/service && python main.py")



