//! AgentRunner — nối DetectedAgent → cách gọi CLI thật (headless/print mode).
//! LocalAgent dùng runner này để thực thi task qua tokio::process (async, không block).

use std::collections::HashMap;

/// Cách gọi một CLI agent ở chế độ không tương tác.
#[derive(Debug, Clone)]
pub struct CliInvocation {
    pub program: String,   // binary gọi (claude, hermes, gemini...)
    pub args: Vec<String>, // args cố định; phần tử "{prompt}" sẽ thay bằng prompt thật
}

impl CliInvocation {
    pub fn new(program: &str, args: &[&str]) -> Self {
        Self {
            program: program.to_string(),
            args: args.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// Dựng args cuối cùng, thay {prompt}.
    fn build_args(&self, prompt: &str) -> Vec<String> {
        self.args
            .iter()
            .map(|a| {
                if a == "{prompt}" {
                    prompt.to_string()
                } else {
                    a.clone()
                }
            })
            .collect()
    }

    /// Gọi CLI thật, trả stdout (hoặc Err với stderr). Có timeout chống treo.
    /// Windows: spawn qua `cmd /C` vì Rust Command::new KHÔNG resolve .cmd/.ps1/PATHEXT.
    pub async fn run(&self, prompt: &str, timeout_secs: u64) -> Result<String, String> {
        let args = self.build_args(prompt);
        let mut cmd = build_command(&self.program, &args);
        let fut = cmd.output();
        let out =
            match tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), fut).await {
                Ok(Ok(o)) => o,
                Ok(Err(e)) => return Err(format!("spawn lỗi {}: {}", self.program, e)),
                Err(_) => {
                    return Err(format!(
                        "timeout {}s khi gọi {}",
                        timeout_secs, self.program
                    ));
                }
            };
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
        }
    }
}

/// Dựng Command đúng theo OS. Windows phải qua `cmd /C` để resolve .cmd/.ps1.
fn build_command(program: &str, args: &[String]) -> tokio::process::Command {
    if cfg!(windows) {
        let mut c = tokio::process::Command::new("cmd");
        c.arg("/C").arg(program);
        for a in args {
            c.arg(a);
        }
        c
    } else {
        let mut c = tokio::process::Command::new(program);
        c.args(args);
        c
    }
}

/// Map tên agent (như scanner trả) → cách gọi CLI. Chỉ map những cái VERIFY THẬT
/// gọi được headless và trả lời (qua 9router localhost:20128).
pub fn invocation_for(agent_name: &str) -> Option<CliInvocation> {
    let mut m: HashMap<&str, CliInvocation> = HashMap::new();
    // claude --model opus -p "prompt" : print mode. PHẢI có --model (config mặc định lỗi model).
    m.insert(
        "Claude Code",
        CliInvocation::new("claude", &["--model", "opus", "-p", "{prompt}"]),
    );
    // codex: gọi qua `node <codex.js> exec`. Đường dẫn codex.js resolve ĐỘNG (không hardcode máy).
    let codex = match resolve_codex_js() {
        Some(path) => CliInvocation::new("node", &[path.as_str(), "exec", "{prompt}"]),
        // Fallback cuối: gọi thẳng `codex exec` (build_command sẽ wrap cmd /C trên Windows).
        None => CliInvocation::new("codex", &["exec", "{prompt}"]),
    };
    m.insert("OpenAI Codex CLI", codex);
    // hermes -p default -z "task" (chính nhà mình — verify trả "OK").
    m.insert(
        "Hermes Agent",
        CliInvocation::new("hermes", &["-p", "default", "-z", "{prompt}"]),
    );
    // NOTE: Gemini (treo chờ env GEMINI_API_KEY trong subprocess) + OpenCode (opencode.exe
    //       treo chờ tương tác/auth) — TẠM out tới khi có cách gọi headless ổn định.
    m.remove(agent_name)
}

/// Tìm đường dẫn codex.js mà KHÔNG hardcode máy cụ thể.
/// Thứ tự: env CODEX_JS → %APPDATA%/npm (Windows) → $HOME/.npm-global & /usr/lib (Unix).
fn resolve_codex_js() -> Option<String> {
    // 1) Override tường minh.
    if let Ok(p) = std::env::var("CODEX_JS")
        && !p.is_empty()
        && std::path::Path::new(&p).exists()
    {
        return Some(p);
    }
    const SUFFIX: &str = "node_modules/@openai/codex/bin/codex.js";
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    // 2) Windows: %APPDATA%\npm\node_modules\...
    if let Ok(appdata) = std::env::var("APPDATA") {
        candidates.push(std::path::Path::new(&appdata).join("npm").join(SUFFIX));
    }
    // 3) Unix: npm global prefix phổ biến.
    if let Ok(home) = std::env::var("HOME") {
        let home = std::path::Path::new(&home);
        candidates.push(home.join(".npm-global/lib").join(SUFFIX));
        candidates.push(home.join(".local/lib").join(SUFFIX));
    }
    candidates.push(std::path::Path::new("/usr/lib").join(SUFFIX));
    candidates.push(std::path::Path::new("/usr/local/lib").join(SUFFIX));

    candidates
        .into_iter()
        .find(|p| p.exists())
        .map(|p| p.to_string_lossy().into_owned())
}
