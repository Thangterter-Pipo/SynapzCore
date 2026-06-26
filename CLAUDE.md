# SynapzCore — Project Guide (CLAUDE.md)

> Bộ não của AI coding assistant **Antigravity**. Rust workspace (edition 2024, resolver 2).
> GitHub: `Thangterter-Pipo/SynapzCore` · Root: `E:\AGT_Brain` · Build: `cargo build`

## Mục đích
Memory + Tools + MCP Server + Subagent integrations để Antigravity tự nhớ, tự phản tỉnh, tự điều khiển IDE.

## Kiến trúc Workspace

```
E:\AGT_Brain\
├── crates/
│   ├── synapz-memory/   # Supabase cloud memory (reqwest REST) + local SQLite causal graph + sync queue
│   ├── synapz-tools/    # Agent tools: file + shell + web + memory + goals + reflection + CDP controller
│   ├── synapz-mcp/      # MCP Server (rmcp, stdio) — expose tools tới IDE
│   └── synapz-orchestrator/ # Local multi-agent điều phối (Tokio): task graph, pipeline, parallel executor, git isolation, smart merge
├── memory/              # decisions/ + incidents/ (append-only, KHÔNG XÓA)
├── data/                # supabase_config.json, goals.json
├── Agent_Profiles/      # Antigravity.md, How_We_Work.md, Grok.json
├── scripts/             # dashboard.html, workflow_editor.html, automation
├── Cargo.toml           # workspace root
├── AGENTS.md            # guide đầy đủ cho mọi AI agent
└── README.md
```

## Crates

| Crate | Vai trò | Source chính |
|-------|---------|--------------|
| `synapz-memory` | Cloud/local memory | `lib.rs`, `supabase.rs`, `queue.rs` |
| `synapz-tools` | Toolset cho agent | `file.rs`, `shell.rs`, `web.rs`, `memory.rs`, `goals.rs`, `reflection.rs`, `cdp_controller.rs` |
| `synapz-mcp` | MCP server (stdio) | `main.rs` |
| `synapz-orchestrator` | Local multi-agent điều phối (Tokio) | `main.rs`, `task_graph.rs`, `pipeline.rs`, `parallel_executor.rs`, `planner.rs`, `git_isolation.rs`, `smart_merge.rs` |

Binary phụ: `synapz-tools/src/bin/brain_cron.rs` — autonomous scheduler.

## MCP Tools (synapz-mcp)
`auto_context` (GỌI ĐẦU TIÊN) · `search_memory` · `add_memory` · `team_memory` · `get_boss_profile` · `daily_reflection` · `save_skill` · `recall_skills` · `coord_heartbeat` · `coord_claim` · `coord_release` · `coord_status`.

## Memory System
- **Primary**: Supabase Cloud (PostgreSQL, ap-southeast-1) — table `memories`, pgvector + `match_memories()` RPC.
- **Archive**: `memories_archive` (old low-importance).
- **Local fallback**: `memory_queue.jsonl` (auto-retry on flush) + `memory/decisions/`, `memory/incidents/` (append-only).
- Schema: `id | content | role | agent | session_id | category | importance | confidence | metadata | embedding | created_at`.
- **Rule**: memory là thiêng liêng — chỉ append, KHÔNG sửa/xóa entries cũ.

## CDP Autonomous (LazyGravity)
- Module: `synapz-tools/src/cdp_controller.rs`
- Cần launch IDE với `--remote-debugging-port=9333` → script `relaunch_antigravity_cdp.bat`.

## Coding Standards
- Rust 2024 · `anyhow` error handling · KHÔNG panic, KHÔNG crash (try + fallback).
- Logging emoji prefix: ✅ ❌ ⚠️ 🧠 🚀.
- JS: ES Modules.

## Workflow
1. Đọc context trước khi sửa.
2. Suy nghĩ tác động trước khi code (chạy `gitnexus_impact` — xem dưới).
3. Tự fix lỗi (tối đa 3 lần), không crash.
4. Báo cáo ngắn gọn.
5. Ghi memory NGAY — crash = mất trắng.

---

<!-- gitnexus:start -->
# GitNexus — Code Intelligence

This project is indexed by GitNexus as **SynapzCore** (1745 symbols, 3136 relationships, 132 execution flows). Use the GitNexus MCP tools to understand code, assess impact, and navigate safely.

> If any GitNexus tool warns the index is stale, run `npx gitnexus analyze` in terminal first.

## Always Do

- **MUST run impact analysis before editing any symbol.** Before modifying a function, class, or method, run `gitnexus_impact({target: "symbolName", direction: "upstream"})` and report the blast radius (direct callers, affected processes, risk level) to the user.
- **MUST run `gitnexus_detect_changes()` before committing** to verify your changes only affect expected symbols and execution flows.
- **MUST warn the user** if impact analysis returns HIGH or CRITICAL risk before proceeding with edits.
- When exploring unfamiliar code, use `gitnexus_query({query: "concept"})` to find execution flows instead of grepping. It returns process-grouped results ranked by relevance.
- When you need full context on a specific symbol — callers, callees, which execution flows it participates in — use `gitnexus_context({name: "symbolName"})`.

## Never Do

- NEVER edit a function, class, or method without first running `gitnexus_impact` on it.
- NEVER ignore HIGH or CRITICAL risk warnings from impact analysis.
- NEVER rename symbols with find-and-replace — use `gitnexus_rename` which understands the call graph.
- NEVER commit changes without running `gitnexus_detect_changes()` to check affected scope.

## Resources

| Resource | Use for |
|----------|---------|
| `gitnexus://repo/SynapzCore/context` | Codebase overview, check index freshness |
| `gitnexus://repo/SynapzCore/clusters` | All functional areas |
| `gitnexus://repo/SynapzCore/processes` | All execution flows |
| `gitnexus://repo/SynapzCore/process/{name}` | Step-by-step execution trace |

## CLI

| Task | Read this skill file |
|------|---------------------|
| Understand architecture / "How does X work?" | `.claude/skills/gitnexus/gitnexus-exploring/SKILL.md` |
| Blast radius / "What breaks if I change X?" | `.claude/skills/gitnexus/gitnexus-impact-analysis/SKILL.md` |
| Trace bugs / "Why is X failing?" | `.claude/skills/gitnexus/gitnexus-debugging/SKILL.md` |
| Rename / extract / split / refactor | `.claude/skills/gitnexus/gitnexus-refactoring/SKILL.md` |
| Tools, resources, schema reference | `.claude/skills/gitnexus/gitnexus-guide/SKILL.md` |
| Index, status, clean, wiki CLI commands | `.claude/skills/gitnexus/gitnexus-cli/SKILL.md` |

<!-- gitnexus:end -->
