//! self_correct — Giai đoạn 4: Vòng lặp sửa lỗi song song (Parallel Self-Correcting Loop).
//!
//! Sau khi Fan-In phát hiện lỗi (test fail / merge conflict), orchestrator KHÔNG
//! bắt cả đội dừng. Nó CÔ LẬP vùng lỗi, đóng gói kèm log, đẩy vào RetryQueue cho
//! đúng agent chuyên trách sửa — trong khi các task khác vẫn chạy tiếp.
//!
//! Cơ chế:
//!   - BugReport: gói (task_id, vùng lỗi, log) tách biệt.
//!   - RetryQueue: hàng đợi task cần sửa, có giới hạn số lần thử (max_attempts)
//!     để tránh lặp vô hạn. Hết lượt → đánh dấu GiveUp (báo người).

use crate::parallel_executor::{RunReport, TaskOutcome};
use std::collections::VecDeque;

/// Gói lỗi cô lập của một task, kèm ngữ cảnh để agent sửa.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BugReport {
    pub task_id: String,
    /// Log/thông báo lỗi để agent đọc.
    pub error_log: String,
    /// Đã thử sửa bao nhiêu lần.
    pub attempts: usize,
}

impl BugReport {
    pub fn new(task_id: impl Into<String>, error_log: impl Into<String>) -> Self {
        Self {
            task_id: task_id.into(),
            error_log: error_log.into(),
            attempts: 0,
        }
    }
}

/// Trạng thái một task trong vòng sửa lỗi.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FixStatus {
    /// Còn lượt thử → đưa lại vào hàng đợi.
    Requeued(BugReport),
    /// Hết lượt thử → bỏ cuộc, cần người can thiệp.
    GiveUp(BugReport),
}

/// Hàng đợi retry — cô lập lỗi, giới hạn số lần thử.
#[derive(Debug)]
pub struct RetryQueue {
    queue: VecDeque<BugReport>,
    max_attempts: usize,
}

impl RetryQueue {
    pub fn new(max_attempts: usize) -> Self {
        Self {
            queue: VecDeque::new(),
            max_attempts,
        }
    }

    /// CÔ LẬP LỖI: quét RunReport, gói mọi task hỏng (Failed/Timeout) thành BugReport.
    /// Task chạy tốt KHÔNG bị đụng → không gián đoạn tiến độ.
    pub fn isolate_from_report(report: &RunReport, max_attempts: usize) -> Self {
        let mut q = RetryQueue::new(max_attempts);
        for r in &report.results {
            match &r.outcome {
                TaskOutcome::Failed(e) => q.push(BugReport::new(&r.task_id, e.clone())),
                TaskOutcome::Timeout => q.push(BugReport::new(&r.task_id, "timeout".to_string())),
                TaskOutcome::Success(_) => {}
            }
        }
        q
    }

    pub fn push(&mut self, bug: BugReport) {
        self.queue.push_back(bug);
    }

    /// Lấy bug kế tiếp cần sửa (None nếu rỗng).
    pub fn next_bug(&mut self) -> Option<BugReport> {
        self.queue.pop_front()
    }

    /// Sau một lần thử sửa: nếu vẫn fail và còn lượt → requeue; hết lượt → GiveUp.
    pub fn record_attempt(&mut self, mut bug: BugReport, fixed: bool) -> Option<FixStatus> {
        if fixed {
            return None; // đã sửa xong, ra khỏi vòng.
        }
        bug.attempts += 1;
        if bug.attempts >= self.max_attempts {
            Some(FixStatus::GiveUp(bug))
        } else {
            self.queue.push_back(bug.clone());
            Some(FixStatus::Requeued(bug))
        }
    }

    pub fn pending(&self) -> usize {
        self.queue.len()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// Danh sách task_id đang chờ sửa (cho dashboard).
    pub fn pending_ids(&self) -> Vec<String> {
        self.queue.iter().map(|b| b.task_id.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parallel_executor::{RunReport, TaskOutcome, TaskResult};

    fn result(id: &str, outcome: TaskOutcome) -> TaskResult {
        TaskResult {
            task_id: id.into(),
            layer: 0,
            outcome,
            elapsed_ms: 1,
        }
    }

    #[test]
    fn test_isolate_only_failures() {
        // login OK, payment fail, notify timeout → chỉ 2 task vào queue (login không bị đụng).
        let report = RunReport {
            total_layers: 1,
            results: vec![
                result("login", TaskOutcome::Success("ok".into())),
                result("payment", TaskOutcome::Failed("unit test fail".into())),
                result("notify", TaskOutcome::Timeout),
            ],
        };
        let q = RetryQueue::isolate_from_report(&report, 3);
        assert_eq!(q.pending(), 2);
        let ids = q.pending_ids();
        assert!(ids.contains(&"payment".to_string()));
        assert!(ids.contains(&"notify".to_string()));
        assert!(!ids.contains(&"login".to_string())); // task tốt không bị đụng
    }

    #[test]
    fn test_retry_until_fixed() {
        let mut q = RetryQueue::new(3);
        q.push(BugReport::new("payment", "fail"));
        let bug = q.next_bug().unwrap();
        // Lần 1 chưa sửa được → requeue.
        let st = q.record_attempt(bug, false).unwrap();
        assert!(matches!(st, FixStatus::Requeued(_)));
        assert_eq!(q.pending(), 1);
        // Lấy lại, lần này sửa xong → ra khỏi vòng.
        let bug2 = q.next_bug().unwrap();
        assert_eq!(bug2.attempts, 1);
        assert!(q.record_attempt(bug2, true).is_none());
        assert_eq!(q.pending(), 0);
    }

    #[test]
    fn test_give_up_after_max_attempts() {
        let mut q = RetryQueue::new(2);
        q.push(BugReport::new("hard-bug", "persistent fail"));
        // Lần 1: fail → requeue (attempts=1).
        let b = q.next_bug().unwrap();
        assert!(matches!(
            q.record_attempt(b, false),
            Some(FixStatus::Requeued(_))
        ));
        // Lần 2: fail → attempts=2 >= max → GiveUp.
        let b = q.next_bug().unwrap();
        match q.record_attempt(b, false) {
            Some(FixStatus::GiveUp(bug)) => assert_eq!(bug.attempts, 2),
            other => panic!("kỳ vọng GiveUp, nhận {:?}", other),
        }
        assert_eq!(q.pending(), 0); // không requeue nữa
    }

    #[test]
    fn test_clean_report_empty_queue() {
        let report = RunReport {
            total_layers: 1,
            results: vec![result("a", TaskOutcome::Success("ok".into()))],
        };
        let q = RetryQueue::isolate_from_report(&report, 3);
        assert!(q.is_empty());
    }
}
