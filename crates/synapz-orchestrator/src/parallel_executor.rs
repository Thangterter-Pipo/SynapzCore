//! parallel_executor — Giai đoạn 1 (Spawning) + Giai đoạn 3 sơ khởi (Barrier Sync).
//!
//! Nhận một TaskGraph đã validate, chạy theo TẦNG:
//!   - Mọi task trong cùng tầng được spawn ĐỒNG THỜI (tokio task) → fan-out.
//!   - Orchestrator đứng ở "barrier": đợi TẤT CẢ task của tầng báo về (Await All)
//!     rồi mới mở tầng kế. Task quá hạn → đánh dấu Timeout, không treo cả hệ.
//!
//! Mỗi task được thực thi bởi một executor closure (inject vào) — tách rời khỏi
//! cách gọi CLI thật để dễ test (test dùng echo executor, production dùng CliInvocation).

use crate::task_graph::{TaskGraph, TaskNode};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

/// Trần ký tự output mỗi dependency được bơm vào prompt downstream (chống phình context).
const MAX_DEP_CHARS: usize = 6000;

/// Kết quả thực thi một task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskOutcome {
    Success(String),
    Failed(String),
    Timeout,
}

impl TaskOutcome {
    pub fn is_success(&self) -> bool {
        matches!(self, TaskOutcome::Success(_))
    }
}

/// Kết quả một task kèm metadata (tầng nào, mất bao lâu).
#[derive(Debug, Clone)]
pub struct TaskResult {
    pub task_id: String,
    pub layer: usize,
    pub outcome: TaskOutcome,
    pub elapsed_ms: u128,
}

/// Báo cáo toàn phiên chạy song song.
#[derive(Debug, Clone, Default)]
pub struct RunReport {
    pub results: Vec<TaskResult>,
    pub total_layers: usize,
}

impl RunReport {
    pub fn succeeded(&self) -> usize {
        self.results
            .iter()
            .filter(|r| r.outcome.is_success())
            .count()
    }
    pub fn failed(&self) -> usize {
        self.results
            .iter()
            .filter(|r| !r.outcome.is_success())
            .count()
    }
    /// Có task nào hỏng (failed/timeout) không?
    pub fn has_failures(&self) -> bool {
        self.failed() > 0
    }
    pub fn get(&self, task_id: &str) -> Option<&TaskResult> {
        self.results.iter().find(|r| r.task_id == task_id)
    }
}

/// Kiểu executor: nhận (task_id, prompt) → future trả output thật.
/// Box-pinned để có thể là async closure bất kỳ (CLI thật, echo, mock...).
pub type Executor = Arc<
    dyn Fn(String, String) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send>>
        + Send
        + Sync,
>;

/// Bộ thực thi song song theo tầng.
pub struct ParallelExecutor {
    executor: Executor,
    /// Timeout cho mỗi task (giây).
    task_timeout: Duration,
    /// Số task tối đa chạy ĐỒNG THỜI (0 = không giới hạn). Bảo vệ RAM máy yếu.
    max_concurrent: usize,
}

impl ParallelExecutor {
    pub fn new(executor: Executor, task_timeout_secs: u64) -> Self {
        Self {
            executor,
            task_timeout: Duration::from_secs(task_timeout_secs),
            max_concurrent: 0, // mặc định không giới hạn (tương thích cũ).
        }
    }

    /// Đặt giới hạn số task chạy đồng thời (semaphore). 0 = không giới hạn.
    pub fn with_max_concurrent(mut self, n: usize) -> Self {
        self.max_concurrent = n;
        self
    }

    /// Chạy toàn đồ thị theo tầng. Trả RunReport tổng hợp.
    /// FAIL-FAST tùy chọn: nếu `stop_on_failure` và một tầng có task hỏng,
    /// dừng không mở tầng kế (vì tầng sau phụ thuộc tầng này).
    pub async fn run(&self, graph: &TaskGraph, stop_on_failure: bool) -> anyhow::Result<RunReport> {
        graph.validate().map_err(|e| anyhow::anyhow!("{}", e))?;
        let layers = graph.layers().map_err(|e| anyhow::anyhow!("{}", e))?;

        let mut report = RunReport {
            results: Vec::new(),
            total_layers: layers.len(),
        };

        // Semaphore giới hạn concurrency toàn cục (None = không giới hạn).
        let sem = if self.max_concurrent > 0 {
            Some(std::sync::Arc::new(tokio::sync::Semaphore::new(
                self.max_concurrent,
            )))
        } else {
            None
        };

        // LUỒNG DỮ LIỆU THEO CẠNH: map task_id → output (chỉ task Success).
        // Trước khi chạy một task, output của các `depends_on` được bơm vào prompt
        // → biến DAG từ "chỉ điều khiển thứ tự" thành "điều phối có dòng chảy dữ liệu".
        let mut outputs: HashMap<String, String> = HashMap::new();

        for (layer_idx, layer) in layers.iter().enumerate() {
            // FAN-OUT: spawn mọi task trong tầng đồng thời.
            let mut handles = Vec::with_capacity(layer.len());
            for node in layer {
                let exec = self.executor.clone();
                let timeout = self.task_timeout;
                let task_id = node.id.clone();
                // Bơm output các dependency (đã xong ở tầng trước) vào prompt.
                let prompt = compose_prompt(node, &outputs);
                let sem = sem.clone();

                handles.push(tokio::spawn(async move {
                    // Acquire permit nếu có semaphore — giữ tới hết task để giới hạn
                    // số task ĐỒNG THỜI (bảo vệ RAM). Permit tự release khi _permit drop.
                    let _permit = match &sem {
                        Some(s) => Some(s.clone().acquire_owned().await.unwrap()),
                        None => None,
                    };
                    let start = std::time::Instant::now();
                    let fut = exec(task_id.clone(), prompt);
                    let outcome = match tokio::time::timeout(timeout, fut).await {
                        Ok(Ok(out)) => TaskOutcome::Success(out),
                        Ok(Err(err)) => TaskOutcome::Failed(err),
                        Err(_) => TaskOutcome::Timeout,
                    };
                    TaskResult {
                        task_id,
                        layer: layer_idx,
                        outcome,
                        elapsed_ms: start.elapsed().as_millis(),
                    }
                }));
            }

            // BARRIER: Await All — đợi mọi task của tầng này về.
            for h in handles {
                match h.await {
                    Ok(res) => report.results.push(res),
                    Err(join_err) => {
                        // panic trong task → ghi nhận là Failed thay vì sập cả hệ.
                        report.results.push(TaskResult {
                            task_id: "<panicked>".into(),
                            layer: layer_idx,
                            outcome: TaskOutcome::Failed(format!("task panic: {}", join_err)),
                            elapsed_ms: 0,
                        });
                    }
                }
            }

            // Ghi output thành công của tầng này vào map để tầng kế dùng làm context.
            for r in report.results.iter().filter(|r| r.layer == layer_idx) {
                if let TaskOutcome::Success(out) = &r.outcome {
                    outputs.insert(r.task_id.clone(), out.clone());
                }
            }

            // Tầng này có hỏng không?
            let layer_failed = report
                .results
                .iter()
                .filter(|r| r.layer == layer_idx)
                .any(|r| !r.outcome.is_success());

            if layer_failed && stop_on_failure {
                break; // không mở tầng phụ thuộc.
            }
        }

        Ok(report)
    }
}

/// Helper: executor "echo" cho test/PoC — trả ngay output giả.
pub fn echo_executor() -> Executor {
    Arc::new(|task_id: String, prompt: String| {
        Box::pin(async move {
            // mô phỏng công việc ngắn
            tokio::time::sleep(Duration::from_millis(10)).await;
            Ok(format!("[{}] done: {}", task_id, prompt))
        })
    })
}

/// Ghép output các dependency vào prompt của task để truyền dữ liệu theo cạnh DAG.
/// Task không có dependency (hoặc dep chưa có output) → trả prompt gốc nguyên vẹn.
fn compose_prompt(node: &TaskNode, outputs: &HashMap<String, String>) -> String {
    let deps: Vec<(&String, &String)> = node
        .depends_on
        .iter()
        .filter_map(|d| outputs.get(d).map(|o| (d, o)))
        .collect();
    if deps.is_empty() {
        return node.prompt.clone();
    }
    let mut s = node.prompt.clone();
    s.push_str("\n\n## Kết quả từ các bước phụ thuộc (dùng làm ngữ cảnh):");
    for (id, out) in deps {
        s.push_str(&format!(
            "\n\n### [{}]\n{}",
            id,
            truncate_chars(out, MAX_DEP_CHARS)
        ));
    }
    s
}

/// Cắt chuỗi theo SỐ KÝ TỰ (an toàn UTF-8, không cắt giữa ký tự tiếng Việt).
fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let head: String = s.chars().take(max_chars).collect();
    format!(
        "{head}…(đã cắt bớt {} ký tự)",
        s.chars().count() - max_chars
    )
}

/// Dùng khi muốn cả đồ thị chạy trên 1 loại agent. Timeout do ParallelExecutor lo,
/// nên ở đây truyền timeout lớn để CliInvocation không cắt sớm hơn barrier.
pub fn cli_executor(inv: crate::runner::CliInvocation) -> Executor {
    Arc::new(move |_task_id: String, prompt: String| {
        let inv = inv.clone();
        Box::pin(async move { inv.run(&prompt, 600).await })
    })
}

/// Executor THẬT theo ROLE: định tuyến task tới CLI phù hợp role.
/// `routing` map role-name → CliInvocation. Task không khớp role → dùng `fallback`.
/// Lưu ý: prompt đã gắn role ở tầng TaskNode; ở đây chỉ chọn binary.
pub fn role_routed_executor(
    routing: std::collections::HashMap<String, crate::runner::CliInvocation>,
    fallback: crate::runner::CliInvocation,
    role_of: std::collections::HashMap<String, String>,
) -> Executor {
    let routing = Arc::new(routing);
    let fallback = Arc::new(fallback);
    let role_of = Arc::new(role_of);
    Arc::new(move |task_id: String, prompt: String| {
        let routing = routing.clone();
        let fallback = fallback.clone();
        let role_of = role_of.clone();
        Box::pin(async move {
            let inv = role_of
                .get(&task_id)
                .and_then(|r| routing.get(r))
                .cloned()
                .unwrap_or_else(|| (*fallback).clone());
            inv.run(&prompt, 600).await
        })
    })
}

/// Xây map task_id → role-name từ TaskGraph (cho role_routed_executor).
/// Task không khai báo role → bỏ qua (sẽ dùng fallback).
pub fn build_role_of(
    graph: &crate::task_graph::TaskGraph,
) -> std::collections::HashMap<String, String> {
    graph
        .nodes
        .iter()
        .filter_map(|n| n.role.as_ref().map(|r| (n.id.clone(), r.to_string())))
        .collect()
}

/// Bảng routing role → CLI mặc định cho hệ thống này (verify thật qua 9router).
/// Coder → Claude Code (code mạnh); Builder → pipo-hermes (dựng/triển khai dự án);
/// Tester → OpenAI Codex (test/exec); Researcher → Hermes (tra cứu).
/// Role khác → None (caller tự lo fallback).
pub fn default_role_routing() -> std::collections::HashMap<String, crate::runner::CliInvocation> {
    let mut m = std::collections::HashMap::new();
    if let Some(inv) = crate::runner::invocation_for("Claude Code") {
        m.insert("Coder".to_string(), inv);
    }
    if let Some(inv) = crate::runner::invocation_for("OpenAI Codex CLI") {
        m.insert("Tester".to_string(), inv);
    }
    // pipo-hermes — builder chính thức: nội ứng (Rust orchestrator) ↔ ngoại hợp (CLI Hermes).
    if let Some(inv) = crate::runner::invocation_for("Hermes Agent") {
        m.insert("Builder".to_string(), inv.clone());
        m.insert("Researcher".to_string(), inv);
    }
    m
}

/// Tiện ích: tóm tắt report ra map task_id → outcome (cho UI/JSON).
pub fn report_to_map(report: &RunReport) -> HashMap<String, String> {
    report
        .results
        .iter()
        .map(|r| {
            let v = match &r.outcome {
                TaskOutcome::Success(o) => format!("ok: {}", o),
                TaskOutcome::Failed(e) => format!("fail: {}", e),
                TaskOutcome::Timeout => "timeout".to_string(),
            };
            (r.task_id.clone(), v)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task_graph::TaskNode;

    fn website_graph() -> TaskGraph {
        let mut g = TaskGraph::new();
        g.add(TaskNode::new("api-login", "code API login"));
        g.add(TaskNode::new("ui-frontend", "design UI"));
        g.add(TaskNode::new("integration", "ghép FE+BE").depends_on(&["api-login", "ui-frontend"]));
        g
    }

    #[tokio::test]
    async fn test_parallel_run_all_success() {
        let exec = ParallelExecutor::new(echo_executor(), 30);
        let report = exec.run(&website_graph(), false).await.unwrap();
        assert_eq!(report.total_layers, 2);
        assert_eq!(report.succeeded(), 3);
        assert_eq!(report.failed(), 0);
        // integration ở tầng 1.
        assert_eq!(report.get("integration").unwrap().layer, 1);
    }

    #[tokio::test]
    async fn test_layer_runs_concurrently() {
        // Mỗi task ngủ 100ms. Tầng 0 có 2 task → nếu CHẠY SONG SONG tổng < 200ms.
        let exec: Executor = Arc::new(|id: String, _p: String| {
            Box::pin(async move {
                tokio::time::sleep(Duration::from_millis(100)).await;
                Ok(format!("{} done", id))
            })
        });
        let mut g = TaskGraph::new();
        g.add(TaskNode::new("a", "A"));
        g.add(TaskNode::new("b", "B")); // a,b cùng tầng 0, độc lập

        let pe = ParallelExecutor::new(exec, 30);
        let start = std::time::Instant::now();
        let report = pe.run(&g, false).await.unwrap();
        let elapsed = start.elapsed().as_millis();

        assert_eq!(report.succeeded(), 2);
        // Song song: ~100ms, KHÔNG phải 200ms. Cho biên 80ms.
        assert!(
            elapsed < 180,
            "kỳ vọng chạy song song <180ms, thực {}ms",
            elapsed
        );
    }

    #[tokio::test]
    async fn test_semaphore_limits_concurrency() {
        // 4 task mỗi cái ngủ 100ms. Với max_concurrent=2 → chạy 2 đợt → tổng >= 200ms
        // (KHÔNG phải ~100ms như chạy hết song song). Chứng minh semaphore giới hạn thật.
        let exec: Executor = Arc::new(|id: String, _p: String| {
            Box::pin(async move {
                tokio::time::sleep(Duration::from_millis(100)).await;
                Ok(format!("{} done", id))
            })
        });
        let mut g = TaskGraph::new();
        for id in ["a", "b", "c", "d"] {
            g.add(TaskNode::new(id, "x")); // 4 task độc lập cùng tầng 0
        }

        let pe = ParallelExecutor::new(exec, 30).with_max_concurrent(2);
        let start = std::time::Instant::now();
        let report = pe.run(&g, false).await.unwrap();
        let elapsed = start.elapsed().as_millis();

        assert_eq!(report.succeeded(), 4);
        // 4 task / 2 đồng thời = 2 đợt × 100ms = ~200ms+. Nếu không giới hạn sẽ ~100ms.
        assert!(
            elapsed >= 190,
            "kỳ vọng >=190ms (2 đợt), thực {}ms",
            elapsed
        );
        assert!(
            elapsed < 350,
            "kỳ vọng <350ms (không tuần tự hết), thực {}ms",
            elapsed
        );
    }

    #[tokio::test]
    async fn test_timeout_marks_task() {
        // Task ngủ 5s nhưng timeout 1s → Timeout.
        let exec: Executor = Arc::new(|id: String, _p: String| {
            Box::pin(async move {
                tokio::time::sleep(Duration::from_secs(5)).await;
                Ok(format!("{} done", id))
            })
        });
        let mut g = TaskGraph::new();
        g.add(TaskNode::new("slow", "task chậm"));

        let pe = ParallelExecutor::new(exec, 1);
        let report = pe.run(&g, false).await.unwrap();
        assert_eq!(report.get("slow").unwrap().outcome, TaskOutcome::Timeout);
        assert!(report.has_failures());
    }

    #[tokio::test]
    async fn test_dependency_output_flows_to_downstream() {
        use tokio::sync::Mutex;
        // Executor ghi lại prompt MÀ NÓ NHẬN, trả output có dấu nhận biết.
        let seen: Arc<Mutex<std::collections::HashMap<String, String>>> =
            Arc::new(Mutex::new(std::collections::HashMap::new()));
        let seen2 = seen.clone();
        let exec: Executor = Arc::new(move |id: String, prompt: String| {
            let seen = seen2.clone();
            Box::pin(async move {
                seen.lock().await.insert(id.clone(), prompt);
                Ok(format!("OUTPUT_OF_{}", id))
            })
        });

        let pe = ParallelExecutor::new(exec, 30);
        let report = pe.run(&website_graph(), false).await.unwrap();
        assert_eq!(report.succeeded(), 3);

        let seen = seen.lock().await;
        // integration (tầng 1) PHẢI nhận output của cả 2 dependency tầng 0.
        let integ = seen.get("integration").unwrap();
        assert!(
            integ.contains("OUTPUT_OF_api-login"),
            "thiếu output api-login: {integ}"
        );
        assert!(
            integ.contains("OUTPUT_OF_ui-frontend"),
            "thiếu output ui-frontend: {integ}"
        );
        assert!(integ.contains("Kết quả từ các bước phụ thuộc"));

        // Task tầng 0 (không dep) KHÔNG bị chèn context thừa — prompt giữ nguyên.
        let login = seen.get("api-login").unwrap();
        assert_eq!(login, "code API login");
    }

    #[tokio::test]
    async fn test_no_deps_prompt_unchanged() {
        use tokio::sync::Mutex;
        let seen: Arc<Mutex<std::collections::HashMap<String, String>>> =
            Arc::new(Mutex::new(std::collections::HashMap::new()));
        let seen2 = seen.clone();
        let exec: Executor = Arc::new(move |id: String, prompt: String| {
            let seen = seen2.clone();
            Box::pin(async move {
                seen.lock().await.insert(id, prompt);
                Ok("ok".to_string())
            })
        });
        let mut g = TaskGraph::new();
        g.add(TaskNode::new("solo", "làm việc một mình"));
        let pe = ParallelExecutor::new(exec, 30);
        pe.run(&g, false).await.unwrap();
        assert_eq!(seen.lock().await.get("solo").unwrap(), "làm việc một mình");
    }

    #[test]
    fn test_builder_role_routes_to_hermes() {
        // pipo-hermes là builder chính thức: Builder → hermes.
        let routing = default_role_routing();
        let builder = routing.get("Builder").expect("phải có route cho Builder");
        assert_eq!(builder.program, "hermes", "Builder phải route tới hermes");
        // Coder vẫn là Claude (không bị đụng).
        assert_eq!(routing.get("Coder").unwrap().program, "claude");
    }

    #[test]
    fn test_build_role_of_maps_builder_task() {
        use crate::roles::AgentRole;
        let mut g = TaskGraph::new();
        g.add(TaskNode::new("scaffold", "dựng khung dự án").with_role(AgentRole::Builder));
        g.add(TaskNode::new("plan", "lên kế hoạch")); // không role
        let role_of = build_role_of(&g);
        assert_eq!(role_of.get("scaffold").map(|s| s.as_str()), Some("Builder"));
        assert!(!role_of.contains_key("plan")); // task không role → fallback
    }

    #[tokio::test]
    async fn test_stop_on_failure_skips_dependent_layer() {
        // Tầng 0 fail → tầng 1 (integration) KHÔNG chạy khi stop_on_failure=true.
        let exec: Executor = Arc::new(|id: String, _p: String| {
            Box::pin(async move {
                if id == "api-login" {
                    Err("compile error".to_string())
                } else {
                    Ok(format!("{} done", id))
                }
            })
        });
        let pe = ParallelExecutor::new(exec, 30);
        let report = pe.run(&website_graph(), true).await.unwrap();
        assert!(report.get("integration").is_none());
        assert!(report.has_failures());
    }
}
