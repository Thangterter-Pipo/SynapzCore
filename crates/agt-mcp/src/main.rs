//! agt-mcp — MCP Server exposing Antigravity memory tools to IDE.
//! 10 tools for 2-AI team (Antigravity + Grok).

use rmcp::ServerHandler;
use rmcp::ServiceExt;
use rmcp::model::*;
use rmcp::tool;

#[derive(Debug, Clone)]
struct AgentMcp;

#[tool(tool_box)]
impl AgentMcp {
    /// Tìm kiếm thông tin từ bộ nhớ dài hạn dựa trên ngữ nghĩa.
    #[tool(description = "Search memories by keyword. Optionally filter by agent (antigravity/grok).")]
    async fn search_memory(&self, #[tool(param)] query: String, #[tool(param)] n_results: Option<u64>, #[tool(param)] agent: Option<String>) -> String {
        let limit = n_results.unwrap_or(5) as usize;
        let config = get_config_path();
        match agt_memory::SupabaseMemory::from_config(&config) {
            Ok(mem) => {
                let results = if let Some(ref agent) = agent {
                    mem.recall_by_agent(agent, limit).await
                } else {
                    mem.recall(&query, limit).await
                };
                match results {
                    Ok(results) if results.is_empty() => "Không tìm thấy thông tin liên quan.".to_string(),
                    Ok(results) => results.iter()
                        .map(|m| format!("[{}] ({}/{}) [imp:{}]: {}",
                            m.created_at.as_deref().unwrap_or("?"),
                            m.agent, m.category, m.importance, m.content))
                        .collect::<Vec<_>>()
                        .join("\n---\n"),
                    Err(e) => format!("❌ Search error: {e}"),
                }
            }
            Err(e) => format!("❌ Config error: {e}"),
        }
    }

    /// Ghi lại một thông tin quan trọng vào bộ nhớ dài hạn.
    #[tool(description = "Save important information to long-term shared memory. Optionally specify agent and category.")]
    async fn add_memory(
        &self,
        #[tool(param)] message: String,
        #[tool(param)] speaker: Option<String>,
        #[tool(param)] context: Option<String>,
        #[tool(param)] agent: Option<String>,
        #[tool(param)] category: Option<String>,
        #[tool(param)] importance: Option<i16>,
    ) -> String {
        let speaker = speaker.unwrap_or_else(|| "User".to_string());
        let agent = agent.unwrap_or_else(|| "antigravity".to_string());
        let category = category.unwrap_or_else(|| "general".to_string());
        let importance = importance.unwrap_or(3);
        let config = get_config_path();
        match agt_memory::SupabaseMemory::from_config(&config) {
            Ok(mem) => {
                let metadata = serde_json::json!({ "context": context.unwrap_or_else(|| "general".to_string()) });
                match mem.remember_as(&message, &speaker, &agent, &category, importance, 3, &metadata).await {
                    Ok(()) => format!("✅ Đã ghi nhớ [{agent}/{category}/imp:{importance}]: '{message}'"),
                    Err(e) => format!("❌ Save error: {e}"),
                }
            }
            Err(e) => format!("❌ Config error: {e}"),
        }
    }

    /// Lấy ký ức gần đây từ toàn bộ team (2 AI: Antigravity, Grok).
    /// Chỉ lấy memories có importance >= 3.
    #[tool(description = "Get recent high-importance team memories from all agents (Antigravity, Grok). Used for shared context.")]
    async fn team_memory(&self, #[tool(param)] limit: Option<u64>) -> String {
        let limit = limit.unwrap_or(5) as usize;
        let config = get_config_path();
        match agt_memory::SupabaseMemory::from_config(&config) {
            Ok(mem) => {
                match mem.recall_team(limit).await {
                    Ok(results) if results.is_empty() => "Chưa có team memory.".to_string(),
                    Ok(results) => {
                        let lines: Vec<String> = results.iter().map(|m| {
                            format!("[{}] 🤖{} ({}/imp:{}/conf:{}): {}",
                                m.created_at.as_deref().unwrap_or("?"),
                                m.agent, m.category, m.importance, m.confidence,
                                m.content)
                        }).collect();
                        format!("🧠 Team Memory ({} entries):\n{}", lines.len(), lines.join("\n---\n"))
                    }
                    Err(e) => format!("❌ Team memory error: {e}"),
                }
            }
            Err(e) => format!("❌ Config error: {e}"),
        }
    }

    /// Truy xuất hồ sơ và yêu cầu của Bố.
    #[tool(description = "Get boss profile and preferences from memory")]
    async fn get_boss_profile(&self) -> String {
        let config = get_config_path();
        match agt_memory::SupabaseMemory::from_config(&config) {
            Ok(mem) => {
                match mem.recall("Bố sở thích yêu cầu", 10).await {
                    Ok(results) if results.is_empty() => "Chưa có thông tin chi tiết về Bố.".to_string(),
                    Ok(results) => {
                        let lines: Vec<String> = results.iter().map(|m| format!("- {}", m.content)).collect();
                        format!("Hồ sơ Bố:\n{}", lines.join("\n"))
                    }
                    Err(e) => format!("❌ Recall error: {e}"),
                }
            }
            Err(e) => format!("❌ Config error: {e}"),
        }
    }

    /// 🧠 Gọi Grok Subagent — research, suy nghĩ, review code, brainstorm.
    /// Auto-saves prompt + response to shared team memory.
    #[tool(description = "Call Grok AI subagent for research, thinking, code review, or brainstorming. Auto-saves to shared memory.")]
    async fn ask_grok(
        &self,
        #[tool(param)] prompt: String,
        #[tool(param)] mode: Option<String>,
        #[tool(param)] model: Option<String>,
    ) -> String {
        let mode = mode.unwrap_or_else(|| "chat".to_string());
        let model_name = model.unwrap_or_else(|| "grok-4-heavy".to_string());

        let params = serde_json::json!({
            "prompt": prompt,
            "mode": mode,
            "model": model_name,
        });

        match agt_tools::grok::ask_grok(params).await {
            Ok(result) => {
                if let Some(response) = result.get("response").and_then(|v| v.as_str()) {
                    format!("🧠 [Grok/{mode}] {response}")
                } else {
                    format!("🧠 Grok response: {result}")
                }
            }
            Err(e) => format!("❌ Grok Subagent error: {e}"),
        }
    }

    /// Kiểm tra Grok API health.
    #[tool(description = "Check if Grok API is healthy and responsive")]
    async fn grok_health(&self) -> String {
        match agt_tools::grok::grok_health(serde_json::json!({})).await {
            Ok(result) => {
                let healthy = result.get("healthy").and_then(|v| v.as_bool()).unwrap_or(false);
                let endpoint = result.get("endpoint").and_then(|v| v.as_str()).unwrap_or("unknown");
                if healthy {
                    format!("✅ Grok API healthy at {endpoint}")
                } else {
                    format!("❌ Grok API unreachable at {endpoint}")
                }
            }
            Err(e) => format!("❌ Health check error: {e}"),
        }
    }

    /// 🧠 Auto-Context Loader — CALL THIS AT SESSION START.
    /// Loads critical context so the AI "remembers" immediately:
    /// - Recent architectural decisions
    /// - High-importance team memories
    /// - Current goals
    /// - Last incident (to avoid repeating mistakes)
    /// This eliminates the "zero-state" problem in new conversations.
    #[tool(description = "Auto-load critical context at session start. Returns: recent decisions, high-importance team memories, current goals, and last incident. Call this FIRST in every new conversation to instantly recover context.")]
    async fn auto_context(&self) -> String {
        let config = get_config_path();
        let base_dir = std::env::var("AGT_BRAIN_ROOT").unwrap_or_else(|_| "E:\\AGT_Brain".to_string());
        let mut sections: Vec<String> = Vec::new();

        // Section 1: High-importance team memories (importance >= 4)
        if let Ok(mem) = agt_memory::SupabaseMemory::from_config(&config) {
            match mem.recall_team(8).await {
                Ok(results) => {
                    if !results.is_empty() {
                        let lines: Vec<String> = results.iter()
                            .filter(|m| m.importance >= 4)
                            .take(5)
                            .map(|m| format!("  [{}/{}] imp:{} — {}",
                                m.agent, m.category, m.importance,
                                if m.content.len() > 200 { format!("{}...", &m.content[..200]) } else { m.content.clone() }
                            ))
                            .collect();
                        if !lines.is_empty() {
                            sections.push(format!("📌 CRITICAL MEMORIES ({}):\n{}", lines.len(), lines.join("\n")));
                        }
                    }
                }
                Err(e) => sections.push(format!("⚠️ Memory recall error: {e}")),
            }

            // Section 2: Recent decisions (category=decision, last 5)
            match mem.recall("decision", 5).await {
                Ok(decisions) => {
                    if !decisions.is_empty() {
                        let lines: Vec<String> = decisions.iter()
                            .filter(|m| m.category == "decision")
                            .take(3)
                            .map(|m| format!("  [{}] {}",
                                m.created_at.as_deref().unwrap_or("?"),
                                if m.content.len() > 150 { format!("{}...", &m.content[..150]) } else { m.content.clone() }
                            ))
                            .collect();
                        if !lines.is_empty() {
                            sections.push(format!("📋 RECENT DECISIONS ({}):\n{}", lines.len(), lines.join("\n")));
                        }
                    }
                }
                Err(_) => {}
            }
        }

        // Section 3: Local decisions files (most recent)
        let decisions_dir = format!("{base_dir}\\memory\\decisions");
        if let Ok(entries) = std::fs::read_dir(&decisions_dir) {
            let mut files: Vec<_> = entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().map(|ext| ext == "json").unwrap_or(false))
                .collect();
            files.sort_by(|a, b| b.file_name().cmp(&a.file_name())); // newest first
            let recent: Vec<String> = files.iter().take(3).filter_map(|f| {
                let content = std::fs::read_to_string(f.path()).ok()?;
                let json: serde_json::Value = serde_json::from_str(&content).ok()?;
                let title = json.get("decision").and_then(|v| v.as_str()).unwrap_or("?");
                let date = json.get("timestamp").and_then(|v| v.as_str()).unwrap_or("?");
                Some(format!("  [{}] {}", date, title))
            }).collect();
            if !recent.is_empty() {
                sections.push(format!("🏛️ LOCAL DECISIONS ({}):\n{}", recent.len(), recent.join("\n")));
            }
        }

        // Section 4: Last incident (avoid repeating mistakes)
        let incidents_dir = format!("{base_dir}\\memory\\incidents");
        if let Ok(entries) = std::fs::read_dir(&incidents_dir) {
            let mut files: Vec<_> = entries.filter_map(|e| e.ok()).collect();
            files.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
            if let Some(latest) = files.first() {
                if let Ok(content) = std::fs::read_to_string(latest.path()) {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                        let title = json.get("title").and_then(|v| v.as_str()).unwrap_or("?");
                        let lesson = json.get("lesson").and_then(|v| v.as_str()).unwrap_or("?");
                        sections.push(format!("⚠️ LAST INCIDENT: {title}\n  Lesson: {lesson}"));
                    }
                }
            }
        }

        // Section 5: Current goals
        let goals_path = format!("{base_dir}\\data\\goals.json");
        if let Ok(content) = std::fs::read_to_string(&goals_path) {
            if let Ok(goals) = serde_json::from_str::<Vec<serde_json::Value>>(&content) {
                let active: Vec<String> = goals.iter()
                    .filter(|g| {
                        g.get("status").and_then(|v| v.as_str()).unwrap_or("") != "completed"
                    })
                    .take(3)
                    .map(|g| {
                        let title = g.get("title").and_then(|v| v.as_str()).unwrap_or("?");
                        let priority = g.get("priority").and_then(|v| v.as_u64()).unwrap_or(0);
                        format!("  [P{}] {}", priority, title)
                    })
                    .collect();
                if !active.is_empty() {
                    sections.push(format!("🎯 ACTIVE GOALS ({}):\n{}", active.len(), active.join("\n")));
                }
            }
        }

        if sections.is_empty() {
            "🧠 Auto-context loaded, but no critical data found. Fresh start!".to_string()
        } else {
            format!("🧠 AUTO-CONTEXT LOADED — {} sections:\n\n{}", sections.len(), sections.join("\n\n"))
        }
    }

    /// 📝 Daily Self-Reflection — AI reflects on today's work.
    /// Reviews recent decisions, incidents, and memories to generate insights.
    /// Saves reflection as a new memory with importance=5 for future recall.
    #[tool(description = "Daily self-reflection: review today's decisions, incidents, and team memories. Generates insights and improvement suggestions. Auto-saves reflection to memory.")]
    async fn daily_reflection(&self) -> String {
        let config = get_config_path();
        let base_dir = std::env::var("AGT_BRAIN_ROOT").unwrap_or_else(|_| "E:\\AGT_Brain".to_string());
        let mut digest_parts: Vec<String> = Vec::new();

        // Count today's memories
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();

        if let Ok(mem) = agt_memory::SupabaseMemory::from_config(&config) {
            // Get recent memories
            match mem.recall_team(20).await {
                Ok(results) => {
                    let today_count = results.iter()
                        .filter(|m| m.created_at.as_deref().unwrap_or("").starts_with(&today))
                        .count();
                    let total_importance: i16 = results.iter().map(|m| m.importance).sum();
                    let avg_importance = if results.is_empty() { 0.0 } else { total_importance as f64 / results.len() as f64 };

                    digest_parts.push(format!("📊 STATS: {} memories today, avg importance: {:.1}", today_count, avg_importance));

                    // Most important memory
                    if let Some(top) = results.iter().max_by_key(|m| m.importance) {
                        digest_parts.push(format!("⭐ TOP MEMORY: [imp:{}] {}", top.importance,
                            if top.content.len() > 120 { format!("{}...", &top.content[..120]) } else { top.content.clone() }
                        ));
                    }

                    // Agent distribution
                    let mut agent_counts = std::collections::HashMap::new();
                    for m in &results {
                        *agent_counts.entry(m.agent.clone()).or_insert(0usize) += 1;
                    }
                    let dist: Vec<String> = agent_counts.iter()
                        .map(|(a, c)| format!("{a}:{c}"))
                        .collect();
                    digest_parts.push(format!("🤖 AGENTS: {}", dist.join(", ")));
                }
                Err(e) => digest_parts.push(format!("⚠️ Memory error: {e}")),
            }
        }

        // Check decisions made today
        let decisions_dir = format!("{base_dir}\\memory\\decisions");
        if let Ok(entries) = std::fs::read_dir(&decisions_dir) {
            let today_decisions: Vec<String> = entries
                .filter_map(|e| e.ok())
                .filter(|e| e.file_name().to_string_lossy().starts_with(&today))
                .filter_map(|f| {
                    let content = std::fs::read_to_string(f.path()).ok()?;
                    let json: serde_json::Value = serde_json::from_str(&content).ok()?;
                    json.get("decision").and_then(|v| v.as_str()).map(|s| s.to_string())
                })
                .collect();
            if !today_decisions.is_empty() {
                digest_parts.push(format!("🏛️ DECISIONS TODAY ({}):\n{}", today_decisions.len(),
                    today_decisions.iter().map(|d| format!("  - {d}")).collect::<Vec<_>>().join("\n")));
            }
        }

        // Generate reflection summary
        let reflection = if digest_parts.is_empty() {
            "🪞 Hôm nay yên tĩnh — không có hoạt động đáng kể. Tốt để lên kế hoạch cho ngày mai.".to_string()
        } else {
            format!("🪞 DAILY REFLECTION — {today}\n\n{}\n\n💡 INSIGHT: Continue building momentum. Every memory saved is compound knowledge.", digest_parts.join("\n"))
        };

        // Auto-save reflection to memory
        if let Ok(mem) = agt_memory::SupabaseMemory::from_config(&config) {
            let metadata = serde_json::json!({ "type": "daily_reflection", "date": today });
            let _ = mem.remember_as(&reflection, "Antigravity", "antigravity", "reflection", 5, 5, &metadata).await;
        }

        reflection
    }

    /// 📚 Save a Skill — Capture a reusable pattern, solution, or technique.
    /// Skills are high-importance memories tagged for easy retrieval.
    /// Use when: solving a complex problem, discovering a pattern, or finding a workaround.
    #[tool(description = "Save a reusable skill/pattern/technique to the knowledge base. Skills are high-importance memories tagged for instant recall. Include: what problem it solves, the solution, and when to use it.")]
    async fn save_skill(
        &self,
        #[tool(param)] name: String,
        #[tool(param)] problem: String,
        #[tool(param)] solution: String,
        #[tool(param)] tags: Option<String>,
    ) -> String {
        let config = get_config_path();
        let content = format!("SKILL: {name}\nPROBLEM: {problem}\nSOLUTION: {solution}");
        let tags_str = tags.unwrap_or_else(|| "general".to_string());
        let metadata = serde_json::json!({
            "type": "skill",
            "name": name,
            "tags": tags_str,
        });

        match agt_memory::SupabaseMemory::from_config(&config) {
            Ok(mem) => {
                match mem.remember_as(&content, "Antigravity", "antigravity", "skill", 5, 5, &metadata).await {
                    Ok(()) => format!("📚 Skill saved: '{name}' [tags: {tags_str}]\n  Problem: {problem}\n  Solution: {solution}"),
                    Err(e) => format!("❌ Save skill error: {e}"),
                }
            }
            Err(e) => format!("❌ Config error: {e}"),
        }
    }

    /// 🔍 Recall Skills — Search saved skills by keyword or tags.
    /// Returns matching skills with their problem/solution pairs.
    #[tool(description = "Search saved skills/patterns by keyword. Returns matching skills with problem/solution pairs.")]
    async fn recall_skills(&self, #[tool(param)] query: String) -> String {
        let config = get_config_path();
        match agt_memory::SupabaseMemory::from_config(&config) {
            Ok(mem) => {
                // Search for skills specifically
                let search_query = format!("SKILL {query}");
                match mem.recall(&search_query, 10).await {
                    Ok(results) => {
                        let skills: Vec<String> = results.iter()
                            .filter(|m| m.category == "skill" || m.content.starts_with("SKILL:"))
                            .map(|m| format!("📚 {}", m.content))
                            .collect();
                        if skills.is_empty() {
                            format!("🔍 No skills found for '{query}'. Save skills with save_skill tool.")
                        } else {
                            format!("📚 Found {} skills:\n\n{}", skills.len(), skills.join("\n---\n"))
                        }
                    }
                    Err(e) => format!("❌ Recall error: {e}"),
                }
            }
            Err(e) => format!("❌ Config error: {e}"),
        }
    }
}

impl ServerHandler for AgentMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some("Antigravity MCP Server — 10 tools: shared memory + Grok Subagent + Auto-Context + Self-Reflection + Skill Library. Call auto_context FIRST!".into()),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}

#[allow(dead_code)]
fn get_config_path() -> String {
    let base = std::env::var("AGT_BRAIN_ROOT").unwrap_or_else(|_| "E:\\AGT_Brain".to_string());
    format!("{base}\\data\\supabase_config.json")
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    eprintln!("🚀 Antigravity MCP Server starting... (10 tools, auto-context + skills enabled)");
    let service = AgentMcp.serve(rmcp::transport::io::stdio()).await
        .inspect_err(|e| eprintln!("❌ MCP Server error: {e}"))?;
    service.waiting().await?;
    Ok(())
}
