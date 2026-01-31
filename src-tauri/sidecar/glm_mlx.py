#!/usr/bin/env python3
import argparse
import json
import os
import sys
import tempfile
from http.server import BaseHTTPRequestHandler, HTTPServer


def load_model(model_id):
    from mlx_audio.stt.utils import load_model

    return load_model(model_id)


def run_transcription(model, audio_path):
    from mlx_audio.stt.generate import generate_transcription

    tmp = tempfile.NamedTemporaryFile(suffix=".txt", delete=False)
    tmp.close()
    try:
        result = generate_transcription(
            model=model,
            audio_path=audio_path,
            output_path=tmp.name,
            format="txt",
            verbose=False,
        )
    finally:
        try:
            os.remove(tmp.name)
        except OSError:
            pass
    text = getattr(result, "text", "")
    return text.strip()


class Handler(BaseHTTPRequestHandler):
    model = None

    def log_message(self, *args, **kwargs):
        return

    def _send_json(self, status, payload):
        data = json.dumps(payload).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        if self.path != "/health":
            self._send_json(404, {"error": "not found"})
            return
        self._send_json(200, {"status": "ok"})

    def do_POST(self):
        if self.path != "/transcribe":
            self._send_json(404, {"error": "not found"})
            return
        length = int(self.headers.get("Content-Length", "0"))
        try:
            payload = json.loads(self.rfile.read(length) or b"{}")
        except json.JSONDecodeError:
            self._send_json(400, {"error": "invalid json"})
            return
        audio_path = payload.get("audio_path")
        if not audio_path:
            self._send_json(400, {"error": "audio_path is required"})
            return
        if not os.path.exists(audio_path):
            self._send_json(400, {"error": "audio file not found"})
            return
        try:
            text = run_transcription(self.model, audio_path)
        except Exception as exc:
            self._send_json(500, {"error": str(exc)})
            return
        self._send_json(200, {"text": text})


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--model", required=True)
    parser.add_argument("--port", type=int, default=8791)
    parser.add_argument("--preload", action="store_true")
    args = parser.parse_args()

    model = load_model(args.model)
    if args.preload:
        print("ready")
        return 0

    Handler.model = model
    server = HTTPServer(("127.0.0.1", args.port), Handler)
    server.serve_forever()
    return 0


if __name__ == "__main__":
    sys.exit(main())
