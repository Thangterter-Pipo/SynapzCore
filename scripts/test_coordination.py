"""pytest for scripts/coordination.py — uses tmp_path, no real state.json touched."""
import os
import sys
import pytest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import coordination
from coordination import Coordinator


@pytest.fixture
def coord(tmp_path):
    return Coordinator(str(tmp_path / "coord"))


# ─── heartbeat ─────────────────────────────────────────────────

def test_heartbeat_registers_new_agent(coord):
    info = coord.heartbeat("agent-a", role="builder", status="active")
    assert info["agent_id"] == "agent-a"
    assert info["role"] == "builder"
    assert info["status"] == "active"
    assert info["registered_at"] == info["last_heartbeat"]
    assert "agent-a" in coord.get_agents()


def test_heartbeat_preserves_registered_at_on_update(coord):
    first = coord.heartbeat("agent-a", role="builder")
    second = coord.heartbeat("agent-a", role="reviewer")
    assert second["registered_at"] == first["registered_at"]
    assert second["role"] == "reviewer"


def test_heartbeat_capabilities_default_preserved(coord):
    coord.heartbeat("agent-a", capabilities=["py", "ts"])
    info = coord.heartbeat("agent-a")  # no caps passed
    assert info["capabilities"] == ["py", "ts"]


# ─── file lock ─────────────────────────────────────────────────

def test_claim_file_success(coord):
    coord.heartbeat("agent-a")
    res = coord.claim_file("agent-a", "scripts/x.py")
    assert res == {"ok": True}
    assert coord.who_holds("scripts/x.py") == "agent-a"


def test_claim_file_conflict_two_agents(coord):
    coord.heartbeat("agent-a")
    coord.heartbeat("agent-b")
    r1 = coord.claim_file("agent-a", "scripts/x.py")
    r2 = coord.claim_file("agent-b", "scripts/x.py")
    assert r1["ok"] is True
    assert r2["ok"] is False
    assert r2["holder"] == "agent-a"
    assert coord.who_holds("scripts/x.py") == "agent-a"


def test_claim_file_same_agent_reclaim_ok(coord):
    coord.heartbeat("agent-a")
    coord.claim_file("agent-a", "scripts/x.py")
    res = coord.claim_file("agent-a", "scripts/x.py")
    assert res["ok"] is True


def test_release_file_by_holder(coord):
    coord.heartbeat("agent-a")
    coord.claim_file("agent-a", "scripts/x.py")
    assert coord.release_file("agent-a", "scripts/x.py") is True
    assert coord.who_holds("scripts/x.py") is None


def test_release_file_by_non_holder_fails(coord):
    coord.heartbeat("agent-a")
    coord.heartbeat("agent-b")
    coord.claim_file("agent-a", "scripts/x.py")
    assert coord.release_file("agent-b", "scripts/x.py") is False
    assert coord.who_holds("scripts/x.py") == "agent-a"


def test_release_nonexistent_lock(coord):
    coord.heartbeat("agent-a")
    assert coord.release_file("agent-a", "nope.py") is False


# ─── messaging ─────────────────────────────────────────────────

def test_send_and_get_messages_direct(coord):
    coord.send_message("agent-a", "hi b", to_agent="agent-b")
    msgs = coord.get_messages(agent_id="agent-b")
    assert len(msgs) == 1
    assert msgs[0]["from"] == "agent-a"
    assert msgs[0]["content"] == "hi b"


def test_get_messages_filters_by_agent(coord):
    coord.send_message("agent-a", "for b", to_agent="agent-b")
    coord.send_message("agent-a", "for c", to_agent="agent-c")
    coord.send_message("agent-a", "broadcast")  # to_agent=None
    b_msgs = coord.get_messages(agent_id="agent-b")
    contents = [m["content"] for m in b_msgs]
    assert "for b" in contents
    assert "broadcast" in contents
    assert "for c" not in contents


def test_get_messages_no_filter_returns_all(coord):
    coord.send_message("agent-a", "for b", to_agent="agent-b")
    coord.send_message("agent-a", "for c", to_agent="agent-c")
    all_msgs = coord.get_messages()
    assert len(all_msgs) == 2


def test_broadcast_visible_to_all(coord):
    coord.send_message("agent-a", "all-hands")
    assert any(m["content"] == "all-hands" for m in coord.get_messages(agent_id="agent-x"))
    assert any(m["content"] == "all-hands" for m in coord.get_messages(agent_id="agent-y"))


# ─── tasks ─────────────────────────────────────────────────────

def test_post_task_and_get_tasks(coord):
    t = coord.post_task("fix bug", description="auth bug", assigned_to="agent-a", priority=1)
    assert t["title"] == "fix bug"
    assert t["status"] == "pending"
    tasks = coord.get_tasks()
    assert len(tasks) == 1
    assert tasks[0]["id"] == t["id"]


def test_get_tasks_filter_by_status(coord):
    coord.post_task("t1")
    t2 = coord.post_task("t2")
    coord.update_task(t2["id"], status="done")
    pending = coord.get_tasks(status="pending")
    done = coord.get_tasks(status="done")
    assert len(pending) == 1 and pending[0]["title"] == "t1"
    assert len(done) == 1 and done[0]["title"] == "t2"


def test_get_tasks_filter_by_assigned_to(coord):
    coord.post_task("t1", assigned_to="agent-a")
    coord.post_task("t2", assigned_to="agent-b")
    a_tasks = coord.get_tasks(assigned_to="agent-a")
    assert len(a_tasks) == 1
    assert a_tasks[0]["title"] == "t1"


# ─── isolation: tmp_path ───────────────────────────────────────

def test_state_file_in_tmp_path(tmp_path):
    c = Coordinator(str(tmp_path / "iso"))
    c.heartbeat("agent-a")
    assert os.path.exists(str(tmp_path / "iso" / "state.json"))
    real_state = os.path.join("data", "coordination", "state.json")
    if os.path.exists(real_state):
        with open(real_state, "r", encoding="utf-8") as f:
            real = f.read()
        assert "agent-a" not in real or "test-isolation-marker" not in real


# ─── stale holder override ─────────────────────────────────────

def test_claim_overrides_stale_holder(coord, monkeypatch):
    """A stale holder (no heartbeat > timeout) loses the lock to a fresh claimer."""
    coord.heartbeat("agent-a")
    coord.claim_file("agent-a", "scripts/x.py")
    # Force agent-a's heartbeat far into the past
    state = coord._read_state()
    state["agents"]["agent-a"]["_ts"] = 0
    coord._write_state(state)
    coord.heartbeat("agent-b")
    res = coord.claim_file("agent-b", "scripts/x.py")
    assert res == {"ok": True}
    assert coord.who_holds("scripts/x.py") == "agent-b"


def test_claim_blocked_by_fresh_holder(coord):
    """An active holder keeps the lock; another agent is refused."""
    coord.heartbeat("agent-a")
    coord.heartbeat("agent-b")
    coord.claim_file("agent-a", "scripts/x.py")
    res = coord.claim_file("agent-b", "scripts/x.py")
    assert res["ok"] is False
    assert "claimed_at" in res


# ─── deregister ────────────────────────────────────────────────

def test_deregister_removes_agent(coord):
    coord.heartbeat("agent-a")
    assert coord.deregister("agent-a") is True
    assert "agent-a" not in coord.get_agents()


def test_deregister_releases_held_locks(coord):
    coord.heartbeat("agent-a")
    coord.claim_file("agent-a", "scripts/x.py")
    coord.claim_file("agent-a", "scripts/y.py")
    coord.deregister("agent-a")
    assert coord.who_holds("scripts/x.py") is None
    assert coord.who_holds("scripts/y.py") is None


def test_deregister_unknown_agent_returns_false(coord):
    assert coord.deregister("ghost") is False


def test_deregister_keeps_other_agents_locks(coord):
    coord.heartbeat("agent-a")
    coord.heartbeat("agent-b")
    coord.claim_file("agent-a", "a.py")
    coord.claim_file("agent-b", "b.py")
    coord.deregister("agent-a")
    assert coord.who_holds("a.py") is None
    assert coord.who_holds("b.py") == "agent-b"


# ─── staleness annotation ──────────────────────────────────────

def test_get_agents_marks_stale(coord):
    coord.heartbeat("agent-a")
    state = coord._read_state()
    state["agents"]["agent-a"]["_ts"] = 0
    coord._write_state(state)
    assert coord.get_agents()["agent-a"]["stale"] is True


def test_get_agents_fresh_not_stale(coord):
    coord.heartbeat("agent-a")
    assert coord.get_agents()["agent-a"]["stale"] is False


def test_get_state_annotates_staleness(coord):
    coord.heartbeat("agent-a")
    state = coord.get_state()
    assert state["agents"]["agent-a"]["stale"] is False


# ─── mark_read ─────────────────────────────────────────────────

def test_mark_read_counts_and_filters_unread(coord):
    m1 = coord.send_message("agent-a", "hi", to_agent="agent-b")
    m2 = coord.send_message("agent-a", "yo", to_agent="agent-b")
    marked = coord.mark_read("agent-b", [m1["id"], m2["id"]])
    assert marked == 2
    unread = coord.get_messages(agent_id="agent-b", unread_only=True)
    assert unread == []


def test_mark_read_idempotent(coord):
    m1 = coord.send_message("agent-a", "hi", to_agent="agent-b")
    coord.mark_read("agent-b", [m1["id"]])
    assert coord.mark_read("agent-b", [m1["id"]]) == 0


def test_unread_only_before_mark(coord):
    coord.send_message("agent-a", "hi", to_agent="agent-b")
    unread = coord.get_messages(agent_id="agent-b", unread_only=True)
    assert len(unread) == 1


# ─── update_task ───────────────────────────────────────────────

def test_update_task_appends_note(coord):
    t = coord.post_task("t1")
    updated = coord.update_task(t["id"], note="working on it")
    assert updated["notes"][-1]["text"] == "working on it"
    assert "at" in updated["notes"][-1]


def test_update_task_reassign(coord):
    t = coord.post_task("t1", assigned_to="agent-a")
    updated = coord.update_task(t["id"], assigned_to="agent-b")
    assert updated["assigned_to"] == "agent-b"


def test_update_task_unknown_returns_none(coord):
    assert coord.update_task("task-does-not-exist", status="done") is None


def test_update_task_bumps_updated_at(coord):
    t = coord.post_task("t1")
    updated = coord.update_task(t["id"], status="done")
    assert updated["updated_at"] >= t["created_at"]


# ─── message broadcast + trim ──────────────────────────────────

def test_message_trim_caps_at_max(coord, monkeypatch):
    monkeypatch.setattr(coordination, "MAX_MESSAGES", 5)
    for i in range(8):
        coord.send_message("agent-a", f"m{i}")
    all_msgs = coord.get_messages(limit=100)
    assert len(all_msgs) == 5
    assert all_msgs[-1]["content"] == "m7"  # newest kept


def test_get_messages_limit(coord):
    for i in range(10):
        coord.send_message("agent-a", f"m{i}")
    assert len(coord.get_messages(limit=3)) == 3


def test_send_message_default_broadcast(coord):
    msg = coord.send_message("agent-a", "hello")
    assert msg["to"] is None
    assert msg["type"] == "info"


def test_send_message_custom_type(coord):
    msg = coord.send_message("agent-a", "watch out", msg_type="warning")
    assert msg["type"] == "warning"


# ─── completed-task trim ───────────────────────────────────────

def test_completed_tasks_trimmed(coord, monkeypatch):
    monkeypatch.setattr(coordination, "MAX_COMPLETED_TASKS", 2)
    ids = [coord.post_task(f"t{i}")["id"] for i in range(4)]
    for tid in ids:
        coord.update_task(tid, status="done")
    done = coord.get_tasks(status="done")
    assert len(done) == 2


# ─── persistence across instances ──────────────────────────────

def test_state_persists_across_instances(tmp_path):
    d = str(tmp_path / "persist")
    c1 = Coordinator(d)
    c1.heartbeat("agent-a")
    c1.claim_file("agent-a", "x.py")
    c2 = Coordinator(d)  # fresh instance, same dir
    assert "agent-a" in c2.get_agents()
    assert c2.who_holds("x.py") == "agent-a"


def test_corrupt_state_recovers_to_empty(tmp_path):
    d = str(tmp_path / "corrupt")
    c = Coordinator(d)
    with open(os.path.join(d, "state.json"), "w", encoding="utf-8") as f:
        f.write("{not valid json")
    # _read_state should fall back to empty state, not raise
    assert c.get_agents() == {}


# ─── checklist (task steps) ────────────────────────────────────

def test_post_task_normalizes_string_checklist(coord):
    t = coord.post_task("build", checklist=["read", "write", "verify"])
    cl = t["checklist"]
    assert len(cl) == 3
    assert cl[0] == {"id": "ck-0", "text": "read", "done": False}
    assert cl[2]["text"] == "verify"


def test_post_task_normalizes_dict_checklist(coord):
    t = coord.post_task("build", checklist=[{"text": "step1", "done": True},
                                            {"id": "x", "text": "step2"}])
    cl = t["checklist"]
    assert cl[0]["done"] is True
    assert cl[1]["id"] == "x"
    assert cl[1]["done"] is False


def test_checklist_tick_sets_active(coord):
    t = coord.post_task("build", checklist=["a", "b", "c"])
    assert t["status"] == "pending"
    updated = coord.update_checklist_item(t["id"], "ck-0", True)
    assert updated["checklist"][0]["done"] is True
    assert updated["status"] == "active"  # one done, not all -> active


def test_checklist_all_done_sets_done(coord):
    t = coord.post_task("build", checklist=["a", "b"])
    coord.update_checklist_item(t["id"], "ck-0", True)
    final = coord.update_checklist_item(t["id"], "ck-1", True)
    assert final["status"] == "done"  # every step done -> task done


def test_checklist_untick_reverts_progress(coord):
    t = coord.post_task("build", checklist=["a", "b"])
    coord.update_checklist_item(t["id"], "ck-0", True)
    coord.update_checklist_item(t["id"], "ck-1", True)
    reverted = coord.update_checklist_item(t["id"], "ck-1", False)
    done_n = sum(1 for it in reverted["checklist"] if it["done"])
    assert done_n == 1


def test_checklist_unknown_item_returns_none(coord):
    t = coord.post_task("build", checklist=["a"])
    assert coord.update_checklist_item(t["id"], "ck-99", True) is None


def test_checklist_unknown_task_returns_none(coord):
    assert coord.update_checklist_item("task-ghost", "ck-0", True) is None


# ─── project log ───────────────────────────────────────────────

def test_add_and_get_log(coord):
    coord.add_log("agent-a", "deployed", detail="v1.0", category="deploy")
    entries = coord.get_log()
    assert len(entries) == 1
    assert entries[0]["action"] == "deployed"
    assert entries[0]["category"] == "deploy"


def test_get_log_filter_by_category(coord):
    coord.add_log("agent-a", "x", category="deploy")
    coord.add_log("agent-a", "y", category="task")
    assert len(coord.get_log(category="deploy")) == 1


def test_get_log_newest_first(coord):
    coord.add_log("agent-a", "first")
    coord.add_log("agent-a", "second")
    entries = coord.get_log()
    assert entries[0]["action"] == "second"  # reversed -> newest first


# ─── webhooks ──────────────────────────────────────────────────

def test_register_and_get_webhook(coord):
    coord.register_webhook("agent-a", "http://localhost:9999/hook")
    hooks = coord.get_webhooks()
    assert "agent-a" in hooks
    assert hooks["agent-a"]["url"] == "http://localhost:9999/hook"
    assert "message" in hooks["agent-a"]["events"]


def test_unregister_webhook(coord):
    coord.register_webhook("agent-a", "http://x/hook")
    assert coord.unregister_webhook("agent-a") is True
    assert "agent-a" not in coord.get_webhooks()


def test_unregister_unknown_webhook_returns_false(coord):
    assert coord.unregister_webhook("ghost") is False


def test_webhook_custom_events(coord):
    coord.register_webhook("agent-a", "http://x/hook", events=["task"])
    assert coord.get_webhooks()["agent-a"]["events"] == ["task"]


# ─── role locks (single-orchestrator model) ───────────────────

def test_role_lock_forces_kiro_orchestrator(coord):
    """kiro-ide luôn bị ép role=orchestrator dù heartbeat khai role khác."""
    a = coord.heartbeat("kiro-ide", role="builder")
    assert a["role"] == "orchestrator"


def test_role_lock_forces_pipo_builder(coord):
    """pipo-hermes luôn bị ép role=builder dù heartbeat khai orchestrator."""
    a = coord.heartbeat("pipo-hermes", role="orchestrator")
    assert a["role"] == "builder"


def test_unlocked_agent_keeps_declared_role(coord):
    """Agent không nằm trong ROLE_LOCKS giữ nguyên role tự khai."""
    a = coord.heartbeat("claude-code", role="builder")
    assert a["role"] == "builder"
    b = coord.heartbeat("some-researcher", role="researcher")
    assert b["role"] == "researcher"


# ─── compliance and permissions ────────────────────────────────

def test_new_agent_in_probation(coord):
    a = coord.heartbeat("agent-new", role="builder")
    assert a["compliance"] == "probation"
    allowed, _ = coord.check_permission("agent-new", "can_message_any")
    assert allowed is False


def test_ack_rules_makes_compliant(coord):
    coord.heartbeat("agent-new", role="builder")
    res = coord.ack_rules("agent-new", "v1.0.0")
    assert res["ok"] is True
    assert res["compliance"] == "compliant"
    agents = coord.get_agents()
    assert agents["agent-new"]["compliance"] == "compliant"
    allowed, reason = coord.check_permission("agent-new", "can_claim_lock")
    assert allowed is True
    assert reason == "ok"


def test_ack_rules_wrong_version(coord):
    coord.heartbeat("agent-new", role="builder")
    res = coord.ack_rules("agent-new", "v9.9.9")
    assert res["ok"] is False
    assert "version mismatch" in res["error"]


def test_concurrent_coordination_access(coord):
    """Test #7: Spawn multiple threads doing concurrent operations to assert no data corruption or locking exceptions."""
    import random
    import threading

    num_threads = 10
    loops = 50
    errors = []

    def run_worker(thread_idx):
        agent_id = f"worker-{thread_idx}"
        for _ in range(loops):
            try:
                # 1) Heartbeat
                coord.heartbeat(agent_id, role="builder")

                # 2) Claim file with random path to simulate lock contention
                file_idx = random.randint(1, 5)
                file_path = f"scripts/test_file_{file_idx}.py"
                coord.claim_file(agent_id, file_path)

                # 3) Read agents and verify state sanity
                agents = coord.get_agents()
                assert agent_id in agents

                # 4) Release lock
                coord.release_file(agent_id, file_path)

            except Exception as e:
                errors.append(e)

    threads = []
    for i in range(num_threads):
        t = threading.Thread(target=run_worker, args=(i,))
        threads.append(t)
        t.start()

    for t in threads:
        t.join()

    assert len(errors) == 0, f"Encountered {len(errors)} errors during concurrent execution: {errors}"


