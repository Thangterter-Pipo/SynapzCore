//! Memory tools — wrappers around agt-memory for tool registry.
//! Supports shared memory across all 3 agents.

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

// These tools call Supabase directly via HTTP (stateless, no shared state needed).

pub async fn remember(params: Value) -> Result<Value> {
    let query = params.get("query").and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'query'"))?;
    let limit = params.get("n_results").and_then(|v| v.as_u64()).unwrap_or(5) as usize;
    let agent = params.get("agent").and_then(|v| v.as_str());

    let config = get_config_path();
    let mem = agt_memory::SupabaseMemory::from_config(&config)?;

    let results = if let Some(agent) = agent {
        mem.recall_by_agent(agent, limit).await?
    } else {
        mem.recall(query, limit).await?
    };

    let formatted: Vec<Value> = results.iter().map(|m| {
        json!({
            "agent": m.agent,
            "role": m.role,
            "category": m.category,
            "importance": m.importance,
            "content": m.content,
            "created_at": m.created_at,
        })
    }).collect();

    Ok(json!(formatted))
}

pub async fn save_memory(params: Value) -> Result<Value> {
    let message = params.get("message").and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'message'"))?;
    let speaker = params.get("speaker").and_then(|v| v.as_str()).unwrap_or("Antigravity");
    let context = params.get("context").and_then(|v| v.as_str()).unwrap_or("general");
    let agent = params.get("agent").and_then(|v| v.as_str()).unwrap_or("antigravity");
    let category = params.get("category").and_then(|v| v.as_str()).unwrap_or("general");
    let importance = params.get("importance").and_then(|v| v.as_i64()).unwrap_or(3) as i16;

    let config = get_config_path();
    let mem = agt_memory::SupabaseMemory::from_config(&config)?;
    let metadata = json!({ "context": context });
    mem.remember_as(message, speaker, agent, category, importance, 3, &metadata).await?;

    Ok(json!(format!("✅ [{agent}/{category}/imp:{importance}] Đã ghi nhớ: {}", &message[..message.len().min(80)])))
}

pub async fn recall_boss(_params: Value) -> Result<Value> {
    let config = get_config_path();
    let mem = agt_memory::SupabaseMemory::from_config(&config)?;
    let results = mem.recall("Bố sở thích yêu cầu", 10).await?;

    let formatted: Vec<String> = results.iter()
        .map(|m| format!("- {}", m.content))
        .collect();

    if formatted.is_empty() {
        Ok(json!("Chưa có thông tin về Bố."))
    } else {
        Ok(json!(format!("Hồ sơ Bố:\n{}", formatted.join("\n"))))
    }
}

/// Fetch high-importance team memories across all agents.
pub async fn recall_team(params: Value) -> Result<Value> {
    let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(5) as usize;
    let config = get_config_path();
    let mem = agt_memory::SupabaseMemory::from_config(&config)?;
    let results = mem.recall_team(limit).await?;

    let formatted: Vec<Value> = results.iter().map(|m| {
        json!({
            "agent": m.agent,
            "category": m.category,
            "importance": m.importance,
            "confidence": m.confidence,
            "content": m.content,
            "created_at": m.created_at,
        })
    }).collect();

    Ok(json!(formatted))
}

pub async fn search_code(params: Value) -> Result<Value> {
    let query = params.get("query").and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'query'"))?;
    // For now, simple file search as fallback (vector search requires embedding model)
    let pattern = format!("E:\\AGT_Brain\\**\\*.rs");
    let matches: Vec<String> = glob::glob(&pattern)?
        .filter_map(|p| p.ok())
        .filter(|p| {
            std::fs::read_to_string(p)
                .map(|content| content.to_lowercase().contains(&query.to_lowercase()))
                .unwrap_or(false)
        })
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    Ok(json!(matches))
}

fn get_config_path() -> String {
    let base = std::env::var("AGT_BRAIN_ROOT").unwrap_or_else(|_| "E:\\AGT_Brain".to_string());
    format!("{base}\\data\\supabase_config.json")
}
