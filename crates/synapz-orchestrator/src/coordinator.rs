//! Coordinator — hub trung tâm. Phát Command qua broadcast tới mọi agent,
//! gom Report qua mpsc. Cập nhật SharedState.

use crate::roles::{Command, Report};
use crate::state::SharedState;
use tokio::sync::{broadcast, mpsc};

pub struct Coordinator {
    pub tx_command: broadcast::Sender<Command>, // phát lệnh
    pub state: SharedState,
}

impl Coordinator {
    /// Tạo coordinator. Trả về (coordinator, rx_report) — rx_report để gom báo cáo.
    pub fn new(
        state: SharedState,
        command_capacity: usize,
        report_capacity: usize,
    ) -> (Self, CoordinatorChannels) {
        let (tx_command, _) = broadcast::channel(command_capacity);
        let (tx_report, rx_report) = mpsc::channel(report_capacity);
        let coord = Self {
            tx_command: tx_command.clone(),
            state,
        };
        (
            coord,
            CoordinatorChannels {
                tx_command,
                tx_report,
                rx_report,
            },
        )
    }

    /// Phát một lệnh xuống tất cả agent.
    pub fn dispatch(&self, cmd: Command) -> anyhow::Result<usize> {
        let n = self
            .tx_command
            .send(cmd)
            .map_err(|e| anyhow::anyhow!("dispatch lỗi: {}", e))?;
        Ok(n)
    }

    /// Vòng lặp gom báo cáo từ agents, cập nhật state.
    pub async fn collect_reports(&self, mut rx_report: mpsc::Receiver<Report>) {
        while let Some(report) = rx_report.recv().await {
            match report {
                Report::Registered { agent_id, role } => {
                    println!("📋 Đăng ký: {} ({})", agent_id, role);
                }
                Report::TaskDone {
                    agent_id,
                    task_id,
                    output,
                } => {
                    let mut st = self.state.write().await;
                    st.tasks_completed += 1;
                    println!("✅ [{}] task {} xong: {}", agent_id, task_id, output);
                }
                Report::TaskError {
                    agent_id,
                    task_id,
                    error,
                } => {
                    let mut st = self.state.write().await;
                    st.tasks_failed += 1;
                    eprintln!("❌ [{}] task {} lỗi: {}", agent_id, task_id, error);
                }
                Report::Pong { agent_id } => {
                    println!("🏓 Pong từ {}", agent_id);
                }
            }
        }
    }

    /// Gom báo cáo + thu kết quả vào Vec JSON (cho server/UI). quiet=true thì không in.
    pub async fn collect_reports_json(
        &self,
        mut rx_report: mpsc::Receiver<Report>,
        sink: std::sync::Arc<tokio::sync::Mutex<Vec<serde_json::Value>>>,
        quiet: bool,
    ) {
        while let Some(report) = rx_report.recv().await {
            match report {
                Report::Registered { agent_id, role } => {
                    if !quiet {
                        println!("📋 Đăng ký: {} ({})", agent_id, role);
                    }
                }
                Report::TaskDone {
                    agent_id,
                    task_id,
                    output,
                } => {
                    {
                        let mut st = self.state.write().await;
                        st.tasks_completed += 1;
                    }
                    if !quiet {
                        println!("✅ [{}] task {} xong: {}", agent_id, task_id, output);
                    }
                    sink.lock().await.push(serde_json::json!({
                        "agent": agent_id, "task": task_id, "ok": true, "output": output,
                    }));
                }
                Report::TaskError {
                    agent_id,
                    task_id,
                    error,
                } => {
                    {
                        let mut st = self.state.write().await;
                        st.tasks_failed += 1;
                    }
                    if !quiet {
                        eprintln!("❌ [{}] task {} lỗi: {}", agent_id, task_id, error);
                    }
                    sink.lock().await.push(serde_json::json!({
                        "agent": agent_id, "task": task_id, "ok": false, "error": error,
                    }));
                }
                Report::Pong { agent_id } => {
                    if !quiet {
                        println!("🏓 Pong từ {}", agent_id);
                    }
                }
            }
        }
    }
}

/// Các kênh phụ trợ trả ra khi tạo Coordinator.
pub struct CoordinatorChannels {
    pub tx_command: broadcast::Sender<Command>,
    pub tx_report: mpsc::Sender<Report>,
    pub rx_report: mpsc::Receiver<Report>,
}
