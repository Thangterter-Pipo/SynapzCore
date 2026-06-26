//! AI Agent Detector — quét toàn hệ thống tìm các AI coding agent đang có
//! (Claude Code, Codex, Cursor, Hermes, Cline, Gemini, OpenCode...), phân loại
//! trạng thái Connected / Not configured / Not installed / Unknown, để nối cứng
//! vào SystemState. Dùng tokio::process (async, không block runtime).

use crate::roles::{DetectStatus, DetectedAgent};

/// Một định nghĩa agent cần dò.
struct AgentSpec {
    name: &'static str,
    /// Các tên binary để thử `which` (ưu tiên đầu list).
    binaries: &'static [&'static str],
    /// Đường con trong $HOME để coi là "đã configured" (vd ".claude/.codex").
    /// Tồn tại 1 trong số này + binary CÓ → Connected.
    config_hints: &'static [&'static str],
    /// File auth/config cụ thể (relative $HOME) — có file này = chắc chắn configured.
    auth_files: &'static [&'static str],
    /// Tên package npm global (relative npm node_modules). Có package này =
    /// coi như binary CÓ (fallback khi `where` trượt vì exe là .ps1/.js).
    npm_packages: &'static [&'static str],
}

/// Danh mục agent dò — khớp UI trong ảnh bố gửi.
const SPECS: &[AgentSpec] = &[
    AgentSpec {
        name: "Claude Code",
        binaries: &["claude"],
        config_hints: &[".claude"],
        auth_files: &[".claude.json", ".claude/settings.json"],
        npm_packages: &["@anthropic-ai/claude-code"],
    },
    AgentSpec {
        name: "OpenAI Codex CLI",
        binaries: &["codex"],
        config_hints: &[".codex"],
        auth_files: &[".codex/auth.json"],
        npm_packages: &["@openai/codex"],
    },
    AgentSpec {
        name: "Cursor",
        binaries: &["cursor"],
        config_hints: &[".cursor"],
        auth_files: &[],
        npm_packages: &[],
    },
    AgentSpec {
        name: "Cline",
        binaries: &["cline"],
        config_hints: &[],
        auth_files: &[],
        npm_packages: &["cline"],
    },
    AgentSpec {
        name: "Continue",
        binaries: &["continue", "cn"],
        config_hints: &[".continue"],
        auth_files: &[],
        npm_packages: &[],
    },
    AgentSpec {
        name: "Gemini CLI",
        binaries: &["gemini"],
        config_hints: &[".gemini"],
        auth_files: &[".gemini/settings.json"],
        npm_packages: &["@google/gemini-cli"],
    },
    AgentSpec {
        name: "OpenCode",
        binaries: &["opencode"],
        config_hints: &[".opencode", ".config/opencode"],
        auth_files: &[],
        npm_packages: &["opencode-windows-x64", "opencode-ai"],
    },
    AgentSpec {
        name: "Hermes Agent",
        binaries: &["hermes"],
        config_hints: &[".hermes", "AppData/Local/hermes"],
        auth_files: &[],
        npm_packages: &[],
    },
    AgentSpec {
        name: "Qwen Code",
        binaries: &["qwen"],
        config_hints: &[".qwen"],
        auth_files: &[],
        npm_packages: &["@qwen-code/qwen-code"],
    },
    AgentSpec {
        name: "Aider",
        binaries: &["aider"],
        config_hints: &[".aider"],
        auth_files: &[],
        npm_packages: &[],
    },
    AgentSpec {
        name: "Factory Droid",
        binaries: &["droid"],
        config_hints: &[".factory"],
        auth_files: &[],
        npm_packages: &[],
    },
    AgentSpec {
        name: "Kilo Code",
        binaries: &["kilo"],
        config_hints: &[],
        auth_files: &[],
        npm_packages: &[],
    },
    AgentSpec {
        name: "Amp CLI",
        binaries: &["amp"],
        config_hints: &[],
        auth_files: &[],
        npm_packages: &["@sourcegraph/amp"],
    },
    AgentSpec {
        name: "Roo",
        binaries: &["roo"],
        config_hints: &[],
        auth_files: &[],
        npm_packages: &[],
    },
    AgentSpec {
        name: "DeepSeek TUI",
        binaries: &["deepseek"],
        config_hints: &[],
        auth_files: &[],
        npm_packages: &[],
    },
];

/// Quét toàn bộ — trả danh sách agent đã dò (mọi trạng thái, như UI).
pub async fn scan_ai_agents() -> Vec<DetectedAgent> {
    let home = home_dir();
    let mut found = Vec::with_capacity(SPECS.len());
    for spec in SPECS {
        found.push(detect_one(spec, &home).await);
    }
    found
}

/// Chỉ lấy các agent Connected (sẵn sàng nối cứng vào hệ thống).
#[allow(dead_code)]
pub async fn scan_connected_agents() -> Vec<DetectedAgent> {
    scan_ai_agents()
        .await
        .into_iter()
        .filter(|a| a.status == DetectStatus::Connected)
        .collect()
}

async fn detect_one(spec: &AgentSpec, home: &str) -> DetectedAgent {
    // 1. Tìm binary qua PATH.
    let mut binary_path = None;
    for bin in spec.binaries {
        if let Some(p) = which(bin).await {
            binary_path = Some(p);
            break;
        }
    }

    // 1b. Fallback: package npm global tồn tại → coi như binary CÓ
    //     (fix Codex/OpenCode: exe là .ps1/.js nên `where` trượt).
    let mut via_npm = false;
    if binary_path.is_none() {
        for pkg in spec.npm_packages {
            if let Some(p) = npm_package_path(pkg) {
                binary_path = Some(p);
                via_npm = true;
                break;
            }
        }
    }

    // 2. Tìm config/auth.
    let auth_present = spec.auth_files.iter().any(|f| path_exists(&join(home, f)));
    let mut config_path = None;
    for hint in spec.config_hints {
        let full = join(home, hint);
        if path_exists(&full) {
            config_path = Some(full);
            break;
        }
    }

    // 3. Lấy version (chỉ khi binary trong PATH thật — npm-only thì bỏ qua, tránh treo).
    let version = if binary_path.is_some() && !via_npm {
        get_version(spec.binaries[0]).await
    } else {
        None
    };

    // 4. Phân loại trạng thái.
    let status = match (&binary_path, auth_present, &config_path) {
        (Some(_), true, _) => DetectStatus::Connected, // binary + auth file rõ ràng
        (Some(_), false, Some(_)) => DetectStatus::Connected, // binary + có config dir
        (Some(_), false, None) => DetectStatus::NotConfigured, // binary nhưng chưa config
        (None, _, Some(_)) => DetectStatus::Unknown,   // có config nhưng ko thấy binary (PATH?)
        (None, _, None) => DetectStatus::NotInstalled,
    };

    DetectedAgent {
        name: spec.name.to_string(),
        status,
        binary_path,
        version,
        config_path,
    }
}

/// `which <bin>` async — trả full path nếu có.
async fn which(bin: &str) -> Option<String> {
    // Windows: where; Unix: which. Thử cả hai cho chắc trên MSYS.
    for finder in ["where", "which"] {
        if let Ok(out) = tokio::process::Command::new(finder).arg(bin).output().await
            && out.status.success()
        {
            let s = String::from_utf8_lossy(&out.stdout);
            if let Some(first) = s.lines().next() {
                let p = first.trim();
                if !p.is_empty() {
                    return Some(p.to_string());
                }
            }
        }
    }
    None
}

/// Tìm package npm global trong node_modules — trả path nếu tồn tại.
/// Fix Codex/OpenCode: exe là .ps1/.js nên `where` trượt nhưng package vẫn cài.
fn npm_package_path(pkg: &str) -> Option<String> {
    let home = home_dir();
    // Các vị trí npm global node_modules phổ biến trên Windows + Unix.
    let roots = [
        join(&home, "AppData/Roaming/npm/node_modules"),
        "/usr/local/lib/node_modules".to_string(),
        "/usr/lib/node_modules".to_string(),
    ];
    for root in &roots {
        let full = format!("{}/{}", root, pkg);
        if path_exists(&full) {
            return Some(full);
        }
    }
    None
}

/// Lấy version: `<bin> --version`, cắt dòng đầu.
async fn get_version(bin: &str) -> Option<String> {
    let out = tokio::process::Command::new(bin)
        .arg("--version")
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout);
    s.lines()
        .next()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
}

fn home_dir() -> String {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string())
}

fn join(base: &str, sub: &str) -> String {
    format!("{}/{}", base.trim_end_matches(['/', '\\']), sub)
}

fn path_exists(p: &str) -> bool {
    std::path::Path::new(p).exists()
}

/// Giữ tương thích: capability scan cũ (python/git/cargo...).
pub async fn scan_local_environment() -> Vec<String> {
    let checks = [
        ("python_compiler", "python", "--version"),
        ("git_vcs", "git", "--version"),
        ("rust_compiler", "cargo", "--version"),
        ("node_runtime", "node", "--version"),
        ("docker", "docker", "--version"),
    ];
    let mut caps = Vec::new();
    for (cap, bin, arg) in checks {
        if let Ok(out) = tokio::process::Command::new(bin).arg(arg).output().await
            && out.status.success()
        {
            caps.push(cap.to_string());
        }
    }
    caps
}
