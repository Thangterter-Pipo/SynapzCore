#!/usr/bin/env python3
"""
dashboard_server.py — Custom http server for SynapzCore Command Center.
Serves static files and proxies CDP commands/transcript logs for IDE integration.
"""

import os
import sys
import json
import socket
import urllib.parse
import urllib.request
import base64
import time
import threading
from http.server import SimpleHTTPRequestHandler, HTTPServer
from socketserver import ThreadingMixIn

PORT = 8899

# Multi-AI Coordination
from coordination import Coordinator
COORD = None  # initialized in main() after chdir

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
                        messages.append({
                            "role": "user",
                            "content": content,
                            "step_index": step_idx
                        })
                    # Parse agent response
                    elif data.get("source") == "MODEL" or data.get("type") in ("PLANNER_RESPONSE", "MODEL_RESPONSE"):
                        content = data.get("content", "") or ""
                        
                        # Formatting tool calls
                        tool_calls = data.get("tool_calls", [])
                        tool_desc = ""
                        if tool_calls:
                            tool_desc = "\n🔧 *Tool Calls:*\n" + "\n".join(
                                f"- {t.get('name')} ({json.dumps(t.get('arguments', {}))})"
                                for t in tool_calls
                            )
                        
                        full = (content + tool_desc).strip()
                        if not full:
                            continue
                        messages.append({
                            "role": "assistant",
                            "content": full,
                            "step_index": step_idx
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
      var items = document.querySelectorAll('[role="menuitem"],[role="option"]');
      var models = [];
      for(var i=0;i<items.length;i++){
        var t=(items[i].innerText||'').trim();
        if(t && t.length<60 && models.indexOf(t)===-1) models.push(t);
      }
      // close the menu
      document.body.dispatchEvent(new KeyboardEvent('keydown',{bubbles:true,key:'Escape',code:'Escape',keyCode:27,which:27}));
      resolve(JSON.stringify({current: current, models: models}));
    }, 350);
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
      var items = document.querySelectorAll('[role="menuitem"],[role="option"]');
      var hit = null;
      for(var i=0;i<items.length;i++){
        var t=(items[i].innerText||'').trim().toLowerCase();
        if(t.indexOf(target.toLowerCase())!==-1){ hit=items[i]; break; }
      }
      if(hit){ hit.click(); resolve('switched'); }
      else {
        document.body.dispatchEvent(new KeyboardEvent('keydown',{bubbles:true,key:'Escape',code:'Escape',keyCode:27,which:27}));
        resolve('model_not_found');
      }
    }, 350);
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

    def _json_ok(self, data):
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Access-Control-Allow-Origin", "*")
        self.end_headers()
        self.wfile.write(json.dumps(data, ensure_ascii=False).encode("utf-8"))

    def _json_err(self, code, msg):
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        self.wfile.write(json.dumps({"ok": False, "error": msg}).encode("utf-8"))

    def _read_body(self):
        length = int(self.headers.get("Content-Length", 0))
        return json.loads(self.rfile.read(length).decode("utf-8")) if length else {}

    def do_GET(self):
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
            agents = COORD.get_agents()
            self._json_ok(agents)
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

        # Fallback to serving static files
        super().do_GET()

    def do_POST(self):
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

                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.end_headers()
                self.wfile.write(json.dumps({"ok": True, "inject": inject_result}).encode('utf-8'))
            except Exception as e:
                self.send_response(500)
                self.end_headers()
                self.wfile.write(str(e).encode('utf-8'))
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
                
                cache_dir = os.path.join(os.path.dirname(os.path.abspath(__file__)), ".upload_cache")
                os.makedirs(cache_dir, exist_ok=True)
                
                for old_f in os.listdir(cache_dir):
                    try:
                        os.remove(os.path.join(cache_dir, old_f))
                    except Exception:
                        pass
                
                abs_paths = []
                for idx, f in enumerate(files):
                    name = f.get("name", f"file_{idx}")
                    data_str = f.get("data", "")
                    if "," in data_str:
                        data_str = data_str.split(",", 1)[1]
                    import base64
                    file_data = base64.b64decode(data_str)
                    dest_path = os.path.join(cache_dir, name)
                    with open(dest_path, "wb") as out_f:
                        out_f.write(file_data)
                    abs_paths.append(os.path.abspath(dest_path))
                
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

        # ─── Coordination API (POST) ─────────────────────────────
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

        elif parsed_path.path == "/api/coord/claim":
            try:
                body = self._read_body()
                agent_id = body.get("agent_id")
                file_path = body.get("file_path")
                if not agent_id or not file_path:
                    self._json_err(400, "agent_id and file_path required")
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
                task = COORD.post_task(
                    title=title,
                    description=body.get("description", ""),
                    assigned_to=body.get("assigned_to"),
                    priority=body.get("priority", 5),
                    posted_by=body.get("posted_by", "dashboard"),
                )
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
                msg = COORD.send_message(
                    from_agent=from_agent,
                    content=content,
                    to_agent=body.get("to_agent"),
                    msg_type=body.get("type", "info"),
                )
                self._json_ok({"ok": True, "message": msg})
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

    server = ThreadingHTTPServer(('127.0.0.1', PORT), CustomHandler)
    print(f"🚀 SynapzCore Dashboard Server started on http://localhost:{PORT}")
    print(f"📂 Repository root: {repo_root}")
    print("💡 Connecting to Antigravity IDE at debug port: 9333")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        print("\n👋 Server stopped. Goodbye!")
        sys.exit(0)

if __name__ == "__main__":
    main()
