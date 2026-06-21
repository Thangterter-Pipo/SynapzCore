//! # agt-memory — Antigravity Memory Engine
//!
//! Manages long-term shared memory for the 3-AI team via Supabase cloud + local JSON queue.
//! All agents (Antigravity, Grok, ChatGPT) read/write to the same memory.

mod supabase;
mod queue;

pub use supabase::SupabaseMemory;
pub use queue::MemoryQueue;

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// A single memory entry — shared across all 3 agents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Memory {
    pub id: Option<i64>,
    pub content: String,
    pub role: String,
    #[serde(default = "default_agent")]
    pub agent: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default = "default_category")]
    pub category: String,
    #[serde(default = "default_importance")]
    pub importance: i16,
    #[serde(default = "default_confidence")]
    pub confidence: i16,
    pub metadata: serde_json::Value,
    pub created_at: Option<String>,
}

fn default_agent() -> String { "antigravity".to_string() }
fn default_category() -> String { "general".to_string() }
fn default_importance() -> i16 { 3 }
fn default_confidence() -> i16 { 3 }

/// Main memory interface — combines Supabase + local queue.
pub struct MemoryBrain {
    pub supabase: SupabaseMemory,
    pub queue: MemoryQueue,
    buffer: Vec<Memory>,
}

impl MemoryBrain {
    pub fn new(config_path: &str) -> Result<Self> {
        let supabase = SupabaseMemory::from_config(config_path)?;
        let queue = MemoryQueue::new(config_path)?;
        Ok(Self {
            supabase,
            queue,
            buffer: Vec::new(),
        })
    }

    /// Save to working memory buffer. Auto-flush at 5 items.
    pub async fn save(&mut self, content: &str, role: &str, context: Option<&str>) -> Result<()> {
        let mem = Memory {
            id: None,
            content: content.to_string(),
            role: role.to_string(),
            agent: "antigravity".to_string(),
            session_id: None,
            category: "general".to_string(),
            importance: 3,
            confidence: 3,
            metadata: serde_json::json!({ "context": context.unwrap_or("general") }),
            created_at: None,
        };
        self.buffer.push(mem);

        if self.buffer.len() >= 5 {
            self.flush().await?;
        }
        Ok(())
    }

    /// Save with full agent metadata — used by subagent auto-save.
    pub async fn save_as(
        &mut self,
        content: &str,
        role: &str,
        agent: &str,
        category: &str,
        importance: i16,
        confidence: i16,
        metadata: &serde_json::Value,
    ) -> Result<()> {
        self.supabase.remember_as(content, role, agent, category, importance, confidence, metadata).await
    }

    /// Flush buffer to Supabase.
    pub async fn flush(&mut self) -> Result<()> {
        for mem in self.buffer.drain(..) {
            if let Err(e) = self.supabase.remember(&mem.content, &mem.role, &mem.metadata).await {
                eprintln!("⚠️ Supabase save failed: {e}, queueing locally");
                self.queue.enqueue(&mem.content, &mem.role, mem.metadata.get("context").and_then(|v| v.as_str()))?;
            }
        }
        Ok(())
    }

    /// Search memories by keyword.
    pub async fn recall(&self, query: &str, limit: usize) -> Result<Vec<Memory>> {
        self.supabase.recall(query, limit).await
    }

    /// Fetch N most recent memories.
    pub async fn fetch_recent(&self, limit: usize) -> Result<Vec<Memory>> {
        self.supabase.fetch_recent(limit).await
    }

    /// Fetch memories by specific agent.
    pub async fn recall_by_agent(&self, agent: &str, limit: usize) -> Result<Vec<Memory>> {
        self.supabase.recall_by_agent(agent, limit).await
    }

    /// Fetch team memories — high importance, recent, across all agents.
    pub async fn recall_team(&self, limit: usize) -> Result<Vec<Memory>> {
        self.supabase.recall_team(limit).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_struct_serializes() {
        let mem = Memory {
            id: None,
            content: "test".to_string(),
            role: "antigravity".to_string(),
            agent: "antigravity".to_string(),
            session_id: None,
            category: "general".to_string(),
            importance: 3,
            confidence: 3,
            metadata: serde_json::json!({}),
            created_at: None,
        };
        let json = serde_json::to_string(&mem).unwrap();
        assert!(json.contains("test"));
        assert!(json.contains("antigravity"));
    }

    #[test]
    fn memory_struct_deserializes_with_new_fields() {
        let json = r#"{"id":1,"content":"hello","role":"bố","agent":"grok","category":"research","importance":5,"confidence":4,"metadata":{},"created_at":"2026-04-17"}"#;
        let mem: Memory = serde_json::from_str(json).unwrap();
        assert_eq!(mem.agent, "grok");
        assert_eq!(mem.importance, 5);
        assert_eq!(mem.confidence, 4);
    }

    #[test]
    fn memory_struct_deserializes_with_defaults() {
        let json = r#"{"id":1,"content":"old","role":"user","metadata":{}}"#;
        let mem: Memory = serde_json::from_str(json).unwrap();
        assert_eq!(mem.agent, "antigravity");
        assert_eq!(mem.category, "general");
        assert_eq!(mem.importance, 3);
    }
}
