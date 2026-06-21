//! brain-cron — Autonomous scheduler for Antigravity Brain.
//! Runs daily reflection, health checks, and memory maintenance on a timer.
//!
//! Usage:
//!   brain-cron                     # Run once (immediate reflection)
//!   brain-cron --daemon            # Background daemon, runs every N hours
//!   brain-cron --interval 6        # Custom interval in hours (default: 12)
//!   brain-cron --health-only       # Only run health checks

use clap::Parser;
use chrono::Local;

#[derive(Parser, Debug)]
#[command(name = "brain-cron", about = "🧠 Antigravity Brain — Autonomous Scheduler")]
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
    let base = std::env::var("AGT_BRAIN_ROOT").unwrap_or_else(|_| "E:\\AGT_Brain".to_string());
    format!("{base}\\data\\supabase_config.json")
}

async fn check_health() -> (bool, bool) {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .unwrap_or_default();

    // Supabase
    let supabase_ok = {
        let config = get_config_path();
        match agt_memory::SupabaseMemory::from_config(&config) {
            Ok(mem) => mem.recall("health check", 1).await.is_ok(),
            Err(_) => false,
        }
    };

    let grok_base = std::env::var("GROK_API_BASE")
        .unwrap_or_else(|_| "http://127.0.0.1:8000".to_string());
    let grok_ok = client.get(format!("{grok_base}/v1/models"))
        .send().await
        .map(|r| r.status().is_success())
        .unwrap_or(false);

    (supabase_ok, grok_ok)
}

async fn run_daily_reflection() -> String {
    let config = get_config_path();
    let today = Local::now().format("%Y-%m-%d").to_string();

    let mem = match agt_memory::SupabaseMemory::from_config(&config) {
        Ok(m) => m,
        Err(e) => return format!("❌ Config error: {e}"),
    };

    // Gather data
    let recent = mem.recall("", 20).await.unwrap_or_default();
    let (supabase_ok, grok_ok) = check_health().await;

    // Count by agent
    let (mut ag, mut gr) = (0usize, 0usize);
    for m in &recent {
        match m.agent.as_str() {
            "antigravity" => ag += 1,
            "grok" => gr += 1,
            _ => {}
        }
    }

    // Top importance memories
    let mut top: Vec<_> = recent.iter()
        .filter(|m| m.importance >= 4)
        .collect();
    top.sort_by(|a, b| b.importance.cmp(&a.importance));
    let top_items: Vec<String> = top.iter().take(5)
        .map(|m| format!("  [imp:{}] {}: {}", m.importance, m.agent, 
            if m.content.len() > 80 { &m.content[..80] } else { &m.content }))
        .collect();

    // Decisions today
    let decisions_dir = std::env::var("AGT_BRAIN_ROOT")
        .unwrap_or_else(|_| "E:\\AGT_Brain".to_string());
    let decisions_path = format!("{decisions_dir}/memory/decisions");
    let today_decisions: Vec<String> = if let Ok(entries) = std::fs::read_dir(&decisions_path) {
        entries.filter_map(|e| e.ok())
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
         - Recent memories: {} (AG:{ag} GR:{gr})\n\
         - High-importance: {}\n\
         - Decisions today: {}\n\
         \n\
         🏥 Health:\n\
         - Supabase: {}\n\
         - Grok API: {}\n\
         \n\
         ⭐ Top Memories:\n{}\n\
         \n\
         📋 Decisions:\n{}\n\
         \n\
         💡 Auto-insight: System is {} with {} memories and {} decisions today.",
        recent.len(), top.len(), today_decisions.len(),
        if supabase_ok { "✅" } else { "❌" },
        if grok_ok { "✅" } else { "❌" },
        if top_items.is_empty() { "  (none)".to_string() } else { top_items.join("\n") },
        if today_decisions.is_empty() { "  (none today)".to_string() } else { today_decisions.join("\n") },
        if supabase_ok { "healthy" } else { "degraded" },
        recent.len(),
        today_decisions.len(),
    );

    // Save reflection to memory
    let metadata = serde_json::json!({
        "type": "daily_reflection",
        "date": today,
        "health": { "supabase": supabase_ok, "grok": grok_ok },
        "stats": { "total": recent.len(), "antigravity": ag, "grok": gr }
    });

    if let Err(e) = mem.remember_as(
        &reflection, "Antigravity", "antigravity", "reflection", 4, 5, &metadata
    ).await {
        eprintln!("⚠️ Failed to save reflection: {e}");
    }

    reflection
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    if args.health_only {
        println!("🏥 Running health check...");
        let (s, g) = check_health().await;
        println!("  Supabase: {}", if s { "✅" } else { "❌" });
        println!("  Grok:     {}", if g { "✅" } else { "❌" });
        return;
    }

    if args.daemon {
        let interval = std::time::Duration::from_secs(args.interval * 3600);
        println!("🧠 brain-cron daemon started — interval: {}h", args.interval);
        println!("   Press Ctrl+C to stop.\n");

        loop {
            let now = Local::now().format("%Y-%m-%d %H:%M:%S");
            println!("⏰ [{now}] Running scheduled reflection...");
            let result = run_daily_reflection().await;
            println!("{result}\n");
            println!("💤 Sleeping {}h until next run...\n", args.interval);
            tokio::time::sleep(interval).await;
        }
    } else {
        println!("🧠 Running one-shot reflection...\n");
        let result = run_daily_reflection().await;
        println!("{result}");
    }
}
