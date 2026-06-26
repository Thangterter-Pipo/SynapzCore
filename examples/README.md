# Examples — run the orchestrator offline

These run with the **echo executor** (`--echo`): no Supabase, no LLM/CLI, no network.
A stranger can clone and watch the multi-agent pipeline work in one command.

## Parallel pipeline (4 stages)

```bash
cargo run -p synapz-orchestrator -- --pipeline examples/demo_graph.json --echo --roles
```

`demo_graph.json` describes a login feature as a task graph:

- `research-auth` (Researcher) → no deps
- `api-login` (Coder) → after research
- `ui-login` (Coder) → after research (runs in parallel with api-login)
- `test-login` (Tester) → after both

The run prints all 4 stages and writes `data/last_pipeline_run.json` +
`data/last_buffer_snapshot.json`:

```
━━━ GIAI ĐOẠN 1: FAN-OUT (phân tầng phụ thuộc) ━━━
   Tầng 0: [research-auth[Researcher]] — 1 task song song
   Tầng 1: [api-login[Coder], ui-login[Coder]] — 2 task song song
   Tầng 2: [test-login[Tester]] — 1 task song song
...
🏁 4 task xong | 0 hỏng | 3 tầng | ✅ TẤT CẢ XANH
```

## Other offline modes

```bash
cargo run -p synapz-orchestrator -- --graph examples/demo_graph.json --echo   # fan-out only
cargo run -p synapz-orchestrator -- --scan                                    # print machine capabilities
```

## Live mode (needs a real CLI agent + keys)

```bash
cargo run -p synapz-orchestrator -- --live "your prompt"   # routes to a real CLI agent
```

Drop `--echo` from `--pipeline`/`--graph` to use live executors. That path needs the
`NINEROUTER_*` env vars (see `.env.example`) and an installed CLI agent.
