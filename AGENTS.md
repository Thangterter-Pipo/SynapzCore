# Antigravity Brain — AGENTS.md

Tài liệu này hướng dẫn mọi AI agent khi làm việc trong workspace `AGT_Brain`.

## Workspace Identity
- **Tên hệ thống**: Antigravity Brain
- **Ngôn ngữ chính**: Rust (Edition 2024, workspace resolver 2)
- **Mục đích**: Bộ não của AI coding assistant — Memory, Tools, MCP Server, Subagent Integrations
- **Build**: `cargo build` tại root

## 2-AI Team Architecture

```
Bố (User) → Antigravity (THE BUILDER) → Grok "Gravity" (RESEARCHER)
```

| Tác vụ | Route tới |
|--------|-----------|
| Research công nghệ mới | **Grok** (research mode) |
| Quyết định kiến trúc | **Grok** (think mode) |
| Code review | **Grok** (review mode) |
| Debug logic / verify | **Grok** (think mode) |
| Viết documentation | **Antigravity** (trực tiếp) |
| Implement feature | **Antigravity** (trực tiếp) |

## Architecture

```
E:\AGT_Brain\
├── crates/
│   ├── agt-memory/     # Supabase cloud memory (reqwest REST + sync queue + archive + pgvector)
│   ├── agt-tools/      # Agent tools: file(6) + shell(1) + web(2) + memory(5) + grok(2)
│   └── agt-mcp/        # MCP Server (rmcp, stdio — 10 tools exposed to IDE)
├── memory/             # Local persistent: decisions/ + incidents/ (append-only)
├── data/               # supabase_config.json, goals.json
├── Agent_Profiles/     # Identity docs (Antigravity.md, How_We_Work.md, Grok.json)
├── scripts/            # Automation scripts + dashboard.html
├── Cargo.toml          # Workspace root
└── .gitignore
```

## MCP Tools (10)

| # | Tool | Description |
|---|------|-------------|
| 1 | `auto_context` | 🧠 **CALL FIRST** — Auto-load decisions, memories, goals, incidents at session start |
| 2 | `search_memory` | Search memories by keyword, optionally filter by agent |
| 3 | `add_memory` | Save important information to shared memory |
| 4 | `team_memory` | Get recent high-importance memories from team |
| 5 | `get_boss_profile` | Retrieve boss profile and preferences |
| 6 | `ask_grok` | Call Grok subagent (research/think/review/brainstorm) |
| 7 | `grok_health` | Check Grok API health |
| 8 | `daily_reflection` | 🪞 Self-review: stats, top memories, decisions, auto-save insights |
| 9 | `save_skill` | 📚 Save reusable pattern/solution as high-importance skill |
| 10 | `recall_skills` | 🔍 Search saved skills by keyword |

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

### Grok — `crates/agt-tools/src/grok.rs`
- **Endpoint**: `http://127.0.0.1:8000` (grok2api)
- **Models**: grok-3, grok-3-mini, grok-3-thinking, grok-4, grok-4-thinking, grok-4-heavy
- **Modes**: research, think, review, brainstorm, chat
- **CLI**: `ask-grok --mode research "topic"`

## CDP Autonomous Mode

**Module**: `crates/agt-tools/src/cdp_controller.rs`
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
| `Agent_Profiles/Grok.json` | Grok subagent config + Gravity Framework |
| `scripts/dashboard.html` | Admin dashboard (open in browser) |
| `relaunch_antigravity_cdp.bat` | CDP launch for autonomous control |

<!-- gitnexus:start -->
# GitNexus — Code Intelligence

This project is indexed by GitNexus as **AGT_Brain** (6351 symbols, 11094 relationships, 300 execution flows). Use the GitNexus MCP tools to understand code, assess impact, and navigate safely.

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
| `gitnexus://repo/AGT_Brain/context` | Codebase overview, check index freshness |
| `gitnexus://repo/AGT_Brain/clusters` | All functional areas |
| `gitnexus://repo/AGT_Brain/processes` | All execution flows |
| `gitnexus://repo/AGT_Brain/process/{name}` | Step-by-step execution trace |

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
