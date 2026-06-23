//! Supabase REST client for shared memory storage.
//! Supports all 3 agents: Antigravity, Grok, ChatGPT.

use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::fs;

use crate::Memory;

#[derive(Debug, Deserialize)]
struct SupabaseConfig {
    supabase_url: String,
    supabase_key: String,
}

/// Supabase cloud memory backend — shared across all agents.
pub struct SupabaseMemory {
    client: Client,
    url: String,
    key: String,
}

/// Legacy insert payload (backward compatible).
#[derive(Serialize)]
struct InsertPayload {
    content: String,
    role: String,
    metadata: serde_json::Value,
}

/// Full insert payload with agent fields.
#[derive(Serialize)]
struct InsertPayloadFull {
    content: String,
    role: String,
    agent: String,
    category: String,
    importance: i16,
    confidence: i16,
    #[serde(skip_serializing_if = "Option::is_none")]
    session_id: Option<String>,
    metadata: serde_json::Value,
}

impl SupabaseMemory {
    /// Load config from JSON file.
    pub fn from_config(config_path: &str) -> Result<Self> {
        let content = fs::read_to_string(config_path)
            .map_err(|e| anyhow!("❌ Config not found at {config_path}: {e}"))?;
        let config: SupabaseConfig = serde_json::from_str(&content)?;

        Ok(Self {
            client: Client::new(),
            url: config.supabase_url,
            key: config.supabase_key,
        })
    }

    /// Helper: build auth headers.
    fn auth_headers(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        req.header("apikey", &self.key)
           .header("Authorization", format!("Bearer {}", self.key))
           .header("Content-Type", "application/json")
    }

    /// Insert a memory (legacy — defaults to antigravity agent).
    pub async fn remember(&self, content: &str, role: &str, metadata: &serde_json::Value) -> Result<()> {
        let payload = InsertPayload {
            content: content.to_string(),
            role: role.to_string(),
            metadata: metadata.clone(),
        };

        let req = self.client
            .post(format!("{}/rest/v1/memories", self.url))
            .header("Prefer", "return=minimal")
            .json(&payload);
        let resp = self.auth_headers(req).send().await?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Supabase insert failed: {body}"));
        }
        Ok(())
    }

    /// Insert a memory with full agent metadata — for shared team memory.
    pub async fn remember_as(
        &self,
        content: &str,
        role: &str,
        agent: &str,
        category: &str,
        importance: i16,
        confidence: i16,
        metadata: &serde_json::Value,
    ) -> Result<()> {
        let payload = InsertPayloadFull {
            content: content.to_string(),
            role: role.to_string(),
            agent: agent.to_string(),
            category: category.to_string(),
            importance,
            confidence,
            session_id: None,
            metadata: metadata.clone(),
        };

        let req = self.client
            .post(format!("{}/rest/v1/memories", self.url))
            .header("Prefer", "return=minimal")
            .json(&payload);
        let resp = self.auth_headers(req).send().await?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Supabase insert (agent={agent}) failed: {body}"));
        }
        eprintln!("🧠 Memory saved: agent={agent}, category={category}, importance={importance}");
        Ok(())
    }

    /// Fetch N most recent memories (all agents).
    pub async fn fetch_recent(&self, limit: usize) -> Result<Vec<Memory>> {
        let req = self.client
            .get(format!("{}/rest/v1/memories", self.url))
            .query(&[
                ("select", "*"),
                ("order", "created_at.desc"),
                ("limit", &limit.to_string()),
            ]);
        let resp = self.auth_headers(req).send().await?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Supabase fetch failed: {body}"));
        }

        let memories: Vec<Memory> = resp.json().await?;
        Ok(memories.into_iter().rev().collect())
    }

    /// Search memories by keyword (ilike) — all agents.
    pub async fn recall(&self, query: &str, limit: usize) -> Result<Vec<Memory>> {
        let req = self.client
            .get(format!("{}/rest/v1/memories", self.url))
            .query(&[
                ("select", "*"),
                ("content", &format!("ilike.%{query}%")),
                ("limit", &limit.to_string()),
            ]);
        let resp = self.auth_headers(req).send().await?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Supabase recall failed: {body}"));
        }

        let memories: Vec<Memory> = resp.json().await?;
        Ok(memories)
    }

    /// Fetch recent memories for a specific agent.
    pub async fn recall_by_agent(&self, agent: &str, limit: usize) -> Result<Vec<Memory>> {
        let req = self.client
            .get(format!("{}/rest/v1/memories", self.url))
            .query(&[
                ("select", "*"),
                ("agent", &format!("eq.{agent}")),
                ("order", "created_at.desc"),
                ("limit", &limit.to_string()),
            ]);
        let resp = self.auth_headers(req).send().await?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Supabase recall_by_agent failed: {body}"));
        }

        let memories: Vec<Memory> = resp.json().await?;
        Ok(memories)
    }

    /// Fetch high-importance team memories across all agents.
    /// Used for injecting shared context into subagent system prompts.
    pub async fn recall_team(&self, limit: usize) -> Result<Vec<Memory>> {
        let req = self.client
            .get(format!("{}/rest/v1/memories", self.url))
            .query(&[
                ("select", "*"),
                ("importance", "gte.3"),
                ("order", "created_at.desc"),
                ("limit", &limit.to_string()),
            ]);
        let resp = self.auth_headers(req).send().await?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Supabase recall_team failed: {body}"));
        }

        let memories: Vec<Memory> = resp.json().await?;
        Ok(memories)
    }

    /// Archive old, low-importance memories.
    /// Moves records older than `days_old` with importance <= `max_importance` to `memories_archive`.
    /// Returns count of archived records.
    pub async fn archive_old(&self, days_old: u32, max_importance: i16) -> Result<usize> {
        // Step 1: Fetch memories to archive
        let cutoff = chrono::Utc::now() - chrono::Duration::days(days_old as i64);
        let cutoff_str = cutoff.to_rfc3339();

        let req = self.client
            .get(format!("{}/rest/v1/memories", self.url))
            .query(&[
                ("select", "*"),
                ("importance", &format!("lte.{max_importance}")),
                ("created_at", &format!("lt.{cutoff_str}")),
                ("limit", "100"),
            ]);
        let resp = self.auth_headers(req).send().await?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Archive fetch failed: {body}"));
        }

        let memories: Vec<Memory> = resp.json().await?;
        if memories.is_empty() {
            eprintln!("📦 No memories to archive (cutoff: {days_old} days, max_importance: {max_importance})");
            return Ok(0);
        }

        let count = memories.len();
        eprintln!("📦 Archiving {count} memories...");

        // Step 2: Insert into memories_archive
        let req = self.client
            .post(format!("{}/rest/v1/memories_archive", self.url))
            .header("Prefer", "return=minimal")
            .json(&memories);
        let resp = self.auth_headers(req).send().await?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Archive insert failed: {body}"));
        }

        // Step 3: Delete from memories
        let ids: Vec<String> = memories.iter()
            .filter_map(|m| m.id.as_ref().map(|id| id.to_string()))
            .collect();

        if !ids.is_empty() {
            let id_filter = format!("in.({})", ids.join(","));
            let req = self.client
                .delete(format!("{}/rest/v1/memories", self.url))
                .query(&[("id", &id_filter)]);
            let resp = self.auth_headers(req).send().await?;

            if !resp.status().is_success() {
                let body = resp.text().await.unwrap_or_default();
                return Err(anyhow!("Archive delete failed: {body}"));
            }
        }

        eprintln!("✅ Archived {count} memories to memories_archive");
        Ok(count)
    }

    /// Move a specific list of memories to memories_archive.
    pub async fn archive_memories(&self, memories: &[Memory]) -> Result<()> {
        if memories.is_empty() {
            return Ok(());
        }

        let count = memories.len();
        
        // Step 1: Insert into memories_archive
        let req = self.client
            .post(format!("{}/rest/v1/memories_archive", self.url))
            .header("Prefer", "return=minimal")
            .json(memories);
        let resp = self.auth_headers(req).send().await?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Archive insert failed: {body}"));
        }

        // Step 2: Delete from memories
        let ids: Vec<String> = memories.iter()
            .filter_map(|m| m.id.as_ref().map(|id| id.to_string()))
            .collect();

        if !ids.is_empty() {
            let id_filter = format!("in.({})", ids.join(","));
            let req = self.client
                .delete(format!("{}/rest/v1/memories", self.url))
                .query(&[("id", &id_filter)]);
            let resp = self.auth_headers(req).send().await?;

            if !resp.status().is_success() {
                let body = resp.text().await.unwrap_or_default();
                return Err(anyhow!("Archive delete failed: {body}"));
            }
        }

        eprintln!("✅ Successfully moved {count} memories to archive");
        Ok(())
    }

    /// Get memory statistics.
    pub async fn stats(&self) -> Result<serde_json::Value> {
        // Count total memories
        let req = self.client
            .get(format!("{}/rest/v1/memories", self.url))
            .query(&[("select", "id"), ("limit", "1000")])
            .header("Prefer", "count=exact");
        let resp = self.auth_headers(req).send().await?;

        let total_count = resp.headers()
            .get("content-range")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.split('/').last())
            .and_then(|n| n.parse::<usize>().ok())
            .unwrap_or(0);

        // Count per agent
        let mut agent_counts = serde_json::Map::new();
        for agent in &["antigravity", "grok", "chatgpt"] {
            let req = self.client
                .get(format!("{}/rest/v1/memories", self.url))
                .query(&[("select", "id"), ("agent", &format!("eq.{agent}")), ("limit", "1")])
                .header("Prefer", "count=exact");
            let resp = self.auth_headers(req).send().await?;

            let count = resp.headers()
                .get("content-range")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.split('/').last())
                .and_then(|n| n.parse::<u64>().ok())
                .unwrap_or(0);

            agent_counts.insert(agent.to_string(), serde_json::json!(count));
        }

        // Count archived
        let req = self.client
            .get(format!("{}/rest/v1/memories_archive", self.url))
            .query(&[("select", "id"), ("limit", "1")])
            .header("Prefer", "count=exact");
        let resp = self.auth_headers(req).send().await?;

        let archived = resp.headers()
            .get("content-range")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.split('/').last())
            .and_then(|n| n.parse::<u64>().ok())
            .unwrap_or(0);

        Ok(serde_json::json!({
            "total_memories": total_count,
            "by_agent": agent_counts,
            "archived": archived,
        }))
    }

    /// Generate embedding for text using any OpenAI-compatible endpoint.
    /// Returns 384-dim vector (using text-embedding model).
    /// Set EMBEDDING_API_URL env var to override (default: http://127.0.0.1:8000).
    pub async fn generate_embedding(&self, text: &str) -> Result<Vec<f32>> {
        let api_url = std::env::var("EMBEDDING_API_URL")
            .or_else(|_| std::env::var("GROK_API_URL"))
            .unwrap_or_else(|_| "http://127.0.0.1:8000".to_string());

        let payload = serde_json::json!({
            "model": "text-embedding-3-small",
            "input": text,
            "dimensions": 384,
        });

        let resp = self.client
            .post(format!("{api_url}/v1/embeddings"))
            .header("Authorization", "Bearer grok-key")
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Embedding generation failed: {body}"));
        }

        let body: serde_json::Value = resp.json().await?;
        let embedding = body
            .get("data")
            .and_then(|d| d.get(0))
            .and_then(|d| d.get("embedding"))
            .and_then(|e| e.as_array())
            .ok_or_else(|| anyhow!("Invalid embedding response format"))?;

        let vec: Vec<f32> = embedding
            .iter()
            .filter_map(|v| v.as_f64().map(|f| f as f32))
            .collect();

        if vec.len() != 384 {
            return Err(anyhow!("Expected 384-dim embedding, got {}", vec.len()));
        }

        Ok(vec)
    }

    /// Semantic search using pgvector — find memories by meaning, not exact text.
    pub async fn semantic_search(
        &self,
        query: &str,
        limit: usize,
        threshold: f64,
    ) -> Result<Vec<Memory>> {
        // Generate embedding for query
        let embedding = self.generate_embedding(query).await?;
        let embedding_str = format!("[{}]", embedding.iter().map(|f| f.to_string()).collect::<Vec<_>>().join(","));

        // Call match_memories RPC
        let payload = serde_json::json!({
            "query_embedding": embedding_str,
            "match_threshold": threshold,
            "match_count": limit,
        });

        let req = self.client
            .post(format!("{}/rest/v1/rpc/match_memories", self.url))
            .json(&payload);
        let resp = self.auth_headers(req).send().await?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Semantic search failed: {body}"));
        }

        let memories: Vec<Memory> = resp.json().await?;
        Ok(memories)
    }

    /// Update embedding for a specific memory by id.
    pub async fn update_embedding(&self, memory_id: i64, embedding: &[f32]) -> Result<()> {
        let embedding_str = format!("[{}]", embedding.iter().map(|f| f.to_string()).collect::<Vec<_>>().join(","));

        let payload = serde_json::json!({
            "embedding": embedding_str,
        });

        let req = self.client
            .patch(format!("{}/rest/v1/memories?id=eq.{memory_id}", self.url))
            .header("Prefer", "return=minimal")
            .json(&payload);
        let resp = self.auth_headers(req).send().await?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Update embedding failed: {body}"));
        }

        Ok(())
    }

    /// Fetch goals from memories.
    pub async fn fetch_active_goals(&self, limit: usize) -> Result<Vec<Memory>> {
        let req = self.client
            .get(format!("{}/rest/v1/memories", self.url))
            .query(&[
                ("select", "*"),
                ("category", "eq.goal"),
                ("order", "created_at.desc"),
                ("limit", &limit.to_string()),
            ]);
        let resp = self.auth_headers(req).send().await?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Supabase fetch goals failed: {body}"));
        }

        let memories: Vec<Memory> = resp.json().await?;
        Ok(memories)
    }

    /// Backfill embeddings for memories that don't have one yet.
    /// Processes in batches to avoid rate limiting.
    pub async fn backfill_embeddings(&self, batch_size: usize) -> Result<usize> {
        // Fetch memories without embeddings
        let req = self.client
            .get(format!("{}/rest/v1/memories", self.url))
            .query(&[
                ("select", "id,content"),
                ("embedding", "is.null"),
                ("limit", &batch_size.to_string()),
            ]);
        let resp = self.auth_headers(req).send().await?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Backfill fetch failed: {body}"));
        }

        let rows: Vec<serde_json::Value> = resp.json().await?;
        if rows.is_empty() {
            eprintln!("✅ All memories already have embeddings");
            return Ok(0);
        }

        let mut count = 0;
        for row in &rows {
            let id = row.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
            let content = row.get("content").and_then(|v| v.as_str()).unwrap_or("");

            if content.is_empty() || id == 0 { continue; }

            match self.generate_embedding(content).await {
                Ok(emb) => {
                    if let Err(e) = self.update_embedding(id, &emb).await {
                        eprintln!("⚠️ Failed to update embedding for id={id}: {e}");
                    } else {
                        count += 1;
                        if count % 10 == 0 {
                            eprintln!("🔄 Embedded {count}/{} memories...", rows.len());
                        }
                    }
                }
                Err(e) => {
                    eprintln!("⚠️ Embedding generation failed for id={id}: {e}");
                }
            }

            // Rate limit: small delay between calls
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        eprintln!("✅ Backfilled {count} embeddings");
        Ok(count)
    }
}
