"""
agent_sdk.py — SynapzCore Agent SDK

One-liner integration for any AI agent (Python, Node via subprocess, or HTTP).
Self-registers, keeps heartbeat alive, guards file edits, auto-claims tasks.

Usage:
    from agent_sdk import AgentSession

    with AgentSession("claude-code", role="builder",
                      capabilities=["code","debug"],
                      server="http://127.0.0.1:8899") as agent:

        # Edit a file safely — auto claim + release
        with agent.editing("scripts/dashboard.html"):
            # ... write file here ...
            pass

        # Claim a task and work it
        task = agent.claim_next_task()  # picks highest-priority pending task assigned to me
        if task:
            agent.working_on(task["id"], note="Starting implementation")
            # ... do work ...
            agent.done(task["id"])

        # Message another agent
        agent.tell("antigravity-ide", "dashboard.html updated, you can resume.")

        # Broadcast status
        agent.broadcast("Finished coordination module, going idle.")
"""

import os
import sys
import json
import time
import threading
import urllib.request
import urllib.parse
import urllib.error
import contextlib
from datetime import datetime, timezone


class AgentSession:
    """
    Manages an agent's lifecycle: registration, heartbeat, file locks, tasks.

    Thread-safe. Heartbeat runs in daemon thread — survives interpreter shutdown.
    """

    def __init__(
        self,
        agent_id: str,
        role: str = "unknown",
        capabilities: list = None,
        server: str = "http://127.0.0.1:8899",
        heartbeat_interval: float = 45.0,
        auto_start: bool = True,
        parent_id: str = None,
    ):
        self.agent_id = agent_id
        self.role = role
        self.capabilities = capabilities or []
        self.server = server.rstrip("/")
        self.heartbeat_interval = heartbeat_interval
        self.parent_id = parent_id  # None = agent cấp 1; có giá trị = sub-agent (nhánh con)

        self._current_task: str | None = None
        self._status = "active"
        self._lock = threading.Lock()
        self._stop_event = threading.Event()
        self._hb_thread: threading.Thread | None = None
        self._held_locks: set[str] = set()  # track files we hold

        if auto_start:
            self.start()

    # ─── Lifecycle ────────────────────────────────────────────────

    def start(self):
        """Register and start heartbeat daemon."""
        self._register()
        self._ack_rules()  # clear probation ngay → được phép nhắn/reply (tránh 403)
        self._hb_thread = threading.Thread(target=self._heartbeat_loop, daemon=True)
        self._hb_thread.start()
        print(f"🤝 [{self.agent_id}] registered, heartbeat every {self.heartbeat_interval}s")

    def _ack_rules(self):
        """Xác nhận đã đọc rules → chuyển probation→compliant (mở quyền message/claim).
        /api/coord/rules đăng ký ở POST nên dùng _post; fallback rules_version mặc định."""
        try:
            rules = self._post("/api/coord/rules", {}) or {}
            rv = rules.get("rules_version") or "v1.0.0"
            self._post("/api/coord/ack-rules", {"agent_id": self.agent_id, "rules_version": rv})
        except Exception:
            pass

    def stop(self):
        """Release all locks, deregister, stop heartbeat."""
        self._stop_event.set()
        # Release all held file locks
        for fp in list(self._held_locks):
            self._release(fp)
        self._post("/api/coord/deregister", {"agent_id": self.agent_id})
        print(f"👋 [{self.agent_id}] deregistered")

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.stop()

    # ─── Context manager: file editing ────────────────────────────

    @contextlib.contextmanager
    def editing(self, file_path: str, timeout: float = 30.0, retry_interval: float = 2.0):
        """
        Context manager — auto claim file before edit, release on exit.
        Blocks until claim succeeds or timeout. Sends conflict message if needed.

        Usage:
            with agent.editing("scripts/dashboard.html"):
                # safe to modify
        """
        deadline = time.time() + timeout
        claimed = False
        while time.time() < deadline:
            result = self._post("/api/coord/claim", {
                "agent_id": self.agent_id,
                "file_path": file_path,
            })
            if result and result.get("ok"):
                claimed = True
                with self._lock:
                    self._held_locks.add(file_path)
                break
            # Claim failed — another agent holds it
            holder = result.get("holder") if result else None
            if holder and holder != self.agent_id:
                # Politely ask the holder to yield
                self._post("/api/coord/message", {
                    "from_agent": self.agent_id,
                    "to_agent": holder,
                    "content": f"Tôi cần sửa `{file_path}`. Bạn có thể release không?",
                    "type": "request",
                })
            time.sleep(retry_interval)

        if not claimed:
            raise TimeoutError(
                f"[{self.agent_id}] Could not claim {file_path} within {timeout}s"
            )
        try:
            yield
        finally:
            self._release(file_path)

    def _release(self, file_path: str):
        self._post("/api/coord/release", {
            "agent_id": self.agent_id,
            "file_path": file_path,
        })
        with self._lock:
            self._held_locks.discard(file_path)

    # ─── Task management ──────────────────────────────────────────

    def claim_next_task(self, match_capabilities: bool = True) -> dict | None:
        """
        Find and claim the highest-priority pending task suitable for this agent.
        Prefers tasks assigned to me; falls back to unassigned tasks that match capabilities.
        Returns the task dict or None if nothing available.
        """
        tasks = self._get("/api/coord/tasks?status=pending") or []
        if isinstance(tasks, dict):
            tasks = []  # error response

        # Sort by priority descending
        tasks = sorted(tasks, key=lambda t: t.get("priority", 5), reverse=True)

        for task in tasks:
            assigned = task.get("assigned_to")
            # Accept if assigned to me, or unassigned
            if assigned and assigned != self.agent_id:
                continue
            task_id = task["id"]
            # Claim it by updating status + assigned_to
            result = self._post("/api/coord/task/update", {
                "task_id": task_id,
                "status": "active",
                "assigned_to": self.agent_id,
                "note": f"Claimed by {self.agent_id} at {self._now()}",
            })
            if result and result.get("ok"):
                self._current_task = task.get("title")
                self._send_heartbeat()  # update current_task in registry
                return result.get("task", task)
        return None

    def working_on(self, task_id: str, note: str = ""):
        """Update task status to active with a note."""
        self._post("/api/coord/task/update", {
            "task_id": task_id,
            "status": "active",
            "assigned_to": self.agent_id,
            "note": note or f"In progress [{self.agent_id}]",
        })

    def done(self, task_id: str, note: str = ""):
        """Mark task as done."""
        self._post("/api/coord/task/update", {
            "task_id": task_id,
            "status": "done",
            "note": note or f"Completed by {self.agent_id} at {self._now()}",
        })
        self._current_task = None
        self._send_heartbeat()

    def fail(self, task_id: str, reason: str = ""):
        """Mark task as failed."""
        self._post("/api/coord/task/update", {
            "task_id": task_id,
            "status": "cancelled",
            "note": f"Failed by {self.agent_id}: {reason}",
        })
        self._current_task = None
        self._send_heartbeat()

    # ─── Messaging ────────────────────────────────────────────────

    def register_webhook(self, url: str, events: list = None):
        """Register a webhook URL to receive push events."""
        return self._post("/api/coord/webhook/register", {
            "agent_id": self.agent_id,
            "url": url,
            "events": events or ["message", "task", "log"],
        })

    def unregister_webhook(self):
        """Unregister this agent's webhook."""
        return self._post("/api/coord/webhook/unregister", {
            "agent_id": self.agent_id,
        })

    def tell(self, to_agent: str, content: str, msg_type: str = "info"):
        """Send a direct message to another agent."""
        self._post("/api/coord/message", {
            "from_agent": self.agent_id,
            "to_agent": to_agent,
            "content": content,
            "type": msg_type,
        })

    def broadcast(self, content: str, msg_type: str = "info"):
        """Broadcast a message to all agents."""
        self._post("/api/coord/message", {
            "from_agent": self.agent_id,
            "to_agent": None,
            "content": content,
            "type": msg_type,
        })

    def get_messages(self, unread_only: bool = True) -> list:
        """Get messages addressed to me (or broadcast)."""
        qs = f"agent_id={urllib.parse.quote(self.agent_id)}&unread=true" if unread_only else f"agent_id={urllib.parse.quote(self.agent_id)}"
        return self._get(f"/api/coord/messages?{qs}") or []

    def mark_read(self, message_ids: list):
        self._post("/api/coord/messages/read", {
            "agent_id": self.agent_id,
            "message_ids": message_ids,
        })

    # ─── Status helpers ───────────────────────────────────────────

    def set_status(self, status: str, current_task: str = None):
        """Update agent status (active/idle/busy/away)."""
        self._status = status
        if current_task is not None:
            self._current_task = current_task
        self._send_heartbeat()

    def get_agents(self) -> dict:
        """Get all registered agents."""
        return self._get("/api/coord/agents") or {}

    def get_locks(self) -> dict:
        """Get all active file locks."""
        return self._get("/api/coord/locks") or {}

    def is_file_free(self, file_path: str) -> bool:
        """Check if a file is free to edit."""
        locks = self.get_locks()
        lock = locks.get(file_path)
        if not lock:
            return True
        return lock.get("holder") == self.agent_id

    # ─── Internal ─────────────────────────────────────────────────

    def _register(self):
        self._post("/api/coord/heartbeat", {
            "agent_id": self.agent_id,
            "role": self.role,
            "status": self._status,
            "capabilities": self.capabilities,
            "current_task": self._current_task,
            "parent_id": self.parent_id,
        })

    def _send_heartbeat(self):
        self._post("/api/coord/heartbeat", {
            "agent_id": self.agent_id,
            "role": self.role,
            "status": self._status,
            "capabilities": self.capabilities,
            "current_task": self._current_task,
            "parent_id": self.parent_id,
        })

    def _heartbeat_loop(self):
        while not self._stop_event.wait(self.heartbeat_interval):
            try:
                self._send_heartbeat()
            except Exception as e:
                print(f"⚠️ [{self.agent_id}] heartbeat failed: {e}", file=sys.stderr)

    def _post(self, path: str, data: dict) -> dict | None:
        try:
            payload = json.dumps(data).encode("utf-8")
            req = urllib.request.Request(
                f"{self.server}{path}",
                data=payload,
                headers={"Content-Type": "application/json"},
                method="POST",
            )
            with urllib.request.urlopen(req, timeout=5) as resp:
                return json.loads(resp.read().decode("utf-8"))
        except Exception as e:
            print(f"⚠️ [{self.agent_id}] POST {path} failed: {e}", file=sys.stderr)
            return None

    def _get(self, path: str) -> dict | list | None:
        try:
            req = urllib.request.Request(f"{self.server}{path}")
            with urllib.request.urlopen(req, timeout=5) as resp:
                return json.loads(resp.read().decode("utf-8"))
        except Exception:
            return None

    @staticmethod
    def _now():
        return datetime.now(timezone.utc).isoformat(timespec="seconds")


def _msg_epoch_ms(m: dict) -> float | None:
    """Trả epoch (ms) của 1 message để so với mốc khởi động watch.
    Ưu tiên id dạng 'msg-<ms>'; fallback parse created_at ISO; None nếu không suy ra được."""
    mid = m.get("id") or ""
    if isinstance(mid, str) and mid.startswith("msg-"):
        tail = mid[4:]
        if tail.isdigit():
            try:
                return float(tail)
            except Exception:
                pass
    ca = m.get("created_at")
    if isinstance(ca, str) and ca:
        try:
            dt = datetime.fromisoformat(ca.replace("Z", "+00:00"))
            if dt.tzinfo is None:
                dt = dt.replace(tzinfo=timezone.utc)
            return dt.timestamp() * 1000.0
        except Exception:
            pass
    return None


# ─── CLI — called by MCP Rust subprocess ─────────────────────────────────────
#

def _load_env_file():
    """Nạp .env ở repo root vào os.environ (không ghi đè biến đã có)."""
    import os
    base = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    p = os.path.join(base, ".env")
    if os.path.isfile(p):
        with open(p, encoding="utf-8") as f:
            for line in f:
                line = line.strip()
                if "=" in line and not line.startswith("#"):
                    k, v = line.split("=", 1)
                    os.environ.setdefault(k.strip(), v.strip())


def llm_reply(agent_id, role, content):
    """Gọi 9router sinh câu trả lời NGẮN cho 1 tin nhắn. Trả str, hoặc None nếu không cấu hình/lỗi."""
    import os, json as _j, urllib.request
    url = os.environ.get("NINEROUTER_URL")
    key = os.environ.get("NINEROUTER_KEY")
    model = os.environ.get("NINEROUTER_MODEL", "claude-opus-4.8")
    if not url or not key:
        return None
    sys_prompt = (f"Bạn là agent '{agent_id}' (vai trò {role}) trong hệ điều phối đa tác nhân SynapzCore. "
                  f"Trả lời NGẮN GỌN, tiếng Việt, đúng trọng tâm. Nếu được giao việc thì xác nhận sẽ làm.")
    body = _j.dumps({"model": model, "stream": False, "messages": [
        {"role": "system", "content": sys_prompt},
        {"role": "user", "content": content}], "max_tokens": 400}).encode()
    req = urllib.request.Request(url.rstrip("/") + "/v1/chat/completions", data=body,
                                 headers={"Content-Type": "application/json", "Authorization": "Bearer " + key})
    try:
        raw = urllib.request.urlopen(req, timeout=60).read().decode("utf-8", "replace")
        # 9router có thể trả SSE streaming (data: {chunk delta}) HOẶC 1 JSON object.
        if "chat.completion.chunk" in raw or raw.lstrip().startswith("data:"):
            parts = []
            for line in raw.splitlines():
                line = line.strip()
                if line.startswith("data:"):
                    pl = line[5:].strip()
                    if pl and pl != "[DONE]":
                        try:
                            d = _j.loads(pl)
                            delta = (d.get("choices") or [{}])[0].get("delta", {}).get("content")
                            if delta:
                                parts.append(delta)
                        except Exception:
                            pass
            return ("".join(parts).strip() or None)
        idx = raw.find("data:")  # non-stream: cắt đuôi 'data: [DONE]' nếu có
        obj = raw[:idx].strip() if idx > 0 else raw
        v = _j.loads(obj)
        return (v["choices"][0]["message"]["content"] or "").strip() or None
    except Exception:
        return None


# ─── REAL CLI EXECUTORS ──────────────────────────────────────────────────────
# Một số builder là CLI agentic THẬT trên máy (claude-code, codex, gemini) → tự
# ghi file + chạy lệnh trong repo, không chỉ sinh text như 9router. Bật bằng env
# SYNAPZ_CLI_EXEC=1 (mặc định TẮT cho an toàn — autonomous edit là rủi ro cao).
# Prompt truyền qua STDIN để tránh lỗi quoting; chạy qua PowerShell để resolve
# .cmd/.ps1 shim của npm đồng nhất.
def _cli_spec(agent_id: str):
    """Trả (mode, cmd) cho agent: mode='stdin' (prompt qua $input) hoặc 'arg' (prompt qua
    $env:SYNAPZ_TASK_PROMPT). None nếu không map. $MODEL thay bằng env tương ứng."""
    model = os.environ.get("CLAUDE_CLI_MODEL", "claude-opus-4.8")
    oc_model = os.environ.get("OPENCODE_CLI_MODEL", "9router/claude-opus-4.6")
    mimo_model = os.environ.get("MIMO_CLI_MODEL", "9router/claude-opus-4.8")
    cline_model = os.environ.get("CLINE_CLI_MODEL", "claude-opus-4.8")
    table = {
        "claude-code": ("stdin", f"claude -p --model {model} --permission-mode acceptEdits"),
        "claude":      ("stdin", f"claude -p --model {model} --permission-mode acceptEdits"),
        "codex":       ("stdin", "codex exec -"),
        "gemini":      ("stdin", "gemini -p --yolo"),
        "opencode":    ("stdin", f"opencode run -m {oc_model}"),
        "mimo":        ("stdin", f"mimo run -m {mimo_model}"),
        "cline":       ("arg",   f"cline -a -y --json -m {cline_model}"),
    }
    return table.get(agent_id)


def cli_execute_task(agent_id, role, task_desc, repo_root=None, timeout=600):
    """THỰC THI task bằng CLI agentic THẬT (claude/codex/gemini/opencode/mimo/cline) trong repo.
    Trả str (output + danh sách file đổi) hoặc None nếu không map / lỗi / chưa bật."""
    import os as _os, subprocess as _sp
    if _os.environ.get("SYNAPZ_CLI_EXEC", "").strip() not in ("1", "true", "yes", "on"):
        return None  # tính năng chưa bật → fallback sang llm_execute_task
    spec = _cli_spec(agent_id)
    if not spec:
        return None
    mode, cmd = spec
    root = repo_root or _os.path.dirname(_os.path.dirname(_os.path.abspath(__file__)))
    # gemini cần GEMINI_API_KEY
    if agent_id == "gemini" and not _os.environ.get("GEMINI_API_KEY"):
        return None
    env = dict(_os.environ)
    if mode == "arg":
        env["SYNAPZ_TASK_PROMPT"] = task_desc
        ps_cmd = f"& {cmd} $env:SYNAPZ_TASK_PROMPT"
        stdin_data = None
    else:  # stdin
        ps_cmd = f"$input | & {cmd}"
        stdin_data = task_desc
    try:
        proc = _sp.run(
            ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", ps_cmd],
            input=stdin_data, cwd=root, capture_output=True, text=True,
            timeout=timeout, encoding="utf-8", errors="replace", env=env,
        )
    except Exception as e:
        return f"⚠️ CLI '{agent_id}' lỗi chạy: {e}"
    out = (proc.stdout or "").strip()
    err = (proc.stderr or "").strip()
    # Liệt kê file đã đổi để chứng minh LÀM THẬT (không chỉ nói).
    changed = ""
    try:
        gp = _sp.run(["git", "status", "--porcelain"], cwd=root,
                     capture_output=True, text=True, timeout=20)
        lines = [l for l in (gp.stdout or "").splitlines() if l.strip()]
        if lines:
            changed = "\n\n📝 File thay đổi:\n" + "\n".join(lines[:40])
    except Exception:
        pass
    body = out or err or "(CLI không in output)"
    tail = body[-3500:]
    return f"[{agent_id} CLI · exit {proc.returncode}]\n{tail}{changed}"


def llm_execute_task(agent_id, role, task_desc):
    """Builder THỰC THI task: gọi 9router sinh DELIVERABLE hoàn chỉnh (code/kết quả), không phải ack.
    Trả str (kết quả) hoặc None."""
    import os, json as _j, urllib.request
    url = os.environ.get("NINEROUTER_URL")
    key = os.environ.get("NINEROUTER_KEY")
    model = os.environ.get("NINEROUTER_MODEL", "claude-opus-4.8")
    if not url or not key:
        return None
    sys_prompt = (f"Bạn là builder '{agent_id}' (vai trò {role}) trong SynapzCore. THỰC HIỆN task được giao "
                  f"và TRẢ VỀ KẾT QUẢ HOÀN CHỈNH (code đầy đủ trong khối ```, hoặc nội dung cụ thể). "
                  f"Ngắn gọn phần giải thích, tập trung vào sản phẩm. Tiếng Việt cho phần mô tả.")
    body = _j.dumps({"model": model, "stream": False, "messages": [
        {"role": "system", "content": sys_prompt},
        {"role": "user", "content": "Task: " + task_desc}], "max_tokens": 1500}).encode()
    req = urllib.request.Request(url.rstrip("/") + "/v1/chat/completions", data=body,
                                 headers={"Content-Type": "application/json", "Authorization": "Bearer " + key})
    try:
        raw = urllib.request.urlopen(req, timeout=120).read().decode("utf-8", "replace")
        if "chat.completion.chunk" in raw or raw.lstrip().startswith("data:"):
            parts = []
            for line in raw.splitlines():
                line = line.strip()
                if line.startswith("data:"):
                    pl = line[5:].strip()
                    if pl and pl != "[DONE]":
                        try:
                            d = _j.loads(pl)
                            delta = (d.get("choices") or [{}])[0].get("delta", {}).get("content")
                            if delta:
                                parts.append(delta)
                        except Exception:
                            pass
            return "".join(parts).strip() or None
        idx = raw.find("data:")
        obj = raw[:idx].strip() if idx > 0 else raw
        return (_j.loads(obj)["choices"][0]["message"]["content"] or "").strip() or None
    except Exception:
        return None


# Usage (all output JSON to stdout):
#   python agent_sdk.py --action heartbeat --agent antigravity-ide --role builder [--task "..."] [--status active]
#   python agent_sdk.py --action claim     --agent antigravity-ide --file scripts/dashboard.html
#   python agent_sdk.py --action release   --agent antigravity-ide --file scripts/dashboard.html
#   python agent_sdk.py --action status                              # full coord state summary
#   python agent_sdk.py --action messages  --agent antigravity-ide  # unread messages
#   python agent_sdk.py --action tell      --agent antigravity-ide --to claude-code --msg "..."
#   python agent_sdk.py --action broadcast --agent antigravity-ide --msg "..."
#   python agent_sdk.py --action demo      --agent test-agent

if __name__ == "__main__":
    import argparse

    p = argparse.ArgumentParser(description="SynapzCore Agent SDK CLI")
    p.add_argument("--action", required=True,
                   choices=["heartbeat", "claim", "release", "status", "messages", "tell", "broadcast", "watch", "demo"])
    p.add_argument("--agent",  default="antigravity-ide")
    p.add_argument("--role",   default="builder")
    p.add_argument("--status-val", dest="status_val", default="active")
    p.add_argument("--task",   default=None, help="current_task description")
    p.add_argument("--capabilities", default="code,file,shell,cdp")
    p.add_argument("--file",   default=None, help="file path for claim/release")
    p.add_argument("--to",     default=None, help="target agent for tell")
    p.add_argument("--msg",    default="", help="message content")
    p.add_argument("--server", default="http://127.0.0.1:8899")
    p.add_argument("--once", action="store_true", help="watch: print current unread + open tasks then exit")
    p.add_argument("--interval", type=int, default=10, help="watch: poll seconds")
    p.add_argument("--parent", default=None, help="parent agent_id → đăng ký làm sub-agent (nhánh con)")
    args = p.parse_args()

    caps = [c.strip() for c in args.capabilities.split(",") if c.strip()]

    if args.action == "heartbeat":
        # One-shot heartbeat — no daemon, just register + exit
        session = AgentSession(args.agent, role=args.role,
                               capabilities=caps,
                               server=args.server,
                               heartbeat_interval=9999,
                               auto_start=False)
        session._current_task = args.task
        session._status = args.status_val
        result = session._post("/api/coord/heartbeat", {
            "agent_id": args.agent,
            "role": args.role,
            "status": args.status_val,
            "capabilities": caps,
            "current_task": args.task,
        })
        print(json.dumps(result or {"ok": False, "error": "server unreachable"}))

    elif args.action == "claim":
        if not args.file:
            print(json.dumps({"ok": False, "error": "file required"}))
            sys.exit(1)
        session = AgentSession.__new__(AgentSession)
        session.agent_id = args.agent
        session.server = args.server
        result = session._post("/api/coord/claim", {
            "agent_id": args.agent,
            "file_path": args.file,
        })
        print(json.dumps(result or {"ok": False, "error": "server unreachable"}))

    elif args.action == "release":
        if not args.file:
            print(json.dumps({"ok": False, "error": "file required"}))
            sys.exit(1)
        session = AgentSession.__new__(AgentSession)
        session.agent_id = args.agent
        session.server = args.server
        result = session._post("/api/coord/release", {
            "agent_id": args.agent,
            "file_path": args.file,
        })
        print(json.dumps(result or {"ok": False, "error": "server unreachable"}))

    elif args.action == "status":
        session = AgentSession.__new__(AgentSession)
        session.server = args.server
        state = session._get("/api/coord/state") or {}
        agents = state.get("agents", {})
        locks  = state.get("file_locks", {})
        tasks  = [t for t in state.get("task_queue", []) if t.get("status") not in ("done","cancelled")]
        msgs   = state.get("messages", [])
        print(json.dumps({
            "agents": len(agents),
            "active": [a for a, v in agents.items() if not v.get("stale")],
            "stale":  [a for a, v in agents.items() if v.get("stale")],
            "locks":  locks,
            "open_tasks": len(tasks),
            "messages": len(msgs),
        }, ensure_ascii=False))

    elif args.action == "messages":
        session = AgentSession.__new__(AgentSession)
        session.server = args.server
        msgs = session._get(f"/api/coord/messages?agent_id={urllib.parse.quote(args.agent)}&unread=true") or []
        print(json.dumps(msgs, ensure_ascii=False))

    elif args.action == "tell":
        if not args.to or not args.msg:
            print(json.dumps({"ok": False, "error": "to and msg required"}))
            sys.exit(1)
        session = AgentSession.__new__(AgentSession)
        session.server = args.server
        result = session._post("/api/coord/message", {
            "from_agent": args.agent,
            "to_agent": args.to,
            "content": args.msg,
            "type": "info",
        })
        print(json.dumps(result or {"ok": False}))

    elif args.action == "broadcast":
        session = AgentSession.__new__(AgentSession)
        session.server = args.server
        result = session._post("/api/coord/message", {
            "from_agent": args.agent,
            "to_agent": None,
            "content": args.msg,
            "type": "info",
        })
        print(json.dumps(result or {"ok": False}))

    elif args.action == "watch":
        # Webhook-style listener: an AI registers, then watches the feed.
        # Tin nhắn gửi ĐÚNG agent này (không phải 'reply') → tự gọi 9router trả lời (2 chiều).
        _load_env_file()
        session = AgentSession(args.agent, role=args.role, capabilities=caps,
                               server=args.server,
                               heartbeat_interval=max(30, args.interval * 3),
                               parent_id=args.parent)
        try:
            session.broadcast(f"👀 {args.agent} ({args.role}) online & watching. Sẵn sàng nhận việc.")
        except Exception:
            pass

        seen_msgs = set()
        seen_tasks = set()
        # Mốc khởi động: chỉ THỰC THI/TRẢ LỜI tin tạo SAU thời điểm này.
        # Tin cũ (stale) vẫn được mark-read để drain, nhưng KHÔNG chạy lại.
        # --once không gate (giữ hành vi kiểm tra tức thời).
        watch_start_ms = time.time() * 1000.0
        gate_stale = not args.once

        def poll_once():
            events = []
            # 1) Unread messages addressed to me or broadcast
            for m in session.get_messages(unread_only=True):
                mid = m.get("id")
                if mid and mid not in seen_msgs and m.get("from") != args.agent:
                    seen_msgs.add(mid)
                    # Tin tạo trước khi watch khởi động → stale: drain (mark-read), không xử lý.
                    ep = _msg_epoch_ms(m)
                    is_stale = gate_stale and ep is not None and ep < watch_start_ms
                    events.append({"kind": "message", "id": mid,
                                   "from": m.get("from"), "to": m.get("to"),
                                   "content": m.get("content"),
                                   "stale": is_stale})
                    if mid:
                        try: session.mark_read([mid])
                        except Exception: pass
                    if is_stale:
                        events.append({"kind": "skip_stale", "id": mid,
                                       "from": m.get("from"), "type": m.get("type")})
                        continue
                    # AUTO-REPLY / THỰC THI: tin gửi ĐÚNG mình & KHÔNG phải 'reply'.
                    if m.get("to") == args.agent and m.get("type") != "reply" and m.get("from"):
                        if m.get("type") == "task":
                            # THỰC THI task → ưu tiên CLI agentic THẬT (tự sửa file),
                            # fallback 9router text nếu chưa bật/không map.
                            result = cli_execute_task(args.agent, args.role, m.get("content") or "")
                            if not result:
                                result = llm_execute_task(args.agent, args.role, m.get("content") or "")
                            if result:
                                try:
                                    session.tell(m["from"], "✅ Kết quả:\n" + result, msg_type="reply")
                                    events.append({"kind": "task_done", "to": m["from"], "result": result[:80]})
                                except Exception:
                                    pass
                        else:
                            reply = llm_reply(args.agent, args.role, m.get("content") or "")
                            if reply:
                                try:
                                    session.tell(m["from"], reply, msg_type="reply")
                                    events.append({"kind": "auto_reply", "to": m["from"], "reply": reply[:80]})
                                except Exception:
                                    pass
            # 2) Open tasks I could claim (pending, unassigned or assigned to me)
            state = session._get("/api/coord/state") or {}
            for t in state.get("task_queue", []):
                tid = t.get("id")
                if t.get("status") != "pending" or tid in seen_tasks:
                    continue
                assignee = t.get("assigned_to")
                if assignee in (None, "", args.agent):
                    seen_tasks.add(tid)
                    events.append({"kind": "open_task", "id": tid,
                                   "title": t.get("title"),
                                   "description": t.get("description"),
                                   "priority": t.get("priority"),
                                   "assigned_to": assignee})
            return events

        if args.once:
            for ev in poll_once():
                print(json.dumps(ev, ensure_ascii=False), flush=True)
        else:
            try:
                while True:
                    for ev in poll_once():
                        print(json.dumps(ev, ensure_ascii=False), flush=True)
                    time.sleep(args.interval)
            except KeyboardInterrupt:
                pass
            finally:
                session.stop()

    elif args.action == "demo":
        with AgentSession(args.agent, role=args.role, capabilities=caps,
                          server=args.server) as agent:
            task = agent.claim_next_task()
            if task:
                print(json.dumps({"claimed": task.get("title")}))
                time.sleep(0.5)
                agent.done(task["id"], "Demo completed")
            msgs = agent.get_messages()
            agent.broadcast(f"{args.agent} demo done, going idle.")
            agent.set_status("idle")
            print(json.dumps({"status": "demo_done", "messages": len(msgs)}))
