# SynapzCore — Software Factory Workflow (SDAF-8 v3)

> Quy trình vận hành đội AI Agents trên **mọi loại dự án**.
> Khác bản cũ: gắn trực tiếp vào hạ tầng thật của SynapzCore (MCP coord/memory tools,
> crate `synapz-orchestrator`, GitNexus impact, git branch isolation), tiêu chí **đo được**,
> state machine rõ ràng, retry/escalation có giới hạn. Bỏ phần lý thuyết lặp lại.

---

## 0. Thực tế triển khai (đọc trước)

- Hệ thống chạy **Single-Agent**: Antigravity đóng đủ 8 **vai trò (role)** theo từng stage,
  hoặc spawn subagent qua `synapz-orchestrator` khi cần chạy song song.
  → "8 agent" = **8 chiếc mũ**, không bắt buộc 8 process.
- Mọi state vận hành dùng **tool thật**, không bịa file JSON tay:
  - File lock  → `coord_claim` / `coord_release` / `coord_status` (MCP).
  - Memory/quyết định → `add_memory`, `auto_context`, `search_memory`, `daily_reflection`.
  - Song song + git isolation + smart merge → crate `synapz-orchestrator`.
  - Impact trước khi sửa symbol → GitNexus `gitnexus_impact` (bắt buộc, theo AGENTS.md).
- File `.md` báo cáo (research/architecture/test/review/deploy) là **artifact**,
  trạng thái sống nằm trong tool + Task Board.

---

## 1. Đội hình & 1 dòng trách nhiệm

| # | Role | Làm gì | KHÔNG được làm |
|---|------|--------|----------------|
| 1 | **Orchestrator** | chia task, giao việc, gác gate, merge, nghiệm thu | code lớn, bỏ qua gate |
| 2 | **Researcher** | so sánh giải pháp, tìm rủi ro, đề xuất | ghi vào code |
| 3 | **Architect** | kiến trúc, module map, API/DB contract, ranh giới task | viết impl chi tiết |
| 4 | **Builder-Core** | backend/core/db/business logic + self-test | sửa ngoài Allowed Files |
| 5 | **Builder-Integration** | UI/integration/CLI/config + self-test | sửa file Core chưa unlock |
| 6 | **Tester** | viết & chạy test 4 lớp, ra PASS/FAIL + bug list | sửa code chính |
| 7 | **Reviewer** | chất lượng/security/scope → APPROVED/CHANGES/REJECTED | tự sửa rồi tự duyệt |
| 8 | **DevOps** | build/deploy/release note/rollback | sửa business logic |

---

## 2. Pipeline & các gate cứng

```
REQUEST
  → [G0] INTAKE        Orchestrator: chuẩn hóa spec, phân loại dự án
  → [G1] SCAN          (existing project) auto_context + gitnexus query → MODULE_MAP
  → RESEARCH           Researcher → RESEARCH_REPORT
  → ARCHITECTURE       Architect → ARCHITECTURE_PLAN + ADR + contracts
  → TASK SPLIT         Orchestrator: task atomic, allowed/forbidden files
  → [G2] BUILD ∥       Builder-Core + Builder-Integration (coord_claim trước khi sửa)
  → INTEGRATION        orchestrator smart-merge vào integration branch
  → [G3] TEST          Tester: 4 lớp, ngưỡng pass đo được
  → [G4] REVIEW        Reviewer: checklist + impact
  → [G5] BUILD/DEPLOY  DevOps: build sạch, staging, rollback plan
  → [G6] ACCEPT        Orchestrator: đối chiếu Definition of Done → FINAL_REPORT
```

**Gate là bắt buộc, không nhảy cóc:**
- G2: cấm code symbol chưa chạy `gitnexus_impact`; HIGH/CRITICAL phải báo trước khi sửa.
- G3: không review khi test fail.
- G4: không deploy khi chưa APPROVED.
- G6: không DONE khi thiếu bất kỳ mục Definition of Done.

---

## 3. Vòng đời Task (state machine)

```
BACKLOG → READY → CLAIMED → WORKING
   → WAITING_TEST → (fail) RANGE WORKING
   → WAITING_REVIEW → (changes) WORKING
   → WAITING_DEPLOY → DONE
                    ↘ BLOCKED  (thiếu input/secret/quyết định)
```

Chuyển trạng thái = ghi `add_memory(category="task_state")` + cập nhật Task Board.

### Retry / Escalation có giới hạn (chống loop vô hạn)
- Builder tự fix bug **tối đa 3 lần**. Lần 4 fail → `BLOCKED`, đẩy Architect/Researcher.
- Cùng một lỗi lặp 2 lần → **dừng vá lẻ**, phân tích root cause, đổi hướng (theo How_We_Work).
- BLOCKED quá 1 chu kỳ mà thiếu input của Bố → Orchestrator hỏi Bố đúng 1 câu gọn.

---

## 4. Task template (atomic, đo được)

```md
# TASK-001: <tên>
state: READY
role: Builder-Core
depends_on: []                 # task khác phải DONE trước
parallel_safe: true            # cho phép chạy cùng task khác

## Goal            <1 câu, 1 mục tiêu>
## Context         <link RESEARCH/ARCHITECTURE liên quan>
## Allowed Files   <whitelist — chỉ sửa trong đây>
## Forbidden Files <blacklist tường minh>
## Requirements    R1 / R2 / R3
## Acceptance (đo được, có thể tick máy)
- [ ] cargo build OK (0 error)
- [ ] test mới phủ Requirements, PASS
- [ ] không giảm coverage vùng đụng tới
- [ ] gitnexus_impact đã chạy, risk ≤ MEDIUM hoặc đã được duyệt
- [ ] không phá flow cũ (regression PASS)
## Output          summary / files changed / commands / test result / risks
```

Quy tắc chia: 1 task = 1 mục tiêu, ≤ ~1 vùng module, có thể test độc lập.
2 task `parallel_safe` chỉ chạy song song khi **tập Allowed Files rời nhau**.

---

## 5. File Lock — dùng coord tools, không JSON tay

```
Trước khi sửa file X:
  coord_heartbeat            # đăng ký hiện diện
  coord_claim(file=X, task=TASK-00n)   # khóa độc quyền
  ... sửa ...
  coord_release(file=X)      # nhả khóa khi xong
Kẹt khóa? coord_status để xem ai đang giữ file gì.
```

Builder-Core và Builder-Integration **không bao giờ** cùng giữ một file.
Nếu cần file của nhau → tách task hoặc xếp tuần tự qua `depends_on`.

---

## 6. Git isolation & merge (qua synapz-orchestrator)

```
main                      # chỉ release, không ai push thẳng
develop                   # tích hợp ổn định
integration/TASK-xxx      # orchestrator gom diff các builder
feature/TASK-xxx-slug     # mỗi builder 1 nhánh
```

- Mỗi subagent build trong worktree/branch riêng (git isolation của crate).
- Merge dùng **smart merge** của orchestrator vào `integration/*`, rồi mới qua Tester.
- Trước merge: `gitnexus_detect_changes()` xác nhận chỉ đụng symbol/flow dự kiến.

---

## 7. Tiêu chí Test 4 lớp (Tester)

1. Happy path  — luồng đúng.
2. Edge case   — biên dữ liệu (rỗng/null/max/unicode).
3. Error case  — input sai, thiếu, sai kiểu → phải xử lý, không panic.
4. Regression  — flow cũ còn chạy (đối chiếu gitnexus processes).

Output cứng: `PASS | FAIL` + bug list. FAIL → tạo `BUG-xxx` → Orchestrator gán Builder.
Rust: `cargo test` + smoke build; không có framework sẵn thì để **1 self-check chạy được**
(assert-based hoặc 1 test nhỏ), không kéo framework mới.

---

## 8. Review checklist (Reviewer → APPROVED/CHANGES_REQUESTED/REJECTED)

- Đúng Requirements & Acceptance.
- Trong phạm vi Allowed Files, không file thừa.
- Không lộ secret (env/key/token).
- Error handling: `anyhow`, **không panic** (chuẩn SynapzCore).
- Logging emoji prefix (✅ ❌ ⚠️ 🧠 🚀).
- Không phá kiến trúc/contract của Architect.
- Có test tương ứng thay đổi.

---

## 9. Definition of Done (đủ 10 mới DONE)

1. Đúng yêu cầu user.  2. `cargo build` 0 error.  3. Test 4 lớp PASS.
4. Không phá flow cũ (regression).  5. Reviewer APPROVED.  6. Không lộ secret.
7. Có error handling + log hợp lý.  8. Không sửa ngoài scope.
9. DevOps build/deploy được + có rollback.  10. Orchestrator duyệt + FINAL_REPORT.

---

## 10. Project Adapter (1 quy trình, nhiều loại dự án)

Cố định **quy trình**, thay **adapter** theo loại:

| Loại | Builder-Core | Builder-Integration |
|------|--------------|---------------------|
| Web | API/DB/auth/business | FE/UI/integration/validation |
| Desktop | core/db/service/file | UI/event/binding/export |
| Bot/Automation | engine/runner/api client | dashboard/config/log/notify |
| AI system | agent runtime/tool/memory | pipeline UI/monitor/prompt/report |
| Rust crate (SynapzCore) | crate logic, MCP tool impl | wiring, config, script, CLI |

Mode → output bắt buộc:
- `new`: brief, architecture, code, test, deploy report
- `existing`: scan, module_map, change_plan, regression report
- `bug_fix`: reproduce, root_cause, patch, regression test
- `feature`: spec, contract, impl, test cases
- `refactor`: plan, behavior-preserving tests, diff
- `deploy`: build_log, env config, release note, rollback plan

---

## 11. Artifacts (chứng cứ, không phải state)

```
.agents/
├── tasks/TASK-*.md
├── reports/{research,architecture,testing,review,deployment}/
├── memory/decisions.md      # mirror — nguồn thật là Supabase memory
└── pipeline.yaml            # mục 12
```
Quyết định lớn → `add_memory` **ngay** (crash = mất trắng), rồi mirror vào decisions.md.

---

## 12. pipeline.yaml (lõi máy đọc)

```yaml
pipeline:
  name: SDAF-8
  version: 3.0
  roles: [orchestrator, researcher, architect, builder_core,
          builder_integration, tester, reviewer, devops]
  stages: [intake, scan, research, architecture, planning,
           build, integration, testing, review, deploy, accept]
  gates:
    impact_before_edit: true        # gitnexus_impact bắt buộc
    file_lock_via_coord: true        # coord_claim/release
    no_direct_main_write: true
    test_before_review: true
    review_before_deploy: true
    orchestrator_final_approval: true
  limits:
    builder_self_fix_max: 3
    same_error_repeat_max: 2
  parallel:
    allowed_when: disjoint_allowed_files
```

---

## 13. Tóm tắt 1 câu

> **SCAN → CLASSIFY → PLAN → BUILD∥ → TEST → REVIEW → DEPLOY → ACCEPT**, mỗi bước có
> gate đo được, lock qua coord tools, song song qua orchestrator, an toàn qua gitnexus impact.
> Đổi loại dự án chỉ đổi **adapter**, không đổi quy trình.
