//! brain-cron — Autonomous scheduler for SynapzCore.
//! Runs daily reflection, health checks, and memory maintenance on a timer.
//!
//! Usage:
//!   brain-cron                     # Run once (immediate reflection)
//!   brain-cron --daemon            # Background daemon, runs every N hours
//!   brain-cron --interval 6        # Custom interval in hours (default: 12)
//!   brain-cron --health-only       # Only run health checks

use anyhow::Result;
use chrono::Local;
use clap::Parser;

/// Resolve the SynapzCore root portably: SYNAPZ_ROOT env -> infer from exe location
/// (target/<profile>/ -> repo root if it looks like the repo) -> current dir.
fn synapz_root() -> String {
    if let Ok(r) = std::env::var("SYNAPZ_ROOT")
        && !r.is_empty()
    {
        return r;
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(root) = exe
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.parent())
        && (root.join("Cargo.toml").exists() || root.join("crates").exists())
    {
        return root.to_string_lossy().into_owned();
    }
    std::env::current_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| ".".to_string())
}

#[derive(Parser, Debug)]
#[command(name = "brain-cron", about = "🧠 SynapzCore — Autonomous Scheduler")]
struct Args {
    /// Run as background daemon
    #[arg(long, default_value_t = false)]
    daemon: bool,

    /// Interval in hours between runs (daemon mode)
    #[arg(long, default_value_t = 12)]
    interval: u64,

    /// Only run health checks
    #[arg(long, default_value_t = false)]
    health_only: bool,
}

fn get_config_path() -> String {
    let base = synapz_root();
    format!("{base}\\data\\supabase_config.json")
}

async fn check_health() -> bool {
    let config = get_config_path();
    match synapz_memory::SupabaseMemory::from_config(&config) {
        Ok(mem) => mem.recall("health check", 1).await.is_ok(),
        Err(_) => false,
    }
}

async fn run_daily_reflection() -> String {
    let config = get_config_path();
    let today = Local::now().format("%Y-%m-%d").to_string();

    let mem = match synapz_memory::SupabaseMemory::from_config(&config) {
        Ok(m) => m,
        Err(e) => return format!("❌ Config error: {e}"),
    };

    // Gather data
    let recent = mem.recall("", 20).await.unwrap_or_default();
    let supabase_ok = check_health().await;

    // Count by agent
    let mut ag = 0usize;
    for m in &recent {
        if m.agent.as_str() == "antigravity" {
            ag += 1;
        }
    }

    // Top importance memories
    let mut top: Vec<_> = recent.iter().filter(|m| m.importance >= 4).collect();
    top.sort_by(|a, b| b.importance.cmp(&a.importance));
    let top_items: Vec<String> = top
        .iter()
        .take(5)
        .map(|m| {
            format!(
                "  [imp:{}] {}: {}",
                m.importance,
                m.agent,
                truncate_chars(&m.content, 80)
            )
        })
        .collect();

    // Decisions today
    let decisions_dir = synapz_root();
    let decisions_path = format!("{decisions_dir}/memory/decisions");
    let today_decisions: Vec<String> = if let Ok(entries) = std::fs::read_dir(&decisions_path) {
        entries
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with(&today))
            .map(|e| format!("  📋 {}", e.file_name().to_string_lossy()))
            .collect()
    } else {
        vec![]
    };

    // Build reflection
    let reflection = format!(
        "🪞 DAILY REFLECTION — {today}\n\
         \n\
         📊 Stats:\n\
         - Recent memories: {} (AG:{ag})\n\
         - High-importance: {}\n\
         - Decisions today: {}\n\
         \n\
         🏥 Health:\n\
         - Supabase: {}\n\
         \n\
         ⭐ Top Memories:\n{}\n\
         \n\
         📋 Decisions:\n{}\n\
         \n\
         💡 Auto-insight: System is {} with {} memories and {} decisions today.",
        recent.len(),
        top.len(),
        today_decisions.len(),
        if supabase_ok { "✅" } else { "❌" },
        if top_items.is_empty() {
            "  (none)".to_string()
        } else {
            top_items.join("\n")
        },
        if today_decisions.is_empty() {
            "  (none today)".to_string()
        } else {
            today_decisions.join("\n")
        },
        if supabase_ok { "healthy" } else { "degraded" },
        recent.len(),
        today_decisions.len(),
    );

    // Save reflection to memory
    let metadata = serde_json::json!({
        "type": "daily_reflection",
        "date": today,
        "health": { "supabase": supabase_ok },
        "stats": { "total": recent.len(), "antigravity": ag }
    });

    if let Err(e) = mem
        .remember_as(
            &reflection,
            "Antigravity",
            "antigravity",
            "reflection",
            4,
            5,
            &metadata,
        )
        .await
    {
        eprintln!("⚠️ Failed to save reflection: {e}");
    }

    reflection
}

async fn run_dream_compression() -> Result<()> {
    let base_dir = synapz_root();
    let script_path = format!("{base_dir}\\scripts\\synapz_memory.py");

    println!("💤 [Dreaming] Calling Python unified memory engine to run dreaming...");
    let output = std::process::Command::new("python")
        .arg(&script_path)
        .arg("--dream")
        .output()?;

    if output.status.success() {
        let stdout_str = String::from_utf8_lossy(&output.stdout);
        println!("{stdout_str}");
        println!("✅ [Dreaming] Memory compression cycle completed successfully!");
    } else {
        let stderr_str = String::from_utf8_lossy(&output.stderr);
        eprintln!("⚠️ Memory compression failed:\n{stderr_str}");
        return Err(anyhow::anyhow!("Python dreaming execution failed"));
    }
    Ok(())
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    if args.health_only {
        println!("🏥 Running health check...");
        let s = check_health().await;
        println!("  Supabase: {}", if s { "✅" } else { "❌" });
        return;
    }

    if args.daemon {
        let base_dir = synapz_root();
        let watcher_path = format!("{base_dir}\\scripts\\folder_watcher.py");

        println!("🚀 Starting folder watcher in background...");
        match std::process::Command::new("python")
            .arg(&watcher_path)
            .spawn()
        {
            Ok(_) => println!("✅ Folder watcher launched successfully."),
            Err(e) => eprintln!("⚠️ Failed to launch folder watcher: {e}"),
        }

        let interval = std::time::Duration::from_secs(args.interval * 3600);
        println!(
            "🧠 brain-cron daemon started — interval: {}h",
            args.interval
        );
        println!("   Press Ctrl+C to stop.\n");

        loop {
            let now = Local::now().format("%Y-%m-%d %H:%M:%S");
            println!("⏰ [{now}] Running scheduled reflection...");
            let result = run_daily_reflection().await;
            println!("{result}\n");

            println!("💤 [{now}] Running background memory compression (Dreaming)...");
            if let Err(e) = run_dream_compression().await {
                eprintln!("⚠️ Memory compression failed: {e}");
            }

            println!("💤 Sleeping {}h until next run...\n", args.interval);
            tokio::time::sleep(interval).await;
        }
    } else {
        println!("🧠 Running one-shot reflection...\n");
        let result = run_daily_reflection().await;
        println!("{result}\n");

        println!("💤 Running one-shot memory compression (Dreaming)...");
        if let Err(e) = run_dream_compression().await {
            eprintln!("⚠️ Memory compression failed: {e}");
        }

        println!("🔄 Running one-shot SQLite Graph Sync...");
        let base_dir = synapz_root();
        let script_path = format!("{base_dir}\\scripts\\synapz_memory.py");
        let _ = std::process::Command::new("python")
            .arg(&script_path)
            .arg("--sync-graph")
            .status();
    }
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
