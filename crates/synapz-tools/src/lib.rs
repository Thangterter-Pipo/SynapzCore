//! # agt-tools — Tool Registry & Built-in Tools for Antigravity
//!
//! 14 tools: file (6) + shell (1) + web (2) + memory (5)
//! Plus goals manager, reflection logger, and CDP autonomous controller.

pub mod cdp_controller;
pub mod file;
pub mod goals;
pub mod memory;
pub mod reflection;
pub mod shell;
pub mod web;

use anyhow::Result;
use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

/// Tool function signature (async).
pub type ToolFn =
    Box<dyn Fn(Value) -> Pin<Box<dyn Future<Output = Result<Value>> + Send>> + Send + Sync>;

/// A registered tool.
pub struct Tool {
    pub name: String,
    pub description: String,
    pub handler: ToolFn,
}

/// Central tool registry.
pub struct Registry {
    tools: HashMap<String, Tool>,
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

impl Registry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Register a tool.
    pub fn register<F, Fut>(&mut self, name: &str, description: &str, handler: F)
    where
        F: Fn(Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Value>> + Send + 'static,
    {
        let name_str = name.to_string();
        self.tools.insert(
            name_str.clone(),
            Tool {
                name: name_str,
                description: description.to_string(),
                handler: Box::new(move |params| Box::pin(handler(params))),
            },
        );
    }

    /// Execute a tool by name.
    pub async fn execute(&self, name: &str, params: Value) -> Result<Value> {
        let tool = self.tools.get(name).ok_or_else(|| {
            anyhow::anyhow!(
                "Tool '{}' not found. Available: {:?}",
                name,
                self.list_names()
            )
        })?;
        (tool.handler)(params).await
    }

    /// List all tool names.
    pub fn list_names(&self) -> Vec<&str> {
        self.tools.keys().map(|s| s.as_str()).collect()
    }

    /// List all tools with descriptions.
    pub fn list(&self) -> Vec<(&str, &str)> {
        self.tools
            .values()
            .map(|t| (t.name.as_str(), t.description.as_str()))
            .collect()
    }

    /// Number of registered tools.
    pub fn count(&self) -> usize {
        self.tools.len()
    }
}

/// Build a registry with all 14 built-in tools pre-registered.
pub fn build_default_registry() -> Registry {
    let mut r = Registry::new();

    // File tools (6)
    r.register("read_file", "Đọc nội dung file", file::read_file);
    r.register("write_file", "Ghi nội dung vào file", file::write_file);
    r.register(
        "append_file",
        "Thêm nội dung vào cuối file",
        file::append_file,
    );
    r.register("list_dir", "Liệt kê nội dung thư mục", file::list_dir);
    r.register(
        "search_files",
        "Tìm file theo glob pattern",
        file::search_files,
    );
    r.register("file_exists", "Kiểm tra file có tồn tại", file::file_exists);

    // Shell tools (1)
    r.register(
        "run_command",
        "Chạy lệnh shell (có blocklist)",
        shell::run_command,
    );

    // Web tools (2)
    r.register("http_get", "Gửi HTTP GET request", web::http_get);
    r.register("http_post", "Gửi HTTP POST request", web::http_post);

    // Memory tools (5) — shared team memory via Supabase
    r.register(
        "remember",
        "Tìm ký ức (semantic search, filter by agent)",
        memory::remember,
    );
    r.register(
        "save_memory",
        "Lưu ký ức mới (with agent/category/importance)",
        memory::save_memory,
    );
    r.register(
        "recall_boss",
        "Lấy profile Bố từ memory",
        memory::recall_boss,
    );
    r.register(
        "recall_team",
        "Lấy team memories (importance >= 3)",
        memory::recall_team,
    );
    r.register(
        "search_code",
        "Tìm code trong codebase",
        memory::search_code,
    );

    r
}

pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

pub fn factorial(n: u64) -> u64 {
    (1..=n).product()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_has_14_tools() {
        let r = build_default_registry();
        assert_eq!(
            r.count(),
            14,
            "Registry should have 14 function-based tools"
        );
    }

    #[test]
    fn test_add() {
        assert_eq!(add(5, 10), 15); // kiểm dương
        assert_eq!(add(-5, -10), -15); // kiểm âm
        assert_eq!(add(0, 0), 0); // kiểm zero
    }

    #[test]
    fn test_factorial() {
        assert_eq!(factorial(0), 1);
        assert_eq!(factorial(1), 1);
        assert_eq!(factorial(5), 120);
        assert_eq!(factorial(10), 3628800);
    }
}
