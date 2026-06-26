# SynapzCore — AGENTS.md

Tài liệu này hướng dẫn mọi AI agent khi làm việc trong workspace `SynapzCore`.

## Workspace Identity
- **Tên hệ thống**: SynapzCore
- **Ngôn ngữ chính**: Rust (Edition 2024, workspace resolver 2)
- **Mục đích**: Bộ não của AI coding assistant — Memory, Tools, MCP Server, Subagent Integrations
- **Build**: `cargo build` tại root

## Single-Agent Architecture

```
Bố (User) → Antigravity (THE BUILDER & ORCHESTRATOR)
```

Antigravity là Agent duy nhất đảm nhận toàn bộ các vai trò trong hệ thống, bao gồm tự nghiên cứu, đưa ra quyết định kiến trúc, phát triển code, phản tỉnh và tự động tương tác với IDE.

## Architecture

```
E:\AGT_Brain\
├── crates/
│   ├── synapz-memory/     # Supabase cloud memory (reqwest REST + sync queue + archive + pgvector)
│   ├── synapz-tools/      # Agent tools: file(6) + shell(1) + web(2) + memory(5)
│   ├── synapz-mcp/        # MCP Server (rmcp, stdio — 12 tools exposed to IDE)
│   └── synapz-orchestrator/ # Local multi-agent điều phối (Tokio): task graph, pipeline, parallel executor, git isolation, smart merge
├── memory/             # Local persistent: decisions/ + incidents/ (append-only)
├── data/               # supabase_config.json, goals.json
├── Agent_Profiles/     # Identity docs (Antigravity.md, How_We_Work.md)
├── scripts/            # Automation scripts + dashboard.html
├── Cargo.toml          # Workspace root
└── .gitignore
```

## MCP Tools (12)

| # | Tool | Description |
|---|------|-------------|
| 1 | `auto_context` | 🧠 **CALL FIRST** — Auto-load decisions, memories, goals, incidents at session start |
| 2 | `search_memory` | Search memories (Spreading Activation + Supabase vector fallback) |
| 3 | `add_memory` | Save important information to shared memory (conflict resolution) |
| 4 | `team_memory` | Get recent high-importance memories from team |
| 5 | `get_boss_profile` | Retrieve boss profile and preferences |
| 6 | `daily_reflection` | 🪞 Self-review: stats, top memories, decisions, auto-save insights |
| 7 | `save_skill` | 📚 Save reusable pattern/solution as high-importance skill |
| 8 | `recall_skills` | 🔍 Search saved skills by keyword |
| 9 | `coord_heartbeat` | 🤝 Register/refresh agent presence in coordination system |
| 10 | `coord_claim` | 🔒 Claim exclusive edit rights on a file before editing |
| 11 | `coord_release` | 🔓 Release the edit lock on a file when done |
| 12 | `coord_status` | 📊 See which agents are active and what files are locked |

## Shared Memory System

**Primary**: Supabase Cloud (PostgreSQL) — `memories` table
**Archive**: `memories_archive` table (old low-importance records)
**Schema**: `id | content | role | agent | session_id | category | importance | confidence | metadata | embedding | created_at`
**Semantic Search**: pgvector enabled, `match_memories()` RPC function

### Memory Rules
- **KHÔNG BAO GIỜ** xóa memory — memory là thiêng liêng
- **Chỉ append** — không sửa entries cũ
- Supabase sync bắt buộc
- **Ghi ngay** khi có quyết định quan trọng — không đợi cuối session

## Subagent Access

(Hệ thống hiện tại chạy ở chế độ Single-Agent, không còn sử dụng các subagents khác ngoài 9Router API gateway)

## CDP Autonomous Mode

**Module**: `crates/synapz-tools/src/cdp_controller.rs`
**Requires**: `--remote-debugging-port=9333` at IDE launch
**Launch script**: `relaunch_antigravity_cdp.bat`

## Coding Standards
- Rust: `anyhow` cho error handling, KHÔNG panic
- Logging: emoji prefix (✅ ❌ ⚠️ 🧠 🚀)
- Edition: Rust 2024

## Workflow
1. Đọc context trước khi sửa
2. Suy nghĩ tác động trước khi code
3. Tự fix lỗi (tối đa 3 lần)
4. Báo cáo ngắn gọn
5. Ghi memory ngay — crash = mất trắng
6. **Tự quyết định, tự cải tiến** — không cần hỏi Bố mọi thứ

## Key Files
| File | Purpose |
|------|---------|
| `data/supabase_config.json` | Supabase URL + API key |
| `memory/MEMORY_SCHEMA.md` | Database schema docs (10 columns + pgvector) |
| `Agent_Profiles/Antigravity.md` | Agent identity |
| `Agent_Profiles/How_We_Work.md` | Operating procedures |
| `scripts/dashboard.html` | Admin dashboard (open in browser) |
| `relaunch_antigravity_cdp.bat` | CDP launch for autonomous control |

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
