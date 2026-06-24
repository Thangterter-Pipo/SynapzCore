"""
coordination.py — Multi-AI Coordination Module for SynapzCore.

Local file-based coordination layer. Zero external dependencies.
Any AI agent (Antigravity, Grok, Claude, ChatGPT, Gemini, etc.) can
register, claim files, post tasks, and exchange messages.

State file: data/coordination/state.json
Protocol: JSON file lock pattern (read → modify → atomic write).

Usage from Python:
    coord = Coordinator("e:/AGT_Brain/data/coordination")
    coord.heartbeat("antigravity-ide", role="builder", status="active")
    coord.claim_file("antigravity-ide", "scripts/dashboard.html")
    coord.release_file("antigravity-ide", "scripts/dashboard.html")

Usage from HTTP (via dashboard_server.py):
    GET  /api/coord/state          → full state
    POST /api/coord/heartbeat      → { agent_id, role?, status?, capabilities? }
    POST /api/coord/claim          → { agent_id, file_path }
    POST /api/coord/release        → { agent_id, file_path }
    POST /api/coord/task           → { title, description?, assigned_to?, priority? }
    POST /api/coord/message        → { from_agent, to_agent?, content }
    GET  /api/coord/messages       → { agent_id? } → filtered messages
    POST /api/coord/deregister     → { agent_id }
"""

import json
import os
import time
import threading
import urllib.request
from datetime import datetime, timezone

# Agent heartbeat timeout: if no heartbeat for 120s, mark stale.
HEARTBEAT_TIMEOUT = 120
# Max messages kept in state file.
MAX_MESSAGES = 200
# Max completed tasks kept.
MAX_COMPLETED_TASKS = 50

_lock = threading.Lock()


class Coordinator:
    def __init__(self, coord_dir: str):
        self.coord_dir = coord_dir
        self.state_path = os.path.join(coord_dir, "state.json")
        os.makedirs(coord_dir, exist_ok=True)
        if not os.path.exists(self.state_path):
            self._write_state(self._empty_state())

    @staticmethod
    def _empty_state():
        return {
            "_schema": "synapzcore-coordination-v1",
            "agents": {},
            "file_locks": {},
            "task_queue": [],
            "messages": [],
            "project_log": [],
            "webhooks": {},
        }

    def _read_state(self) -> dict:
        try:
            with open(self.state_path, "r", encoding="utf-8") as f:
                return json.load(f)
        except (json.JSONDecodeError, FileNotFoundError):
            return self._empty_state()

    def _write_state(self, state: dict):
        tmp = self.state_path + ".tmp"
        with open(tmp, "w", encoding="utf-8") as f:
            json.dump(state, f, indent=2, ensure_ascii=False)
        os.replace(tmp, self.state_path)  # atomic on same filesystem

    def _now_iso(self):
        return datetime.now(timezone.utc).isoformat(timespec="seconds")

    def _now_ts(self):
        return time.time()

    # ─── Agent Registry ───────────────────────────────────────────

    def heartbeat(self, agent_id: str, role: str = "unknown",
                  status: str = "active", capabilities: list = None,
                  current_task: str = None) -> dict:
        """Register or update agent presence. Call every 30-60s."""
        with _lock:
            state = self._read_state()
            now = self._now_iso()
            existing = state["agents"].get(agent_id, {})
            state["agents"][agent_id] = {
                "agent_id": agent_id,
                "role": role,
                "status": status,
                "capabilities": capabilities or existing.get("capabilities", []),
                "current_task": current_task or existing.get("current_task"),
                "registered_at": existing.get("registered_at", now),
                "last_heartbeat": now,
                "_ts": self._now_ts(),
            }
            self._write_state(state)
        return state["agents"][agent_id]

    def deregister(self, agent_id: str) -> bool:
        """Remove agent and release all its file locks."""
        with _lock:
            state = self._read_state()
            removed = agent_id in state["agents"]
            state["agents"].pop(agent_id, None)
            # Release all file locks held by this agent
            to_remove = [fp for fp, lock in state["file_locks"].items()
                         if lock.get("holder") == agent_id]
            for fp in to_remove:
                del state["file_locks"][fp]
            self._write_state(state)
        return removed

    def get_agents(self) -> dict:
        """Get all agents with staleness check."""
        state = self._read_state()
        now = self._now_ts()
        agents = {}
        for aid, info in state["agents"].items():
            info = dict(info)
            ts = info.get("_ts", 0)
            info["stale"] = (now - ts) > HEARTBEAT_TIMEOUT
            agents[aid] = info
        return agents

    # ─── File Locking ─────────────────────────────────────────────

    def claim_file(self, agent_id: str, file_path: str) -> dict:
        """
        Claim exclusive edit rights on a file.
        Returns {"ok": True} or {"ok": False, "holder": "other-agent"}.
        """
        with _lock:
            state = self._read_state()
            existing = state["file_locks"].get(file_path)
            if existing and existing["holder"] != agent_id:
                # Check if holder is stale
                holder_info = state["agents"].get(existing["holder"], {})
                holder_ts = holder_info.get("_ts", 0)
                if (self._now_ts() - holder_ts) > HEARTBEAT_TIMEOUT:
                    pass  # Stale holder → allow override
                else:
                    return {"ok": False, "holder": existing["holder"],
                            "claimed_at": existing.get("claimed_at")}
            state["file_locks"][file_path] = {
                "holder": agent_id,
                "claimed_at": self._now_iso(),
            }
            self._write_state(state)
        return {"ok": True}

    def release_file(self, agent_id: str, file_path: str) -> bool:
        """Release a file lock. Only the holder can release."""
        with _lock:
            state = self._read_state()
            existing = state["file_locks"].get(file_path)
            if existing and existing["holder"] == agent_id:
                del state["file_locks"][file_path]
                self._write_state(state)
                return True
        return False

    def get_locks(self) -> dict:
        """Get all active file locks."""
        return self._read_state().get("file_locks", {})

    def who_holds(self, file_path: str) -> str | None:
        """Check who holds a file lock. Returns agent_id or None."""
        lock = self._read_state().get("file_locks", {}).get(file_path)
        return lock["holder"] if lock else None

    # ─── Task Queue ───────────────────────────────────────────────

    def post_task(self, title: str, description: str = "",
                  assigned_to: str = None, priority: int = 5,
                  posted_by: str = "system") -> dict:
        """Post a task to the shared queue."""
        task = {
            "id": f"task-{int(time.time()*1000)}",
            "title": title,
            "description": description,
            "assigned_to": assigned_to,
            "priority": priority,
            "status": "pending",
            "posted_by": posted_by,
            "created_at": self._now_iso(),
            "updated_at": self._now_iso(),
        }
        with _lock:
            state = self._read_state()
            state["task_queue"].append(task)
            self._write_state(state)
        self.fire_webhooks("task", dict(task))
        return task

    def update_task(self, task_id: str, status: str = None,
                    assigned_to: str = None, note: str = None) -> dict | None:
        """Update task status/assignment."""
        with _lock:
            state = self._read_state()
            for task in state["task_queue"]:
                if task["id"] == task_id:
                    if status:
                        task["status"] = status
                    if assigned_to:
                        task["assigned_to"] = assigned_to
                    if note:
                        task.setdefault("notes", []).append({
                            "text": note, "at": self._now_iso()
                        })
                    task["updated_at"] = self._now_iso()
                    # Trim completed tasks
                    completed = [t for t in state["task_queue"] if t["status"] in ("done", "cancelled")]
                    if len(completed) > MAX_COMPLETED_TASKS:
                        oldest = sorted(completed, key=lambda t: t["updated_at"])
                        for old in oldest[:len(completed) - MAX_COMPLETED_TASKS]:
                            state["task_queue"].remove(old)
                    self._write_state(state)
                    return task
        return None

    def get_tasks(self, status: str = None, assigned_to: str = None) -> list:
        """Get tasks, optionally filtered."""
        tasks = self._read_state().get("task_queue", [])
        if status:
            tasks = [t for t in tasks if t["status"] == status]
        if assigned_to:
            tasks = [t for t in tasks if t.get("assigned_to") == assigned_to]
        return tasks

    # ─── Inter-Agent Messaging ────────────────────────────────────

    def send_message(self, from_agent: str, content: str,
                     to_agent: str = None, msg_type: str = "info") -> dict:
        """
        Send a message to a specific agent or broadcast (to_agent=None).
        msg_type: info, warning, request, response, file_conflict
        """
        msg = {
            "id": f"msg-{int(time.time()*1000)}",
            "from": from_agent,
            "to": to_agent,  # None = broadcast
            "type": msg_type,
            "content": content,
            "created_at": self._now_iso(),
            "read_by": [],
        }
        with _lock:
            state = self._read_state()
            state["messages"].append(msg)
            if len(state["messages"]) > MAX_MESSAGES:
                state["messages"] = state["messages"][-MAX_MESSAGES:]
            self._write_state(state)
        self.fire_webhooks("message", dict(msg))
        return msg

    def get_messages(self, agent_id: str = None, unread_only: bool = False,
                     limit: int = 50) -> list:
        """Get messages for an agent (or all). Broadcast messages included."""
        msgs = self._read_state().get("messages", [])
        if agent_id:
            msgs = [m for m in msgs
                    if m.get("to") is None or m.get("to") == agent_id]
        if unread_only and agent_id:
            msgs = [m for m in msgs if agent_id not in m.get("read_by", [])]
        return msgs[-limit:]

    def mark_read(self, agent_id: str, message_ids: list) -> int:
        """Mark messages as read by agent. Returns count marked."""
        count = 0
        with _lock:
            state = self._read_state()
            for msg in state["messages"]:
                if msg["id"] in message_ids and agent_id not in msg.get("read_by", []):
                    msg.setdefault("read_by", []).append(agent_id)
                    count += 1
            if count:
                self._write_state(state)
        return count

    # ─── Project Log ──────────────────────────────────────────────

    MAX_LOG = 500

    def add_log(self, agent_id: str, action: str, detail: str = "",
                category: str = "general", tags: list = None) -> dict:
        """
        Append an entry to the project history log.
        category: task|file|message|deploy|milestone|general
        """
        entry = {
            "id": f"log-{int(time.time()*1000)}",
            "agent_id": agent_id,
            "action": action,
            "detail": detail,
            "category": category,
            "tags": tags or [],
            "created_at": self._now_iso(),
        }
        with _lock:
            state = self._read_state()
            log = state.setdefault("project_log", [])
            log.append(entry)
            if len(log) > self.MAX_LOG:
                state["project_log"] = log[-self.MAX_LOG:]
            self._write_state(state)
        self.fire_webhooks("log", dict(entry))
        return entry

    def get_log(self, limit: int = 100, category: str = None,
                agent_id: str = None) -> list:
        """Get project log entries, newest first."""
        entries = self._read_state().get("project_log", [])
        if category:
            entries = [e for e in entries if e.get("category") == category]
        if agent_id:
            entries = [e for e in entries if e.get("agent_id") == agent_id]
        return list(reversed(entries[-limit:]))

    # ─── Full State ───────────────────────────────────────────────

    def get_state(self) -> dict:
        """Get full coordination state with staleness annotations."""
        state = self._read_state()
        now = self._now_ts()
        for aid, info in state.get("agents", {}).items():
            info["stale"] = (now - info.get("_ts", 0)) > HEARTBEAT_TIMEOUT
        return state

    # ─── Webhook Registry ─────────────────────────────────────────

    def register_webhook(self, agent_id: str, url: str,
                         events: list = None) -> dict:
        """Register a webhook URL for an agent. Events: message, task, log."""
        if events is None:
            events = ["message", "task", "log"]
        entry = {
            "url": url,
            "events": events,
            "registered_at": self._now_iso(),
        }
        with _lock:
            state = self._read_state()
            state.setdefault("webhooks", {})[agent_id] = entry
            self._write_state(state)
        return entry

    def unregister_webhook(self, agent_id: str) -> bool:
        """Remove a registered webhook for agent_id."""
        with _lock:
            state = self._read_state()
            existed = agent_id in state.get("webhooks", {})
            state.setdefault("webhooks", {}).pop(agent_id, None)
            if existed:
                self._write_state(state)
        return existed

    def get_webhooks(self) -> dict:
        """Return all registered webhooks."""
        return self._read_state().get("webhooks", {})

    def fire_webhooks(self, event_type: str, payload: dict) -> None:
        """POST payload to all webhooks subscribed to event_type (non-blocking)."""
        webhooks = self._read_state().get("webhooks", {})
        payload = dict(payload)
        payload["event"] = event_type
        payload["timestamp"] = self._now_iso()
        data = json.dumps(payload, ensure_ascii=False).encode("utf-8")

        def _post(url: str) -> None:
            try:
                req = urllib.request.Request(
                    url, data=data,
                    headers={"Content-Type": "application/json"},
                    method="POST",
                )
                with urllib.request.urlopen(req, timeout=5):
                    pass
            except Exception:
                pass  # silent fail

        for wh in webhooks.values():
            if event_type in wh.get("events", []):
                t = threading.Thread(target=_post, args=(wh["url"],), daemon=True)
                t.start()
