//! # synapz-memory — SynapzCore Memory Engine
//!
//! Manages long-term memory for the Antigravity agent via Supabase cloud + local JSON queue.

mod queue;
mod supabase;

pub use queue::MemoryQueue;
pub use supabase::SupabaseMemory;

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// A single memory entry.
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

fn default_agent() -> String {
    "antigravity".to_string()
}
fn default_category() -> String {
    "general".to_string()
}
fn default_importance() -> i16 {
    3
}
fn default_confidence() -> i16 {
    3
}

/// Main memory interface — combines Supabase + local write-ahead queue.
pub struct MemoryBrain {
    pub supabase: SupabaseMemory,
    pub queue: MemoryQueue,
}

impl MemoryBrain {
    pub fn new(config_path: &str) -> Result<Self> {
        let supabase = SupabaseMemory::from_config(config_path)?;
        let queue = MemoryQueue::new(config_path)?;
        Ok(Self { supabase, queue })
    }

    /// Write-ahead save — ghi NGAY xuống đĩa (crash-safe) RỒI mới đẩy cloud.
    /// Bản cũ đệm RAM tới 5 item mới flush → crash giữa chừng = mất trắng (vi phạm
    /// nguyên tắc "crash = mất trắng"). Giờ mọi save() persist tức thì vào
    /// memory_queue.jsonl; flush() best-effort đẩy lên Supabase.
    pub async fn save(&mut self, content: &str, role: &str, context: Option<&str>) -> Result<()> {
        self.queue.enqueue(content, role, context)?; // 1) bền vững trước
        self.flush().await // 2) cố đẩy cloud ngay (lỗi → vẫn còn trong queue)
    }

    /// Save with full agent metadata — used by subagent auto-save.
    // Mirrors SupabaseMemory::remember_as; same schema-driven arg list.
    #[allow(clippy::too_many_arguments)]
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
        self.supabase
            .remember_as(
                content, role, agent, category, importance, confidence, metadata,
            )
            .await
    }

    /// Đẩy write-ahead queue lên Supabase. CRASH-SAFE: chỉ ghi đè file SAU khi đã thử
    /// đẩy cloud — entry nào lỗi được GIỮ LẠI để thử lần sau. Crash giữa chừng cùng lắm
    /// gây trùng (re-push) chứ KHÔNG mất data.
    pub async fn flush(&mut self) -> Result<()> {
        let entries = self.queue.peek_all()?; // đọc, KHÔNG xoá
        if entries.is_empty() {
            return Ok(());
        }
        let mut failed = Vec::new();
        for e in entries {
            let metadata = serde_json::json!({
                "context": e.context.clone().unwrap_or_else(|| "general".to_string())
            });
            if let Err(err) = self.supabase.remember(&e.text, &e.speaker, &metadata).await {
                eprintln!("⚠️ Supabase save failed: {err}, giữ lại trong write-ahead queue");
                failed.push(e);
            }
        }
        self.queue.replace_all(&failed)?; // chỉ còn lại entry đẩy lỗi
        Ok(())
    }

    /// Search memories by keyword. Falls back to the local write-ahead queue when the
    /// cloud backend errors (e.g. no Supabase config) so recall still works offline.
    pub async fn recall(&self, query: &str, limit: usize) -> Result<Vec<Memory>> {
        match self.supabase.recall(query, limit).await {
            Ok(v) => Ok(v),
            Err(e) => {
                eprintln!("⚠️ Supabase recall failed ({e}); falling back to local queue");
                Ok(self.local_recall(query, limit))
            }
        }
    }

    /// Read recall results from the local JSONL queue (offline/degraded mode).
    fn local_recall(&self, query: &str, limit: usize) -> Vec<Memory> {
        self.queue
            .search_local(query, limit)
            .unwrap_or_default()
            .into_iter()
            .map(|e| Memory {
                id: None,
                content: e.text,
                role: e.speaker.clone(),
                agent: e.speaker,
                session_id: None,
                category: e.context.unwrap_or_else(|| "general".to_string()),
                importance: default_importance(),
                confidence: default_confidence(),
                metadata: serde_json::json!({}),
                created_at: Some(e.timestamp),
            })
            .collect()
    }

    /// Fetch N most recent memories. Falls back to the local queue on cloud error.
    pub async fn fetch_recent(&self, limit: usize) -> Result<Vec<Memory>> {
        match self.supabase.fetch_recent(limit).await {
            Ok(v) => Ok(v),
            Err(e) => {
                eprintln!("⚠️ Supabase fetch_recent failed ({e}); falling back to local queue");
                Ok(self.local_recall("", limit))
            }
        }
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
        let json = r#"{"id":1,"content":"hello","role":"bố","agent":"antigravity","category":"research","importance":5,"confidence":4,"metadata":{},"created_at":"2026-04-17"}"#;
        let mem: Memory = serde_json::from_str(json).unwrap();
        assert_eq!(mem.agent, "antigravity");
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
