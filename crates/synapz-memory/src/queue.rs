//! Local JSONL queue for memory entries pending cloud sync.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize)]
pub struct QueueEntry {
    pub speaker: String,
    pub text: String,
    pub context: Option<String>,
    pub timestamp: String,
}

/// Manages a local JSONL file of pending memory writes.
pub struct MemoryQueue {
    queue_path: PathBuf,
}

impl MemoryQueue {
    pub fn new(config_path: &str) -> Result<Self> {
        // Queue file lives next to the config's parent (data/) → memory/memory_queue.jsonl
        let config_dir = Path::new(config_path).parent().unwrap_or(Path::new("."));
        let project_root = config_dir.parent().unwrap_or(Path::new("."));
        let queue_path = project_root.join("memory").join("memory_queue.jsonl");
        Ok(Self { queue_path })
    }

    /// Add an entry to the queue.
    pub fn enqueue(&self, text: &str, speaker: &str, context: Option<&str>) -> Result<()> {
        let entry = QueueEntry {
            speaker: speaker.to_string(),
            text: text.to_string(),
            context: context.map(|s| s.to_string()),
            timestamp: chrono::Utc::now().to_rfc3339(),
        };

        if let Some(dir) = self.queue_path.parent() {
            fs::create_dir_all(dir)?; // đảm bảo thư mục memory/ tồn tại
        }
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.queue_path)?;

        let line = serde_json::to_string(&entry)?;
        writeln!(file, "{line}")?;
        Ok(())
    }

    /// Process all pending entries, returning those that failed.
    pub fn drain(&self) -> Result<Vec<QueueEntry>> {
        if !self.queue_path.exists() {
            return Ok(vec![]);
        }

        let file = fs::File::open(&self.queue_path)?;
        let reader = std::io::BufReader::new(file);
        let mut entries = Vec::new();

        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<QueueEntry>(&line) {
                Ok(entry) => entries.push(entry),
                Err(e) => eprintln!("⚠️ Bad queue line: {e}"),
            }
        }

        // Clear the file
        if !entries.is_empty() {
            fs::remove_file(&self.queue_path)?;
        }

        Ok(entries)
    }

    /// Number of pending entries.
    pub fn pending_count(&self) -> usize {
        if !self.queue_path.exists() {
            return 0;
        }
        fs::read_to_string(&self.queue_path)
            .map(|s| s.lines().filter(|l| !l.trim().is_empty()).count())
            .unwrap_or(0)
    }

    /// Đọc TẤT CẢ entry mà KHÔNG xoá file (an toàn: chỉ xoá/ghi đè sau khi đã xử lý cloud).
    pub fn peek_all(&self) -> Result<Vec<QueueEntry>> {
        if !self.queue_path.exists() {
            return Ok(vec![]);
        }
        let file = fs::File::open(&self.queue_path)?;
        let reader = std::io::BufReader::new(file);
        let mut entries = Vec::new();
        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<QueueEntry>(&line) {
                Ok(entry) => entries.push(entry),
                Err(e) => eprintln!("⚠️ Bad queue line: {e}"),
            }
        }
        Ok(entries)
    }

    /// Ghi đè queue bằng danh sách entry còn lại (vd các entry đẩy cloud thất bại).
    /// Rỗng → xoá file. Chỉ gọi SAU khi đã xử lý cloud xong (crash-safe).
    pub fn replace_all(&self, entries: &[QueueEntry]) -> Result<()> {
        if entries.is_empty() {
            if self.queue_path.exists() {
                fs::remove_file(&self.queue_path)?;
            }
            return Ok(());
        }
        if let Some(dir) = self.queue_path.parent() {
            fs::create_dir_all(dir).ok();
        }
        let mut file = fs::File::create(&self.queue_path)?; // truncate + tạo mới
        for e in entries {
            writeln!(file, "{}", serde_json::to_string(e)?)?;
        }
        Ok(())
    }

    /// Local-only keyword read of pending entries -> degraded recall when the cloud
    /// backend is unavailable. Case-insensitive substring match; empty query = all.
    /// Returns newest-first, capped at limit.
    pub fn search_local(&self, query: &str, limit: usize) -> Result<Vec<QueueEntry>> {
        let mut entries = self.peek_all()?;
        entries.reverse(); // append-order file -> newest last
        let q = query.to_lowercase();
        let out: Vec<QueueEntry> = entries
            .into_iter()
            .filter(|e| q.is_empty() || e.text.to_lowercase().contains(&q))
            .take(limit)
            .collect();
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Queue path = <parent of config_dir>/memory/memory_queue.jsonl. Dùng tmp để cô lập.
    fn temp_queue(dir: &std::path::Path) -> MemoryQueue {
        // config_path giả: <dir>/data/cfg.json → queue ở <dir>/memory/memory_queue.jsonl
        let cfg = dir.join("data").join("cfg.json");
        MemoryQueue::new(cfg.to_str().unwrap()).unwrap()
    }

    #[test]
    fn enqueue_then_peek_does_not_delete() {
        let tmp = std::env::temp_dir().join(format!("synapz_q_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let q = temp_queue(&tmp);
        q.enqueue("hello", "Bố", Some("ctx")).unwrap();
        q.enqueue("world", "kiro", None).unwrap();
        // peek đọc cả 2 nhưng KHÔNG xoá → đọc lại vẫn còn (crash-safe).
        assert_eq!(q.peek_all().unwrap().len(), 2);
        assert_eq!(q.peek_all().unwrap().len(), 2);
        assert_eq!(q.pending_count(), 2);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn search_local_filters_and_orders_newest_first() {
        let tmp = std::env::temp_dir().join(format!("synapz_q3_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let q = temp_queue(&tmp);
        q.enqueue("alpha record", "x", None).unwrap();
        q.enqueue("beta record", "y", None).unwrap();
        q.enqueue("gamma alpha", "z", None).unwrap();
        // empty query = all, newest first
        let all = q.search_local("", 10).unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].text, "gamma alpha");
        // keyword filter, case-insensitive
        let hits = q.search_local("ALPHA", 10).unwrap();
        assert_eq!(hits.len(), 2);
        // limit respected
        assert_eq!(q.search_local("", 1).unwrap().len(), 1);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn replace_all_keeps_only_failed() {
        let tmp = std::env::temp_dir().join(format!("synapz_q2_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let q = temp_queue(&tmp);
        q.enqueue("a", "x", None).unwrap();
        q.enqueue("b", "y", None).unwrap();
        let all = q.peek_all().unwrap();
        // giả lập "b" đẩy lỗi → giữ lại mình nó.
        let failed: Vec<QueueEntry> = all.into_iter().filter(|e| e.text == "b").collect();
        q.replace_all(&failed).unwrap();
        let left = q.peek_all().unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].text, "b");
        // replace_all rỗng → xoá sạch.
        q.replace_all(&[]).unwrap();
        assert_eq!(q.pending_count(), 0);
        let _ = fs::remove_dir_all(&tmp);
    }
}
