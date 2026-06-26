//! smart_merge — Giai đoạn 3: Hội tụ dữ liệu & Hợp nhất thông minh (Fan-In).
//!
//! Khi các agent song song nộp artifact về WorkBuffer, orchestrator hợp nhất:
//!   - CODE: gom CodeFile theo path. Nhiều agent ghi CÙNG path → phát hiện
//!     xung đột (cần resolve / SmartMerge), khác path → gộp thẳng.
//!   - DATA: gom DataRecord, KHỬ TRÙNG LẶP (dedupe theo nội dung chuẩn hóa) +
//!     ĐỒNG NHẤT HÓA (normalize whitespace/case cho khóa so sánh) → 1 tập sạch.
//!
//! Tách rời khỏi git (git lo merge nhánh; module này lo merge mức artifact/dữ liệu,
//! phục vụ cả luồng "5 agent cào 5 nguồn → gộp 1 báo cáo").

use crate::work_buffer::{Artifact, ArtifactKind, WorkBuffer};
use std::collections::HashMap;
use std::collections::HashSet;

/// Kết quả hợp nhất một file code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodeMergeEntry {
    /// Chỉ 1 agent ghi path này → lấy thẳng.
    Clean {
        path: String,
        content: String,
        agent: String,
    },
    /// Nhiều agent ghi cùng path → xung đột, liệt kê các phiên bản.
    Conflict {
        path: String,
        versions: Vec<(String, String)>,
    }, // (agent, content)
}

impl CodeMergeEntry {
    pub fn is_conflict(&self) -> bool {
        matches!(self, CodeMergeEntry::Conflict { .. })
    }
    pub fn path(&self) -> &str {
        match self {
            CodeMergeEntry::Clean { path, .. } => path,
            CodeMergeEntry::Conflict { path, .. } => path,
        }
    }
}

/// Báo cáo merge code toàn buffer.
#[derive(Debug, Clone, Default)]
pub struct CodeMergeReport {
    pub entries: Vec<CodeMergeEntry>,
}

impl CodeMergeReport {
    pub fn clean_files(&self) -> usize {
        self.entries.iter().filter(|e| !e.is_conflict()).count()
    }
    pub fn conflicts(&self) -> Vec<&CodeMergeEntry> {
        self.entries.iter().filter(|e| e.is_conflict()).collect()
    }
    pub fn has_conflicts(&self) -> bool {
        self.entries.iter().any(|e| e.is_conflict())
    }
}

/// HỢP NHẤT CODE: gom CodeFile theo path, phát hiện chồng lấn.
pub fn merge_code(buffer: &WorkBuffer) -> CodeMergeReport {
    // path → Vec<(agent, content)>
    let mut by_path: HashMap<String, Vec<(String, String)>> = HashMap::new();
    for art in buffer.code_files() {
        if let ArtifactKind::CodeFile { path } = &art.kind {
            by_path
                .entry(path.clone())
                .or_default()
                .push((art.agent_id.clone(), art.content.clone()));
        }
    }

    let mut entries = Vec::new();
    for (path, mut versions) in by_path {
        if versions.len() == 1 {
            let (agent, content) = versions.pop().unwrap();
            entries.push(CodeMergeEntry::Clean {
                path,
                content,
                agent,
            });
        } else {
            // Nhiều phiên bản — nếu nội dung GIỐNG HỆT nhau thì không phải conflict.
            let first = &versions[0].1;
            if versions.iter().all(|(_, c)| c == first) {
                let (agent, content) = versions.remove(0);
                entries.push(CodeMergeEntry::Clean {
                    path,
                    content,
                    agent,
                });
            } else {
                entries.push(CodeMergeEntry::Conflict { path, versions });
            }
        }
    }
    // sort ổn định theo path để test xác định.
    entries.sort_by(|a, b| a.path().cmp(b.path()));
    CodeMergeReport { entries }
}

/// Chuẩn hóa một record để so sánh dedupe: trim, gộp whitespace, lowercase.
fn normalize_key(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Kết quả dedupe dữ liệu.
#[derive(Debug, Clone, Default)]
pub struct DedupeReport {
    /// Các record duy nhất (giữ bản gặp ĐẦU TIÊN, nội dung gốc).
    pub unique: Vec<String>,
    /// Số record trùng đã loại.
    pub duplicates_removed: usize,
    /// Tổng record đầu vào.
    pub total_input: usize,
}

impl DedupeReport {
    pub fn unique_count(&self) -> usize {
        self.unique.len()
    }
}

/// KHỬ TRÙNG LẶP + ĐỒNG NHẤT HÓA dữ liệu từ nhiều agent.
/// Dùng cho luồng "N agent cào N nguồn → gộp 1 cơ sở dữ liệu sạch".
pub fn dedupe_data(buffer: &WorkBuffer) -> DedupeReport {
    let records: Vec<&Artifact> = buffer.data_records();
    let total_input = records.len();
    let mut seen: HashSet<String> = HashSet::new();
    let mut unique: Vec<String> = Vec::new();

    for art in records {
        let key = normalize_key(&art.content);
        if seen.insert(key) {
            unique.push(art.content.clone());
        }
    }

    DedupeReport {
        duplicates_removed: total_input - unique.len(),
        total_input,
        unique,
    }
}

/// Dedupe một danh sách string thuần (tiện ích độc lập, không cần buffer).
pub fn dedupe_strings(items: &[String]) -> DedupeReport {
    let mut seen = HashSet::new();
    let mut unique = Vec::new();
    for it in items {
        if seen.insert(normalize_key(it)) {
            unique.push(it.clone());
        }
    }
    DedupeReport {
        duplicates_removed: items.len() - unique.len(),
        total_input: items.len(),
        unique,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::work_buffer::{Artifact, ArtifactKind, WorkBuffer};

    fn code(buf: &mut WorkBuffer, task: &str, agent: &str, path: &str, content: &str) {
        buf.stage(Artifact::new(
            task,
            agent,
            ArtifactKind::CodeFile { path: path.into() },
            content,
        ));
    }

    #[test]
    fn test_merge_distinct_paths_clean() {
        let mut buf = WorkBuffer::new();
        code(&mut buf, "t1", "a1", "src/login.rs", "fn login() {}");
        code(&mut buf, "t2", "a2", "src/pay.rs", "fn pay() {}");
        let rep = merge_code(&buf);
        assert_eq!(rep.clean_files(), 2);
        assert!(!rep.has_conflicts());
    }

    #[test]
    fn test_merge_same_path_conflict() {
        let mut buf = WorkBuffer::new();
        code(&mut buf, "t1", "a1", "src/app.rs", "version A");
        code(&mut buf, "t2", "a2", "src/app.rs", "version B");
        let rep = merge_code(&buf);
        assert!(rep.has_conflicts());
        assert_eq!(rep.conflicts().len(), 1);
        if let CodeMergeEntry::Conflict { versions, .. } = rep.conflicts()[0] {
            assert_eq!(versions.len(), 2);
        }
    }

    #[test]
    fn test_merge_same_path_identical_not_conflict() {
        // Hai agent ghi y hệt nhau → KHÔNG conflict.
        let mut buf = WorkBuffer::new();
        code(&mut buf, "t1", "a1", "src/x.rs", "same");
        code(&mut buf, "t2", "a2", "src/x.rs", "same");
        let rep = merge_code(&buf);
        assert!(!rep.has_conflicts());
        assert_eq!(rep.clean_files(), 1);
    }

    #[test]
    fn test_dedupe_data_records() {
        let mut buf = WorkBuffer::new();
        // 3 record, 2 trùng (sau normalize whitespace/case).
        buf.stage(Artifact::new(
            "t1",
            "a1",
            ArtifactKind::DataRecord,
            "Hello  World",
        ));
        buf.stage(Artifact::new(
            "t2",
            "a2",
            ArtifactKind::DataRecord,
            "hello world",
        ));
        buf.stage(Artifact::new(
            "t3",
            "a3",
            ArtifactKind::DataRecord,
            "Khác biệt",
        ));
        let rep = dedupe_data(&buf);
        assert_eq!(rep.total_input, 3);
        assert_eq!(rep.unique_count(), 2);
        assert_eq!(rep.duplicates_removed, 1);
    }

    #[test]
    fn test_dedupe_strings_helper() {
        let items = vec![
            "  apple ".to_string(),
            "APPLE".to_string(),
            "banana".to_string(),
        ];
        let rep = dedupe_strings(&items);
        assert_eq!(rep.unique_count(), 2);
        assert_eq!(rep.duplicates_removed, 1);
    }
}
