//! Grok Subagent — Gọi Grok AI qua grok2api reverse proxy (OpenAI-compatible).
//!
//! Subagent này cho phép Antigravity gọi Grok để:
//! - Research sâu một chủ đề
//! - Phân tích code
//! - Brainstorm ý tưởng
//! - Suy nghĩ (thinking mode) cho các quyết định phức tạp

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Grok API endpoint mặc định (Localhost).
const GROK_API_BASE: &str = "http://127.0.0.1:8000";

/// Models có sẵn.
#[derive(Debug, Clone, Copy)]
pub enum GrokModel {
    Grok3,
    Grok3Mini,
    Grok3Thinking,
    Grok4,
    Grok4Thinking,
    Grok4Heavy,
}

impl GrokModel {
    pub fn as_str(&self) -> &'static str {
        match self {
            GrokModel::Grok3 => "grok-3",
            GrokModel::Grok3Mini => "grok-3-mini",
            GrokModel::Grok3Thinking => "grok-3-thinking",
            GrokModel::Grok4 => "grok-4",
            GrokModel::Grok4Thinking => "grok-4-thinking",
            GrokModel::Grok4Heavy => "grok-4-heavy",
        }
    }

    /// Parse model name từ string (case-insensitive).
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "grok-3" | "grok3" => GrokModel::Grok3,
            "grok-3-mini" | "grok3-mini" | "mini" => GrokModel::Grok3Mini,
            "grok-3-thinking" | "thinking" => GrokModel::Grok3Thinking,
            "grok-4" | "grok4" => GrokModel::Grok4,
            "grok-4-thinking" | "grok4-thinking" => GrokModel::Grok4Thinking,
            "grok-4-heavy" | "heavy" | "expert" => GrokModel::Grok4Heavy,
            _ => GrokModel::Grok4Heavy, // fallback — Bố chỉ dùng heavy
        }
    }
}

/// Helper to map legacy models to the new active Grok models.
pub fn resolve_model_name(name: &str) -> String {
    let base = std::env::var("GROK_MODEL").unwrap_or_else(|_| "".to_string());
    if !base.is_empty() {
        return base;
    }
    match name.to_lowercase().as_str() {
        "grok-4-heavy" | "grok4-heavy" | "heavy" | "expert" | "grok-4" | "grok4" => {
            "grok-4.20-0309-non-reasoning".to_string()
        }
        "grok-3" | "grok3" | "grok-3-mini" | "grok3-mini" | "mini" | "grok-4-fast" | "fast" | "grok-4.20-fast" => {
            "grok-4.20-fast".to_string()
        }
        _ => name.to_string(),
    }
}

/// Message format cho OpenAI-compatible API.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// Response từ Grok API.
#[derive(Debug, Deserialize)]
pub struct ChatResponse {
    pub id: Option<String>,
    pub choices: Option<Vec<ChatChoice>>,
    pub error: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct ChatChoice {
    pub message: Option<ChatMessage>,
    pub finish_reason: Option<String>,
}

/// Grok Subagent client.
pub struct GrokSubagent {
    base_url: String,
    api_key: Option<String>,
    client: reqwest::Client,
}

impl GrokSubagent {
    /// Tạo client mới với default config (VPS Contabo).
    pub fn new() -> Self {
        Self {
            base_url: std::env::var("GROK_API_BASE")
                .unwrap_or_else(|_| GROK_API_BASE.to_string()),
            api_key: std::env::var("GROK_API_KEY").ok(),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .expect("Failed to build reqwest client"),
        }
    }

    /// Tạo client với URL tùy chỉnh.
    pub fn with_url(url: &str) -> Self {
        Self {
            base_url: url.to_string(),
            api_key: None,
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .expect("Failed to build reqwest client"),
        }
    }

    /// Core chat completion — gọi Grok API chuẩn OpenAI.
    pub async fn chat(
        &self,
        messages: Vec<ChatMessage>,
        model: &str,
    ) -> Result<String> {
        let url = format!("{}/v1/chat/completions", self.base_url);
        let resolved_model = resolve_model_name(model);

        let payload = json!({
            "model": resolved_model,
            "messages": messages,
            "stream": false,
        });

        let mut req = self.client.post(&url)
            .header("Content-Type", "application/json")
            .json(&payload);

        if let Some(ref key) = self.api_key {
            req = req.header("Authorization", format!("Bearer {key}"));
        }

        let resp = req.send().await
            .map_err(|e| anyhow!("🔴 Grok API connection error: {e}"))?;

        let status = resp.status();
        let body = resp.text().await
            .map_err(|e| anyhow!("🔴 Failed to read response: {e}"))?;

        if !status.is_success() {
            return Err(anyhow!("🔴 Grok API error ({}): {}", status, body));
        }

        let chat_resp: ChatResponse = serde_json::from_str(&body)
            .map_err(|e| anyhow!("🔴 Failed to parse Grok response: {e}\nBody: {body}"))?;

        if let Some(err) = chat_resp.error {
            return Err(anyhow!("🔴 Grok error: {err}"));
        }

        chat_resp.choices
            .and_then(|c| c.into_iter().next())
            .and_then(|c| c.message)
            .map(|m| m.content)
            .ok_or_else(|| anyhow!("🔴 Empty response from Grok"))
    }

    /// 🔬 Research — hỏi Grok nghiên cứu một chủ đề.
    pub async fn research(&self, topic: &str) -> Result<String> {
        let messages = vec![
            ChatMessage {
                role: "system".to_string(),
                content: concat!(
                    "You are Grok Research Engine — a principal-level AI researcher with deep analytical rigor and creative synthesis.\n\n",
                    "Core Principles (Grok Framework):\n",
                    "- ANALYTICAL DEPTH: Decompose problems to first principles. Identify assumptions, quantify trade-offs, cite evidence.\n",
                    "- ENGINEERING MASTERY: Deep knowledge of languages, architectures, patterns, tooling, security, scalability.\n",
                    "- COGNITIVE DISCIPLINE: Bias-aware, no hype. Present balanced evaluation with pros/cons.\n",
                    "- INTEGRITY: Prioritize correctness and sustainability over shortcuts. Flag uncertainties honestly.\n",
                    "- CONTEXTUAL AWARENESS: Adapt to the specific ecosystem, constraints, and realities of the question.\n\n",
                    "Think-Act Loop:\n",
                    "1. THINK: Clarify the goal → Decompose → Analyze options with evidence → Identify risks\n",
                    "2. ACT: Synthesize into structured output with Summary, Key Findings, Trade-offs, Recommendation, Sources\n",
                    "3. REFLECT: Highlight uncertainties and suggest follow-up investigations\n\n",
                    "Response Format: Use headers, bullet points, tables where helpful. Bold key decisions. Concise yet comprehensive — no fluff."
                ).to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: topic.to_string(),
            },
        ];
        self.chat(messages, "grok-4-heavy").await
    }

    /// 🧠 Think — chế độ suy nghĩ sâu cho quyết định phức tạp.
    pub async fn think(&self, problem: &str) -> Result<String> {
        let messages = vec![
            ChatMessage {
                role: "system".to_string(),
                content: concat!(
                    "You are Grok Thinking Engine — a battle-tested staff engineer who ships reliable systems at scale.\n\n",
                    "Core Principles (Grok Framework):\n",
                    "- FIRST PRINCIPLES: Never accept assumptions blindly. Break every problem down to fundamentals.\n",
                    "- TRADE-OFF QUANTIFICATION: Every decision has costs. Name them, measure them, compare them.\n",
                    "- GROUNDED PRAGMATISM: Consider real-world constraints — performance, deployment, team capacity, timeline.\n",
                    "- LONG-TERM ORIENTATION: Favor maintainability, testability, simplicity over clever hacks.\n",
                    "- ACTION BIAS: 'Nghĩ trước khi làm, nhưng phải làm trước khi hoàn hảo.'\n\n",
                    "Think-Act Loop:\n",
                    "1. THINK: Define the problem precisely → List constraints → Generate 2-3 viable approaches\n",
                    "2. ACT: Evaluate each approach (decision matrix if helpful) → Give ONE clear recommendation with rationale\n",
                    "3. REFLECT: What could go wrong? What's the rollback plan? What to validate first?\n\n",
                    "Output Format:\n",
                    "## Problem Analysis\n",
                    "## Options Evaluated\n",
                    "## Recommendation (bold the winner)\n",
                    "## Risks & Mitigations\n",
                    "## Next Steps (concrete, actionable)\n\n",
                    "Be decisive. Confidence backed by reasoning, not hedging."
                ).to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: problem.to_string(),
            },
        ];
        self.chat(messages, "grok-4-heavy").await
    }

    /// 💻 Code Review — phân tích code.
    pub async fn review_code(&self, code: &str, context: &str) -> Result<String> {
        let messages = vec![
            ChatMessage {
                role: "system".to_string(),
                content: concat!(
                    "You are Grok Code Review Engine — a principal-level reviewer who ensures engineering excellence.\n\n",
                    "Core Principles (Grok Framework):\n",
                    "- CORRECTNESS FIRST: Identify bugs, logic errors, edge cases, race conditions.\n",
                    "- QUALITY & CRAFT: Evaluate readability, naming, structure, DRY, SOLID principles.\n",
                    "- PERFORMANCE & SECURITY: Spot bottlenecks, memory leaks, injection risks, unsafe patterns.\n",
                    "- PRAGMATIC ACTION: Don't just criticize — provide specific fixes with before/after code diffs.\n",
                    "- COLLABORATIVE CLARITY: Explain WHY something is an issue, not just WHAT.\n\n",
                    "Review Checklist:\n",
                    "1. 🔴 Critical bugs / security issues\n",
                    "2. 🟡 Performance concerns / potential issues\n",
                    "3. 🔵 Code quality / readability improvements\n",
                    "4. 💡 Architecture / design suggestions\n\n",
                    "For each finding: describe issue → show fix → explain rationale.\n",
                    "End with an overall assessment: APPROVE / NEEDS CHANGES / BLOCK."
                ).to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: format!("Context: {context}\n\nCode:\n```\n{code}\n```"),
            },
        ];
        self.chat(messages, "grok-4-heavy").await
    }

    /// 💡 Brainstorm — sinh ý tưởng.
    pub async fn brainstorm(&self, topic: &str) -> Result<String> {
        let messages = vec![
            ChatMessage {
                role: "system".to_string(),
                content: concat!(
                    "You are Grok Brainstorm Engine — a creative innovation partner grounded in engineering reality.\n\n",
                    "Core Principles (Grok Framework):\n",
                    "- CREATIVE SYNTHESIS: Combine ideas across domains. Think laterally, not just linearly.\n",
                    "- GROUNDED INNOVATION: Every idea must be feasible. No pure fantasy — attach implementation hints.\n",
                    "- 1% BETTER MINDSET: 'Hành động nhỏ nhất quán tạo ra thay đổi lớn.' Start small, iterate fast.\n",
                    "- DIVERGE THEN CONVERGE: First generate broadly, then rank by impact × feasibility.\n\n",
                    "Output Format:\n",
                    "## 🚀 Bold Ideas (high impact, may need effort)\n",
                    "## ⚡ Quick Wins (low effort, immediate value)\n",
                    "## 🌱 Long-Term Seeds (plant now, harvest later)\n\n",
                    "For each idea: one-line summary → why it matters → how to start (first action step).\n",
                    "End with: 'If I had to pick ONE thing to do today, it would be: [X]'"
                ).to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: topic.to_string(),
            },
        ];
        self.chat(messages, "grok-4-heavy").await
    }

    /// Kiểm tra health của Grok API.
    pub async fn health_check(&self) -> Result<bool> {
        let url = format!("{}/health", self.base_url);
        let resp = self.client.get(&url).send().await?;
        Ok(resp.status().is_success())
    }
}

// ═══════════════════════════════════════════════════════
// Tool functions cho agt-tools Registry
// ═══════════════════════════════════════════════════════

/// Tool: Gọi Grok Subagent để hỏi/research/suy nghĩ.
/// Auto-saves prompt + response to shared Supabase memory.
///
/// Params:
/// - prompt: Câu hỏi / task
/// - mode: "chat" | "research" | "think" | "review" | "brainstorm" (mặc định: "chat")
/// - model: tên model (mặc định: "grok-3")
/// - code: (optional) code block cho mode "review"
/// - context: (optional) context bổ sung
pub async fn ask_grok(params: Value) -> Result<Value> {
    let prompt = params.get("prompt").and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'prompt'"))?;
    let mode = params.get("mode").and_then(|v| v.as_str()).unwrap_or("chat");
    let model_name = params.get("model").and_then(|v| v.as_str()).unwrap_or("grok-4-heavy");

    let grok = GrokSubagent::new();

    // Determine importance by mode
    let (category, importance) = match mode {
        "research" => ("research", 5_i16),
        "think" => ("decision", 5),
        "review" => ("review", 4),
        "brainstorm" => ("brainstorm", 3),
        _ => ("conversation", 2),
    };

    let result = match mode {
        "research" => grok.research(prompt).await?,
        "think" => grok.think(prompt).await?,
        "review" => {
            let code = params.get("code").and_then(|v| v.as_str()).unwrap_or("");
            let context = params.get("context").and_then(|v| v.as_str()).unwrap_or(prompt);
            grok.review_code(code, context).await?
        },
        "brainstorm" => grok.brainstorm(prompt).await?,
        _ => {
            // Chat mode — custom model
            let messages = vec![ChatMessage {
                role: "user".to_string(),
                content: prompt.to_string(),
            }];
            grok.chat(messages, model_name).await?
        }
    };

    // 🧠 Auto-save to shared memory (non-blocking — errors don't break the flow)
    let config_path = std::env::var("AGT_BRAIN_ROOT")
        .unwrap_or_else(|_| "E:\\AGT_Brain".to_string());
    let config_file = format!("{}/data/supabase_config.json", config_path);
    if let Ok(mem) = agt_memory::SupabaseMemory::from_config(&config_file) {
        // Save prompt (who asked)
        let prompt_truncated = if prompt.len() > 500 { &prompt[..500] } else { prompt };
        let _ = mem.remember_as(
            prompt_truncated, "antigravity", "antigravity", category,
            importance, 4, &json!({"mode": mode, "target": "grok", "model": model_name}),
        ).await;

        // Save response (what Grok said)
        let response_truncated = if result.len() > 2000 { &result[..2000] } else { &result };
        let _ = mem.remember_as(
            response_truncated, "grok", "grok", category,
            importance, 3, &json!({"mode": mode, "model": model_name}),
        ).await;
    }

    Ok(json!({
        "mode": mode,
        "model": model_name,
        "response": result,
    }))
}

/// Tool: Kiểm tra Grok API có hoạt động không.
pub async fn grok_health(params: Value) -> Result<Value> {
    let _ = params; // không cần param
    let grok = GrokSubagent::new();
    let healthy = grok.health_check().await.unwrap_or(false);
    Ok(json!({
        "healthy": healthy,
        "endpoint": grok.base_url,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_parsing() {
        assert_eq!(GrokModel::from_str("grok-3").as_str(), "grok-3");
        assert_eq!(GrokModel::from_str("heavy").as_str(), "grok-4-heavy");
        assert_eq!(GrokModel::from_str("thinking").as_str(), "grok-3-thinking");
        assert_eq!(GrokModel::from_str("mini").as_str(), "grok-3-mini");
    }
}
