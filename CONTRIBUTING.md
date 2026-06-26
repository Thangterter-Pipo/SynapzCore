# Contributing to SynapzCore

Thanks for your interest! SynapzCore is a Rust multi-agent coding orchestrator with
long-term memory and an MCP server. This guide gets you from clone to a passing build.

## Prerequisites

- Rust (stable, edition 2024) - install via <https://rustup.rs>
- Git
- (Optional) A Supabase project + Python 3.10+ only if you want the cloud memory layer.
  The workspace builds and the MCP server starts WITHOUT any Supabase config.

## 5-minute start

```bash
git clone https://github.com/Thangterter-Pipo/SynapzCore.git
cd SynapzCore
cp .env.example .env          # optional: fill SUPABASE_URL/KEY for cloud memory
cargo build --workspace       # builds all 4 crates
cargo test --workspace        # all tests pass with no secrets (cloud tests self-skip)
```

Run the MCP server (stdio) and point your IDE (Cursor / VS Code / Claude) at it:

```bash
cargo run -p synapz-mcp
```

Without Supabase credentials, memory tools return a clear "config error" message
instead of crashing - everything else (coordination, reflection scaffolding) works.

## Project layout

| Crate | Role |
|-------|------|
| `synapz-memory` | Supabase REST client + crash-safe write-ahead queue |
| `synapz-tools` | Agent tools (file/shell/web/memory) + CDP controller |
| `synapz-mcp` | MCP server (rmcp, stdio) exposing 12 tools to an IDE |
| `synapz-orchestrator` | Multi-agent: task graph, git isolation, smart merge, self-correct |

Workflow philosophy lives in `structure.md` (SDAF-8). Open-source direction lives in
`ROADMAP_OPENSOURCE.md`.

## Before opening a PR

1. `cargo build --workspace --all-targets` is green.
2. `cargo test --workspace` is green.
3. `cargo fmt --all` applied (CI reports fmt/clippy but does not block yet).
4. Keep changes scoped; do not commit secrets (`.env`, `data/`, `*_config.json` are gitignored).
5. Describe what changed and why in the PR.

## Security

Found a vulnerability? Do not open a public issue — see [SECURITY.md](SECURITY.md).

## Reporting bugs / requesting features

Use the issue templates under `.github/ISSUE_TEMPLATE`. Include OS, Rust version,
and exact commands when reporting a build/runtime problem.

## License

By contributing you agree your contributions are licensed under the MIT License.
