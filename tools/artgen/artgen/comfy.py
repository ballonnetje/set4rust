"""Talk to a local ComfyUI server over its HTTP API; start a headless one when none answers.

The server is ComfyUI Desktop's own install (its Python venv and the ComfyUI source shipped inside
the app), started with ``--base-directory`` on the Desktop's model folder, on a port of its own so
it does not clash with the Desktop window (port 8000). Override with the environment variables
``COMFY_URL``, ``COMFY_PYTHON``, ``COMFY_MAIN`` and ``COMFY_BASE``.
"""

from __future__ import annotations

import io
import json
import os
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid
from pathlib import Path

DEFAULT_URL = "http://127.0.0.1:8189"


def _desktop_paths() -> tuple[Path, Path, Path]:
    home = Path.home()
    base = Path(os.environ.get("COMFY_BASE", home / "Documents" / "ComfyUI"))
    py = Path(os.environ.get("COMFY_PYTHON", base / ".venv" / "Scripts" / "python.exe"))
    main = Path(
        os.environ.get(
            "COMFY_MAIN",
            home / "AppData" / "Local" / "Programs" / "ComfyUI" / "resources" / "ComfyUI" / "main.py",
        )
    )
    return py, main, base


class Comfy:
    def __init__(self, url: str | None = None):
        self.url = (url or os.environ.get("COMFY_URL") or DEFAULT_URL).rstrip("/")
        self.client_id = uuid.uuid4().hex
        self.proc: subprocess.Popen | None = None

    # --- server ---------------------------------------------------------------------------------

    def alive(self) -> bool:
        try:
            with urllib.request.urlopen(self.url + "/system_stats", timeout=2) as r:
                return r.status == 200
        except (urllib.error.URLError, OSError):
            return False

    def ensure(self, timeout: float = 180.0):
        if self.alive():
            return
        py, main, base = _desktop_paths()
        for p in (py, main, base):
            if not p.exists():
                raise SystemExit(f"ComfyUI not found at {p}; set COMFY_URL to a running server or COMFY_* paths")
        port = urllib.parse.urlparse(self.url).port or 8189
        log = Path(base) / "artgen-server.log"
        print(f"starting ComfyUI on port {port} (log: {log})", file=sys.stderr)
        flags = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0
        args = [str(py), str(main), "--base-directory", str(base), "--listen", "127.0.0.1",
                "--port", str(port), "--disable-auto-launch"]
        # ComfyUI Desktop ships its web front end next to the source, not as a pip package.
        ui = main.parent / "web_custom_versions" / "desktop_app"
        if ui.is_dir():
            args += ["--front-end-root", str(ui)]
        self.proc = subprocess.Popen(
            args,
            cwd=str(main.parent), stdout=open(log, "w", encoding="utf-8"), stderr=subprocess.STDOUT,
            creationflags=flags,
        )
        t0 = time.time()
        while time.time() - t0 < timeout:
            if self.proc.poll() is not None:
                raise SystemExit(f"ComfyUI exited with {self.proc.returncode}; see {log}")
            if self.alive():
                return
            time.sleep(1.0)
        raise SystemExit(f"ComfyUI did not answer within {timeout:.0f} s; see {log}")

    def stop(self):
        if self.proc and self.proc.poll() is None:
            self.proc.terminate()
            try:
                self.proc.wait(20)
            except subprocess.TimeoutExpired:
                self.proc.kill()

    # --- API ------------------------------------------------------------------------------------

    def _json(self, path: str, body: dict | None = None):
        data = json.dumps(body).encode() if body is not None else None
        req = urllib.request.Request(self.url + path, data=data, headers={"Content-Type": "application/json"})
        try:
            with urllib.request.urlopen(req, timeout=60) as r:
                return json.loads(r.read() or b"null")
        except urllib.error.HTTPError as e:
            raise SystemExit(f"ComfyUI {path}: HTTP {e.code}: {e.read().decode(errors='replace')[:4000]}")

    def models(self, folder: str) -> list[str]:
        return self._json(f"/models/{folder}") or []

    def upload(self, image, name: str) -> str:
        """Upload a PIL image to ComfyUI's input folder; returns the name to pass to LoadImage."""
        buf = io.BytesIO()
        image.save(buf, format="PNG")
        boundary = uuid.uuid4().hex
        body = (
            f"--{boundary}\r\nContent-Disposition: form-data; name=\"image\"; filename=\"{name}\"\r\n"
            f"Content-Type: image/png\r\n\r\n"
        ).encode() + buf.getvalue() + (
            f"\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"subfolder\"\r\n\r\nartgen"
            f"\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"overwrite\"\r\n\r\ntrue"
            f"\r\n--{boundary}--\r\n"
        ).encode()
        req = urllib.request.Request(
            self.url + "/upload/image", data=body,
            headers={"Content-Type": f"multipart/form-data; boundary={boundary}"},
        )
        with urllib.request.urlopen(req, timeout=60) as r:
            res = json.loads(r.read())
        return f"{res['subfolder']}/{res['name']}" if res.get("subfolder") else res["name"]

    def run(self, graph: dict, timeout: float = 900.0) -> dict[str, list]:
        """Queue an API-format graph, wait for it, return {node_id: [PIL images]} of its outputs."""
        from PIL import Image

        res = self._json("/prompt", {"prompt": graph, "client_id": self.client_id})
        pid = res["prompt_id"]
        t0 = time.time()
        while True:
            hist = self._json(f"/history/{pid}")
            if hist and pid in hist:
                entry = hist[pid]
                status = entry.get("status", {})
                if status.get("status_str") == "error":
                    msgs = [m for m in status.get("messages", []) if m[0] == "execution_error"]
                    raise SystemExit(f"ComfyUI error: {json.dumps(msgs)[:4000]}")
                if status.get("completed", True):
                    break
            if time.time() - t0 > timeout:
                raise SystemExit(f"ComfyUI prompt {pid} timed out")
            time.sleep(0.5)
        out: dict[str, list] = {}
        for node, o in entry.get("outputs", {}).items():
            for im in o.get("images", []):
                q = urllib.parse.urlencode({"filename": im["filename"], "subfolder": im["subfolder"], "type": im["type"]})
                with urllib.request.urlopen(f"{self.url}/view?{q}", timeout=60) as r:
                    out.setdefault(node, []).append(Image.open(io.BytesIO(r.read())).copy())
        return out
