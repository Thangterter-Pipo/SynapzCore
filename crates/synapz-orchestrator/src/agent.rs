//! LocalAgent — tác nhân async siêu nhẹ. Nhận Command qua broadcast, gửi Report qua mpsc.

use crate::roles::{AgentManifest, AgentRole, Command, Report};
use crate::runner::CliInvocation;
use tokio::sync::{broadcast, mpsc};

pub struct LocalAgent {
    pub manifest: AgentManifest,
    pub rx_command: broadcast::Receiver<Command>, // Nhận lệnh từ Orchestrator
    pub tx_report: mpsc::Sender<Report>,          // Báo cáo kết quả lên Server
    /// Nếu Some → gọi CLI thật; None → chế độ echo (PoC/test).
    pub invocation: Option<CliInvocation>,
}

impl LocalAgent {
    pub fn new(
        manifest: AgentManifest,
        rx_command: broadcast::Receiver<Command>,
        tx_report: mpsc::Sender<Report>,
    ) -> Self {
        Self {
            manifest,
            rx_command,
            tx_report,
            invocation: None,
        }
    }

    /// Gắn cách gọi CLI thật cho agent.
    pub fn with_invocation(mut self, inv: CliInvocation) -> Self {
        self.invocation = Some(inv);
        self
    }

    pub async fn run(mut self) {
        let id = self.manifest.id.clone();
        println!(
            "🟢 Agent [{}] ({}) đang lắng nghe lệnh...",
            id, self.manifest.current_role
        );

        // Báo đã đăng ký.
        let _ = self
            .tx_report
            .send(Report::Registered {
                agent_id: id.clone(),
                role: self.manifest.current_role.clone(),
            })
            .await;

        loop {
            // Fix lỗi #2: broadcast::recv trả Err(Lagged) khi orchestrator phát nhanh hơn
            // agent xử lý — KHÔNG được thoát loop (agent tự sát). Phải match và bỏ qua.
            match self.rx_command.recv().await {
                Ok(cmd) => {
                    if !self.handle(cmd).await {
                        break; // chỉ thoát khi nhận Shutdown
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    eprintln!("⚠️  Agent [{}] bị trễ {} lệnh, bỏ qua và tiếp tục", id, n);
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => {
                    println!("🔌 Agent [{}] — kênh lệnh đã đóng, thoát.", id);
                    break;
                }
            }
        }
    }

    /// Xử lý một lệnh. Trả false nếu cần dừng agent.
    async fn handle(&self, cmd: Command) -> bool {
        match cmd {
            Command::Ping => {
                let _ = self
                    .tx_report
                    .send(Report::Pong {
                        agent_id: self.manifest.id.clone(),
                    })
                    .await;
                true
            }
            Command::Shutdown => {
                println!("🛑 Agent [{}] nhận Shutdown.", self.manifest.id);
                false
            }
            Command::Assign {
                task_id,
                target_role,
                prompt,
            } => {
                // Chỉ xử lý nếu lệnh nhắm đúng role mình (hoặc broadcast None).
                if let Some(role) = &target_role
                    && *role != self.manifest.current_role
                {
                    return true; // không phải việc của mình
                }
                self.process_task(task_id, prompt).await;
                true
            }
        }
    }

    /// Xử lý task thật. Có invocation → gọi CLI thật; không thì echo (PoC/test).
    async fn process_task(&self, task_id: String, prompt: String) {
        let id = self.manifest.id.clone();
        match &self.invocation {
            Some(inv) => {
                // Gọi CLI thật qua tokio::process, timeout 120s.
                match inv.run(&prompt, 120).await {
                    Ok(output) => {
                        let _ = self
                            .tx_report
                            .send(Report::TaskDone {
                                agent_id: id,
                                task_id,
                                output,
                            })
                            .await;
                    }
                    Err(error) => {
                        let _ = self
                            .tx_report
                            .send(Report::TaskError {
                                agent_id: id,
                                task_id,
                                error,
                            })
                            .await;
                    }
                }
            }
            None => {
                let output = format!(
                    "[{}] role={} model={} (echo) prompt: \"{}\"",
                    id, self.manifest.current_role, self.manifest.model_name, prompt
                );
                let _ = self
                    .tx_report
                    .send(Report::TaskDone {
                        agent_id: id,
                        task_id,
                        output,
                    })
                    .await;
            }
        }
    }
}

/// Helper tạo agent nhanh cho PoC (chế độ echo).
pub fn spawn_agent(
    id: &str,
    model: &str,
    role: AgentRole,
    rx: broadcast::Receiver<Command>,
    tx: mpsc::Sender<Report>,
) -> tokio::task::JoinHandle<()> {
    let agent = LocalAgent::new(AgentManifest::new(id, model, role), rx, tx);
    tokio::spawn(agent.run())
}

/// Spawn agent gọi CLI thật.
pub fn spawn_live_agent(
    id: &str,
    model: &str,
    role: AgentRole,
    inv: CliInvocation,
    rx: broadcast::Receiver<Command>,
    tx: mpsc::Sender<Report>,
) -> tokio::task::JoinHandle<()> {
    let agent = LocalAgent::new(AgentManifest::new(id, model, role), rx, tx).with_invocation(inv);
    tokio::spawn(agent.run())
}
