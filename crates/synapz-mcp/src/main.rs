//! agt-mcp — MCP Server exposing Antigravity memory tools to IDE.
//! 12 tools for Antigravity AI brain.

use rmcp::ServerHandler;
use rmcp::ServiceExt;
use rmcp::model::*;
use rmcp::tool;
use std::process::Command;

#[derive(Debug, Clone)]
struct AgentMcp;

#[tool(tool_box)]
impl AgentMcp {
    /// Tìm kiếm thông tin từ bộ nhớ dài hạn dựa trên ngữ nghĩa và liên tưởng đồ thị SQLite.
    #[tool(
        description = "Search memories by keyword. Combines Spreading Activation (SQLite Graph) and Fallback Vector Search."
    )]
    async fn search_memory(
        &self,
        #[tool(param)] query: String,
        #[tool(param)] n_results: Option<u64>,
        #[tool(param)] agent: Option<String>,
    ) -> String {
        let base_dir = synapz_root();
        let script_path = format!("{base_dir}\\scripts\\synapz_memory.py");

        // Gọi Python script để thực hiện Spreading Activation query
        // Python lỗi/không chạy được → fallback xuống Supabase bên dưới.
        if let Ok(output) = Command::new("python")
            .arg(&script_path)
            .arg("--query")
            .arg(&query)
            .output()
            && output.status.success()
        {
            let stdout_str = String::from_utf8_lossy(&output.stdout).to_string();
            if stdout_str.trim().contains("Spreading Activation Results") {
                return stdout_str;
            }
        }

        // Fallback: Tìm kiếm trực tiếp bằng Supabase vector search như cũ
        let limit = n_results.unwrap_or(5) as usize;
        let config = get_config_path();
        match synapz_memory::SupabaseMemory::from_config(&config) {
            Ok(mem) => {
                let results = if let Some(ref agent) = agent {
                    mem.recall_by_agent(agent, limit).await
                } else {
                    mem.recall(&query, limit).await
                };
                match results {
                    Ok(results) if results.is_empty() => {
                        "Không tìm thấy thông tin liên quan.".to_string()
                    }
                    Ok(results) => results
                        .iter()
                        .map(|m| {
                            format!(
                                "[{}] ({}/{}) [imp:{}]: {}",
                                m.created_at.as_deref().unwrap_or("?"),
                                m.agent,
                                m.category,
                                m.importance,
                                m.content
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n---\n"),
                    Err(e) => format!("❌ Search error: {e}"),
                }
            }
            Err(e) => format!("❌ Config error: {e}"),
        }
    }

    /// Ghi lại một thông tin quan trọng vào bộ nhớ dài hạn có kiểm tra mâu thuẫn LLM-assisted.
    #[tool(
        description = "Save important information to long-term shared memory with conflict resolution. Optionally specify agent and category."
    )]
    async fn add_memory(
        &self,
        #[tool(param)] message: String,
        #[tool(param)] speaker: Option<String>,
        #[tool(param)] context: Option<String>,
        #[tool(param)] agent: Option<String>,
        #[tool(param)] category: Option<String>,
        #[tool(param)] importance: Option<i16>,
    ) -> String {
        let agent_str = agent.unwrap_or_else(|| "antigravity".to_string());
        let category_str = category.unwrap_or_else(|| "general".to_string());
        let importance_val = importance.unwrap_or(3);

        let base_dir = synapz_root();
        let script_path = format!("{base_dir}\\scripts\\synapz_memory.py");

        // Gọi Python script để xử lý Conflict Resolution (Mem0-style) và lưu vào Supabase
        if let Ok(output) = Command::new("python")
            .arg(&script_path)
            .arg("--save")
            .arg(&message)
            .arg("--agent")
            .arg(&agent_str)
            .arg("--category")
            .arg(&category_str)
            .arg("--importance")
            .arg(importance_val.to_string())
            .output()
        {
            // Fallback to direct supabase save on failure.
            if output.status.success() {
                let stdout_str = String::from_utf8_lossy(&output.stdout).to_string();
                if stdout_str.trim().contains("Memory saved")
                    || stdout_str.trim().contains("updated successfully")
                {
                    return stdout_str;
                }
            }
        }

        // Fallback: Lưu trực tiếp lên Supabase
        let speaker = speaker.unwrap_or_else(|| "User".to_string());
        let config = get_config_path();
        match synapz_memory::SupabaseMemory::from_config(&config) {
            Ok(mem) => {
                let metadata = serde_json::json!({ "context": context.unwrap_or_else(|| "general".to_string()) });
                match mem
                    .remember_as(
                        &message,
                        &speaker,
                        &agent_str,
                        &category_str,
                        importance_val,
                        3,
                        &metadata,
                    )
                    .await
                {
                    Ok(()) => format!(
                        "✅ Đã ghi nhớ (Direct Fallback) [{agent_str}/{category_str}/imp:{importance_val}]: '{message}'"
                    ),
                    Err(e) => format!("❌ Save error: {e}"),
                }
            }
            Err(e) => format!("❌ Config error: {e}"),
        }
    }

    /// Lấy ký ức gần đây có độ quan trọng cao.
    /// Chỉ lấy memories có importance >= 3.
    #[tool(description = "Get recent high-importance memories. Used for context.")]
    async fn team_memory(&self, #[tool(param)] limit: Option<u64>) -> String {
        let limit = limit.unwrap_or(5) as usize;
        let config = get_config_path();
        match synapz_memory::SupabaseMemory::from_config(&config) {
            Ok(mem) => match mem.recall_team(limit).await {
                Ok(results) if results.is_empty() => "Chưa có team memory.".to_string(),
                Ok(results) => {
                    let lines: Vec<String> = results
                        .iter()
                        .map(|m| {
                            format!(
                                "[{}] 🤖{} ({}/imp:{}/conf:{}): {}",
                                m.created_at.as_deref().unwrap_or("?"),
                                m.agent,
                                m.category,
                                m.importance,
                                m.confidence,
                                m.content
                            )
                        })
                        .collect();
                    format!(
                        "🧠 Team Memory ({} entries):\n{}",
                        lines.len(),
                        lines.join("\n---\n")
                    )
                }
                Err(e) => format!("❌ Team memory error: {e}"),
            },
            Err(e) => format!("❌ Config error: {e}"),
        }
    }

    /// Truy xuất hồ sơ và yêu cầu của Bố.
    #[tool(description = "Get boss profile and preferences from memory")]
    async fn get_boss_profile(&self) -> String {
        let config = get_config_path();
        match synapz_memory::SupabaseMemory::from_config(&config) {
            Ok(mem) => match mem.recall("Bố sở thích yêu cầu", 10).await {
                Ok(results) if results.is_empty() => {
                    "Chưa có thông tin chi tiết về Bố.".to_string()
                }
                Ok(results) => {
                    let lines: Vec<String> =
                        results.iter().map(|m| format!("- {}", m.content)).collect();
                    format!("Hồ sơ Bố:\n{}", lines.join("\n"))
                }
                Err(e) => format!("❌ Recall error: {e}"),
            },
            Err(e) => format!("❌ Config error: {e}"),
        }
    }

    /// 🧠 Auto-Context Loader — CALL THIS AT SESSION START.
    /// Loads critical context so the AI "remembers" immediately:
    /// - Recent architectural decisions
    /// - High-importance team memories
    /// - Current goals
    /// - Last incident (to avoid repeating mistakes)
    /// This eliminates the "zero-state" problem in new conversations.
    #[tool(
        description = "Auto-load critical context at session start. Returns: recent decisions, high-importance team memories, current goals, and last incident. Call this FIRST in every new conversation to instantly recover context."
    )]
    async fn auto_context(&self) -> String {
        let config = get_config_path();
        let base_dir = synapz_root();
        let mut sections: Vec<String> = Vec::new();

        // Section 1: High-importance team memories (importance >= 4)
        if let Ok(mem) = synapz_memory::SupabaseMemory::from_config(&config) {
            match mem.recall_team(8).await {
                Ok(results) => {
                    if !results.is_empty() {
                        let lines: Vec<String> = results
                            .iter()
                            .filter(|m| m.importance >= 4)
                            .take(5)
                            .map(|m| {
                                format!(
                                    "  [{}/{}] imp:{} — {}",
                                    m.agent,
                                    m.category,
                                    m.importance,
                                    truncate_chars(&m.content, 200)
                                )
                            })
                            .collect();
                        if !lines.is_empty() {
                            sections.push(format!(
                                "📌 CRITICAL MEMORIES ({}):\n{}",
                                lines.len(),
                                lines.join("\n")
                            ));
                        }
                    }
                }
                Err(e) => sections.push(format!("⚠️ Memory recall error: {e}")),
            }

            // Section 2: Recent decisions (category=decision, last 5)
            if let Ok(decisions) = mem.recall("decision", 5).await
                && !decisions.is_empty()
            {
                let lines: Vec<String> = decisions
                    .iter()
                    .filter(|m| m.category == "decision")
                    .take(3)
                    .map(|m| {
                        format!(
                            "  [{}] {}",
                            m.created_at.as_deref().unwrap_or("?"),
                            truncate_chars(&m.content, 150)
                        )
                    })
                    .collect();
                if !lines.is_empty() {
                    sections.push(format!(
                        "📋 RECENT DECISIONS ({}):\n{}",
                        lines.len(),
                        lines.join("\n")
                    ));
                }
            }
        }

        // Section 3: Local decisions files (most recent)
        let decisions_dir = format!("{base_dir}\\memory\\decisions");
        if let Ok(entries) = std::fs::read_dir(&decisions_dir) {
            let mut files: Vec<_> = entries
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.path()
                        .extension()
                        .map(|ext| ext == "json")
                        .unwrap_or(false)
                })
                .collect();
            files.sort_by_key(|b| std::cmp::Reverse(b.file_name())); // newest first
            let recent: Vec<String> = files
                .iter()
                .take(3)
                .filter_map(|f| {
                    let content = std::fs::read_to_string(f.path()).ok()?;
                    let json: serde_json::Value = serde_json::from_str(&content).ok()?;
                    let title = json.get("decision").and_then(|v| v.as_str()).unwrap_or("?");
                    let date = json
                        .get("timestamp")
                        .and_then(|v| v.as_str())
                        .unwrap_or("?");
                    Some(format!("  [{}] {}", date, title))
                })
                .collect();
            if !recent.is_empty() {
                sections.push(format!(
                    "🏛️ LOCAL DECISIONS ({}):\n{}",
                    recent.len(),
                    recent.join("\n")
                ));
            }
        }

        // Section 4: Last incident (avoid repeating mistakes)
        let incidents_dir = format!("{base_dir}\\memory\\incidents");
        if let Ok(entries) = std::fs::read_dir(&incidents_dir) {
            let mut files: Vec<_> = entries.filter_map(|e| e.ok()).collect();
            files.sort_by_key(|b| std::cmp::Reverse(b.file_name()));
            if let Some(latest) = files.first()
                && let Ok(content) = std::fs::read_to_string(latest.path())
                && let Ok(json) = serde_json::from_str::<serde_json::Value>(&content)
            {
                let title = json.get("title").and_then(|v| v.as_str()).unwrap_or("?");
                let lesson = json.get("lesson").and_then(|v| v.as_str()).unwrap_or("?");
                sections.push(format!("⚠️ LAST INCIDENT: {title}\n  Lesson: {lesson}"));
            }
        }

        // Section 5: Current goals (Primary: Supabase, Fallback: Local JSON)
        let mut goals_loaded = false;
        if let Ok(mem) = synapz_memory::SupabaseMemory::from_config(&config)
            && let Ok(goals) = mem.fetch_active_goals(10).await
        {
            let active: Vec<String> = goals
                .iter()
                .filter(|g| {
                    let status = g
                        .metadata
                        .get("status")
                        .and_then(|v| v.as_str())
                        .unwrap_or("active");
                    status == "active" || status == "pending"
                })
                .take(5)
                .map(|g| {
                    let title = g
                        .metadata
                        .get("title")
                        .and_then(|v| v.as_str())
                        .unwrap_or(g.content.as_str());
                    let priority = g.importance;
                    let steps = g.metadata.get("steps").and_then(|v| v.as_array());

                    let steps_str = if let Some(steps) = steps {
                        let steps_list: Vec<String> = steps
                            .iter()
                            .map(|s| {
                                let text = s.get("text").and_then(|v| v.as_str()).unwrap_or("");
                                let done = s.get("done").and_then(|v| v.as_bool()).unwrap_or(false);
                                format!("    - [{}] {}", if done { "x" } else { " " }, text)
                            })
                            .collect();
                        if steps_list.is_empty() {
                            "".to_string()
                        } else {
                            format!("\n{}", steps_list.join("\n"))
                        }
                    } else {
                        "".to_string()
                    };

                    format!("  [P{}] {}: {}{}", priority, title, g.content, steps_str)
                })
                .collect();
            if !active.is_empty() {
                sections.push(format!(
                    "🎯 ACTIVE GOALS ({}):\n{}",
                    active.len(),
                    active.join("\n\n")
                ));
                goals_loaded = true;
            }
        }

        if !goals_loaded {
            let goals_path = format!("{base_dir}\\data\\goals.json");
            if let Ok(content) = std::fs::read_to_string(&goals_path)
                && let Ok(goals) = serde_json::from_str::<Vec<serde_json::Value>>(&content)
            {
                let active: Vec<String> = goals
                    .iter()
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
                    sections.push(format!(
                        "🎯 ACTIVE GOALS ({}):\n{}",
                        active.len(),
                        active.join("\n")
                    ));
                }
            }
        }

        if sections.is_empty() {
            "🧠 Auto-context loaded, but no critical data found. Fresh start!".to_string()
        } else {
            format!(
                "🧠 AUTO-CONTEXT LOADED — {} sections:\n\n{}",
                sections.len(),
                sections.join("\n\n")
            )
        }
    }

    /// 📝 Daily Self-Reflection — AI reflects on today's work.
    /// Reviews recent decisions, incidents, and memories to generate insights.
    /// Saves reflection as a new memory with importance=5 for future recall.
    #[tool(
        description = "Daily self-reflection: review today's decisions, incidents, and team memories. Generates insights and improvement suggestions. Auto-saves reflection to memory."
    )]
    async fn daily_reflection(&self) -> String {
        let config = get_config_path();
        let base_dir = synapz_root();
        let mut digest_parts: Vec<String> = Vec::new();

        // Count today's memories
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();

        if let Ok(mem) = synapz_memory::SupabaseMemory::from_config(&config) {
            // Get recent memories
            match mem.recall_team(20).await {
                Ok(results) => {
                    let today_count = results
                        .iter()
                        .filter(|m| m.created_at.as_deref().unwrap_or("").starts_with(&today))
                        .count();
                    let total_importance: i16 = results.iter().map(|m| m.importance).sum();
                    let avg_importance = if results.is_empty() {
                        0.0
                    } else {
                        total_importance as f64 / results.len() as f64
                    };

                    digest_parts.push(format!(
                        "📊 STATS: {} memories today, avg importance: {:.1}",
                        today_count, avg_importance
                    ));

                    // Most important memory
                    if let Some(top) = results.iter().max_by_key(|m| m.importance) {
                        digest_parts.push(format!(
                            "⭐ TOP MEMORY: [imp:{}] {}",
                            top.importance,
                            truncate_chars(&top.content, 120)
                        ));
                    }

                    // Agent distribution
                    let mut agent_counts = std::collections::HashMap::new();
                    for m in &results {
                        *agent_counts.entry(m.agent.clone()).or_insert(0usize) += 1;
                    }
                    let dist: Vec<String> = agent_counts
                        .iter()
                        .map(|(a, c)| format!("{a}:{c}"))
                        .collect();
                    digest_parts.push(format!("🤖 AGENTS: {}", dist.join(", ")));
                }
                Err(e) => digest_parts.push(format!("⚠️ Memory error: {e}")),
            }
            // Count completed goals today
            if let Ok(goals) = mem.fetch_active_goals(30).await {
                let completed_today = goals
                    .iter()
                    .filter(|g| {
                        let status = g
                            .metadata
                            .get("status")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        status == "completed"
                    })
                    .filter(|g| {
                        let comp_at = g
                            .metadata
                            .get("completed_at")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        comp_at.starts_with(&today)
                    })
                    .count();
                if completed_today > 0 {
                    digest_parts.push(format!("🎯 MISSIONS: {} completed today", completed_today));
                }
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
                    json.get("decision")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                })
                .collect();
            if !today_decisions.is_empty() {
                digest_parts.push(format!(
                    "🏛️ DECISIONS TODAY ({}):\n{}",
                    today_decisions.len(),
                    today_decisions
                        .iter()
                        .map(|d| format!("  - {d}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                ));
            }
        }

        // Generate reflection summary
        let reflection = if digest_parts.is_empty() {
            "🪞 Hôm nay yên tĩnh — không có hoạt động đáng kể. Tốt để lên kế hoạch cho ngày mai."
                .to_string()
        } else {
            format!(
                "🪞 DAILY REFLECTION — {today}\n\n{}\n\n💡 INSIGHT: Continue building momentum. Every memory saved is compound knowledge.",
                digest_parts.join("\n")
            )
        };

        // Auto-save reflection to memory
        if let Ok(mem) = synapz_memory::SupabaseMemory::from_config(&config) {
            let metadata = serde_json::json!({ "type": "daily_reflection", "date": today });
            let _ = mem
                .remember_as(
                    &reflection,
                    "Antigravity",
                    "antigravity",
                    "reflection",
                    5,
                    5,
                    &metadata,
                )
                .await;
        }

        reflection
    }

    /// 📚 Save a Skill — Capture a reusable pattern, solution, or technique.
    /// Skills are high-importance memories tagged for easy retrieval.
    /// Use when: solving a complex problem, discovering a pattern, or finding a workaround.
    #[tool(
        description = "Save a reusable skill/pattern/technique to the knowledge base. Skills are high-importance memories tagged for instant recall. Include: what problem it solves, the solution, and when to use it."
    )]
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

        match synapz_memory::SupabaseMemory::from_config(&config) {
            Ok(mem) => {
                match mem
                    .remember_as(
                        &content,
                        "Antigravity",
                        "antigravity",
                        "skill",
                        5,
                        5,
                        &metadata,
                    )
                    .await
                {
                    Ok(()) => format!(
                        "📚 Skill saved: '{name}' [tags: {tags_str}]\n  Problem: {problem}\n  Solution: {solution}"
                    ),
                    Err(e) => format!("❌ Save skill error: {e}"),
                }
            }
            Err(e) => format!("❌ Config error: {e}"),
        }
    }

    /// 🤝 Coordination: Heartbeat — Register or refresh agent presence.
    /// Call this at session start and periodically to stay visible to other agents.
    #[tool(
        description = "Register or refresh this agent in the multi-agent coordination system. Call at session start and every ~60s. Params: agent_id (e.g. 'antigravity-ide'), role ('builder'/'researcher'), current_task (what you are working on now), status ('active'/'idle'/'busy')."
    )]
    async fn coord_heartbeat(
        &self,
        #[tool(param)] agent_id: String,
        #[tool(param)] role: Option<String>,
        #[tool(param)] current_task: Option<String>,
        #[tool(param)] status: Option<String>,
    ) -> String {
        let base_dir = synapz_root();
        let sdk = format!("{base_dir}\\scripts\\agent_sdk.py");
        let role_val = role.unwrap_or_else(|| "builder".to_string());
        let status_val = status.unwrap_or_else(|| "active".to_string());

        let mut cmd = Command::new("python");
        cmd.arg(&sdk)
            .arg("--action")
            .arg("heartbeat")
            .arg("--agent")
            .arg(&agent_id)
            .arg("--role")
            .arg(&role_val)
            .arg("--status-val")
            .arg(&status_val);
        if let Some(ref task) = current_task {
            cmd.arg("--task").arg(task);
        }
        match cmd.output() {
            Ok(out) if out.status.success() => {
                let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
                format!("🤝 Heartbeat sent [{agent_id}/{role_val}]: {s}")
            }
            Ok(out) => format!(
                "⚠️ Heartbeat error: {}",
                String::from_utf8_lossy(&out.stderr)
            ),
            Err(e) => format!("❌ SDK error: {e}"),
        }
    }

    /// 🔒 Coordination: Claim File — Reserve exclusive edit rights on a file.
    /// MUST call before editing any file. Prevents conflicts with other agents.
    /// Returns ok=true if claimed, ok=false with holder name if another agent holds it.
    #[tool(
        description = "Claim exclusive edit rights on a file before modifying it. Prevents conflicts when multiple agents work simultaneously. Call coord_release when done editing. Params: agent_id, file_path (relative to repo root, e.g. 'scripts/dashboard.html')."
    )]
    async fn coord_claim(
        &self,
        #[tool(param)] agent_id: String,
        #[tool(param)] file_path: String,
    ) -> String {
        let base_dir = synapz_root();
        let sdk = format!("{base_dir}\\scripts\\agent_sdk.py");
        match Command::new("python")
            .arg(&sdk)
            .arg("--action")
            .arg("claim")
            .arg("--agent")
            .arg(&agent_id)
            .arg("--file")
            .arg(&file_path)
            .output()
        {
            Ok(out) if out.status.success() => {
                let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
                // Parse JSON result for readable output
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&s) {
                    if val.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
                        format!("🔒 [{agent_id}] claimed `{file_path}` — safe to edit")
                    } else {
                        let holder = val
                            .get("holder")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown");
                        format!(
                            "⚠️ `{file_path}` is held by [{holder}] — wait or coordinate before editing"
                        )
                    }
                } else {
                    s
                }
            }
            Ok(out) => format!("⚠️ Claim error: {}", String::from_utf8_lossy(&out.stderr)),
            Err(e) => format!("❌ SDK error: {e}"),
        }
    }

    /// 🔓 Coordination: Release File — Release edit lock on a file when done.
    /// MUST call after finishing edits so other agents can proceed.
    #[tool(
        description = "Release the edit lock on a file after you finish modifying it. Always call this after coord_claim when edits are complete. Params: agent_id, file_path."
    )]
    async fn coord_release(
        &self,
        #[tool(param)] agent_id: String,
        #[tool(param)] file_path: String,
    ) -> String {
        let base_dir = synapz_root();
        let sdk = format!("{base_dir}\\scripts\\agent_sdk.py");
        match Command::new("python")
            .arg(&sdk)
            .arg("--action")
            .arg("release")
            .arg("--agent")
            .arg(&agent_id)
            .arg("--file")
            .arg(&file_path)
            .output()
        {
            Ok(out) if out.status.success() => {
                format!("🔓 [{agent_id}] released `{file_path}`")
            }
            Ok(out) => format!("⚠️ Release error: {}", String::from_utf8_lossy(&out.stderr)),
            Err(e) => format!("❌ SDK error: {e}"),
        }
    }

    /// 📊 Coordination: Status — See who is working on what right now.
    /// Use before editing to check if another agent holds the file.
    #[tool(
        description = "Get current multi-agent coordination state: which agents are active, what files are locked, open tasks, and pending messages. Use before editing files to avoid conflicts."
    )]
    async fn coord_status(&self) -> String {
        let base_dir = synapz_root();
        let sdk = format!("{base_dir}\\scripts\\agent_sdk.py");
        match Command::new("python")
            .arg(&sdk)
            .arg("--action")
            .arg("status")
            .output()
        {
            Ok(out) if out.status.success() => {
                let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&s) {
                    let agents = val.get("agents").and_then(|v| v.as_u64()).unwrap_or(0);
                    let active = val
                        .get("active")
                        .and_then(|v| v.as_array())
                        .map(|a| {
                            a.iter()
                                .filter_map(|v| v.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        })
                        .unwrap_or_default();
                    let stale = val
                        .get("stale")
                        .and_then(|v| v.as_array())
                        .map(|a| {
                            a.iter()
                                .filter_map(|v| v.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        })
                        .unwrap_or_default();
                    let locks = val
                        .get("locks")
                        .and_then(|v| v.as_object())
                        .map(|m| {
                            m.iter()
                                .map(|(f, l)| {
                                    format!(
                                        "  🔒 `{f}` → {}",
                                        l.get("holder").and_then(|h| h.as_str()).unwrap_or("?")
                                    )
                                })
                                .collect::<Vec<_>>()
                                .join("\n")
                        })
                        .unwrap_or_else(|| "  (none)".to_string());
                    let tasks = val.get("open_tasks").and_then(|v| v.as_u64()).unwrap_or(0);
                    let msgs = val.get("messages").and_then(|v| v.as_u64()).unwrap_or(0);
                    format!(
                        "📊 Coordination Status:\n🤖 Agents: {agents} total | Active: [{active}] | Stale: [{stale}]\n🔒 File Locks:\n{locks}\n📋 Open tasks: {tasks} | 💬 Messages: {msgs}"
                    )
                } else {
                    s
                }
            }
            Ok(out) => format!("⚠️ Status error: {}", String::from_utf8_lossy(&out.stderr)),
            Err(e) => format!("❌ SDK error: {e}"),
        }
    }

    /// 🔍 Recall Skills — Search saved skills by keyword or tags.
    /// Returns matching skills with their problem/solution pairs.
    #[tool(
        description = "Search saved skills/patterns by keyword. Returns matching skills with problem/solution pairs."
    )]
    async fn recall_skills(&self, #[tool(param)] query: String) -> String {
        let config = get_config_path();
        match synapz_memory::SupabaseMemory::from_config(&config) {
            Ok(mem) => {
                // Search for skills specifically
                let search_query = format!("SKILL {query}");
                match mem.recall(&search_query, 10).await {
                    Ok(results) => {
                        let skills: Vec<String> = results
                            .iter()
                            .filter(|m| m.category == "skill" || m.content.starts_with("SKILL:"))
                            .map(|m| format!("📚 {}", m.content))
                            .collect();
                        if skills.is_empty() {
                            format!(
                                "🔍 No skills found for '{query}'. Save skills with save_skill tool."
                            )
                        } else {
                            format!(
                                "📚 Found {} skills:\n\n{}",
                                skills.len(),
                                skills.join("\n---\n")
                            )
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
            instructions: Some("SynapzCore MCP Server — 12 tools: shared memory + Auto-Context + Self-Reflection + Skill Library + Multi-Agent Coordination. WORKFLOW: (1) Call auto_context FIRST. (2) Call coord_heartbeat to register presence. (3) Before editing any file, call coord_claim. (4) After editing, call coord_release. Use coord_status to check who holds what.".into()),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}

/// Resolve thư mục gốc SynapzCore — KHÔNG hardcode máy cụ thể.
/// Thứ tự: env SYNAPZ_ROOT → suy từ vị trí exe (target/<profile>/ → lên 2 cấp) → cwd.
fn synapz_root() -> String {
    if let Ok(r) = std::env::var("SYNAPZ_ROOT")
        && !r.is_empty()
    {
        return r;
    }
    // exe thường ở <root>/target/<debug|release>/synapz-mcp(.exe) → root = lên 3 cấp.
    if let Ok(exe) = std::env::current_exe()
        && let Some(root) = exe
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.parent())
    {
        // chỉ nhận nếu trông giống repo (có Cargo.toml hoặc thư mục crates).
        if root.join("Cargo.toml").exists() || root.join("crates").exists() {
            return root.to_string_lossy().into_owned();
        }
    }
    std::env::current_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| ".".to_string())
}

#[allow(dead_code)]
fn get_config_path() -> String {
    let base = synapz_root();
    format!("{base}\\data\\supabase_config.json")
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    eprintln!(
        "🚀 SynapzCore MCP Server starting... (12 tools: memory + auto-context + skills + coordination)"
    );

    // Spawn folder watcher in the background to automatically sync changes
    let base_dir = synapz_root();
    let watcher_path = format!("{base_dir}\\scripts\\folder_watcher.py");
    eprintln!("🚀 Launching local memory watcher from MCP Server...");
    match std::process::Command::new("python")
        .arg(&watcher_path)
        .spawn()
    {
        Ok(_) => eprintln!("✅ Memory watcher spawned successfully."),
        Err(e) => eprintln!("⚠️ Failed to spawn memory watcher from MCP: {e}"),
    }

    let service = AgentMcp
        .serve(rmcp::transport::io::stdio())
        .await
        .inspect_err(|e| eprintln!("❌ MCP Server error: {e}"))?;
    service.waiting().await?;
    Ok(())
}

/// Truncate to at most max chars (not bytes) — UTF-8 safe; appends … if cut.
// Called only from `#[tool]`-macro methods; rustc dead-code analysis cannot see
// through the proc-macro expansion, so the allow is required despite real use.
#[allow(dead_code)]
fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let head: String = s.chars().take(max).collect();
        format!("{head}…")
    }
}
