//! synapz-orchestrator — Local Multi-Agent điều phối bằng Tokio.
//!
//! PoC: tạo 3 agent (Coder/Tester/Researcher), Orchestrator phát lệnh qua broadcast,
//! gom báo cáo qua mpsc, in capability máy. RAM siêu nhẹ, 1 binary.
//!
//! NOTE: crate đang xây dở — một số API (task_graph, work_buffer, smart_merge,
//! git_isolation...) đã viết sẵn nhưng chưa wire hết vào luồng chính. Cho phép
//! dead_code ở mức crate cho tới khi orchestrator chạy full pipeline.
#![allow(dead_code)]

mod agent;
mod coordinator;
mod git_isolation;
mod parallel_executor;
mod pipeline;
mod planner;
mod roles;
mod runner;
mod scanner;
mod self_correct;
mod smart_merge;
mod state;
mod task_graph;
mod work_buffer;

use agent::spawn_agent;
use coordinator::Coordinator;
use roles::{AgentRole, Command};

/// Default demo roster (id, model, role). Overridable without code changes via the
/// SYNAPZ_AGENTS env var: comma-separated id:model:role triples, e.g.
/// `SYNAPZ_AGENTS="dev:gpt-4o:Coder,qa:gpt-4o-mini:Tester"`. This decouples the engine
/// from any one person's model/provider choices.
fn default_roster() -> Vec<(String, String, AgentRole)> {
    if let Ok(spec) = std::env::var("SYNAPZ_AGENTS") {
        let parsed: Vec<(String, String, AgentRole)> = spec
            .split(',')
            .filter_map(|entry| {
                let parts: Vec<&str> = entry.split(':').map(|s| s.trim()).collect();
                match parts.as_slice() {
                    [id, model, role] if !id.is_empty() && !model.is_empty() => {
                        parse_role(role).map(|r| (id.to_string(), model.to_string(), r))
                    }
                    _ => None,
                }
            })
            .collect();
        if !parsed.is_empty() {
            return parsed;
        }
        eprintln!("⚠️ SYNAPZ_AGENTS set but unparsable; using built-in roster");
    }
    vec![
        (
            "coder-01".to_string(),
            "model-coder".to_string(),
            AgentRole::Coder,
        ),
        (
            "tester-01".to_string(),
            "model-tester".to_string(),
            AgentRole::Tester,
        ),
        (
            "researcher-01".to_string(),
            "model-researcher".to_string(),
            AgentRole::Researcher,
        ),
    ]
}

/// Parse a role name (case-insensitive) into an AgentRole.
fn parse_role(s: &str) -> Option<AgentRole> {
    match s.to_ascii_lowercase().as_str() {
        "orchestrator" => Some(AgentRole::Orchestrator),
        "coder" => Some(AgentRole::Coder),
        "builder" => Some(AgentRole::Builder),
        "tester" => Some(AgentRole::Tester),
        "researcher" => Some(AgentRole::Researcher),
        "unassigned" => Some(AgentRole::Unassigned),
        _ => None,
    }
}
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    load_dotenv(); // nạp NINEROUTER_* / SUPABASE_* từ .env nếu chưa có trong env

    // Chế độ quét AI agent: `synapz-orchestrator --scan`
    if std::env::args().any(|a| a == "--scan") {
        return run_scan().await;
    }

    // Chế độ live: quét → nối agent CLI thật → giao 1 task.
    // `synapz-orchestrator --live "prompt"` (gom hết args sau --live thành prompt)
    let args: Vec<String> = std::env::args().collect();
    if let Some(pos) = args.iter().position(|a| a == "--live") {
        let rest = &args[pos + 1..];
        let prompt = if rest.is_empty() {
            "trả lời ngắn: OK".to_string()
        } else {
            rest.join(" ")
        };
        return run_live(prompt).await;
    }

    // Chế độ GRAPH: chạy một đồ thị task song song từ file JSON.
    // `synapz-orchestrator --graph path/to/graph.json [--echo]`
    //   --echo : dùng executor giả (không gọi CLI thật) để demo/test fan-out.
    if let Some(pos) = args.iter().position(|a| a == "--graph") {
        let path = args.get(pos + 1).cloned().unwrap_or_default();
        if path.is_empty() {
            eprintln!("❌ Thiếu đường dẫn: --graph <file.json>");
            std::process::exit(2);
        }
        let echo = args.iter().any(|a| a == "--echo");
        return run_graph(&path, echo).await;
    }

    // Chế độ PIPELINE: chạy TRỌN 4 giai đoạn (fan-out → buffer → merge → self-correct).
    // `synapz-orchestrator --pipeline path/to/graph.json [--echo] [--roles] [--git]`
    //   --roles : route task theo role (Coder→Claude, Tester→Codex, Researcher→Hermes)
    //   --git   : mỗi task code trên 1 branch git riêng, merge sau (sandbox isolation)
    if let Some(pos) = args.iter().position(|a| a == "--pipeline") {
        let path = args.get(pos + 1).cloned().unwrap_or_default();
        if path.is_empty() {
            eprintln!("❌ Thiếu đường dẫn: --pipeline <file.json>");
            std::process::exit(2);
        }
        let echo = args.iter().any(|a| a == "--echo");
        let roles = args.iter().any(|a| a == "--roles");
        let git = args.iter().any(|a| a == "--git");
        // --jobs N: giới hạn số task chạy đồng thời (0/bỏ trống = auto theo RAM).
        let jobs = args
            .iter()
            .position(|a| a == "--jobs")
            .and_then(|p| args.get(p + 1))
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(0);
        return run_pipeline(&path, echo, roles, git, jobs).await;
    }

    // Chế độ PLAN: orchestrator tự phân rã mục tiêu NN tự nhiên → TaskGraph (qua LLM).
    // `synapz-orchestrator --plan "mục tiêu" [--run] [--echo|--roles|--git] [--jobs N]`
    //   không --run : chỉ sinh + in + lưu data/last_plan.json (xem trước kế hoạch).
    //   --run       : sinh xong chạy luôn pipeline trên graph vừa sinh.
    if let Some(pos) = args.iter().position(|a| a == "--plan") {
        // Gom các token sau --plan (tới flag kế tiếp bắt đầu bằng "--") thành mục tiêu.
        let goal: String = args[pos + 1..]
            .iter()
            .take_while(|a| !a.starts_with("--"))
            .cloned()
            .collect::<Vec<_>>()
            .join(" ");
        if goal.trim().is_empty() {
            eprintln!("❌ Thiếu mục tiêu: --plan \"mô tả mục tiêu\"");
            std::process::exit(2);
        }
        let run = args.iter().any(|a| a == "--run");
        let echo = args.iter().any(|a| a == "--echo");
        let roles = args.iter().any(|a| a == "--roles");
        let git = args.iter().any(|a| a == "--git");
        let jobs = args
            .iter()
            .position(|a| a == "--jobs")
            .and_then(|p| args.get(p + 1))
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(0);
        return run_plan(&goal, run, echo, roles, git, jobs).await;
    }

    println!("🧠 SynapzCore Orchestrator — khởi động Tokio runtime\n");

    // 1. Quét năng lực máy (async, không block).
    let caps = scanner::scan_local_environment().await;
    println!("🔍 Capability phát hiện: {:?}\n", caps);

    // 2. Shared state + coordinator.
    let st = state::new_shared_state();
    let (coord, chans) = Coordinator::new(st.clone(), 64, 256);

    // 3. Spawn agent từ roster (mặc định hoặc SYNAPZ_AGENTS). Mỗi agent 1 receiver.
    let roster = default_roster();
    let handles: Vec<_> = roster
        .iter()
        .map(|(id, model, role)| {
            spawn_agent(
                id,
                model,
                role.clone(),
                chans.tx_command.subscribe(),
                chans.tx_report.clone(),
            )
        })
        .collect();

    // Đăng ký vào state từ cùng roster (single source of truth).
    {
        let mut s = st.write().await;
        for (id, model, role) in &roster {
            s.register(roles::AgentManifest::new(id, model, role.clone()));
        }
    }

    // Drop bản tx_report gốc để khi mọi agent xong, rx_report đóng đúng cách.
    drop(chans.tx_report);

    // 4. Spawn task gom báo cáo.
    let report_task = {
        let coord_ref = Coordinator {
            tx_command: coord.tx_command.clone(),
            state: st.clone(),
        };
        let rx = chans.rx_report;
        tokio::spawn(async move {
            coord_ref.collect_reports(rx).await;
        })
    };

    // 5. Orchestrator phát lệnh.
    tokio::time::sleep(Duration::from_millis(50)).await; // chờ agent subscribe
    println!("\n📡 Orchestrator phát lệnh...\n");

    coord.dispatch(Command::Ping)?;
    coord.dispatch(Command::Assign {
        task_id: "T1".into(),
        target_role: Some(AgentRole::Coder),
        prompt: "Viết hàm fibonacci bằng Rust".into(),
    })?;
    coord.dispatch(Command::Assign {
        task_id: "T2".into(),
        target_role: Some(AgentRole::Tester),
        prompt: "Viết unit test cho fibonacci".into(),
    })?;
    coord.dispatch(Command::Assign {
        task_id: "T3".into(),
        target_role: None, // broadcast — mọi agent
        prompt: "Báo cáo trạng thái".into(),
    })?;

    tokio::time::sleep(Duration::from_millis(200)).await;
    coord.dispatch(Command::Shutdown)?;

    // 6. Đợi agent dừng + report task kết thúc.
    for h in handles {
        let _ = h.await;
    }
    let _ = report_task.await;

    // 7. Tổng kết từ state.
    let s = st.read().await;
    println!(
        "\n📊 Tổng kết: {} agent | {} task xong | {} task lỗi",
        s.agent_count(),
        s.tasks_completed,
        s.tasks_failed
    );
    println!("🏁 Orchestrator kết thúc.");
    Ok(())
}

/// Quét toàn hệ thống tìm AI agent (như UI bố gửi) + nối cứng Connected vào state.
async fn run_scan() -> anyhow::Result<()> {
    println!("🔎 SynapzCore — Quét AI Agents trên hệ thống\n");
    let agents = scanner::scan_ai_agents().await;

    let (mut n_conn, mut n_cfg, mut n_none, mut n_unk) = (0, 0, 0, 0);
    for a in &agents {
        use roles::DetectStatus::*;
        let icon = match a.status {
            Connected => {
                n_conn += 1;
                "🟢"
            }
            NotConfigured => {
                n_cfg += 1;
                "🟡"
            }
            NotInstalled => {
                n_none += 1;
                "⚫"
            }
            Unknown => {
                n_unk += 1;
                "⚪"
            }
        };
        let ver = a.version.as_deref().unwrap_or("");
        println!(
            "{} {:<20} {:<16} {}",
            icon,
            a.name,
            a.status.to_string(),
            ver
        );
    }

    println!(
        "\n📊 {} agent | 🟢 {} Connected | 🟡 {} chưa cấu hình | ⚫ {} chưa cài | ⚪ {} unknown",
        agents.len(),
        n_conn,
        n_cfg,
        n_none,
        n_unk
    );

    // Nối cứng các Connected agent vào SystemState.
    let connected: Vec<_> = agents
        .iter()
        .filter(|a| a.status == roles::DetectStatus::Connected)
        .collect();
    if !connected.is_empty() {
        let st = state::new_shared_state();
        {
            let mut s = st.write().await;
            for a in &connected {
                s.register(roles::AgentManifest::new(
                    &a.name,
                    a.version.clone().unwrap_or_default(),
                    AgentRole::Unassigned,
                ));
            }
        }
        println!(
            "\n🔗 Đã nối cứng {} agent Connected vào SystemState:",
            connected.len()
        );
        for a in &connected {
            println!(
                "   • {} → {}",
                a.name,
                a.binary_path.as_deref().unwrap_or("?")
            );
        }
    }

    // Xuất JSON cho UI/dashboard dùng.
    let json = serde_json::to_string_pretty(&agents)?;
    std::fs::write("data/detected_agents.json", &json)?;
    println!("\n💾 Lưu data/detected_agents.json ({} bytes)", json.len());
    Ok(())
}

/// Live mode: quét → spawn agent CLI thật cho mỗi Connected có invocation → giao task.
async fn run_live(prompt: String) -> anyhow::Result<()> {
    let json_mode = std::env::args().any(|a| a == "--json");
    use roles::DetectStatus;
    if !json_mode {
        println!("🚀 SynapzCore LIVE — quét + nối agent CLI thật\n");
    }

    let agents = scanner::scan_ai_agents().await;
    let connected: Vec<_> = agents
        .into_iter()
        .filter(|a| a.status == DetectStatus::Connected)
        .collect();

    // Lọc agent có cách gọi CLI headless.
    let runnable: Vec<_> = connected
        .iter()
        .filter_map(|a| runner::invocation_for(&a.name).map(|inv| (a.name.clone(), inv)))
        .collect();

    if runnable.is_empty() {
        if json_mode {
            println!(
                "{}",
                serde_json::json!({"ok": false, "reason": "no runnable agent", "results": []})
            );
        } else {
            println!("⚠️  Không agent Connected nào có cách gọi CLI headless.");
        }
        return Ok(());
    }

    if !json_mode {
        println!("🔗 {} agent sẵn sàng nhận task:", runnable.len());
        for (name, inv) in &runnable {
            println!("   • {} → `{} {}`", name, inv.program, inv.args.join(" "));
        }
    }

    let st = state::new_shared_state();
    let (coord, chans) = Coordinator::new(st.clone(), 64, 256);

    // Spawn live agent cho mỗi runnable.
    let mut handles = Vec::new();
    for (i, (name, inv)) in runnable.iter().enumerate() {
        let id = format!("{}-{}", name.replace(' ', "_").to_lowercase(), i);
        handles.push(agent::spawn_live_agent(
            &id,
            name,
            AgentRole::Coder,
            inv.clone(),
            chans.tx_command.subscribe(),
            chans.tx_report.clone(),
        ));
    }
    let expected = runnable.len();
    drop(chans.tx_report);

    // Gom report — JSON mode thu vào Vec, in cuối; text mode in live.
    let collected = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::<serde_json::Value>::new()));
    let report_task = {
        let coord_ref = Coordinator {
            tx_command: coord.tx_command.clone(),
            state: st.clone(),
        };
        let rx = chans.rx_report;
        let col = collected.clone();
        tokio::spawn(async move {
            coord_ref.collect_reports_json(rx, col, json_mode).await;
        })
    };

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    if !json_mode {
        println!("\n📡 Giao task cho tất cả: \"{}\"\n", prompt);
    }
    coord.dispatch(Command::Assign {
        task_id: "LIVE-1".into(),
        target_role: None,
        prompt: prompt.clone(),
    })?;

    // Đợi đủ report (mỗi agent 1 TaskDone/TaskError) hoặc timeout cứng.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(180);
    loop {
        if collected.lock().await.len() >= expected {
            break;
        }
        if std::time::Instant::now() > deadline {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    }
    coord.dispatch(Command::Shutdown)?;
    for h in handles {
        let _ = h.await;
    }
    let _ = report_task.await;

    let s = st.read().await;
    if json_mode {
        let results = collected.lock().await.clone();
        let out = serde_json::json!({
            "ok": true,
            "prompt": prompt,
            "agents": expected,
            "completed": s.tasks_completed,
            "failed": s.tasks_failed,
            "results": results,
        });
        println!("{}", serde_json::to_string(&out)?);
    } else {
        println!(
            "\n📊 {} task xong | {} task lỗi",
            s.tasks_completed, s.tasks_failed
        );
    }
    Ok(())
}

/// GRAPH mode: nạp TaskGraph từ JSON, phân tích tầng, chạy song song theo tầng.
/// Đây là hiện thực Giai đoạn 1 (Fan-Out) + barrier Giai đoạn 3 ở mức điều phối.
async fn run_graph(path: &str, echo: bool) -> anyhow::Result<()> {
    use parallel_executor::{ParallelExecutor, cli_executor, echo_executor};
    use task_graph::TaskGraph;

    let raw = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("không đọc được {}: {}", path, e))?;
    let graph: TaskGraph = serde_json::from_str(&raw)
        .map_err(|e| anyhow::anyhow!("JSON đồ thị sai định dạng: {}", e))?;

    graph.validate().map_err(|e| anyhow::anyhow!("{}", e))?;
    let layers = graph.layers().map_err(|e| anyhow::anyhow!("{}", e))?;

    println!("🧠 SynapzCore — Parallel Graph Executor");
    println!(
        "📂 Đồ thị: {} ({} task, {} tầng)",
        path,
        graph.len(),
        layers.len()
    );
    println!("⚡ Độ song song tối đa: {}\n", graph.max_parallelism());
    for (i, layer) in layers.iter().enumerate() {
        let ids: Vec<&str> = layer.iter().map(|n| n.id.as_str()).collect();
        println!(
            "   Tầng {}: [{}] — {} task song song",
            i,
            ids.join(", "),
            layer.len()
        );
    }
    println!();

    // Chọn executor: echo (demo) hoặc CLI thật (Claude Code).
    let executor = if echo {
        println!("🔧 Executor: ECHO (giả lập, không gọi CLI thật)\n");
        echo_executor()
    } else {
        // Mặc định dùng Claude Code qua 9router (đã verify trong runner).
        let inv = runner::invocation_for("Claude Code")
            .ok_or_else(|| anyhow::anyhow!("không có invocation cho Claude Code"))?;
        println!("🔧 Executor: Claude Code (CLI thật, --model opus)\n");
        cli_executor(inv)
    };

    let pe = ParallelExecutor::new(executor, 600);
    let start = std::time::Instant::now();
    let report = pe.run(&graph, false).await?;
    let elapsed = start.elapsed();

    println!("\n📊 KẾT QUẢ ({:?}):", elapsed);
    for r in &report.results {
        let icon = if r.outcome.is_success() { "✅" } else { "❌" };
        let detail = match &r.outcome {
            parallel_executor::TaskOutcome::Success(o) => {
                let s = o.replace('\n', " ");
                truncate_chars(&s, 80)
            }
            parallel_executor::TaskOutcome::Failed(e) => format!("LỖI: {}", e),
            parallel_executor::TaskOutcome::Timeout => "TIMEOUT".to_string(),
        };
        println!(
            "   {} [tầng {}] {} ({}ms): {}",
            icon, r.layer, r.task_id, r.elapsed_ms, detail
        );
    }
    println!(
        "\n🏁 {} task xong | {} hỏng | {} tầng",
        report.succeeded(),
        report.failed(),
        report.total_layers
    );

    // Lưu report JSON cho UI/dashboard.
    let json = serde_json::json!({
        "graph": path,
        "total_layers": report.total_layers,
        "succeeded": report.succeeded(),
        "failed": report.failed(),
        "elapsed_ms": elapsed.as_millis(),
        "results": report.results.iter().map(|r| serde_json::json!({
            "task_id": r.task_id,
            "layer": r.layer,
            "elapsed_ms": r.elapsed_ms,
            "ok": r.outcome.is_success(),
        })).collect::<Vec<_>>(),
    });
    let _ = std::fs::create_dir_all("data");
    std::fs::write(
        "data/last_graph_run.json",
        serde_json::to_string_pretty(&json)?,
    )?;
    println!("💾 Lưu data/last_graph_run.json");
    Ok(())
}

/// Bọc một executor để mỗi task chạy trong branch git riêng (sandbox isolation).
/// Phần GỌI AI (đắt, ~40s) chạy SONG SONG bình thường; thao tác git (rẻ, <1s:
/// tạo branch, add, commit, về base) được SERIALIZE qua Mutex vì 1 working tree
/// không thể ở nhiều branch cùng lúc. Mỗi task: lock → checkout base → tạo branch
/// agent/<task> → unlock → chạy AI → lock → ghi output ra file → commit → về base.
fn git_wrap_executor(
    inner: parallel_executor::Executor,
    repo: std::path::PathBuf,
) -> parallel_executor::Executor {
    use git_isolation::GitBranchManager;
    let gitlock = std::sync::Arc::new(tokio::sync::Mutex::new(()));
    let mgr = std::sync::Arc::new(GitBranchManager::new(repo));
    std::sync::Arc::new(move |task_id: String, prompt: String| {
        let inner = inner.clone();
        let gitlock = gitlock.clone();
        let mgr = mgr.clone();
        Box::pin(async move {
            // 1. Tạo branch cô lập (serialize).
            {
                let _g = gitlock.lock().await;
                if let Err(e) = mgr.create_isolated(&task_id, "HEAD").await {
                    return Err(format!("git tạo branch lỗi: {}", e));
                }
            }
            // 2. Chạy AI (SONG SONG — không giữ lock).
            let result = inner(task_id.clone(), prompt).await;
            // 3. Ghi output + commit vào branch của task (serialize).
            {
                let _g = gitlock.lock().await;
                let branch = mgr.branch_name(&task_id);
                // checkout lại đúng branch task (vì task khác có thể đã đổi branch).
                let _ = mgr.checkout(&branch).await;
                if let Ok(out) = &result {
                    let fname = format!("{}.out", task_id.replace('/', "_"));
                    let _ = std::fs::write(mgr.repo.join(&fname), out);
                    let _ = mgr
                        .commit_all(&format!("feat({}): output từ agent", task_id))
                        .await;
                }
                // về base để task kế / merge sau làm việc.
                let _ = mgr.checkout("main").await;
            }
            result
        })
    })
}

/// Sau khi mọi task xong: merge các branch agent/<task> về base (main).
/// Báo branch nào sạch, branch nào conflict (cần SmartMerge / người xử lý).
async fn merge_agent_branches(graph: &task_graph::TaskGraph, repo: std::path::PathBuf) {
    use git_isolation::GitBranchManager;
    let mgr = GitBranchManager::new(repo);
    if !mgr.is_repo().await {
        println!("⚠️  Không phải git repo — bỏ qua merge branch.");
        return;
    }
    let _ = mgr.checkout("main").await;
    println!("\n━━━ GIT MERGE: hội tụ branch agent về main ━━━");
    let mut clean = 0;
    let mut conflict = 0;
    for node in &graph.nodes {
        let branch = mgr.branch_name(&node.id);
        match mgr.merge_into_current(&branch).await {
            Ok(true) => {
                clean += 1;
                println!("   ✅ merge {} sạch", branch);
            }
            Ok(false) => {
                conflict += 1;
                let files = mgr.conflicted_files().await;
                println!(
                    "   ⚠️  {} CONFLICT: {:?} → abort (cần xử lý tay)",
                    branch, files
                );
                let _ = mgr.abort_merge().await;
            }
            Err(e) => println!("   ⏭️  {} bỏ qua ({})", branch, e),
        }
    }
    println!("   📊 {} branch merge sạch | {} conflict", clean, conflict);
}

/// Tự tính số task đồng thời an toàn theo RAM trống của máy.
/// Mỗi agent AI (Claude/Codex qua CLI) tốn ~1.5GB. Trả max(1, free_GB/1.5), trần 4.
/// Windows: đọc qua `wmic OS get FreePhysicalMemory` (KB). Lỗi đọc → mặc định 2 (an toàn).
fn auto_concurrency() -> usize {
    let free_kb = read_free_mem_kb().unwrap_or(0);
    if free_kb == 0 {
        return 2; // không đọc được → an toàn cho máy yếu.
    }
    let free_gb = free_kb as f64 / 1024.0 / 1024.0;
    let n = (free_gb / 1.5).floor() as usize;
    n.clamp(1, 4)
}

/// Đọc RAM trống (KB). Windows dùng wmic; fallback None.
fn read_free_mem_kb() -> Option<u64> {
    if cfg!(windows) {
        let out = std::process::Command::new("wmic")
            .args(["OS", "get", "FreePhysicalMemory"])
            .output()
            .ok()?;
        let s = String::from_utf8_lossy(&out.stdout);
        s.lines()
            .filter_map(|l| l.trim().parse::<u64>().ok())
            .next()
    } else {
        // Linux: /proc/meminfo MemAvailable.
        let s = std::fs::read_to_string("/proc/meminfo").ok()?;
        s.lines()
            .find(|l| l.starts_with("MemAvailable:"))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|v| v.parse::<u64>().ok())
    }
}

/// PIPELINE mode: chạy trọn 4 giai đoạn parallel orchestration từ file JSON.
async fn run_pipeline(
    path: &str,
    echo: bool,
    roles: bool,
    git: bool,
    jobs: usize,
) -> anyhow::Result<()> {
    use pipeline::{Pipeline, PipelineConfig, always_ok_executor};
    use task_graph::TaskGraph;
    use work_buffer::WorkBuffer;

    let raw = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("không đọc được {}: {}", path, e))?;
    let graph: TaskGraph = serde_json::from_str(&raw)
        .map_err(|e| anyhow::anyhow!("JSON đồ thị sai định dạng: {}", e))?;
    graph.validate().map_err(|e| anyhow::anyhow!("{}", e))?;
    let layers = graph.layers().map_err(|e| anyhow::anyhow!("{}", e))?;

    // Quyết định giới hạn concurrency: --jobs N rõ ràng, hoặc auto theo RAM khi chạy
    // executor thật (echo nhẹ → không giới hạn). Auto: ~mỗi agent AI tốn ~1.5GB →
    // số job = max(1, free_GB / 1.5), trần 4. Bảo vệ máy yếu (i3-8GB) khỏi cạn RAM.
    let max_concurrent = if jobs > 0 {
        jobs
    } else if echo {
        0 // echo nhẹ, chạy hết song song
    } else {
        auto_concurrency()
    };

    println!("🧠 SynapzCore — PIPELINE (4 giai đoạn parallel orchestration)");
    println!(
        "📂 Đồ thị: {} ({} task, {} tầng)",
        path,
        graph.len(),
        layers.len()
    );
    println!("⚡ Độ song song tối đa: {}\n", graph.max_parallelism());
    println!("━━━ GIAI ĐOẠN 1: FAN-OUT (phân tầng phụ thuộc) ━━━");
    for (i, layer) in layers.iter().enumerate() {
        let ids: Vec<&str> = layer.iter().map(|n| n.id.as_str()).collect();
        let roleinfo: Vec<String> = layer
            .iter()
            .map(|n| {
                format!(
                    "{}{}",
                    n.id,
                    n.role
                        .as_ref()
                        .map(|r| format!("[{}]", r))
                        .unwrap_or_default()
                )
            })
            .collect();
        let _ = ids;
        println!(
            "   Tầng {}: [{}] — {} task song song",
            i,
            roleinfo.join(", "),
            layer.len()
        );
    }
    println!();

    // Chọn executor: echo / role-routed / single CLI.
    let executor = if echo {
        println!("🔧 Executor: ECHO/always-ok (giả lập)\n");
        always_ok_executor()
    } else if roles {
        let routing = parallel_executor::default_role_routing();
        let role_of = parallel_executor::build_role_of(&graph);
        let fallback = runner::invocation_for("Claude Code")
            .ok_or_else(|| anyhow::anyhow!("không có invocation fallback Claude Code"))?;
        println!("🔧 Executor: ROLE-ROUTED");
        for (role, inv) in &routing {
            println!("   • {} → {}", role, inv.program);
        }
        println!("   • (khác) → {} (fallback)\n", fallback.program);
        parallel_executor::role_routed_executor(routing, fallback, role_of)
    } else {
        let inv = runner::invocation_for("Claude Code")
            .ok_or_else(|| anyhow::anyhow!("không có invocation cho Claude Code"))?;
        println!("🔧 Executor: Claude Code (CLI thật, mọi task)\n");
        parallel_executor::cli_executor(inv)
    };

    // Bọc git isolation nếu --git: mỗi task tạo branch riêng → code → commit.
    let executor = if git {
        println!("🌿 Git isolation: BẬT — mỗi task 1 branch agent/<task>, merge sau\n");
        git_wrap_executor(executor, std::env::current_dir()?)
    } else {
        executor
    };

    if max_concurrent > 0 {
        println!(
            "🚦 Concurrency: tối đa {} task đồng thời{}\n",
            max_concurrent,
            if jobs == 0 {
                " (auto theo RAM)"
            } else {
                " (--jobs)"
            }
        );
    } else {
        println!("🚦 Concurrency: không giới hạn\n");
    }
    let cfg = PipelineConfig {
        max_concurrent,
        ..PipelineConfig::default()
    };
    let pipe = Pipeline::new(executor, cfg);
    let mut buffer = WorkBuffer::new();
    let start = std::time::Instant::now();
    let report = pipe.run(&graph, &mut buffer, None).await?;
    let elapsed = start.elapsed();

    // Sau khi tất cả xong, nếu --git thì merge các branch agent về base.
    if git {
        merge_agent_branches(&graph, std::env::current_dir()?).await;
    }

    println!("━━━ GIAI ĐOẠN 2: STATE ISOLATION (staging buffer) ━━━");
    println!(
        "   📦 {} artifact staged | {} task có output\n",
        buffer.total(),
        buffer.task_count()
    );

    println!("━━━ GIAI ĐOẠN 3: FAN-IN / SMART MERGE ━━━");
    println!(
        "   📄 Code: {} file sạch | {} xung đột",
        report.code_clean, report.code_conflicts
    );
    println!(
        "   🗂️  Data: {} record duy nhất | {} trùng đã loại\n",
        report.data_unique, report.data_dupes_removed
    );

    println!("━━━ GIAI ĐOẠN 4: SELF-CORRECT (cô lập + retry) ━━━");
    println!(
        "   🔧 {} task sửa được | 🛑 {} bỏ cuộc",
        report.fixed.len(),
        report.gave_up.len()
    );
    if !report.gave_up.is_empty() {
        println!("   ⚠️  Cần người can thiệp: {}", report.gave_up.join(", "));
    }
    println!();

    println!("📊 KẾT QUẢ ({:?}):", elapsed);
    for r in &report.run.results {
        let icon = if r.outcome.is_success() { "✅" } else { "❌" };
        println!(
            "   {} [tầng {}] {} ({}ms)",
            icon, r.layer, r.task_id, r.elapsed_ms
        );
    }
    println!(
        "\n🏁 {} task xong | {} hỏng | {} tầng | {}",
        report.run.succeeded(),
        report.run.failed(),
        report.run.total_layers,
        if report.gave_up.is_empty() {
            "✅ TẤT CẢ XANH"
        } else {
            "⚠️ CÓ TASK BỎ CUỘC"
        }
    );

    let _ = std::fs::create_dir_all("data");
    std::fs::write(
        "data/last_pipeline_run.json",
        serde_json::to_string_pretty(&report.to_json())?,
    )?;
    std::fs::write(
        "data/last_buffer_snapshot.json",
        serde_json::to_string_pretty(&buffer.snapshot_json())?,
    )?;
    println!("💾 Lưu data/last_pipeline_run.json + data/last_buffer_snapshot.json");
    Ok(())
}

/// Nạp .env (đơn giản) vào env vars nếu chưa set — để đọc NINEROUTER_* khi chạy CLI.
fn load_dotenv() {
    for candidate in [".env", "../.env", "../../.env"] {
        if let Ok(content) = std::fs::read_to_string(candidate) {
            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if let Some((k, v)) = line.split_once('=') {
                    let k = k.trim();
                    if std::env::var(k).is_err() {
                        unsafe {
                            std::env::set_var(k, v.trim());
                        }
                    }
                }
            }
            return; // nạp file .env đầu tiên tìm thấy
        }
    }
}

/// PLAN mode: orchestrator gọi LLM phân rã mục tiêu → TaskGraph → (tùy chọn) chạy.
async fn run_plan(
    goal: &str,
    run: bool,
    echo: bool,
    roles: bool,
    git: bool,
    jobs: usize,
) -> anyhow::Result<()> {
    println!("🧠 SynapzCore — PLANNER (#3): tự phân rã mục tiêu → TaskGraph");
    println!("🎯 Mục tiêu: {goal}\n");

    // LLM lập kế hoạch: ưu tiên 9router HTTP trực tiếp (đáng tin hơn), fallback CLI chain.
    let mut graph_opt: Option<task_graph::TaskGraph> = None;
    let mut last_err = String::new();

    // 1) 9router trực tiếp nếu có NINEROUTER_KEY.
    if let (Ok(key), Ok(url)) = (
        std::env::var("NINEROUTER_KEY"),
        std::env::var("NINEROUTER_URL"),
    ) && !key.is_empty()
    {
        let model =
            std::env::var("NINEROUTER_MODEL").unwrap_or_else(|_| "claude-opus-4.8".to_string());
        println!("🤖 Lập kế hoạch qua 9router trực tiếp (model {model})...");
        match planner::plan_with_9router(goal, &url, &key, &model, 120).await {
            Ok(g) => {
                println!("✅ 9router trả kế hoạch hợp lệ.");
                graph_opt = Some(g);
            }
            Err(e) => {
                eprintln!("⚠️  9router thất bại: {e}");
                last_err = e;
            }
        }
    }

    // 2) Fallback: chuỗi CLI agent (Claude → Hermes → Codex).
    if graph_opt.is_none() {
        let planner_agents = ["Claude Code", "Hermes Agent", "OpenAI Codex CLI"];
        let per_attempt_timeout = 120u64;
        for name in planner_agents {
            let inv = match runner::invocation_for(name) {
                Some(i) => i,
                None => continue,
            };
            println!(
                "🤖 Thử lập kế hoạch qua {} ({} {})...",
                name,
                inv.program,
                inv.args.join(" ")
            );
            match planner::plan_with_llm(goal, &inv, per_attempt_timeout).await {
                Ok(g) => {
                    println!("✅ {} trả kế hoạch hợp lệ.", name);
                    graph_opt = Some(g);
                    break;
                }
                Err(e) => {
                    eprintln!("⚠️  {} thất bại: {}", name, e);
                    last_err = e;
                }
            }
        }
    }
    let graph = match graph_opt {
        Some(g) => g,
        None => {
            eprintln!("❌ Mọi planner (9router + CLI) đều thất bại. Lỗi cuối: {last_err}");
            std::process::exit(1);
        }
    };

    let layers = graph.layers().map_err(|e| anyhow::anyhow!("{}", e))?;
    println!(
        "\n✅ Kế hoạch sinh ra: {} task, {} tầng (song song tối đa {})",
        graph.len(),
        layers.len(),
        graph.max_parallelism()
    );
    for (i, layer) in layers.iter().enumerate() {
        let info: Vec<String> = layer
            .iter()
            .map(|n| {
                format!(
                    "{}{}",
                    n.id,
                    n.role
                        .as_ref()
                        .map(|r| format!("[{}]", r))
                        .unwrap_or_default()
                )
            })
            .collect();
        println!("   Tầng {}: [{}]", i, info.join(", "));
    }

    // Lưu graph để xem trước / chạy lại.
    let _ = std::fs::create_dir_all("data");
    let graph_json = serde_json::to_string_pretty(&graph)?;
    std::fs::write("data/last_plan.json", &graph_json)?;
    std::fs::write("data/planned_graph.json", &graph_json)?;
    println!("\n💾 Lưu data/last_plan.json + data/planned_graph.json");

    if run {
        println!("\n▶️  --run: chạy pipeline trên kế hoạch vừa sinh...\n");
        return run_pipeline("data/planned_graph.json", echo, roles, git, jobs).await;
    } else {
        println!(
            "\nℹ️  Xem trước kế hoạch. Chạy thật: --pipeline data/planned_graph.json [--roles]"
        );
    }
    Ok(())
}

/// Truncate to at most max chars (not bytes) — UTF-8 safe; appends … if cut.
fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let head: String = s.chars().take(max).collect();
        format!("{head}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use roles::{AgentManifest, Report};

    #[test]
    fn parse_role_is_case_insensitive() {
        assert_eq!(parse_role("coder"), Some(AgentRole::Coder));
        assert_eq!(parse_role("TESTER"), Some(AgentRole::Tester));
        assert_eq!(parse_role("Researcher"), Some(AgentRole::Researcher));
        assert_eq!(parse_role("nope"), None);
    }

    #[test]
    fn default_roster_has_three_distinct_roles_without_env() {
        // Engine ships a usable roster with generic model names (no personal config).
        unsafe { std::env::remove_var("SYNAPZ_AGENTS") };
        let r = default_roster();
        assert_eq!(r.len(), 3);
        assert!(r.iter().any(|(_, _, role)| *role == AgentRole::Coder));
        assert!(r.iter().any(|(_, m, _)| m.starts_with("model-")));
    }

    #[test]
    fn truncate_chars_is_utf8_safe() {
        // Regression: byte-slicing &s[..n] panicked mid-multibyte char.
        let s = "Kết quả từ các bước phụ thuộc";
        // Cutting at a boundary that lands inside a Vietnamese char must not panic.
        let out = truncate_chars(s, 5);
        assert_eq!(out.chars().count(), 6); // 5 chars + ellipsis
        assert!(out.ends_with("…"));
        // Short input is returned unchanged, no ellipsis.
        assert_eq!(truncate_chars("abc", 10), "abc");
        // ASCII boundary still works.
        assert_eq!(truncate_chars("abcdef", 3), "abc…");
    }
    use tokio::sync::{broadcast, mpsc};

    #[tokio::test]
    async fn test_agent_handles_assign_for_its_role() {
        let (tx_cmd, rx_cmd) = broadcast::channel::<Command>(16);
        let (tx_rep, mut rx_rep) = mpsc::channel::<Report>(16);
        let h = spawn_agent("coder-x", "m", AgentRole::Coder, rx_cmd, tx_rep);

        // bỏ qua Report::Registered
        let _ = rx_rep.recv().await;

        tx_cmd
            .send(Command::Assign {
                task_id: "T1".into(),
                target_role: Some(AgentRole::Coder),
                prompt: "hello".into(),
            })
            .unwrap();

        let rep = rx_rep.recv().await.unwrap();
        match rep {
            Report::TaskDone { task_id, .. } => assert_eq!(task_id, "T1"),
            other => panic!("kỳ vọng TaskDone, nhận {:?}", other),
        }
        tx_cmd.send(Command::Shutdown).unwrap();
        let _ = h.await;
    }

    #[tokio::test]
    async fn test_agent_ignores_other_role() {
        let (tx_cmd, rx_cmd) = broadcast::channel::<Command>(16);
        let (tx_rep, mut rx_rep) = mpsc::channel::<Report>(16);
        let h = spawn_agent("tester-x", "m", AgentRole::Tester, rx_cmd, tx_rep);
        let _ = rx_rep.recv().await; // Registered

        // lệnh cho Coder — Tester phải bỏ qua
        tx_cmd
            .send(Command::Assign {
                task_id: "T9".into(),
                target_role: Some(AgentRole::Coder),
                prompt: "x".into(),
            })
            .unwrap();
        // Ping để chắc chắn agent vẫn sống và phản hồi
        tx_cmd.send(Command::Ping).unwrap();

        let rep = rx_rep.recv().await.unwrap();
        match rep {
            Report::Pong { agent_id } => assert_eq!(agent_id, "tester-x"),
            other => panic!("kỳ vọng Pong (bỏ qua Assign), nhận {:?}", other),
        }
        tx_cmd.send(Command::Shutdown).unwrap();
        let _ = h.await;
    }

    #[tokio::test]
    async fn test_state_register() {
        let st = state::new_shared_state();
        {
            let mut s = st.write().await;
            s.register(AgentManifest::new("a1", "m", AgentRole::Coder));
        }
        assert_eq!(st.read().await.agent_count(), 1);
    }
}
