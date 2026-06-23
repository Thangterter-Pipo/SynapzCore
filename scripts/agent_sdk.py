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


# ─── CLI quick-test ───────────────────────────────────────────────────────────

if __name__ == "__main__":
    import argparse
    p = argparse.ArgumentParser(description="SynapzCore Agent SDK quick test")
    p.add_argument("--agent", default="test-agent", help="Agent ID")
    p.add_argument("--role", default="tester")
    p.add_argument("--server", default="http://127.0.0.1:8899")
    p.add_argument("--action", choices=["register","tasks","messages","demo"], default="demo")
    args = p.parse_args()

    with AgentSession(args.agent, role=args.role, server=args.server) as agent:
        if args.action in ("tasks", "demo"):
            task = agent.claim_next_task()
            if task:
                print(f"📋 Claimed task: {task['title']}")
                time.sleep(1)
                agent.done(task["id"], "Test completed")
                print("✅ Task done")
            else:
                print("📭 No tasks available")

        if args.action in ("messages", "demo"):
            msgs = agent.get_messages()
            print(f"📬 {len(msgs)} unread messages")
            for m in msgs:
                print(f"  [{m['from']}] {m['content'][:80]}")

        if args.action == "demo":
            agent.broadcast(f"{args.agent} demo completed, going idle.")
            agent.set_status("idle")
            print(f"🏁 Demo done. Agent '{args.agent}' will deregister now.")
