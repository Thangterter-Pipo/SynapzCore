#!/usr/bin/env python3
"""
dashboard_server.py — Custom http server for SynapzCore Command Center.
Serves static files and proxies CDP commands/transcript logs for IDE integration.
"""

import os
import sys
import json
import socket
import subprocess
import urllib.parse
import urllib.request
import base64
import time
import threading
from http.server import SimpleHTTPRequestHandler, HTTPServer
from socketserver import ThreadingMixIn

PORT = 8899

# ---- Load .env (simple, no external dep) so secrets stay out of code ----
def _load_dotenv():
    """Load KEY=VALUE lines from .env at repo root into os.environ (no override)."""
    try:
        here = os.path.dirname(os.path.abspath(__file__))
        env_path = os.path.join(os.path.dirname(here), ".env")
        if not os.path.isfile(env_path):
            return
        with open(env_path, "r", encoding="utf-8") as f:
            for line in f:
                line = line.strip()
                if not line or line.startswith("#") or "=" not in line:
                    continue
                k, _, v = line.partition("=")
                k, v = k.strip(), v.strip()
                if k and k not in os.environ:
                    os.environ[k] = v
    except Exception as e:
        print(f"⚠️ .env load skipped: {e}")

_load_dotenv()

# ---- API auth token (protects the public-tunnel-exposed HTTP API) ----
# If SYNAPZ_API_TOKEN is set, every NON-localhost request must present it via
# header `X-Synapz-Token` or `?token=`. Localhost (Bố dùng dashboard local) is
# always allowed so nothing breaks on the machine itself. Empty token => auth OFF.
API_TOKEN = os.environ.get("SYNAPZ_API_TOKEN", "").strip()
# Paths always reachable without a token (so the login/landing page can load).
_AUTH_EXEMPT_PREFIXES = ("/scripts/miniapp.html", "/scripts/dashboard.html",
                          "/scripts/", "/favicon")

# Multi-AI Coordination
from coordination import Coordinator
COORD = None  # initialized in main() after chdir

# ---- Attachment persistence (images survive history re-render, like the IDE) ----
# Uploaded images are NOT in the IDE transcript, so we persist them ourselves and
# map each batch to the prompt it was sent with. On history rebuild we re-attach.
_SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
ATTACH_DIR = os.path.join(_SCRIPT_DIR, ".attachments")
ATTACH_LOG = os.path.join(ATTACH_DIR, "attachments_log.json")
_pending_attachments = []          # abs paths saved by /upload, awaiting next /chat
_attach_lock = threading.Lock()

def _load_attach_log():
    try:
        with open(ATTACH_LOG, "r", encoding="utf-8") as f:
            return json.load(f)
    except Exception:
        return []

def _save_attach_log(entries):
    try:
        os.makedirs(ATTACH_DIR, exist_ok=True)
        tmp = ATTACH_LOG + ".tmp"
        with open(tmp, "w", encoding="utf-8") as f:
            json.dump(entries, f, ensure_ascii=False)
        os.replace(tmp, ATTACH_LOG)
    except Exception as e:
        print(f"Error saving attach log: {e}")

def commit_pending_attachments(prompt):
    """Bind images uploaded just before this prompt to the prompt text."""
    global _pending_attachments
    with _attach_lock:
        if not _pending_attachments:
            return
        entries = _load_attach_log()
        entries.append({
            "prompt": (prompt or "").strip(),
            "ts": time.time(),
            "images": list(_pending_attachments),
        })
        # keep last 200 entries
        if len(entries) > 200:
            entries = entries[-200:]
        _save_attach_log(entries)
        _pending_attachments = []

def extract_user_request(content):
    """Pull the user's actual text out of the <USER_REQUEST>...</USER_REQUEST> wrapper."""
    if not content:
        return ""
    s = content
    start = s.find("<USER_REQUEST>")
    end = s.find("</USER_REQUEST>")
    if start != -1 and end != -1:
        return s[start + len("<USER_REQUEST>"):end].strip()
    return s.strip()


# ---- Project file listing (for @-mention autocomplete) ----
_files_cache = {"ts": 0, "files": []}
_files_lock = threading.Lock()
_SKIP_DIRS = {".git", "node_modules", "mingw", "__pycache__", ".attachments",
              ".upload_cache", "target", "dist", "build", ".venv", "venv",
              ".idea", ".vscode", "site-packages"}
_SKIP_EXT = {".pyc", ".pyo", ".o", ".obj", ".exe", ".dll", ".bin", ".lock",
             ".png", ".jpg", ".jpeg", ".gif", ".webp", ".ico", ".pdf",
             ".zip", ".gz", ".7z", ".mp4", ".webm", ".woff", ".woff2", ".ttf"}

def _list_project_files():
    """Relative project file paths, cached for 60s. Skips junk/binary."""
    with _files_lock:
        now = time.time()
        if now - _files_cache["ts"] < 60 and _files_cache["files"]:
            return _files_cache["files"]
        root = os.getcwd()
        out = []
        for dirpath, dirnames, filenames in os.walk(root):
            dirnames[:] = [d for d in dirnames if d not in _SKIP_DIRS and not d.startswith(".")]
            for fn in filenames:
                ext = os.path.splitext(fn)[1].lower()
                if ext in _SKIP_EXT:
                    continue
                rel = os.path.relpath(os.path.join(dirpath, fn), root).replace("\\", "/")
                out.append(rel)
                if len(out) >= 4000:
                    break
            if len(out) >= 4000:
                break
        out.sort()
        _files_cache["ts"] = now
        _files_cache["files"] = out
        return out



# Discover the conversation ID logs path
def find_latest_transcript_path():
    brain_dir = "C:/Users/thang/.gemini/antigravity-ide/brain"
    if not os.path.exists(brain_dir):
        return None
    latest_time = 0
    latest_path = None
    try:
        for entry in os.scandir(brain_dir):
            if entry.is_dir():
                log_file = os.path.join(entry.path, ".system_generated", "logs", "transcript.jsonl")
                if os.path.exists(log_file):
                    mtime = os.path.getmtime(log_file)
                    if mtime > latest_time:
                        latest_time = mtime
                        latest_path = log_file
    except Exception as e:
        print(f"Error scanning brain directory: {e}")
    return latest_path

def get_conversation_history(limit=40):
    path = find_latest_transcript_path()
    if not path:
        return []
    attach_log = _load_attach_log()          # [{prompt, ts, images:[abs...]}]
    used_attach = set()                       # indices already bound to a message
    messages = []
    try:
        with open(path, "r", encoding="utf-8") as f:
            for line in f:
                try:
                    data = json.loads(line)
                    step_idx = data.get("step_index")
                    
                    # Parse user message
                    if data.get("type") == "USER_INPUT":
                        content = (data.get("content", "") or "").strip()
                        if not content:
                            continue
                        # Re-attach any images that were uploaded with this prompt.
                        # Match by the user's actual request text (wrapper stripped).
                        req_text = extract_user_request(content)
                        imgs = []
                        for ai, ent in enumerate(attach_log):
                            if ai in used_attach:
                                continue
                            if ent.get("prompt") and ent["prompt"] == req_text:
                                imgs = [p for p in ent.get("images", []) if os.path.isfile(p)]
                                used_attach.add(ai)
                                break
                        messages.append({
                            "role": "user",
                            "content": content,
                            "step_index": step_idx,
                            "created_at": data.get("created_at"),
                            "images": imgs,
                        })
                    # Parse agent response — ONLY the planner's natural-language
                    # prose, exactly like the IDE. Every other MODEL-sourced entry
                    # (RUN_COMMAND, CODE_ACTION, LIST_DIRECTORY, VIEW_FILE,
                    # GREP_SEARCH, GENERATE_IMAGE, GENERIC, ...) is a tool step and
                    # is hidden.
                    elif data.get("type") in ("PLANNER_RESPONSE", "MODEL_RESPONSE"):
                        content = data.get("content", "") or ""

                        # Hide tool calls entirely, just like the IDE does:
                        # only the model's natural-language text is shown. Steps
                        # that are pure tool invocations (no prose) are skipped.
                        full = content.strip()
                        if not full:
                            continue
                        messages.append({
                            "role": "assistant",
                            "content": full,
                            "step_index": step_idx,
                            "created_at": data.get("created_at"),
                            "images": [],
                        })
                except Exception:
                    pass
    except Exception as e:
        print(f"Error reading transcript: {e}")
    # Only return the most recent `limit` messages to avoid freezing the UI
    if limit and len(messages) > limit:
        messages = messages[-limit:]
    return messages

def discover_ws_url():
    try:
        req = urllib.request.Request("http://127.0.0.1:9333/json")
        with urllib.request.urlopen(req, timeout=1.0) as resp:
            pages = json.loads(resp.read().decode('utf-8'))
            workbench = None
            for p in pages:
                url = p.get("url", "")
                title = p.get("title", "")
                if "workbench.html" in url or "Antigravity" in title:
                    workbench = p
                    break
            if not workbench:
                for p in pages:
                    if p.get("webSocketDebuggerUrl") and p.get("type") == "page":
                        workbench = p
                        break
            if workbench and workbench.get("webSocketDebuggerUrl"):
                return workbench["webSocketDebuggerUrl"]
    except Exception as e:
        print(f"Error discovering WS url: {e}")
    return None

# ---- Launch Antigravity IDE with CDP enabled (port 9333) from SynapzCore ----
ANTIGRAVITY_EXE = os.path.join(
    os.environ.get("LOCALAPPDATA", r"C:\Users\thang\AppData\Local"),
    "Programs", "Antigravity IDE", "Antigravity IDE.exe"
)
CDP_PORT = 9333
_launch_lock = threading.Lock()

# Đường dẫn binary orchestrator (release ưu tiên, fallback debug).
def _orchestrator_bin():
    here = os.path.dirname(os.path.abspath(__file__))
    root = os.path.dirname(here)
    for sub in ("release", "debug"):
        p = os.path.join(root, "target", sub, "synapz-orchestrator.exe")
        if os.path.isfile(p):
            return p
    return None

def run_orchestrator_dispatch(prompt, timeout=200):
    """Gọi synapz-orchestrator --live --json giao task cho mọi agent Connected.

    Trả dict JSON đã parse từ orchestrator: {ok, prompt, agents, completed, failed, results:[...]}.
    """
    binp = _orchestrator_bin()
    if not binp:
        return {"ok": False, "reason": "Chưa build synapz-orchestrator (cargo build --release)"}
    env = dict(os.environ)
    env["PYTHONHOME"] = ""
    env["PYTHONPATH"] = ""
    try:
        proc = subprocess.run(
            [binp, "--json", "--live", prompt],
            cwd=os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
            capture_output=True, text=True, timeout=timeout, env=env,
        )
    except subprocess.TimeoutExpired:
        return {"ok": False, "reason": f"orchestrator timeout {timeout}s"}
    # JSON nằm ở dòng cuối stdout (các dòng trước có thể là log mcp).
    out = (proc.stdout or "").strip()
    for line in reversed(out.splitlines()):
        line = line.strip()
        if line.startswith("{"):
            try:
                return json.loads(line)
            except Exception:
                continue
    return {"ok": False, "reason": "không parse được JSON từ orchestrator", "raw": out[-500:], "stderr": (proc.stderr or "")[-300:]}

def run_orchestrator_pipeline(graph, echo=True, timeout=600):
    """Chạy TRỌN 4 giai đoạn parallel orchestration qua synapz-orchestrator --pipeline.

    `graph` là dict TaskGraph ({"nodes":[{id,prompt,role,depends_on}...]}).
    Ghi ra file tạm, chạy binary, rồi đọc data/last_pipeline_run.json (binary tự ghi).
    Trả dict: {ok, report:{...}, layers:[...]} hoặc {ok:False, reason}.
    """
    binp = _orchestrator_bin()
    if not binp:
        return {"ok": False, "reason": "Chưa build synapz-orchestrator (cargo build --release)"}
    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    # Ghi graph ra file tạm trong data/.
    data_dir = os.path.join(root, "data")
    os.makedirs(data_dir, exist_ok=True)
    graph_path = os.path.join(data_dir, "pipeline_input.json")
    try:
        with open(graph_path, "w", encoding="utf-8") as f:
            json.dump(graph, f, ensure_ascii=False, indent=2)
    except Exception as e:
        return {"ok": False, "reason": f"không ghi được graph tạm: {e}"}

    env = dict(os.environ)
    env["PYTHONHOME"] = ""
    env["PYTHONPATH"] = ""
    args = [binp, "--pipeline", graph_path]
    if echo:
        args.append("--echo")
    try:
        proc = subprocess.run(
            args, cwd=root, capture_output=True, text=True, timeout=timeout, env=env,
        )
    except subprocess.TimeoutExpired:
        return {"ok": False, "reason": f"pipeline timeout {timeout}s"}

    # Binary ghi kết quả vào data/last_pipeline_run.json.
    run_path = os.path.join(data_dir, "last_pipeline_run.json")
    buf_path = os.path.join(data_dir, "last_buffer_snapshot.json")
    report = None
    buffer_snap = None
    try:
        if os.path.isfile(run_path):
            with open(run_path, "r", encoding="utf-8") as f:
                report = json.load(f)
        if os.path.isfile(buf_path):
            with open(buf_path, "r", encoding="utf-8") as f:
                buffer_snap = json.load(f)
    except Exception as e:
        return {"ok": False, "reason": f"không đọc được kết quả pipeline: {e}",
                "stdout": (proc.stdout or "")[-500:]}

    if report is None:
        return {"ok": False, "reason": "pipeline không sinh kết quả",
                "stdout": (proc.stdout or "")[-800:], "stderr": (proc.stderr or "")[-300:]}
    return {"ok": True, "report": report, "buffer": buffer_snap,
            "stdout_tail": (proc.stdout or "")[-1200:]}

def launch_antigravity_cdp(force_restart=False, wait_timeout=40):
    """Start Antigravity IDE with --remote-debugging-port=9333.

    If CDP is already responsive and force_restart is False, do nothing.
    When force_restart is True, kill any running instance first (so the
    debug port is actually enabled — a normally-launched IDE has no CDP).
    Polls until discover_ws_url() succeeds or wait_timeout elapses.
    Returns dict {ok, ws_url|reason, restarted, launched}.
    """
    with _launch_lock:
        already = discover_ws_url()
        if already and not force_restart:
            return {"ok": True, "ws_url": already, "restarted": False,
                    "launched": False, "note": "CDP already up"}

        if not os.path.isfile(ANTIGRAVITY_EXE):
            return {"ok": False, "reason": f"Antigravity exe not found: {ANTIGRAVITY_EXE}"}

        restarted = False
        if force_restart or not already:
            # Kill existing instances so the debug port can be (re)bound.
            # A normally-started IDE won't expose CDP, so a restart is required.
            for name in ("Antigravity IDE.exe", "Antigravity.exe"):
                try:
                    subprocess.run(["taskkill", "/F", "/IM", name],
                                   capture_output=True, timeout=10)
                    restarted = True
                except Exception as e:
                    print(f"taskkill {name}: {e}")
            time.sleep(3)

        try:
            # DETACHED so the IDE outlives this request; no console window.
            CREATE_NO_WINDOW = 0x08000000
            DETACHED_PROCESS = 0x00000008
            subprocess.Popen(
                [ANTIGRAVITY_EXE, f"--remote-debugging-port={CDP_PORT}"],
                creationflags=CREATE_NO_WINDOW | DETACHED_PROCESS,
                close_fds=True,
            )
        except Exception as e:
            return {"ok": False, "reason": f"launch failed: {e}", "restarted": restarted}

        # Poll until the workbench page exposes a WS debugger URL.
        deadline = time.time() + wait_timeout
        while time.time() < deadline:
            time.sleep(2)
            ws = discover_ws_url()
            if ws:
                return {"ok": True, "ws_url": ws, "restarted": restarted, "launched": True}
        return {"ok": False, "reason": f"IDE launched but CDP not up within {wait_timeout}s",
                "restarted": restarted, "launched": True}


def inject_prompt_via_cdp(ws_url, prompt):
    parsed = urllib.parse.urlparse(ws_url)
    host = parsed.hostname
    port = parsed.port
    path = parsed.path
    if parsed.query:
        path += "?" + parsed.query
        
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.connect((host, port))
    
    key = base64.b64encode(os.urandom(16)).decode('utf-8')
    handshake = (
        f"GET {path} HTTP/1.1\r\n"
        f"Host: {host}:{port}\r\n"
        f"Upgrade: websocket\r\n"
        f"Connection: Upgrade\r\n"
        f"Sec-WebSocket-Key: {key}\r\n"
        f"Sec-WebSocket-Version: 13\r\n\r\n"
    )
    s.sendall(handshake.encode('utf-8'))
    
    res = b""
    while b"\r\n\r\n" not in res:
        res += s.recv(1024)
        
    # JSON-encode the prompt so it embeds safely as a JS string literal (handles quotes, newlines, unicode).
    prompt_literal = json.dumps(prompt)

    js_code = f"""
    (function() {{
        var text = {prompt_literal};

        // The Antigravity IDE chat box is a Lexical rich-text editor, NOT a textarea.
        // Prefer it explicitly; fall back to a generic contenteditable, then textarea.
        var el = document.querySelector('[data-lexical-editor="true"]')
              || document.querySelector('[aria-label="Message input"]')
              || document.querySelector('div[contenteditable="true"][role="combobox"]')
              || document.querySelector('div[contenteditable="true"]');

        var isTextarea = false;
        if (!el) {{
            el = document.querySelector('textarea');
            isTextarea = true;
        }}
        if (!el) {{ return 'no_input_found'; }}

        el.focus();

        if (isTextarea) {{
            var nativeSet = Object.getOwnPropertyDescriptor(
                window.HTMLTextAreaElement.prototype, 'value'
            ).set;
            nativeSet.call(el, text);
            el.dispatchEvent(new Event('input', {{ bubbles: true }}));
            el.dispatchEvent(new Event('change', {{ bubbles: true }}));
        }} else {{
            // Lexical listens to beforeinput/InputEvent. execCommand('insertText')
            // fires a proper InputEvent that Lexical converts into editor state.
            try {{
                // Clear any existing content first
                var sel = window.getSelection();
                var range = document.createRange();
                range.selectNodeContents(el);
                sel.removeAllRanges();
                sel.addRange(range);
                document.execCommand('delete', false, null);
            }} catch (e) {{}}
            var ok = false;
            try {{ ok = document.execCommand('insertText', false, text); }} catch (e) {{ ok = false; }}
            if (!ok) {{
                // Fallback: dispatch a synthetic InputEvent
                el.dispatchEvent(new InputEvent('beforeinput', {{ bubbles: true, cancelable: true, inputType: 'insertText', data: text }}));
                el.dispatchEvent(new InputEvent('input', {{ bubbles: true, inputType: 'insertText', data: text }}));
            }}
        }}

        // Give Lexical a tick to commit state, then click the Send button.
        return new Promise(function(resolve) {{
            setTimeout(function() {{
                var btns = document.querySelectorAll('button');
                var clicked = false;
                for (var j = 0; j < btns.length; j++) {{
                    var label = (btns[j].getAttribute('aria-label') || '').toLowerCase();
                    if (label === 'send message' || label.indexOf('send') !== -1) {{
                        if (!btns[j].disabled) {{ btns[j].click(); clicked = true; break; }}
                    }}
                }}
                if (!clicked) {{
                    // Fallback: simulate Enter key on the editor
                    el.dispatchEvent(new KeyboardEvent('keydown', {{ bubbles: true, key: 'Enter', code: 'Enter', keyCode: 13, which: 13 }}));
                }}
                resolve(clicked ? 'injected' : 'injected_enter');
            }}, 150);
        }});
    }})()
    """
    
    payload = json.dumps({
        "id": 42,
        "method": "Runtime.evaluate",
        "params": {
            "expression": js_code,
            "returnByValue": True,
            "awaitPromise": True
        }
    })
    
    payload_bytes = payload.encode('utf-8')
    length = len(payload_bytes)

    header = bytearray()
    header.append(0x81)  # FIN + text frame opcode

    # WebSocket frame length encoding (client frames MUST be masked -> 0x80 on length byte)
    if length <= 125:
        header.append(length | 0x80)
    elif length <= 65535:
        header.append(126 | 0x80)
        header.extend(length.to_bytes(2, byteorder='big'))
    else:
        header.append(127 | 0x80)
        header.extend(length.to_bytes(8, byteorder='big'))

    mask_key = os.urandom(4)
    header.extend(mask_key)
    
    masked_payload = bytearray(length)
    for i in range(length):
        masked_payload[i] = payload_bytes[i] ^ mask_key[i % 4]
        
    s.sendall(header + masked_payload)

    # Read server reply frame to learn whether the injection found an input element.
    result = None
    try:
        s.settimeout(3.0)
        resp = s.recv(65535)
        if resp and len(resp) >= 2:
            payload_len = resp[1] & 0x7F
            offset = 2
            if payload_len == 126:
                offset = 4
            elif payload_len == 127:
                offset = 10
            frame_data = resp[offset:]
            try:
                parsed_resp = json.loads(frame_data.decode('utf-8', errors='ignore'))
                result = parsed_resp.get("result", {}).get("result", {}).get("value")
            except Exception:
                result = None
    except Exception:
        result = None
    finally:
        s.close()

    return result


# ============================================================
#  CdpSession — multi-command CDP over a single WebSocket.
#  Needed for DOM.* command chains (setFileInputFiles) and
#  for reading model lists. Frames are masked client->server
#  per RFC6455; server frames are unmasked.
# ============================================================
class CdpSession:
    def __init__(self, ws_url):
        parsed = urllib.parse.urlparse(ws_url)
        self.host = parsed.hostname
        self.port = parsed.port
        path = parsed.path
        if parsed.query:
            path += "?" + parsed.query
        self.path = path
        self._id = 0
        self._buf = b""
        self.sock = None

    def __enter__(self):
        s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        s.connect((self.host, self.port))
        key = base64.b64encode(os.urandom(16)).decode("utf-8")
        handshake = (
            f"GET {self.path} HTTP/1.1\r\n"
            f"Host: {self.host}:{self.port}\r\n"
            f"Upgrade: websocket\r\n"
            f"Connection: Upgrade\r\n"
            f"Sec-WebSocket-Key: {key}\r\n"
            f"Sec-WebSocket-Version: 13\r\n\r\n"
        )
        s.sendall(handshake.encode("utf-8"))
        res = b""
        while b"\r\n\r\n" not in res:
            res += s.recv(1024)
        s.settimeout(10.0)
        self.sock = s
        return self

    def __exit__(self, *a):
        try:
            if self.sock:
                self.sock.close()
        except Exception:
            pass

    def _send(self, method, params=None):
        self._id += 1
        payload = json.dumps({"id": self._id, "method": method, "params": params or {}}).encode("utf-8")
        L = len(payload)
        header = bytearray([0x81])
        if L <= 125:
            header.append(L | 0x80)
        elif L <= 65535:
            header.append(126 | 0x80)
            header.extend(L.to_bytes(2, "big"))
        else:
            header.append(127 | 0x80)
            header.extend(L.to_bytes(8, "big"))
        mask = os.urandom(4)
        header.extend(mask)
        masked = bytearray(L)
        for i in range(L):
            masked[i] = payload[i] ^ mask[i % 4]
        self.sock.sendall(header + masked)
        return self._id

    def _read_frame(self):
        # Read one complete (unmasked) text frame from the server.
        def need(n):
            while len(self._buf) < n:
                chunk = self.sock.recv(65535)
                if not chunk:
                    raise ConnectionError("ws closed")
                self._buf += chunk
        need(2)
        b1, b2 = self._buf[0], self._buf[1]
        plen = b2 & 0x7F
        offset = 2
        if plen == 126:
            need(4)
            plen = int.from_bytes(self._buf[2:4], "big")
            offset = 4
        elif plen == 127:
            need(10)
            plen = int.from_bytes(self._buf[2:10], "big")
            offset = 10
        need(offset + plen)
        data = self._buf[offset:offset + plen]
        self._buf = self._buf[offset + plen:]
        return data

    def call(self, method, params=None):
        """Send a command and return the matching result dict."""
        cmd_id = self._send(method, params)
        for _ in range(200):
            data = self._read_frame()
            try:
                msg = json.loads(data.decode("utf-8", errors="ignore"))
            except Exception:
                continue
            if msg.get("id") == cmd_id:
                if "error" in msg:
                    raise RuntimeError(msg["error"])
                return msg.get("result", {})
        raise TimeoutError(f"No CDP response for {method}")

    def eval(self, expression, await_promise=True):
        r = self.call("Runtime.evaluate", {
            "expression": expression,
            "returnByValue": True,
            "awaitPromise": await_promise,
        })
        return r.get("result", {}).get("value")


def with_session(fn):
    """Run fn(session) on a fresh CDP session; returns dict or error dict."""
    ws_url = discover_ws_url()
    if not ws_url:
        return {"ok": False, "reason": "IDE debug port 9333 not responsive"}
    try:
        with CdpSession(ws_url) as sess:
            return fn(sess)
    except Exception as e:
        return {"ok": False, "reason": str(e)}


# ---- Model list / switch ----
JS_LIST_MODELS = r"""
(function(){
  var btn = document.querySelector('button[aria-label^="Select model, current:"]');
  if(!btn) return JSON.stringify({current:null, models:[], err:'no_model_button'});
  var label = btn.getAttribute('aria-label')||'';
  var current = label.replace('Select model, current:','').trim();
  return JSON.stringify({current: current, models: [], opened: false});
})()
"""

JS_OPEN_AND_LIST = r"""
(function(){
  return new Promise(function(resolve){
    var btn = document.querySelector('button[aria-label^="Select model, current:"]');
    if(!btn){ resolve(JSON.stringify({current:null, models:[], err:'no_model_button'})); return; }
    var label = btn.getAttribute('aria-label')||'';
    var current = label.replace('Select model, current:','').trim();
    btn.click();
    setTimeout(function(){
      // Antigravity model picker = Tailwind popup of full-width left-aligned <button>,
      // NOT a Monaco quick-input widget. Each model is its own selectable button.
      var items = Array.prototype.slice.call(
        document.querySelectorAll('button.select-none, button[class*="w-full"][class*="text-left"]')
      ).filter(function(b){ return b.offsetParent !== null; });
      var rx = /claude|gemini|gpt|opus|sonnet|flash|o3|o1|deepseek|grok/i;
      var models = [];
      for(var i=0;i<items.length;i++){
        // first text line = model name (drop trailing "Fast"/tier hint lines)
        var raw=(items[i].innerText||'').trim();
        var t=raw.split('\n')[0].trim();
        if(t && rx.test(t) && t.length<60 && models.indexOf(t)===-1) models.push(t);
      }
      // close the popup
      document.body.dispatchEvent(new KeyboardEvent('keydown',{bubbles:true,key:'Escape',code:'Escape',keyCode:27,which:27}));
      resolve(JSON.stringify({current: current, models: models}));
    }, 700);
  });
})()
"""

def js_switch_model(model_name):
    lit = json.dumps(model_name)
    return r"""
(function(){
  return new Promise(function(resolve){
    var target = """ + lit + r""";
    var btn = document.querySelector('button[aria-label^="Select model, current:"]');
    if(!btn){ resolve('no_model_button'); return; }
    btn.click();
    setTimeout(function(){
      var items = Array.prototype.slice.call(
        document.querySelectorAll('button.select-none, button[class*="w-full"][class*="text-left"]')
      ).filter(function(b){ return b.offsetParent !== null; });
      var tl = target.toLowerCase();
      var hit = null;
      for(var i=0;i<items.length;i++){
        var t=((items[i].innerText||'').split('\n')[0]||'').trim().toLowerCase();
        if(t.indexOf(tl)!==-1){ hit=items[i]; break; }
      }
      if(hit){ hit.click(); resolve('switched'); }
      else {
        document.body.dispatchEvent(new KeyboardEvent('keydown',{bubbles:true,key:'Escape',code:'Escape',keyCode:27,which:27}));
        resolve('model_not_found');
      }
    }, 700);
  });
})()
"""

JS_STOP = r"""
(function(){
  var btn = document.querySelector('button[aria-label^="Cancel"]');
  if(btn){ btn.click(); return 'stopped'; }
  return 'no_cancel_button';
})()
"""

JS_CLEAR = r"""
(function(){
  var el = document.querySelector('[aria-label="Message input"]') || document.querySelector('[data-lexical-editor="true"]');
  if(!el) return 'no_input';
  el.focus();
  var sel = window.getSelection(); var range = document.createRange();
  range.selectNodeContents(el); sel.removeAllRanges(); sel.addRange(range);
  document.execCommand('delete', false, null);
  return 'cleared';
})()
"""

# Auto-allow: locate an Allow button, verify a matching Deny exists in the same
# container (guard against false-positives), then click Allow. Prefers "Allow once".
JS_AUTO_ALLOW = r"""
(function() {
    var ALLOW_ONCE = ['allow once', 'allow one time', '今回のみ許可', '1回のみ許可'];
    var ALWAYS_ALLOW = ['allow this conversation', 'allow this chat', 'always allow', '常に許可'];
    var ALLOW = ['allow', 'permit', 'accept', 'run command', '許可', '承認'];
    var DENY = ['deny', 'reject', 'decline', 'cancel', '拒否'];
    var norm = function(s){ return (s||'').toLowerCase().replace(/\s+/g,' ').trim(); };
    var all = Array.prototype.slice.call(document.querySelectorAll('button'))
        .filter(function(b){ return b.offsetParent !== null; });
    var approve = all.find(function(b){
        var t = norm(b.textContent);
        return ALLOW_ONCE.some(function(p){ return t.indexOf(p) !== -1; });
    });
    if (!approve) {
        approve = all.find(function(b){
            var t = norm(b.textContent);
            var isAlways = ALWAYS_ALLOW.some(function(p){ return t.indexOf(p) !== -1; });
            return !isAlways && ALLOW.some(function(p){ return t.indexOf(p) !== -1; });
        });
    }
    if (!approve) return 'no_approval';
    var container = approve.closest('[role="dialog"], .modal, .dialog, .monaco-dialog-box')
        || (approve.parentElement && approve.parentElement.parentElement)
        || approve.parentElement || document.body;
    var cbtns = Array.prototype.slice.call(container.querySelectorAll('button'))
        .filter(function(b){ return b.offsetParent !== null; });
    var deny = cbtns.find(function(b){
        var t = norm(b.textContent);
        return DENY.some(function(p){ return t.indexOf(p) !== -1; });
    });
    if (!deny) return 'no_deny_guard';
    approve.click();
    return 'allowed';
})()
"""

# Auto-accept edits: prefer Composer "Accept all" span, fall back to accept/apply buttons.
JS_AUTO_ACCEPT = r"""
(function() {
    var accepted = 0;
    var spans = document.querySelectorAll('span, button');
    for (var i = 0; i < spans.length; i++) {
        var t = (spans[i].innerText || '').trim().toLowerCase();
        if (t === 'accept all') { spans[i].click(); return 1; }
    }
    var btns = document.querySelectorAll('button');
    for (var j = 0; j < btns.length; j++) {
        var text = (btns[j].innerText || '').toLowerCase();
        var label = (btns[j].getAttribute('aria-label') || '').toLowerCase();
        if (text.includes('accept') || text.includes('apply') ||
            label.includes('accept') || label.includes('apply')) {
            btns[j].click(); accepted++;
        }
    }
    return accepted;
})()
"""

# Read an interactive multiple-choice prompt (radiogroup + Submit) if present.
JS_READ_OPTIONS = r"""
(function() {
    var rg = document.querySelector('[role="radiogroup"]');
    var radios = Array.prototype.slice.call(document.querySelectorAll('input[type="radio"]'))
        .filter(function(r){ return r.offsetParent !== null; });
    if (!rg && radios.length === 0) return JSON.stringify({ has_options: false });

    var clean = function(s){
        return (s||'').replace(/\s+/g,' ').trim();
    };
    var options = radios.map(function(r, i){
        // climb to the smallest enclosing element with sensible text,
        // but stop before we swallow sibling options (cap length).
        var txt = '', node = r;
        for (var d = 0; d < 4 && node; d++) {
            var t = clean(node.textContent);
            if (t.length > 1 && t.length < 80) { txt = t; break; }
            node = node.parentElement;
        }
        // strip a leading enumerator like "1", "2)" that the UI prepends
        txt = txt.replace(/^(\d+)[\).:]?\s*/, '');
        // a free-text "other" radio has no own label -> mark it clearly
        if (!txt || txt.length >= 80) txt = '';
        return { idx: i, text: txt, checked: !!r.checked };
    });
    var submit = Array.prototype.slice.call(document.querySelectorAll('button'))
        .find(function(b){ return /submit/i.test(b.textContent||'') && b.offsetParent !== null; });
    return JSON.stringify({
        has_options: true,
        options: options,
        submit_disabled: submit ? !!submit.disabled : null,
        submit_found: !!submit
    });
})()
"""

def js_select_option(index, submit=True):
    """Click the radio option at `index`, then optionally click Submit."""
    return r"""
(function() {
  return new Promise(function(resolve){
    var radios = Array.prototype.slice.call(document.querySelectorAll('input[type="radio"]'))
        .filter(function(r){ return r.offsetParent !== null; });
    var idx = """ + str(int(index)) + r""";
    if (idx < 0 || idx >= radios.length) { resolve('bad_index'); return; }
    var r = radios[idx];
    // click the label/row so the IDE's React state updates, not just the input
    var row = r.closest('label') || r.parentElement || r;
    row.click();
    r.checked = true;
    r.dispatchEvent(new Event('change', { bubbles: true }));
    r.dispatchEvent(new Event('input', { bubbles: true }));
    var doSubmit = """ + ("true" if submit else "false") + r""";
    if (!doSubmit) { resolve('selected'); return; }
    setTimeout(function(){
      var btn = Array.prototype.slice.call(document.querySelectorAll('button'))
          .find(function(b){ return /submit/i.test(b.textContent||'') && b.offsetParent !== null; });
      if (!btn) { resolve('selected_no_submit'); return; }
      if (btn.disabled) { resolve('submit_disabled'); return; }
      btn.click();
      resolve('submitted');
    }, 250);
  });
})()
"""

def attach_images_via_cdp(file_paths):
    """Set image files into the IDE hidden file input via CDP DOM domain."""
    def _run(sess):
        # Click "Add context" first to ensure the file input is wired up (best-effort).
        sess.eval(r"""
            (function(){
              var b=document.querySelector('button[aria-label="Add context"]');
              if(b){ /* don't click: opens a menu. just ensure input exists */ }
              return 'ok';
            })()
        """, await_promise=False)
        doc = sess.call("DOM.getDocument", {"depth": -1})
        root = doc.get("root", {}).get("nodeId")
        node = sess.call("DOM.querySelector", {"nodeId": root, "selector": 'input[type="file"]'})
        node_id = node.get("nodeId")
        if not node_id:
            return {"ok": False, "reason": "no file input in IDE"}
        sess.call("DOM.setFileInputFiles", {"nodeId": node_id, "files": file_paths})
        return {"ok": True, "attached": len(file_paths)}
    return with_session(_run)


# ─── SSE Broadcast ────────────────────────────────────────────────────────────
# Clients subscribe to GET /api/events (Server-Sent Events).
# Any coordination state change triggers a broadcast to all connected clients.

_sse_clients: list = []        # list of queue.Queue
_sse_lock = threading.Lock()

def _sse_broadcast(event: str, data: dict):
    """Push an SSE event to all connected clients."""
    payload = f"event: {event}\ndata: {json.dumps(data, ensure_ascii=False)}\n\n"
    with _sse_lock:
        dead = []
        for q in _sse_clients:
            try:
                q.put_nowait(payload)
            except Exception:
                dead.append(q)
        for q in dead:
            _sse_clients.remove(q)

# Monkey-patch Coordinator to emit SSE on every state change
_original_write = None

def _pid_alive(pid):
    """True if a process with this PID is currently running (Windows tasklist)."""
    try:
        out = subprocess.run(
            ["tasklist", "/FI", f"PID eq {int(pid)}", "/NH"],
            capture_output=True, text=True, timeout=5
        ).stdout
        return str(int(pid)) in out
    except Exception:
        return None  # unknown -> caller falls back to heartbeat timeout

# Map agent_id -> liveness probe. Returns True/False (definitive) or None (unknown).
# This lets the server decide "live" from a REAL signal at view time, instead of
# requiring each agent to ping a heartbeat on a timer (webhook-style / pull model).
def _probe_pipo_hermes():
    """Pipo is live if its Hermes gateway process (gateway.pid) is running."""
    try:
        import json as _json
        pid_file = r"C:\Users\thang\AppData\Local\hermes\gateway.pid"
        if not os.path.isfile(pid_file):
            return None
        with open(pid_file, "r", encoding="utf-8") as f:
            pid = _json.load(f).get("pid")
        return _pid_alive(pid) if pid else None
    except Exception:
        return None

def _probe_antigravity_ide():
    """The IDE agent is live if CDP debug port 9333 answers."""
    return bool(discover_ws_url())

_AGENT_PROBES = {
    "antigravity-ide": _probe_antigravity_ide,
    "pipo-hermes": _probe_pipo_hermes,
}

def _apply_live_probes(agents):
    """Override the heartbeat-based `stale` flag with a real liveness probe
    for agents we know how to check. Agents without a probe keep the
    heartbeat-timeout result. Mutates and returns the dict."""
    for aid, info in agents.items():
        probe = _AGENT_PROBES.get(aid)
        if not probe:
            continue
        try:
            live = probe()
        except Exception:
            live = None
        if live is None:
            continue  # probe inconclusive -> trust heartbeat timeout
        info["stale"] = not live
        info["status"] = "active" if live else info.get("status", "offline")
        info["probed"] = True
    return agents


# ── Catalog of AI agents installed on THIS machine ──────────────
# Each entry: how to detect it (a CLI on PATH, or a known exe path) + the
# coordination agent_id it maps to + a default role + icon. The Agents tab
# lists these so Bố can toggle any of them into/out of the constellation.
_LOCALAPPDATA = os.environ.get("LOCALAPPDATA", r"C:\Users\thang\AppData\Local")
_APPDATA = os.environ.get("APPDATA", r"C:\Users\thang\AppData\Roaming")

_AGENT_CATALOG = [
    {"agent_id": "claude-code", "label": "Claude Code", "role": "builder",
     "icon": "🟠", "kind": "cli", "cmd": "claude"},
    {"agent_id": "codex", "label": "OpenAI Codex", "role": "builder",
     "icon": "🟢", "kind": "cli", "cmd": "codex"},
    {"agent_id": "gemini", "label": "Gemini CLI", "role": "researcher",
     "icon": "🔵", "kind": "cli", "cmd": "gemini"},
    {"agent_id": "opencode", "label": "OpenCode", "role": "builder",
     "icon": "🟣", "kind": "cli", "cmd": "opencode"},
    {"agent_id": "cline", "label": "Cline", "role": "builder",
     "icon": "🟤", "kind": "cli", "cmd": "cline"},
    {"agent_id": "antigravity-ide", "label": "Antigravity IDE", "role": "builder",
     "icon": "🟡", "kind": "exe",
     "path": os.path.join(_LOCALAPPDATA, "Programs", "Antigravity IDE", "Antigravity IDE.exe")},
    {"agent_id": "cursor", "label": "Cursor", "role": "builder",
     "icon": "⚪", "kind": "exe",
     "path": os.path.join("C:\\", "Program Files", "cursor", "resources", "app", "bin", "cursor")},
    {"agent_id": "pipo-hermes", "label": "Pipo (Hermes)", "role": "orchestrator",
     "icon": "🧠", "kind": "always"},
]

# npm global dir — many CLIs are shims here even if not on the probe's PATH.
_NPM_DIR = os.path.join(_APPDATA, "npm")


def _cli_installed(cmd):
    """True if a CLI is reachable on PATH or as an npm global shim."""
    import shutil
    if shutil.which(cmd):
        return True
    for ext in ("", ".cmd", ".exe", ".ps1", ".bat"):
        if os.path.isfile(os.path.join(_NPM_DIR, cmd + ext)):
            return True
    return False


def _detect_machine_agents():
    """Return the agent catalog annotated with installed/enabled/live state.

    - installed: the tool actually exists on this machine
    - enabled: the agent is currently registered in coordination state
    - live: real liveness probe result (when available)
    """
    registered = {}
    try:
        registered = _apply_live_probes(COORD.get_agents()) if COORD else {}
    except Exception:
        registered = {}

    out = []
    for spec in _AGENT_CATALOG:
        kind = spec.get("kind")
        if kind == "cli":
            installed = _cli_installed(spec["cmd"])
        elif kind == "exe":
            installed = os.path.isfile(spec.get("path", ""))
        else:  # always (e.g. the host orchestrator)
            installed = True
        reg = registered.get(spec["agent_id"])
        enabled = reg is not None
        live = None
        if reg is not None:
            live = not reg.get("stale", True)
        out.append({
            "agent_id": spec["agent_id"],
            "label": spec["label"],
            "role": spec.get("role", "builder"),
            "icon": spec.get("icon", "⚪"),
            "installed": installed,
            "enabled": enabled,
            "live": live,
            "detail": spec.get("cmd") or spec.get("path") or "host process",
        })
    return out


def _patch_coordinator():
    global _original_write
    from coordination import Coordinator
    _original_write = Coordinator._write_state

    def _patched_write(self, state):
        _original_write(self, state)
        _sse_broadcast("coord_update", {
            "agents": len(state.get("agents", {})),
            "locks": len(state.get("file_locks", {})),
            "tasks": len(state.get("task_queue", [])),
            "messages": len(state.get("messages", [])),
            "ts": time.time(),
        })

    Coordinator._write_state = _patched_write


class CustomHandler(SimpleHTTPRequestHandler):
    def log_message(self, format, *args):
        # Silence HTTP server logs to keep terminal clean
        pass

    def _client_is_local(self):
        """True only for genuine same-machine requests. A request arriving via
        the cloudflared tunnel also hits the socket from 127.0.0.1, so we must
        NOT treat it as local — detect the proxy by Cloudflare/forwarding headers
        and the public Host. Otherwise the tunnel would bypass auth entirely."""
        try:
            ip = self.client_address[0]
        except Exception:
            return False
        if ip not in ("127.0.0.1", "::1", "localhost"):
            return False
        # Proxy/tunnel fingerprints => treat as REMOTE even though socket is local.
        for h in ("Cf-Connecting-Ip", "Cf-Ray", "X-Forwarded-For", "Cdn-Loop"):
            if self.headers.get(h):
                return False
        host = (self.headers.get("Host") or "").lower()
        if host.startswith("localhost") or host.startswith("127.0.0.1"):
            return True
        # Unknown host with no proxy headers but local socket — be safe: not local.
        return host == "" or host.startswith("[::1]")

    def _check_auth(self):
        """Gate /api/* for non-localhost callers when API_TOKEN is set.
        Returns True if allowed. Sends 401 and returns False if rejected.
        - Auth OFF when API_TOKEN empty (backward compatible).
        - Localhost always allowed (Bố dùng máy này trực tiếp).
        - Only /api/ paths are protected; static UI files load freely.
        """
        if not API_TOKEN:
            return True
        path = urllib.parse.urlparse(self.path).path
        if not path.startswith("/api/"):
            return True
        if self._client_is_local():
            return True
        # Token from header or ?token=
        tok = self.headers.get("X-Synapz-Token", "")
        if not tok:
            qs = urllib.parse.parse_qs(urllib.parse.urlparse(self.path).query)
            tok = (qs.get("token", [""])[0] or "")
        if tok and tok == API_TOKEN:
            return True
        self.send_response(401)
        self.send_header("Content-Type", "application/json")
        self.send_header("Access-Control-Allow-Origin", "*")
        self.end_headers()
        self.wfile.write(json.dumps({"ok": False, "error": "unauthorized"}).encode("utf-8"))
        return False

    def _json_ok(self, data):
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Access-Control-Allow-Origin", "*")
        self.end_headers()
        self.wfile.write(json.dumps(data, ensure_ascii=False).encode("utf-8"))

    def _json_err(self, code, msg):
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Access-Control-Allow-Origin", "*")
        self.end_headers()
        self.wfile.write(json.dumps({"ok": False, "error": msg}).encode("utf-8"))

    def do_OPTIONS(self):
        """CORS preflight — vscode-file:// cần OPTIONS trước POST."""
        self.send_response(204)
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
        self.send_header("Access-Control-Allow-Headers", "Content-Type, Authorization")
        self.send_header("Access-Control-Max-Age", "86400")
        self.end_headers()

    def handle_one_request(self):
        """Override để OPTIONS được dispatch đúng."""
        try:
            self.raw_requestline = self.rfile.readline(65537)
            if len(self.raw_requestline) > 65536:
                self.requestversion = "HTTP/1.0"
                self.send_error(414)
                return
            if not self.raw_requestline:
                self.close_connection = True
                return
            if not self.parse_request():
                return
            mname = "do_" + self.command
            if not hasattr(self, mname):
                self.send_error(501, f"Unsupported method ({self.command!r})")
                return
            method = getattr(self, mname)
            method()
            self.wfile.flush()
        except TimeoutError as e:
            self.log_error("Request timed out: %r", e)
            self.close_connection = True
        except (ConnectionAbortedError, ConnectionResetError, BrokenPipeError):
            # Browser đóng SSE/tab đột ngột — không phải lỗi thật, bỏ qua
            self.close_connection = True

    def _read_body(self):
        length = int(self.headers.get("Content-Length", 0))
        return json.loads(self.rfile.read(length).decode("utf-8")) if length else {}

    def do_GET(self):
        # CORS preflight từ vscode-file:// đôi khi đến qua do_GET với method override
        if self.command == "OPTIONS":
            self.do_OPTIONS()
            return
        if not self._check_auth():
            return
        parsed_path = urllib.parse.urlparse(self.path)
        if parsed_path.path == "/api/ide/status":
            ws_url = discover_ws_url()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            if ws_url:
                self.wfile.write(json.dumps({"ok": True, "ws_url": ws_url}).encode('utf-8'))
            else:
                self.wfile.write(json.dumps({"ok": False, "reason": "IDE debug port 9333 not responsive"}).encode('utf-8'))
            return
            
        elif parsed_path.path == "/api/ide/chat":
            history = get_conversation_history()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(json.dumps({"ok": True, "messages": history}).encode('utf-8'))
            return

        elif parsed_path.path == "/api/ide/localfile":
            # Serve a local image referenced by an Antigravity chat message.
            # Restricted to image files to avoid leaking arbitrary files.
            qs = urllib.parse.parse_qs(parsed_path.query)
            raw = (qs.get("path", [""])[0] or "").strip()
            try:
                fp = os.path.normpath(raw)
                ext = os.path.splitext(fp)[1].lower()
                allowed = {".png": "image/png", ".jpg": "image/jpeg", ".jpeg": "image/jpeg",
                           ".gif": "image/gif", ".webp": "image/webp", ".svg": "image/svg+xml"}
                if ext not in allowed:
                    self.send_response(415); self.end_headers()
                    self.wfile.write(b"unsupported file type"); return
                if not os.path.isfile(fp):
                    self.send_response(404); self.end_headers()
                    self.wfile.write(b"not found"); return
                with open(fp, "rb") as imgf:
                    data = imgf.read()
                self.send_response(200)
                self.send_header("Content-Type", allowed[ext])
                self.send_header("Cache-Control", "max-age=3600")
                self.send_header("Access-Control-Allow-Origin", "*")
                self.end_headers()
                self.wfile.write(data)
            except Exception as e:
                self.send_response(500); self.end_headers()
                self.wfile.write(str(e).encode("utf-8"))
            return

        elif parsed_path.path == "/api/ide/options":
            # Read an interactive multiple-choice prompt from the IDE, if shown.
            def _get_opts(sess):
                res_str = sess.eval(JS_READ_OPTIONS, await_promise=False)
                try:
                    return json.loads(res_str)
                except Exception:
                    return {"has_options": False, "error": str(res_str)}
            res = with_session(_get_opts)
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Access-Control-Allow-Origin", "*")
            self.end_headers()
            self.wfile.write(json.dumps(res).encode('utf-8'))
            return

        elif parsed_path.path == "/api/ide/models":
            def _get(sess):
                res_str = sess.eval(JS_OPEN_AND_LIST)
                try:
                    return json.loads(res_str)
                except Exception:
                    return {"current": None, "models": [], "error": str(res_str)}
            res = with_session(_get)
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(json.dumps(res).encode('utf-8'))
            return
            
        elif parsed_path.path == "/api/ide/files":
            # List project files for @-mention autocomplete. Cached & filtered.
            qs = urllib.parse.parse_qs(parsed_path.query)
            q = (qs.get("q", [""])[0] or "").strip().lower()
            try:
                files = _list_project_files()
                if q:
                    files = [f for f in files if q in f.lower()]
                files = files[:200]
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Access-Control-Allow-Origin", "*")
                self.end_headers()
                self.wfile.write(json.dumps({"files": files}).encode("utf-8"))
            except Exception as e:
                self.send_response(500); self.end_headers()
                self.wfile.write(str(e).encode("utf-8"))
            return

        elif parsed_path.path == "/api/ide/composer_changes":
            def _get_changes(sess):
                js_code = r"""
                (function() {
                    var spans = document.querySelectorAll('span');
                    var acceptBtn = null;
                    for(var i=0; i<spans.length; i++) {
                        if((spans[i].innerText || '').trim() === 'Accept all') {
                            acceptBtn = spans[i];
                            break;
                        }
                    }
                    if(!acceptBtn) return JSON.stringify({has_changes: false});
                    
                    var container = acceptBtn.parentElement;
                    for(var d=0; d<6; d++) {
                        if(container && (container.innerText || '').indexOf('Files With Changes') !== -1) {
                            break;
                        }
                        if(container) container = container.parentElement;
                    }
                    if(!container) return JSON.stringify({has_changes: false});
                    
                    var text = container.innerText || '';
                    var lines = text.split('\n').map(function(l){ return l.trim(); }).filter(Boolean);
                    
                    var files = [];
                    var title = "0 Files With Changes";
                    if(lines.length > 0) {
                        title = lines[0];
                    }
                    
                    for(var i=3; i<lines.length; i+=4) {
                        if(i+3 < lines.length) {
                            files.push({
                                added: lines[i],
                                removed: lines[i+1],
                                filename: lines[i+2],
                                filepath: lines[i+3]
                            });
                        }
                    }
                    return JSON.stringify({has_changes: true, title: title, files: files});
                })()
                """
                try:
                    res_str = sess.eval(js_code, await_promise=False)
                    return json.loads(res_str)
                except Exception as e:
                    return {"has_changes": False, "error": str(e)}
            res = with_session(_get_changes)
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Access-Control-Allow-Origin", "*")
            self.end_headers()
            self.wfile.write(json.dumps(res).encode('utf-8'))
            return
            
        # ─── Coordination API (GET) ──────────────────────────────
        elif parsed_path.path == "/api/events":
            # Server-Sent Events — push coord state changes to browser
            import queue as _queue_mod
            q = _queue_mod.Queue(maxsize=64)
            with _sse_lock:
                _sse_clients.append(q)
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Cache-Control", "no-cache")
            self.send_header("Connection", "keep-alive")
            self.send_header("Access-Control-Allow-Origin", "*")
            self.end_headers()
            # Send initial ping
            try:
                self.wfile.write(b"event: ping\ndata: {}\n\n")
                self.wfile.flush()
            except Exception:
                pass
            # Stream events until client disconnects
            try:
                while True:
                    try:
                        msg = q.get(timeout=20)
                        self.wfile.write(msg.encode("utf-8"))
                        self.wfile.flush()
                    except _queue_mod.Empty:
                        # Keepalive comment
                        self.wfile.write(b": keepalive\n\n")
                        self.wfile.flush()
            except Exception:
                pass
            finally:
                with _sse_lock:
                    if q in _sse_clients:
                        _sse_clients.remove(q)
            return

        elif parsed_path.path == "/api/coord/state":
            state = COORD.get_state()
            self._json_ok(state)
            return

        elif parsed_path.path == "/api/coord/agents":
            agents = _apply_live_probes(COORD.get_agents())
            self._json_ok(agents)
            return

        elif parsed_path.path == "/api/agents/catalog":
            # Tất cả agent AI cài trên máy + trạng thái enabled/live.
            self._json_ok({"agents": _detect_machine_agents()})
            return

        elif parsed_path.path == "/api/coord/locks":
            locks = COORD.get_locks()
            self._json_ok(locks)
            return

        elif parsed_path.path == "/api/coord/tasks":
            qs = urllib.parse.parse_qs(parsed_path.query)
            tasks = COORD.get_tasks(
                status=qs.get("status", [None])[0],
                assigned_to=qs.get("assigned_to", [None])[0],
            )
            self._json_ok(tasks)
            return

        elif parsed_path.path == "/api/coord/messages":
            qs = urllib.parse.parse_qs(parsed_path.query)
            msgs = COORD.get_messages(
                agent_id=qs.get("agent_id", [None])[0],
                unread_only=qs.get("unread", ["false"])[0] == "true",
                limit=int(qs.get("limit", ["50"])[0]),
            )
            self._json_ok(msgs)
            return

        elif parsed_path.path == "/api/coord/log":
            qs = urllib.parse.parse_qs(parsed_path.query)
            entries = COORD.get_log(
                limit=int(qs.get("limit", ["100"])[0]),
                category=qs.get("category", [None])[0],
                agent_id=qs.get("agent_id", [None])[0],
            )
            self._json_ok(entries)
            return

        elif parsed_path.path == "/api/coord/webhooks":
            webhooks = COORD.get_webhooks()
            self._json_ok(webhooks)
            return

        # Fallback to serving static files
        super().do_GET()

    def do_POST(self):
        if not self._check_auth():
            return
        parsed_path = urllib.parse.urlparse(self.path)
        if parsed_path.path == "/api/ide/chat":
            content_length = int(self.headers['Content-Length'])
            post_data = self.rfile.read(content_length)
            try:
                body = json.loads(post_data.decode('utf-8'))
                prompt = body.get("prompt", "")
                if not prompt:
                    self.send_response(400)
                    self.end_headers()
                    self.wfile.write(b"Prompt is empty")
                    return
                    
                ws_url = discover_ws_url()
                if not ws_url:
                    self.send_response(503)
                    self.send_header("Content-Type", "application/json")
                    self.end_headers()
                    self.wfile.write(json.dumps({"ok": False, "reason": "IDE debug port not responsive"}).encode('utf-8'))
                    return
                    
                inject_result = inject_prompt_via_cdp(ws_url, prompt)

                # Bind any images uploaded just before this prompt to it, so they
                # re-appear when history is rebuilt (the transcript drops them).
                commit_pending_attachments(prompt)

                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.end_headers()
                self.wfile.write(json.dumps({"ok": True, "inject": inject_result}).encode('utf-8'))
            except Exception as e:
                self.send_response(500)
                self.end_headers()
                self.wfile.write(str(e).encode('utf-8'))
            return

        elif parsed_path.path == "/api/ide/launch":
            # Start (or restart) Antigravity IDE with CDP debug port enabled.
            # body: {"force": true} -> kill & relaunch even if already running.
            try:
                content_length = int(self.headers.get('Content-Length') or 0)
                body = {}
                if content_length:
                    body = json.loads(self.rfile.read(content_length).decode('utf-8') or "{}")
                force = bool(body.get("force", False))
            except Exception:
                force = False
            res = launch_antigravity_cdp(force_restart=force)
            self.send_response(200 if res.get("ok") else 503)
            self.send_header("Content-Type", "application/json")
            self.send_header("Access-Control-Allow-Origin", "*")
            self.end_headers()
            self.wfile.write(json.dumps(res).encode('utf-8'))
            return

        elif parsed_path.path == "/api/orchestrator/dispatch":
            # Giao 1 task cho mọi agent Connected qua synapz-orchestrator --live --json.
            # body: {"prompt": "..."}
            try:
                content_length = int(self.headers.get('Content-Length') or 0)
                body = json.loads(self.rfile.read(content_length).decode('utf-8') or "{}") if content_length else {}
                prompt = (body.get("prompt") or "").strip()
                if not prompt:
                    self.send_response(400)
                    self.send_header("Content-Type", "application/json")
                    self.end_headers()
                    self.wfile.write(json.dumps({"ok": False, "reason": "prompt rỗng"}).encode('utf-8'))
                    return
                res = run_orchestrator_dispatch(prompt)
                self.send_response(200 if res.get("ok") else 503)
                self.send_header("Content-Type", "application/json")
                self.send_header("Access-Control-Allow-Origin", "*")
                self.end_headers()
                self.wfile.write(json.dumps(res).encode('utf-8'))
            except Exception as e:
                self.send_response(500)
                self.send_header("Content-Type", "application/json")
                self.end_headers()
                self.wfile.write(json.dumps({"ok": False, "reason": str(e)}).encode('utf-8'))
            return

        elif parsed_path.path == "/api/orchestrator/pipeline":
            # Chạy TRỌN 4 giai đoạn parallel orchestration.
            # body: {"graph": {"nodes":[...]}, "echo": true}
            try:
                content_length = int(self.headers.get('Content-Length') or 0)
                body = json.loads(self.rfile.read(content_length).decode('utf-8') or "{}") if content_length else {}
                graph = body.get("graph")
                echo = bool(body.get("echo", True))
                if not graph or not isinstance(graph, dict) or not graph.get("nodes"):
                    self.send_response(400)
                    self.send_header("Content-Type", "application/json")
                    self.send_header("Access-Control-Allow-Origin", "*")
                    self.end_headers()
                    self.wfile.write(json.dumps({"ok": False, "reason": "thiếu graph.nodes"}).encode('utf-8'))
                    return
                res = run_orchestrator_pipeline(graph, echo=echo)
                self.send_response(200 if res.get("ok") else 503)
                self.send_header("Content-Type", "application/json")
                self.send_header("Access-Control-Allow-Origin", "*")
                self.end_headers()
                self.wfile.write(json.dumps(res, ensure_ascii=False).encode('utf-8'))
            except Exception as e:
                self.send_response(500)
                self.send_header("Content-Type", "application/json")
                self.end_headers()
                self.wfile.write(json.dumps({"ok": False, "reason": str(e)}).encode('utf-8'))
            return

        elif parsed_path.path == "/api/ide/model":
            content_length = int(self.headers['Content-Length'])
            post_data = self.rfile.read(content_length)
            try:
                body = json.loads(post_data.decode('utf-8'))
                model_name = body.get("model", "")
                if not model_name:
                    self.send_response(400)
                    self.end_headers()
                    self.wfile.write(b"Model name is empty")
                    return
                def _switch(sess):
                    res_str = sess.eval(js_switch_model(model_name))
                    return {"result": res_str}
                res = with_session(_switch)
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.end_headers()
                self.wfile.write(json.dumps(res).encode('utf-8'))
            except Exception as e:
                self.send_response(500)
                self.end_headers()
                self.wfile.write(str(e).encode('utf-8'))
            return

        elif parsed_path.path == "/api/ide/upload":
            content_length = int(self.headers['Content-Length'])
            post_data = self.rfile.read(content_length)
            try:
                body = json.loads(post_data.decode('utf-8'))
                files = body.get("files", [])
                if not files:
                    self.send_response(400)
                    self.end_headers()
                    self.wfile.write(b"No files provided")
                    return

                # Persist uploads permanently so they survive history re-render.
                # Unique-name each file; DON'T wipe old ones (the IDE keeps them).
                os.makedirs(ATTACH_DIR, exist_ok=True)

                abs_paths = []
                for idx, f in enumerate(files):
                    name = f.get("name", f"file_{idx}")
                    data_str = f.get("data", "")
                    if "," in data_str:
                        data_str = data_str.split(",", 1)[1]
                    file_data = base64.b64decode(data_str)
                    base, ext = os.path.splitext(name)
                    uniq = f"{int(time.time()*1000)}_{idx}_{base}{ext}"
                    dest_path = os.path.join(ATTACH_DIR, uniq)
                    with open(dest_path, "wb") as out_f:
                        out_f.write(file_data)
                    abs_paths.append(os.path.abspath(dest_path))

                # Queue for binding to the next /chat prompt.
                with _attach_lock:
                    _pending_attachments.extend(abs_paths)

                res = attach_images_via_cdp(abs_paths)

                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.end_headers()
                self.wfile.write(json.dumps(res).encode('utf-8'))
            except Exception as e:
                self.send_response(500)
                self.end_headers()
                self.wfile.write(str(e).encode('utf-8'))
            return

        elif parsed_path.path == "/api/ide/action":
            content_length = int(self.headers['Content-Length'])
            post_data = self.rfile.read(content_length)
            try:
                body = json.loads(post_data.decode('utf-8'))
                action = body.get("action", "")
                if action == "stop":
                    def _stop(sess):
                        res_str = sess.eval(JS_STOP)
                        return {"result": res_str}
                    res = with_session(_stop)
                elif action == "clear":
                    def _clear(sess):
                        res_str = sess.eval(JS_CLEAR)
                        return {"result": res_str}
                    res = with_session(_clear)
                else:
                    res = {"ok": False, "reason": f"unknown action: {action}"}
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.end_headers()
                self.wfile.write(json.dumps(res).encode('utf-8'))
            except Exception as e:
                self.send_response(500)
                self.end_headers()
                self.wfile.write(str(e).encode('utf-8'))
            return
            
        elif parsed_path.path == "/api/ide/composer_action":
            content_length = int(self.headers['Content-Length'])
            post_data = self.rfile.read(content_length)
            try:
                body = json.loads(post_data.decode('utf-8'))
                action = body.get("action", "")
                if action not in ("accept", "reject"):
                    self.send_response(400)
                    self.end_headers()
                    self.wfile.write(b"Invalid action")
                    return
                
                target_text = "Accept all" if action == "accept" else "Reject all"
                def _action(sess):
                    js_code = r"""
                    (function(){
                        var spans = document.querySelectorAll('span');
                        var targetText = """ + json.dumps(target_text) + r""";
                        var hit = null;
                        for(var i=0; i<spans.length; i++) {
                            if((spans[i].innerText || '').trim() === targetText) {
                                hit = spans[i];
                                break;
                            }
                        }
                        if(hit) {
                            hit.click();
                            return 'clicked';
                        }
                        return 'not_found';
                    })()
                    """
                    res_str = sess.eval(js_code, await_promise=False)
                    return {"result": res_str}
                res = with_session(_action)
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Access-Control-Allow-Origin", "*")
                self.end_headers()
                self.wfile.write(json.dumps(res).encode('utf-8'))
            except Exception as e:
                self.send_response(500)
                self.end_headers()
                self.wfile.write(str(e).encode('utf-8'))
            return

        elif parsed_path.path == "/api/ide/autopilot":
            # One-shot autopilot: auto-allow any approval dialog, then auto-accept edits.
            # body: {"allow": true, "accept": true}  (both default true)
            try:
                body = self._read_body()
                do_allow = body.get("allow", True)
                do_accept = body.get("accept", True)
                def _pilot(sess):
                    out = {}
                    if do_allow:
                        out["allow"] = sess.eval(JS_AUTO_ALLOW, await_promise=False)
                    if do_accept:
                        out["accept"] = sess.eval(JS_AUTO_ACCEPT, await_promise=False)
                    return out
                res = with_session(_pilot)
                self._json_ok({"ok": True, "result": res})
            except Exception as e:
                self._json_err(500, str(e))
            return

        elif parsed_path.path == "/api/ide/select_option":
            # Pick a multiple-choice option in the IDE and (optionally) submit.
            # body: {"index": 0, "submit": true}
            try:
                body = self._read_body()
                index = int(body.get("index", -1))
                do_submit = body.get("submit", True)
                def _sel(sess):
                    return sess.eval(js_select_option(index, do_submit))
                res = with_session(_sel)
                self._json_ok({"ok": True, "result": res})
            except Exception as e:
                self._json_err(500, str(e))
            return

        # ─── Coordination API (POST) ─────────────────────────────
        elif parsed_path.path == "/api/agents/toggle":
            # Bật/tắt 1 agent: bật = đăng ký vào coordination (heartbeat),
            # tắt = gỡ khỏi mạng lưới (deregister). body: {agent_id, enabled, role?}
            try:
                body = self._read_body()
                agent_id = body.get("agent_id")
                if not agent_id:
                    self._json_err(400, "agent_id required")
                    return
                enabled = bool(body.get("enabled"))
                if enabled:
                    COORD.heartbeat(
                        agent_id,
                        role=body.get("role", "builder"),
                        status="active",
                    )
                else:
                    COORD.deregister(agent_id)
                self._json_ok({"ok": True, "agent_id": agent_id, "enabled": enabled})
            except Exception as e:
                self._json_err(500, str(e))
            return

        elif parsed_path.path == "/api/coord/heartbeat":
            try:
                body = self._read_body()
                agent_id = body.get("agent_id")
                if not agent_id:
                    self._json_err(400, "agent_id required")
                    return
                result = COORD.heartbeat(
                    agent_id,
                    role=body.get("role", "unknown"),
                    status=body.get("status", "active"),
                    capabilities=body.get("capabilities"),
                    current_task=body.get("current_task"),
                    parent_id=body.get("parent_id"),
                )
                self._json_ok({"ok": True, "agent": result})
            except Exception as e:
                self._json_err(500, str(e))
            return

        elif parsed_path.path == "/api/coord/deregister":
            try:
                body = self._read_body()
                removed = COORD.deregister(body.get("agent_id", ""))
                self._json_ok({"ok": True, "removed": removed})
            except Exception as e:
                self._json_err(500, str(e))
            return

        elif parsed_path.path == "/api/coord/ack-rules":
            try:
                body = self._read_body()
                agent_id = body.get("agent_id")
                rules_version = body.get("rules_version")
                if not agent_id or not rules_version:
                    self._json_err(400, "agent_id and rules_version required")
                    return
                result = COORD.ack_rules(agent_id, rules_version)
                if result.get("ok"):
                    self._json_ok(result)
                else:
                    self._json_err(400, result.get("error", "ack failed"))
            except Exception as e:
                self._json_err(500, str(e))
            return

        elif parsed_path.path == "/api/coord/rules":
            from coordination import build_rules_payload, RULES_VERSION
            self._json_ok({"rules_version": RULES_VERSION, "roles": {r: build_rules_payload(r) for r in ["orchestrator","builder","researcher","reviewer","tester"]}})
            return

        elif parsed_path.path == "/api/coord/claim":
            try:
                body = self._read_body()
                agent_id = body.get("agent_id")
                file_path = body.get("file_path")
                if not agent_id or not file_path:
                    self._json_err(400, "agent_id and file_path required")
                    return
                # Permission check
                allowed, reason = COORD.check_permission(agent_id, "can_claim_lock")
                if not allowed:
                    self._json_err(403, f"Permission denied: {reason}")
                    return
                result = COORD.claim_file(agent_id, file_path)
                self._json_ok(result)
            except Exception as e:
                self._json_err(500, str(e))
            return

        elif parsed_path.path == "/api/coord/release":
            try:
                body = self._read_body()
                released = COORD.release_file(body.get("agent_id", ""), body.get("file_path", ""))
                self._json_ok({"ok": True, "released": released})
            except Exception as e:
                self._json_err(500, str(e))
            return

        elif parsed_path.path == "/api/coord/task":
            try:
                body = self._read_body()
                title = body.get("title")
                if not title:
                    self._json_err(400, "title required")
                    return
                # Permission: chỉ orchestrator mới được assign task cho agent khác
                posted_by = body.get("posted_by", "dashboard")
                assigned_to = body.get("assigned_to")
                if posted_by != "dashboard" and assigned_to:
                    allowed, reason = COORD.check_permission(posted_by, "can_assign_task")
                    if not allowed:
                        self._json_err(403, f"Permission denied: {reason}")
                        return
                task = COORD.post_task(
                    title=title,
                    description=body.get("description", ""),
                    assigned_to=assigned_to,
                    priority=body.get("priority", 5),
                    posted_by=body.get("posted_by", "dashboard"),
                    checklist=body.get("checklist"),
                )
                self._json_ok({"ok": True, "task": task})
            except Exception as e:
                self._json_err(500, str(e))
            return

        elif parsed_path.path == "/api/coord/checklist":
            # Tick/untick một bước checklist. body: {task_id, item_id, done}
            try:
                body = self._read_body()
                task_id = body.get("task_id")
                item_id = body.get("item_id")
                if not task_id or not item_id:
                    self._json_err(400, "task_id and item_id required")
                    return
                task = COORD.update_checklist_item(task_id, item_id, bool(body.get("done", False)))
                if task is None:
                    self._json_err(404, "task/item not found")
                    return
                self._json_ok({"ok": True, "task": task})
            except Exception as e:
                self._json_err(500, str(e))
            return

        elif parsed_path.path == "/api/coord/task/update":
            try:
                body = self._read_body()
                task_id = body.get("task_id")
                if not task_id:
                    self._json_err(400, "task_id required")
                    return
                task = COORD.update_task(
                    task_id=task_id,
                    status=body.get("status"),
                    assigned_to=body.get("assigned_to"),
                    note=body.get("note"),
                )
                self._json_ok({"ok": True, "task": task})
            except Exception as e:
                self._json_err(500, str(e))
            return

        elif parsed_path.path == "/api/coord/message":
            try:
                body = self._read_body()
                from_agent = body.get("from_agent")
                content = body.get("content")
                if not from_agent or not content:
                    self._json_err(400, "from_agent and content required")
                    return
                msg_type = body.get("type", "info")
                to_agent = body.get("to_agent")
                # Permission check
                if not to_agent:  # broadcast
                    allowed, reason = COORD.check_permission(from_agent, "can_broadcast")
                else:
                    allowed, reason = COORD.check_permission(from_agent, "can_message_any")
                    # builder được phép reply cho orchestrator / người assign task
                    if not allowed:
                        state = COORD._read_state()
                        sender = state["agents"].get(from_agent, {})
                        if sender.get("compliance") != "probation":
                            # builder chỉ cần không ở probation để reply
                            allowed, reason = True, "ok"
                if not allowed:
                    self._json_err(403, f"Permission denied: {reason}")
                    return
                msg = COORD.send_message(
                    from_agent=from_agent,
                    content=content,
                    to_agent=to_agent,
                    msg_type=msg_type,
                )
                auto_task = None
                completed_task = None
                if msg_type == "task" and to_agent:
                    # Giao việc → tạo task in_progress cho to_agent
                    title = body.get("task_title") or content[:80]
                    auto_task = COORD.post_task(
                        title=title,
                        description=content,
                        assigned_to=to_agent,
                        priority=body.get("priority", 5),
                        posted_by=from_agent,
                    )
                    COORD.update_task(auto_task["id"], status="in_progress", assigned_to=to_agent)
                    auto_task["status"] = "in_progress"
                elif msg_type in ("reply", "info", "done") and from_agent:
                    # Agent gửi reply → tự hoàn thành task in_progress của nó
                    state = COORD._read_state()
                    for task in reversed(state.get("task_queue", [])):
                        if (task.get("assigned_to") == from_agent
                                and task.get("status") == "in_progress"):
                            COORD.update_task(task["id"], status="done")
                            completed_task = task["id"]
                            break
                self._json_ok({"ok": True, "message": msg, "auto_task": auto_task, "completed_task": completed_task})
            except Exception as e:
                self._json_err(500, str(e))
            return

        elif parsed_path.path == "/api/coord/messages/read":
            try:
                body = self._read_body()
                count = COORD.mark_read(body.get("agent_id", ""), body.get("message_ids", []))
                self._json_ok({"ok": True, "marked": count})
            except Exception as e:
                self._json_err(500, str(e))
            return

        elif parsed_path.path == "/api/coord/log":
            try:
                body = self._read_body()
                entry = COORD.add_log(
                    agent_id=body.get("agent_id", "unknown"),
                    action=body.get("action", ""),
                    detail=body.get("detail", ""),
                    category=body.get("category", "general"),
                    tags=body.get("tags", []),
                )
                self._json_ok({"ok": True, "entry": entry})
            except Exception as e:
                self._json_err(500, str(e))
            return

        elif parsed_path.path == "/api/coord/webhook/register":
            try:
                body = self._read_body()
                agent_id = body.get("agent_id")
                url = body.get("url")
                if not agent_id or not url:
                    self._json_err(400, "agent_id and url required")
                    return
                result = COORD.register_webhook(agent_id, url, body.get("events"))
                self._json_ok({"ok": True, "webhook": result})
            except Exception as e:
                self._json_err(500, str(e))
            return

        elif parsed_path.path == "/api/coord/webhook/unregister":
            try:
                body = self._read_body()
                agent_id = body.get("agent_id")
                if not agent_id:
                    self._json_err(400, "agent_id required")
                    return
                result = COORD.unregister_webhook(agent_id)
                self._json_ok({"ok": True, "removed": result})
            except Exception as e:
                self._json_err(500, str(e))
            return

        elif parsed_path.path == "/api/coord/webhook/trigger":
            try:
                body = self._read_body()
                event = body.get("event")
                if not event:
                    self._json_err(400, "event type required")
                    return

                prompt = None
                if event == "task":
                    assigned_to = body.get("assigned_to")
                    if assigned_to in ("antigravity-ide", "builder"):
                        title = body.get("title", "No Title")
                        desc = body.get("description", "")
                        priority = body.get("priority", 5)
                        posted_by = body.get("posted_by", "unknown")

                        prompt = (
                            f"[AGENT COORDINATION: TASK ĐƯỢC GIAO]\n"
                            f"📌 Tiêu đề: {title}\n"
                            f"📝 Mô tả: {desc}\n"
                            f"👤 Người giao: {posted_by}\n"
                            f"⚡ Độ ưu tiên: {priority}\n\n"
                            f"Con hãy thực hiện task này nhé thưa Bố."
                        )
                elif event == "message":
                    to_agent = body.get("to")
                    # CHỈ inject vào IDE khi tin nhắn GỬI ĐÍCH DANH antigravity-ide.
                    # Broadcast (to=None) chỉ hiện ở khung chat realtime, KHÔNG dội
                    # vào chat IDE — nếu không mọi báo cáo nội bộ sẽ làm nhiễu phiên
                    # làm việc của Antigravity (đụng prompt gửi qua /api/ide/chat).
                    if to_agent in ("antigravity-ide", "builder"):
                        from_agent = body.get("from", "unknown")
                        if from_agent != "antigravity-ide":
                            content = body.get("content", "")
                            prompt = (
                                f"[AGENT COORDINATION: TIN NHẮN TỪ {from_agent.upper()}]\n"
                                f"💬 Nội dung: {content}\n\n"
                                f"Con hãy phản hồi hoặc xử lý tin nhắn này nhé thưa Bố."
                            )

                if prompt:
                    ws_url = discover_ws_url()
                    if not ws_url:
                        self._json_err(503, "IDE debug port not responsive")
                        return
                    inject_result = inject_prompt_via_cdp(ws_url, prompt)
                    self._json_ok({"ok": True, "injected": True, "result": inject_result})
                else:
                    self._json_ok({"ok": True, "injected": False, "reason": "Event ignored (not assigned to us or self-sent)"})
            except Exception as e:
                self._json_err(500, str(e))
            return

        self.send_response(404)
        self.end_headers()


class ThreadingHTTPServer(ThreadingMixIn, HTTPServer):
    daemon_threads = True


def main():
    global COORD
    # Make sure we are serving from the repository root
    repo_root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    os.chdir(repo_root)

    # Initialize coordination module
    COORD = Coordinator(os.path.join(repo_root, "data", "coordination"))
    _patch_coordinator()
    print("🤝 Coordination module initialized (SSE enabled)")

    # Auto-register webhook for antigravity-ide
    try:
        COORD.register_webhook(
            agent_id="antigravity-ide",
            url=f"http://127.0.0.1:{PORT}/api/coord/webhook/trigger",
            events=["task", "message"]
        )
        print(f"🔌 Auto-registered Antigravity IDE coordination webhook listener to port {PORT}")
    except Exception as e:
        print(f"⚠️ Failed to auto-register webhook: {e}")

    # NOTE: No heartbeat timer needed. Agent liveness is probed at view time in
    # /api/coord/agents (_apply_live_probes): antigravity-ide -> CDP 9333,
    # pipo-hermes -> Hermes gateway.pid process. Pull/probe model, not push/ping.

    server = ThreadingHTTPServer(('0.0.0.0', PORT), CustomHandler)
    print(f"🚀 SynapzCore Dashboard Server started on http://localhost:{PORT}")
    print(f"📂 Repository root: {repo_root}")
    print("💡 Connecting to Antigravity IDE at debug port: 9333")

    # ---- Telegram <-> Antigravity bridge (optional; reads telegram_bridge.json) ----
    try:
        import telegram_bridge
        telegram_bridge.init(
            inject_prompt=inject_prompt_via_cdp,
            discover_ws=discover_ws_url,
            get_history=get_conversation_history,
        )
        if telegram_bridge.start():
            print("📲 Telegram bridge started (Antigravity <-> Telegram).")
    except Exception as e:
        print(f"⚠️ Telegram bridge not started: {e}")

    try:
        server.serve_forever()
    except KeyboardInterrupt:
        print("\n👋 Server stopped. Goodbye!")
        sys.exit(0)

if __name__ == "__main__":
    main()
