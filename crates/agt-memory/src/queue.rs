//! Local JSONL queue for memory entries pending cloud sync.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize)]
pub struct QueueEntry {
    speaker: String,
    text: String,
    context: Option<String>,
    timestamp: String,
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
}
