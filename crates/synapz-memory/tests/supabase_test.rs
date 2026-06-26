//! Integration test - Supabase write/read/search roundtrip.
//!
//! Requires live credentials. Skips automatically (passes as no-op) when neither
//! SUPABASE_URL/SUPABASE_KEY env vars nor a config file are available - so a
//! stranger`s clone and CI stay green without secrets.

use synapz_memory::SupabaseMemory;

fn creds_available(config: &str) -> bool {
    let env_ok = std::env::var("SUPABASE_URL")
        .map(|s| !s.is_empty())
        .unwrap_or(false)
        && std::env::var("SUPABASE_KEY")
            .map(|s| !s.is_empty())
            .unwrap_or(false);
    env_ok || std::path::Path::new(config).exists()
}

#[tokio::test]
async fn supabase_roundtrip() {
    let config =
        std::env::var("AGT_CONFIG").unwrap_or_else(|_| "data/supabase_config.json".to_string());

    if !creds_available(&config) {
        eprintln!("skip: no SUPABASE_URL/SUPABASE_KEY env and no config at {config}");
        return;
    }

    let mem = SupabaseMemory::from_config(&config).expect("Failed to load Supabase config");

    // 1. Write
    let metadata = serde_json::json!({"context": "integration_test"});
    mem.remember(
        "Rust integration test - roundtrip verified",
        "antigravity",
        &metadata,
    )
    .await
    .expect("Failed to write memory");

    // 2. Fetch recent
    let recent = mem.fetch_recent(5).await.expect("Failed to fetch");
    assert!(!recent.is_empty(), "Should have at least 1 memory");
    println!("OK Fetched {} recent memories", recent.len());

    // 3. Search
    let results = mem
        .recall("Rust integration", 5)
        .await
        .expect("Failed to recall");
    assert!(!results.is_empty(), "Should find the test memory");
    println!("OK Found {} search results", results.len());

    println!("OK Supabase roundtrip test PASSED");
}
