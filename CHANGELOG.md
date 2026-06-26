# Changelog

All notable changes to SynapzCore are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/); this project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- English **Quickstart (5 minutes)** section at the top of the README.
- `CONTRIBUTING.md` (EN), issue templates (bug/feature), and a PR template.
- GitHub Actions CI (`.github/workflows/ci.yml`) with four hard gates:
  `cargo fmt --check`, `cargo clippy -D warnings`, `cargo build`, `cargo test`.
- CI / License / Rust-edition badges in the README.
- Offline orchestrator demo: `examples/demo_graph.json` + `examples/README.md`,
  runnable via `cargo run -p synapz-orchestrator -- --pipeline examples/demo_graph.json --echo --roles`
  (no cloud, no LLM, no network).
- Local memory fallback: `MemoryBrain::recall` / `fetch_recent` read the on-disk
  JSONL queue when Supabase is unavailable, so recall works offline.
- Configurable orchestrator roster via the `SYNAPZ_AGENTS` env var
  (`id:model:role` triples) plus a built-in `default_roster()`; documented in `.env.example`.
- Regression and unit tests: UTF-8-safe truncation, local-queue search, role parsing,
  default roster.

### Fixed
- **Crash on non-ASCII content**: byte-slicing (`&s[..N]`) panicked mid-multibyte
  UTF-8 character in `synapz-mcp`, `synapz-orchestrator`, and `brain-cron`. Replaced
  with a char-safe `truncate_chars()` helper. This affected any Vietnamese (or other
  multibyte) memory content shown in summaries.

### Changed
- Removed hardcoded absolute paths (`E:\AGT_Brain`) from `search_code` and `brain-cron`;
  paths now resolve from `SYNAPZ_ROOT` → executable location → current directory.
- Orchestrator agent roster is no longer hardcoded to personal model names; it ships
  generic placeholders and is overridable without editing code.
- Repository is now `cargo fmt` / `cargo clippy -D warnings` clean across the workspace.

### Notes
- Supabase integration tests self-skip when no credentials are present, keeping a
  stranger's clone and CI green without secrets.

## [0.1.0] - initial
- Rust workspace: `synapz-memory`, `synapz-tools`, `synapz-mcp`, `synapz-orchestrator`.
- MCP server exposing memory + coordination tools; multi-agent orchestrator with
  task graph, git isolation, smart merge, and self-correct loop.
