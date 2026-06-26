//! Vai trò agent, manifest, và message enums (typed — không truyền JSON string trần).

use serde::{Deserialize, Serialize};

/// Vai trò của một agent trong hệ thống.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentRole {
    Orchestrator, // AI Tối cao — phát lệnh
    Coder,
    Builder, // pipo-hermes — tay dựng dự án (build/scaffold/triển khai)
    Tester,
    Researcher,
    Unassigned, // Mới đăng ký, chờ xếp việc
}

impl std::fmt::Display for AgentRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            AgentRole::Orchestrator => "Orchestrator",
            AgentRole::Coder => "Coder",
            AgentRole::Builder => "Builder",
            AgentRole::Tester => "Tester",
            AgentRole::Researcher => "Researcher",
            AgentRole::Unassigned => "Unassigned",
        };
        write!(f, "{}", s)
    }
}

/// Trạng thái phát hiện một AI agent trên máy (như UI trong ảnh).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DetectStatus {
    Connected,     // binary CÓ + config/auth CÓ → sẵn sàng nối cứng
    NotConfigured, // binary CÓ nhưng chưa auth/config
    NotInstalled,  // không tìm thấy binary
    Unknown,       // có dấu hiệu mơ hồ, không xác định chắc
}

impl std::fmt::Display for DetectStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            DetectStatus::Connected => "Connected",
            DetectStatus::NotConfigured => "Not configured",
            DetectStatus::NotInstalled => "Not installed",
            DetectStatus::Unknown => "Unknown",
        };
        write!(f, "{}", s)
    }
}

/// Một AI agent được phát hiện trên hệ thống.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedAgent {
    pub name: String, // "Claude Code", "OpenAI Codex CLI"...
    pub status: DetectStatus,
    pub binary_path: Option<String>,
    pub version: Option<String>,
    pub config_path: Option<String>,
}

#[cfg(test)]
mod runner_tests {
    use crate::runner::CliInvocation;

    /// Verify thật: build_command qua `cmd /C` resolve được .cmd/builtin trên Windows.
    /// Gọi `cmd /C echo` (luôn có) → chứng minh spawn không còn "program not found".
    #[tokio::test]
    async fn test_cli_invocation_runs_via_cmd() {
        let inv = CliInvocation::new("echo", &["{prompt}"]);
        let out = inv.run("HELLO_SYNAPZ", 15).await;
        assert!(out.is_ok(), "spawn lỗi: {:?}", out);
        assert!(out.unwrap().contains("HELLO_SYNAPZ"));
    }
}

/// Bản kê khai năng lực của một agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentManifest {
    pub id: String,
    pub model_name: String,
    pub current_role: AgentRole,
    pub local_tools: Vec<String>,
}

impl AgentManifest {
    pub fn new(id: impl Into<String>, model_name: impl Into<String>, role: AgentRole) -> Self {
        Self {
            id: id.into(),
            model_name: model_name.into(),
            current_role: role,
            local_tools: Vec::new(),
        }
    }
}

/// Lệnh phát từ Orchestrator xuống agents (typed, fix lỗi #3 — không dùng String thô).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Command {
    /// Giao một task cho agent có role cụ thể (None = broadcast mọi agent).
    Assign {
        task_id: String,
        target_role: Option<AgentRole>,
        prompt: String,
    },
    /// Yêu cầu agent báo cáo trạng thái.
    Ping,
    /// Yêu cầu toàn hệ thống dừng (graceful shutdown).
    Shutdown,
}

/// Báo cáo từ agent gửi ngược lên Orchestrator/Server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Report {
    /// Agent đã đăng ký vào hệ thống.
    Registered { agent_id: String, role: AgentRole },
    /// Kết quả xử lý một task.
    TaskDone {
        agent_id: String,
        task_id: String,
        output: String,
    },
    /// Lỗi khi xử lý.
    TaskError {
        agent_id: String,
        task_id: String,
        error: String,
    },
    /// Trả lời Ping.
    Pong { agent_id: String },
}
