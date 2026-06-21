//! Shell tools — run commands with blocklist protection.

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::process::Command;

const BLOCKED: &[&str] = &[
    "rm -rf /", "rm -rf /*", "format", "del /f /q",
    "shutdown", "restart", "mkfs", "dd if=",
];

fn is_blocked(cmd: &str) -> bool {
    let lower = cmd.to_lowercase();
    BLOCKED.iter().any(|b| lower.contains(b))
}

pub async fn run_command(params: Value) -> Result<Value> {
    let command = params.get("command").and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'command'"))?;
    let cwd = params.get("cwd").and_then(|v| v.as_str());
    let _timeout = params.get("timeout").and_then(|v| v.as_u64()).unwrap_or(30);

    if is_blocked(command) {
        return Ok(json!({
            "exit_code": -1,
            "stdout": "",
            "stderr": format!("❌ BLOCKED: '{command}' is in blocklist"),
        }));
    }

    let mut cmd = if cfg!(target_os = "windows") {
        let mut c = Command::new("cmd");
        c.args(["/C", command]);
        c
    } else {
        let mut c = Command::new("sh");
        c.args(["-c", command]);
        c
    };

    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }

    match cmd.output() {
        Ok(output) => Ok(json!({
            "exit_code": output.status.code().unwrap_or(-1),
            "stdout": String::from_utf8_lossy(&output.stdout),
            "stderr": String::from_utf8_lossy(&output.stderr),
        })),
        Err(e) => Ok(json!({
            "exit_code": -1,
            "stdout": "",
            "stderr": format!("❌ Error: {e}"),
        })),
    }
}
