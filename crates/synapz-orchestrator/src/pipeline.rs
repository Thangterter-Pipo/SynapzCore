//! pipeline — Ráp 4 giai đoạn thành một luồng điều phối song song hoàn chỉnh.
//!
//! GĐ1 Fan-Out: phân tầng TaskGraph → chạy song song theo tầng (ParallelExecutor).
//! GĐ2 Isolation: kết quả mỗi task staging vào WorkBuffer (không ghi thẳng đích).
//! GĐ3 Fan-In: merge code + dedupe data từ buffer (SmartMerge).
//! GĐ4 Self-Correct: cô lập task hỏng vào RetryQueue, thử lại tới max_attempts.
//!
//! Trả PipelineReport tổng hợp + xuất JSON cho dashboard.

use crate::parallel_executor::{Executor, ParallelExecutor, RunReport, TaskOutcome};
use crate::self_correct::{FixStatus, RetryQueue};
use crate::smart_merge;
use crate::task_graph::TaskGraph;
use crate::work_buffer::{Artifact, ArtifactKind, WorkBuffer};
use std::sync::Arc;

/// Cấu hình một lần chạy pipeline.
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    pub task_timeout_secs: u64,
    pub max_fix_attempts: usize,
    /// stop_on_failure cho executor (tầng hỏng không mở tầng phụ thuộc).
    pub stop_on_failure: bool,
    /// Số task tối đa chạy đồng thời (0 = không giới hạn). Bảo vệ RAM máy yếu.
    pub max_concurrent: usize,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            task_timeout_secs: 600,
            max_fix_attempts: 2,
            stop_on_failure: false,
            max_concurrent: 0,
        }
    }
}

/// Báo cáo toàn pipeline.
#[derive(Debug, Clone)]
pub struct PipelineReport {
    pub run: RunReport,
    pub code_clean: usize,
    pub code_conflicts: usize,
    pub data_unique: usize,
    pub data_dupes_removed: usize,
    /// Task hỏng đã sửa được sau retry.
    pub fixed: Vec<String>,
    /// Task bỏ cuộc sau khi hết lượt.
    pub gave_up: Vec<String>,
}

impl PipelineReport {
    pub fn fully_green(&self) -> bool {
        !self.run.has_failures() || (self.gave_up.is_empty() && !self.fixed.is_empty())
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "succeeded": self.run.succeeded(),
            "failed": self.run.failed(),
            "total_layers": self.run.total_layers,
            "code_clean": self.code_clean,
            "code_conflicts": self.code_conflicts,
            "data_unique": self.data_unique,
            "data_dupes_removed": self.data_dupes_removed,
            "fixed": self.fixed,
            "gave_up": self.gave_up,
            "results": self.run.results.iter().map(|r| serde_json::json!({
                "task_id": r.task_id,
                "layer": r.layer,
                "elapsed_ms": r.elapsed_ms,
                "ok": r.outcome.is_success(),
            })).collect::<Vec<_>>(),
        })
    }
}

/// Bộ điều phối pipeline. `executor` chạy task thật; `fixer` thử sửa 1 bug
/// (trả Ok(output) nếu sửa được). Cả hai inject để test bằng mock.
pub struct Pipeline {
    executor: Executor,
    config: PipelineConfig,
}

impl Pipeline {
    pub fn new(executor: Executor, config: PipelineConfig) -> Self {
        Self { executor, config }
    }

    /// Chạy trọn 4 giai đoạn. `buffer` nhận artifact staging (GĐ2),
    /// `fixer` là executor riêng cho vòng sửa lỗi (GĐ4) — thường cùng executor chính.
    pub async fn run(
        &self,
        graph: &TaskGraph,
        buffer: &mut WorkBuffer,
        fixer: Option<Executor>,
    ) -> anyhow::Result<PipelineReport> {
        // ---- GĐ1 Fan-Out: chạy song song theo tầng ----
        let pe = ParallelExecutor::new(self.executor.clone(), self.config.task_timeout_secs)
            .with_max_concurrent(self.config.max_concurrent);
        let run = pe.run(graph, self.config.stop_on_failure).await?;

        // ---- GĐ2 Isolation: staging mọi output thành công vào buffer ----
        for r in &run.results {
            if let TaskOutcome::Success(out) = &r.outcome {
                buffer.stage(Artifact::new(
                    &r.task_id,
                    "agent",
                    // Heuristic nhận code đa ngôn ngữ: Rust(fn/{), Python(def/import/class),
                    // JS/TS(function/const/=>), khối ```code``` → CodeFile, còn lại Report.
                    if looks_like_code(out) {
                        ArtifactKind::CodeFile {
                            path: format!("{}.out", r.task_id),
                        }
                    } else {
                        ArtifactKind::Report
                    },
                    out.clone(),
                ));
            }
        }

        // ---- GĐ3 Fan-In: merge code + dedupe data ----
        let code_rep = smart_merge::merge_code(buffer);
        let data_rep = smart_merge::dedupe_data(buffer);

        // ---- GĐ4 Self-Correct: cô lập + retry task hỏng ----
        let mut queue = RetryQueue::isolate_from_report(&run, self.config.max_fix_attempts);
        let mut gave_up = Vec::new();

        let fix_exec = fixer.unwrap_or_else(|| self.executor.clone());
        while let Some(bug) = queue.next_bug() {
            // Thử sửa: gọi fixer với prompt kèm log lỗi.
            let prompt = format!("Sửa lỗi task '{}'. Log: {}", bug.task_id, bug.error_log);
            let ok = fix_exec(bug.task_id.clone(), prompt).await.is_ok();
            if let Some(FixStatus::GiveUp(b)) = queue.record_attempt(bug, ok) {
                gave_up.push(b.task_id);
            }
        }
        // Số task sửa được = số task hỏng ban đầu - số bỏ cuộc.
        let total_failed = run.failed();
        let fixed_count = total_failed.saturating_sub(gave_up.len());

        Ok(PipelineReport {
            code_clean: code_rep.clean_files(),
            code_conflicts: code_rep.conflicts().len(),
            data_unique: data_rep.unique_count(),
            data_dupes_removed: data_rep.duplicates_removed,
            fixed: (0..fixed_count).map(|i| format!("fixed-{}", i)).collect(),
            gave_up,
            run,
        })
    }
}

/// Heuristic nhận output là code (đa ngôn ngữ) để phân loại artifact.
fn looks_like_code(s: &str) -> bool {
    // Khối markdown ```...``` gần như chắc chắn là code.
    if s.contains("```") {
        return true;
    }
    // Từ khóa đặc trưng các ngôn ngữ phổ biến.
    const MARKERS: &[&str] = &[
        "fn ",
        "func ",
        "def ",
        "class ",
        "import ",
        "function ",
        "const ",
        "let ",
        "var ",
        "public ",
        "private ",
        "#include",
        "package ",
        "=>",
        "println!",
        "print(",
        "console.log",
    ];
    MARKERS.iter().any(|m| s.contains(m)) || s.contains('{')
}

/// Tiện ích: executor luôn-thành-công (cho test pipeline xanh).
pub fn always_ok_executor() -> Executor {
    Arc::new(|task_id: String, _p: String| {
        Box::pin(async move { Ok(format!("fn {}() {{}}", task_id.replace('-', "_"))) })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task_graph::TaskNode;

    fn website_graph() -> TaskGraph {
        let mut g = TaskGraph::new();
        g.add(TaskNode::new("api-login", "code login"));
        g.add(TaskNode::new("api-payment", "code payment"));
        g.add(TaskNode::new("ui-frontend", "design ui"));
        g.add(TaskNode::new("integration", "ghép").depends_on(&[
            "api-login",
            "api-payment",
            "ui-frontend",
        ]));
        g.add(TaskNode::new("e2e-test", "test login → payment").depends_on(&["integration"]));
        g
    }

    #[test]
    fn test_looks_like_code() {
        assert!(looks_like_code("def hello():\n    print('hi')"));
        assert!(looks_like_code("fn main() {}"));
        assert!(looks_like_code("```python\nx=1\n```"));
        assert!(looks_like_code("const x = () => 1"));
        assert!(!looks_like_code("Đây chỉ là báo cáo text bình thường."));
    }

    #[tokio::test]
    async fn test_pipeline_all_green() {
        let pipe = Pipeline::new(always_ok_executor(), PipelineConfig::default());
        let mut buf = WorkBuffer::new();
        let rep = pipe.run(&website_graph(), &mut buf, None).await.unwrap();
        assert_eq!(rep.run.succeeded(), 5);
        assert_eq!(rep.run.failed(), 0);
        assert!(rep.gave_up.is_empty());
        // 5 output "fn ..." → 5 CodeFile staging, distinct path → clean.
        assert_eq!(rep.code_clean, 5);
        assert_eq!(rep.code_conflicts, 0);
    }

    #[tokio::test]
    async fn test_e2e_login_to_payment_runs_after_integration() {
        let pipe = Pipeline::new(always_ok_executor(), PipelineConfig::default());
        let mut buf = WorkBuffer::new();
        let rep = pipe.run(&website_graph(), &mut buf, None).await.unwrap();

        assert_eq!(rep.run.total_layers, 3);
        assert_eq!(rep.run.get("api-login").unwrap().layer, 0);
        assert_eq!(rep.run.get("api-payment").unwrap().layer, 0);
        assert_eq!(rep.run.get("integration").unwrap().layer, 1);
        assert_eq!(rep.run.get("e2e-test").unwrap().layer, 2);
        assert!(rep.fully_green());
    }

    #[tokio::test]
    async fn test_pipeline_isolates_and_fixes_failure() {
        // Executor chính: api-login FAIL. Fixer: luôn sửa được.
        let main_exec: Executor = Arc::new(|id: String, _p: String| {
            Box::pin(async move {
                if id == "api-login" {
                    Err("compile error".to_string())
                } else {
                    Ok(format!("fn {}() {{}}", id.replace('-', "_")))
                }
            })
        });
        let fixer: Executor =
            Arc::new(|_id: String, _p: String| Box::pin(async move { Ok("fixed".to_string()) }));

        let cfg = PipelineConfig {
            stop_on_failure: false,
            ..Default::default()
        };
        let pipe = Pipeline::new(main_exec, cfg);
        let mut buf = WorkBuffer::new();
        let rep = pipe
            .run(&website_graph(), &mut buf, Some(fixer))
            .await
            .unwrap();

        // api-login hỏng nhưng fixer sửa được → không gave_up.
        assert!(rep.gave_up.is_empty(), "fixer luôn ok nên không bỏ cuộc");
        assert_eq!(rep.fixed.len(), 1, "1 task được sửa");
    }

    #[tokio::test]
    async fn test_pipeline_gives_up_unfixable() {
        let main_exec: Executor = Arc::new(|id: String, _p: String| {
            Box::pin(async move {
                if id == "api-login" {
                    Err("boom".into())
                } else {
                    Ok("fn x() {}".into())
                }
            })
        });
        // Fixer luôn fail → hết lượt → gave_up.
        let fixer: Executor =
            Arc::new(|_id: String, _p: String| Box::pin(async move { Err("vẫn lỗi".to_string()) }));
        let cfg = PipelineConfig {
            max_fix_attempts: 2,
            stop_on_failure: false,
            ..Default::default()
        };
        let pipe = Pipeline::new(main_exec, cfg);
        let mut buf = WorkBuffer::new();
        let rep = pipe
            .run(&website_graph(), &mut buf, Some(fixer))
            .await
            .unwrap();
        assert_eq!(rep.gave_up.len(), 1);
        assert_eq!(rep.gave_up[0], "api-login");
    }
}
