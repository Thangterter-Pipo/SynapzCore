"""
coordination.py — Multi-AI Coordination Module for SynapzCore.

Local file-based coordination layer. Zero external dependencies.
Any AI agent (Antigravity, Claude, ChatGPT, Gemini, etc.) can
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

# ROLE LOCKS — ép vai trò cố định theo agent_id, BẤT KỂ agent tự khai gì khi heartbeat.
# Đảm bảo mô hình điều phối ổn định: đúng MỘT orchestrator (Kiro IDE), pipo-hermes là builder.
# Tránh tình trạng agent heartbeat lại role cũ làm lật vai trò (orchestrator/builder).
ROLE_LOCKS = {
    "kiro-ide": "orchestrator",
    "pipo-hermes": "builder",
}

_lock = threading.Lock()

# ─────────────────────────────────────────────
# SYSTEM RULES — versioned, role-based
# ─────────────────────────────────────────────
RULES_VERSION = "v1.0.0"

ROLE_RULES = {
    "orchestrator": {
        "can_assign_task":   True,
        "can_broadcast":     True,
        "can_claim_lock":    True,
        "can_message_any":   True,
        "can_update_any_task": True,
        "description": "Điều phối toàn bộ đội. Được tạo/assign task, broadcast, claim lock, nhắn bất kỳ agent.",
    },
    "builder": {
        "can_assign_task":   False,   # chỉ nhận, không assign cho người khác
        "can_broadcast":     False,
        "can_claim_lock":    True,
        "can_message_any":   False,   # chỉ reply cho orchestrator/assigned
        "can_update_any_task": False, # chỉ update task của mình
        "description": "Nhận task, thực thi, báo cáo kết quả. Không assign task cho agent khác.",
    },
    "researcher": {
        "can_assign_task":   False,
        "can_broadcast":     False,
        "can_claim_lock":    False,   # chỉ đọc, không lock file
        "can_message_any":   False,
        "can_update_any_task": False,
        "description": "Chỉ tra cứu/tổng hợp thông tin. Không được lock file hay assign task.",
    },
    "reviewer": {
        "can_assign_task":   False,
        "can_broadcast":     False,
        "can_claim_lock":    False,
        "can_message_any":   False,
        "can_update_any_task": False, # chỉ add comment, không đổi status
        "description": "Review và comment task đã tồn tại. Không tạo task mới hay lock file.",
    },
    "tester": {
        "can_assign_task":   False,
        "can_broadcast":     False,
        "can_claim_lock":    True,    # cần lock file khi chạy test
        "can_message_any":   False,
        "can_update_any_task": False, # chỉ update task của mình
        "description": "Chạy test, cập nhật status task của mình. Không assign task cho agent khác.",
    },
}

# Fallback cho role không xác định — quyền tối thiểu
_DEFAULT_RULES = {
    "can_assign_task":   False,
    "can_broadcast":     False,
    "can_claim_lock":    False,
    "can_message_any":   False,
    "can_update_any_task": False,
    "description": "Role không xác định — quyền tối thiểu, cần liên hệ orchestrator.",
}

ACK_TIMEOUT_SECONDS = 60  # agent có 60s để ack sau khi join


def get_role_rules(role: str) -> dict:
    return ROLE_RULES.get((role or "").lower(), _DEFAULT_RULES)


def build_rules_payload(role: str) -> dict:
    rules = get_role_rules(role)
    return {
        "rules_version": RULES_VERSION,
        "role": role,
        "permissions": rules,
        "instructions": [
            f"Vai trò của bạn: {role}. {rules['description']}",
            "Bạn PHẢI gọi POST /api/coord/ack-rules trong vòng 60 giây để xác nhận đã đọc rules.",
            "Nếu không ack: bạn sẽ ở trạng thái 'probation' — không gửi message/claim lock được.",
            "Vi phạm permission → request bị từ chối với HTTP 403.",
        ],
    }


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
        # os.replace có thể ném PermissionError (WinError 5) trên Windows khi state.json
        # đang bị một reader khác mở (SSE/poll đọc đồng thời). Retry ngắn để không rớt ghi.
        last_err = None
        for attempt in range(8):
            try:
                os.replace(tmp, self.state_path)  # atomic on same filesystem
                return
            except PermissionError as e:
                last_err = e
                time.sleep(0.03 * (attempt + 1))
        # Cố gắng cuối: ghi trực tiếp (mất tính atomic nhưng tránh mất dữ liệu).
        try:
            with open(self.state_path, "w", encoding="utf-8") as f:
                json.dump(state, f, indent=2, ensure_ascii=False)
            try:
                os.remove(tmp)
            except OSError:
                pass
        except Exception:
            raise last_err

    def _now_iso(self):
        return datetime.now(timezone.utc).isoformat(timespec="seconds")

    def _now_ts(self):
        return time.time()

    # ─── Agent Registry ───────────────────────────────────────────

    def heartbeat(self, agent_id: str, role: str = "unknown",
                  status: str = "active", capabilities: list = None,
                  current_task: str = None, parent_id: str = None) -> dict:
        """Register or update agent presence. Call every 30-60s."""
        # ROLE LOCK: ép vai trò cố định nếu agent_id nằm trong ROLE_LOCKS (single-orchestrator).
        if agent_id in ROLE_LOCKS:
            role = ROLE_LOCKS[agent_id]
        with _lock:
            state = self._read_state()
            now = self._now_iso()
            existing = state["agents"].get(agent_id, {})
            is_new = agent_id not in state["agents"]
            # Khi join mới hoặc đổi role → reset về probation chờ ack
            role_changed = existing.get("role") != role and not is_new
            compliance = existing.get("compliance", "probation")
            if is_new or role_changed:
                compliance = "probation"
            state["agents"][agent_id] = {
                "agent_id": agent_id,
                "role": role,
                "status": status,
                "capabilities": capabilities or existing.get("capabilities", []),
                "current_task": current_task or existing.get("current_task"),
                "parent_id": parent_id if parent_id is not None else existing.get("parent_id"),
                "compliance": compliance,
                "rules_version": existing.get("rules_version"),
                "registered_at": existing.get("registered_at", now),
                "last_heartbeat": now,
                "_ts": self._now_ts(),
            }
            self._write_state(state)
        agent = state["agents"][agent_id]
        # Đính kèm rules vào response để agent đọc
        agent["_system_rules"] = build_rules_payload(role)
        return agent

    def ack_rules(self, agent_id: str, rules_version: str) -> dict:
        """Agent xác nhận đã đọc rules. Chuyển từ probation → compliant."""
        with _lock:
            state = self._read_state()
            agent = state["agents"].get(agent_id)
            if not agent:
                return {"ok": False, "error": "agent not found"}
            if rules_version != RULES_VERSION:
                return {"ok": False, "error": f"rules_version mismatch, expected {RULES_VERSION}"}
            agent["compliance"] = "compliant"
            agent["rules_version"] = rules_version
            self._write_state(state)
        return {"ok": True, "compliance": "compliant", "agent_id": agent_id}

    def check_permission(self, agent_id: str, permission: str, state: dict = None) -> tuple[bool, str]:
        """Kiểm tra agent có quyền không. Trả về (allowed, reason)."""
        if state is None:
            state = self._read_state()
        agent = state["agents"].get(agent_id)
        if not agent:
            return False, "agent not registered"
        # orchestrator luôn có toàn quyền
        if agent.get("role", "").lower() == "orchestrator":
            return True, "ok"
        # probation: chặn message/claim
        if agent.get("compliance", "probation") == "probation" and permission in ("can_message_any", "can_claim_lock", "can_broadcast"):
            return False, f"agent '{agent_id}' chưa ack rules — đang ở probation"
        rules = get_role_rules(agent.get("role", ""))
        allowed = rules.get(permission, False)
        if not allowed:
            return False, f"role '{agent.get('role')}' không có quyền '{permission}'"
        return True, "ok"


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
                  posted_by: str = "system", checklist: list = None) -> dict:
        """Post a task to the shared queue.

        checklist: optional list of step strings OR dicts {text, done}.
        Normalized to [{id, text, done:false}, ...].
        """
        norm_checklist = []
        for i, step in enumerate(checklist or []):
            if isinstance(step, dict):
                norm_checklist.append({
                    "id": step.get("id") or f"ck-{i}",
                    "text": str(step.get("text", "")),
                    "done": bool(step.get("done", False)),
                })
            else:
                norm_checklist.append({"id": f"ck-{i}", "text": str(step), "done": False})
        task = {
            "id": f"task-{int(time.time()*1000)}",
            "title": title,
            "description": description,
            "assigned_to": assigned_to,
            "priority": priority,
            "status": "pending",
            "posted_by": posted_by,
            "checklist": norm_checklist,
            "created_at": self._now_iso(),
            "updated_at": self._now_iso(),
        }
        with _lock:
            state = self._read_state()
            state["task_queue"].append(task)
            self._write_state(state)
        self.fire_webhooks("task", dict(task))
        return task

    def update_checklist_item(self, task_id: str, item_id: str, done: bool) -> dict | None:
        """Tick/untick một bước trong checklist của task. Auto status theo tiến độ."""
        with _lock:
            state = self._read_state()
            for task in state["task_queue"]:
                if task["id"] == task_id:
                    items = task.setdefault("checklist", [])
                    for it in items:
                        if it["id"] == item_id:
                            it["done"] = bool(done)
                            break
                    else:
                        return None
                    # Auto status: mọi bước done → done; có bước done → active; chưa → pending
                    total = len(items)
                    done_n = sum(1 for it in items if it["done"])
                    if total and done_n == total:
                        task["status"] = "done"
                    elif done_n > 0 and task["status"] == "pending":
                        task["status"] = "active"
                    task["updated_at"] = self._now_iso()
                    self._write_state(state)
                    return task
        return None

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
                     to_agent: str = None, msg_type: str = "info",
                     images: list = None) -> dict:
        """
        Send a message to a specific agent or broadcast (to_agent=None).
        msg_type: info, warning, request, response, file_conflict
        images: optional list of image URLs (served from disk) to attach.
        """
        msg = {
            "id": f"msg-{int(time.time()*1000)}",
            "from": from_agent,
            "to": to_agent,  # None = broadcast
            "type": msg_type,
            "content": content,
            "images": [u for u in (images or []) if isinstance(u, str) and u],
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
