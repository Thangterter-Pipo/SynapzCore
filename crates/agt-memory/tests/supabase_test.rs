//! Integration test — Supabase write/read/search roundtrip.

use agt_memory::SupabaseMemory;

#[tokio::test]
async fn supabase_roundtrip() {
    let config = std::env::var("AGT_CONFIG")
        .unwrap_or_else(|_| "E:\\AGT_Brain\\data\\supabase_config.json".to_string());

    let mem = SupabaseMemory::from_config(&config)
        .expect("Failed to load Supabase config");

    // 1. Write
    let metadata = serde_json::json!({"context": "integration_test"});
    mem.remember("Rust integration test — roundtrip verified", "antigravity", &metadata)
        .await
        .expect("Failed to write memory");

    // 2. Fetch recent
    let recent = mem.fetch_recent(5).await.expect("Failed to fetch");
    assert!(!recent.is_empty(), "Should have at least 1 memory");
    println!("✅ Fetched {} recent memories", recent.len());

    // 3. Search
    let results = mem.recall("Rust integration", 5).await.expect("Failed to recall");
    assert!(!results.is_empty(), "Should find the test memory");
    println!("✅ Found {} search results", results.len());

    println!("✅ Supabase roundtrip test PASSED");
}
