//! work_buffer — Giai đoạn 2: Vùng nhớ đệm tập trung (Central Buffering).
//!
//! Mọi kết quả TRUNG GIAN của agent khi chạy song song KHÔNG ghi thẳng vào
//! Database/filesystem chính — chúng được staging vào một WorkBuffer do
//! Orchestrator quản lý dưới dạng artifact (JSON/text/diff). Chỉ sau khi Fan-In
//! hội tụ + merge thành công, orchestrator mới "commit" buffer xuống đích thật.
//!
//! Lợi ích: tránh hai agent ghi đè nhau, cho phép rollback nguyên phiên,
//! và là đầu vào cho SmartMerge (Giai đoạn 3).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Loại artifact một agent nộp về buffer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArtifactKind {
    /// Nội dung file code (path tương đối + content).
    CodeFile { path: String },
    /// Dữ liệu thô (vd kết quả cào) — sẽ qua dedupe ở GĐ3.
    DataRecord,
    /// Báo cáo/log text thuần.
    Report,
    /// Diff/patch để áp vào file đã có.
    Patch { target: String },
}

/// Một artifact trung gian trong buffer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    /// Task đã sinh ra artifact này.
    pub task_id: String,
    /// Agent thực thi.
    pub agent_id: String,
    pub kind: ArtifactKind,
    /// Nội dung (code, data JSON, text...).
    pub content: String,
    /// Thời điểm nộp (epoch ms) — để sort/audit.
    pub staged_at_ms: u128,
}

impl Artifact {
    pub fn new(
        task_id: impl Into<String>,
        agent_id: impl Into<String>,
        kind: ArtifactKind,
        content: impl Into<String>,
    ) -> Self {
        Self {
            task_id: task_id.into(),
            agent_id: agent_id.into(),
            kind,
            content: content.into(),
            staged_at_ms: now_ms(),
        }
    }
}

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// Vùng đệm trung tâm. Nhiều agent staging song song (write), orchestrator đọc khi merge.
#[derive(Debug, Default)]
pub struct WorkBuffer {
    /// key = task_id → danh sách artifact của task đó.
    artifacts: HashMap<String, Vec<Artifact>>,
}

impl WorkBuffer {
    pub fn new() -> Self {
        Self {
            artifacts: HashMap::new(),
        }
    }

    /// Agent staging một artifact (KHÔNG ghi đích thật).
    pub fn stage(&mut self, artifact: Artifact) {
        self.artifacts
            .entry(artifact.task_id.clone())
            .or_default()
            .push(artifact);
    }

    /// Lấy mọi artifact của một task.
    pub fn for_task(&self, task_id: &str) -> &[Artifact] {
        self.artifacts
            .get(task_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Toàn bộ artifact (phẳng) — cho merge/dedupe ở GĐ3.
    pub fn all(&self) -> Vec<&Artifact> {
        self.artifacts.values().flatten().collect()
    }

    /// Lọc artifact theo loại — vd lấy hết CodeFile để merge.
    pub fn code_files(&self) -> Vec<&Artifact> {
        self.all()
            .into_iter()
            .filter(|a| matches!(a.kind, ArtifactKind::CodeFile { .. }))
            .collect()
    }

    pub fn data_records(&self) -> Vec<&Artifact> {
        self.all()
            .into_iter()
            .filter(|a| a.kind == ArtifactKind::DataRecord)
            .collect()
    }

    pub fn total(&self) -> usize {
        self.artifacts.values().map(|v| v.len()).sum()
    }

    pub fn task_count(&self) -> usize {
        self.artifacts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.artifacts.is_empty()
    }

    /// Xóa buffer của một task (sau khi đã commit).
    pub fn clear_task(&mut self, task_id: &str) {
        self.artifacts.remove(task_id);
    }

    /// Xóa toàn bộ (reset phiên).
    pub fn clear(&mut self) {
        self.artifacts.clear();
    }

    /// Xuất snapshot JSON để dashboard/audit nhìn buffer realtime.
    pub fn snapshot_json(&self) -> serde_json::Value {
        let by_task: serde_json::Map<String, serde_json::Value> = self
            .artifacts
            .iter()
            .map(|(task, arts)| {
                let items: Vec<serde_json::Value> = arts
                    .iter()
                    .map(|a| {
                        serde_json::json!({
                            "agent": a.agent_id,
                            "kind": format!("{:?}", a.kind),
                            "bytes": a.content.len(),
                            "staged_at_ms": a.staged_at_ms,
                        })
                    })
                    .collect();
                (task.clone(), serde_json::Value::Array(items))
            })
            .collect();
        serde_json::json!({
            "total_artifacts": self.total(),
            "tasks": by_task,
        })
    }
}

/// Handle chia sẻ an toàn — agent staging song song qua đây.
pub type SharedBuffer = Arc<RwLock<WorkBuffer>>;

pub fn new_shared_buffer() -> SharedBuffer {
    Arc::new(RwLock::new(WorkBuffer::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stage_and_retrieve() {
        let mut buf = WorkBuffer::new();
        buf.stage(Artifact::new(
            "api-login",
            "coder-01",
            ArtifactKind::CodeFile {
                path: "src/login.rs".into(),
            },
            "fn login() {}",
        ));
        buf.stage(Artifact::new(
            "api-login",
            "coder-01",
            ArtifactKind::CodeFile {
                path: "src/jwt.rs".into(),
            },
            "fn sign() {}",
        ));
        assert_eq!(buf.for_task("api-login").len(), 2);
        assert_eq!(buf.total(), 2);
        assert_eq!(buf.task_count(), 1);
        assert_eq!(buf.code_files().len(), 2);
    }

    #[test]
    fn test_filter_by_kind() {
        let mut buf = WorkBuffer::new();
        buf.stage(Artifact::new(
            "t1",
            "a1",
            ArtifactKind::CodeFile {
                path: "x.rs".into(),
            },
            "code",
        ));
        buf.stage(Artifact::new(
            "t2",
            "a2",
            ArtifactKind::DataRecord,
            "{\"k\":1}",
        ));
        buf.stage(Artifact::new("t3", "a3", ArtifactKind::Report, "done"));
        assert_eq!(buf.code_files().len(), 1);
        assert_eq!(buf.data_records().len(), 1);
        assert_eq!(buf.total(), 3);
    }

    #[test]
    fn test_clear_task() {
        let mut buf = WorkBuffer::new();
        buf.stage(Artifact::new("t1", "a1", ArtifactKind::Report, "x"));
        buf.stage(Artifact::new("t2", "a2", ArtifactKind::Report, "y"));
        buf.clear_task("t1");
        assert_eq!(buf.task_count(), 1);
        assert!(buf.for_task("t1").is_empty());
    }

    #[tokio::test]
    async fn test_shared_buffer_concurrent_stage() {
        // Nhiều agent staging song song → không mất artifact (RwLock).
        let buf = new_shared_buffer();
        let mut handles = Vec::new();
        for i in 0..10 {
            let b = buf.clone();
            handles.push(tokio::spawn(async move {
                let mut g = b.write().await;
                g.stage(Artifact::new(
                    format!("task-{}", i),
                    format!("agent-{}", i),
                    ArtifactKind::Report,
                    format!("result {}", i),
                ));
            }));
        }
        for h in handles {
            h.await.unwrap();
        }
        assert_eq!(buf.read().await.total(), 10);
    }
}
