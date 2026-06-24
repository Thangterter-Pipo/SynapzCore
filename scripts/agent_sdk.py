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
    ):
        self.agent_id = agent_id
        self.role = role
        self.capabilities = capabilities or []
        self.server = server.rstrip("/")
        self.heartbeat_interval = heartbeat_interval

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
        self._hb_thread = threading.Thread(target=self._heartbeat_loop, daemon=True)
        self._hb_thread.start()
        print(f"🤝 [{self.agent_id}] registered, heartbeat every {self.heartbeat_interval}s")

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
        })

    def _send_heartbeat(self):
        self._post("/api/coord/heartbeat", {
            "agent_id": self.agent_id,
            "role": self.role,
            "status": self._status,
            "capabilities": self.capabilities,
            "current_task": self._current_task,
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


# ─── CLI — called by MCP Rust subprocess ─────────────────────────────────────
#
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
        # On new directed message / broadcast / open task → emits a JSON event
        # to stdout (one line each) so a host loop can react & "xin việc".
        # --once: emit current unread + open tasks then exit (for poll-based hosts).
        session = AgentSession(args.agent, role=args.role, capabilities=caps,
                               server=args.server,
                               heartbeat_interval=max(30, args.interval * 3))
        try:
            session.broadcast(f"👀 {args.agent} ({args.role}) online & watching. Sẵn sàng nhận việc.")
        except Exception:
            pass

        seen_msgs = set()
        seen_tasks = set()

        def poll_once():
            events = []
            # 1) Unread messages addressed to me or broadcast
            for m in session.get_messages(unread_only=True):
                mid = m.get("id")
                if mid and mid not in seen_msgs and m.get("from") != args.agent:
                    seen_msgs.add(mid)
                    events.append({"kind": "message", "id": mid,
                                   "from": m.get("from"), "to": m.get("to"),
                                   "content": m.get("content")})
                    if mid:
                        try: session.mark_read([mid])
                        except Exception: pass
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
