//! CDP Controller — Autonomous IDE control via Chrome DevTools Protocol.
//!
//! Connects to Antigravity IDE workbench (launched with --remote-debugging-port=9333)
//! and provides programmatic control: inject prompts, monitor responses,
//! switch models, auto-accept edits.

use anyhow::{anyhow, Result};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio_tungstenite::tungstenite::Message;

/// CDP connection state
pub struct CdpController {
    ws_url: String,
    command_id: u64,
}

/// CDP command
#[derive(Serialize)]
struct CdpCommand {
    id: u64,
    method: String,
    params: serde_json::Value,
}

/// CDP response
#[derive(Deserialize, Debug)]
struct CdpResponse {
    id: Option<u64>,
    result: Option<serde_json::Value>,
    error: Option<serde_json::Value>,
}

/// Debuggable page info from /json endpoint
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DebugPage {
    pub id: String,
    pub title: String,
    #[serde(rename = "type")]
    pub page_type: String,
    pub url: String,
    pub web_socket_debugger_url: Option<String>,
}

impl CdpController {
    /// Discover Antigravity workbench page and get its WebSocket URL.
    /// Requires IDE launched with --remote-debugging-port=9333
    pub async fn discover(port: u16) -> Result<Self> {
        let json_url = format!("http://127.0.0.1:{port}/json");
        let resp = reqwest::get(&json_url).await
            .map_err(|e| anyhow!("Cannot connect to CDP at port {port}: {e}.\nMake sure Antigravity is launched with --remote-debugging-port={port}"))?;

        let pages: Vec<DebugPage> = resp.json().await
            .map_err(|e| anyhow!("Invalid CDP response: {e}"))?;

        // Find the workbench page (main IDE window)
        let workbench = pages.iter()
            .find(|p| p.url.contains("workbench.html") || p.title.contains("Antigravity"))
            .or_else(|| pages.iter().find(|p| p.page_type == "page"))
            .ok_or_else(|| anyhow!("No workbench page found. Pages: {:?}", pages.iter().map(|p| &p.title).collect::<Vec<_>>()))?;

        let ws_url = workbench.web_socket_debugger_url.as_ref()
            .ok_or_else(|| anyhow!("Workbench page has no WebSocket URL"))?;

        eprintln!("✅ Found workbench: '{}' at {}", workbench.title, ws_url);

        Ok(Self {
            ws_url: ws_url.clone(),
            command_id: 0,
        })
    }

    /// Execute a JavaScript expression in the workbench page context.
    pub async fn evaluate_js(&mut self, expression: &str) -> Result<serde_json::Value> {
        self.command_id += 1;
        let cmd = CdpCommand {
            id: self.command_id,
            method: "Runtime.evaluate".to_string(),
            params: serde_json::json!({
                "expression": expression,
                "returnByValue": true,
                "awaitPromise": true,
            }),
        };

        let (mut ws_stream, _) = tokio_tungstenite::connect_async(&self.ws_url).await
            .map_err(|e| anyhow!("WebSocket connect failed: {e}"))?;

        let cmd_json = serde_json::to_string(&cmd)?;
        ws_stream.send(Message::Text(cmd_json)).await
            .map_err(|e| anyhow!("WebSocket send failed: {e}"))?;

        // Wait for response with matching id
        let timeout = tokio::time::Duration::from_secs(30);
        let deadline = tokio::time::Instant::now() + timeout;

        while tokio::time::Instant::now() < deadline {
            match tokio::time::timeout(timeout, ws_stream.next()).await {
                Ok(Some(Ok(Message::Text(text)))) => {
                    if let Ok(resp) = serde_json::from_str::<CdpResponse>(&text) {
                        if resp.id == Some(self.command_id) {
                            if let Some(error) = resp.error {
                                return Err(anyhow!("CDP error: {error}"));
                            }
                            return Ok(resp.result.unwrap_or(serde_json::Value::Null));
                        }
                    }
                }
                Ok(Some(Err(e))) => return Err(anyhow!("WebSocket error: {e}")),
                Ok(None) => return Err(anyhow!("WebSocket closed")),
                Err(_) => return Err(anyhow!("CDP response timeout (30s)")),
                _ => continue,
            }
        }

        Err(anyhow!("CDP response timeout"))
    }

    /// Inject a prompt into the IDE chat input and submit it.
    pub async fn inject_prompt(&mut self, prompt: &str) -> Result<()> {
        let escaped = prompt.replace('\\', "\\\\").replace('\'', "\\'").replace('\n', "\\n");

        // Find the chat input textarea and set its value
        let js = format!(r#"
            (function() {{
                // Try multiple selectors for the chat input
                var selectors = [
                    'textarea[data-placeholder]',
                    'textarea.chat-input',
                    '.chat-input textarea',
                    'div[contenteditable="true"]',
                    'textarea'
                ];
                for (var i = 0; i < selectors.length; i++) {{
                    var el = document.querySelector(selectors[i]);
                    if (el) {{
                        // Set value
                        var nativeSet = Object.getOwnPropertyDescriptor(
                            window.HTMLTextAreaElement.prototype, 'value'
                        ).set;
                        nativeSet.call(el, '{escaped}');
                        el.dispatchEvent(new Event('input', {{ bubbles: true }}));
                        el.dispatchEvent(new Event('change', {{ bubbles: true }}));

                        // Click send button
                        setTimeout(function() {{
                            var btns = document.querySelectorAll('button');
                            for (var j = 0; j < btns.length; j++) {{
                                var label = btns[j].getAttribute('aria-label') || '';
                                if (label.toLowerCase().includes('send') ||
                                    btns[j].querySelector('svg[data-icon="send"]')) {{
                                    btns[j].click();
                                    break;
                                }}
                            }}
                        }}, 100);
                        return 'injected';
                    }}
                }}
                return 'no_input_found';
            }})()
        "#);

        let result = self.evaluate_js(&js).await?;
        eprintln!("🚀 Prompt injection: {:?}", result);
        Ok(())
    }

    /// Monitor for AI response completion.
    /// Returns when the AI stops generating (no more streaming indicators).
    pub async fn wait_for_response(&mut self, timeout_secs: u64) -> Result<String> {
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(timeout_secs);
        let mut last_text = String::new();
        let mut stable_count = 0;

        while tokio::time::Instant::now() < deadline {
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

            // Check if still generating
            let is_generating = self.evaluate_js(r#"
                (function() {
                    // Check for spinning/loading indicators
                    var spinner = document.querySelector('.generating, .streaming, [data-state="generating"]');
                    return !!spinner;
                })()
            "#).await.unwrap_or(serde_json::Value::Bool(false));

            if !is_generating.as_bool().unwrap_or(true) {
                // Get the last response text
                let response = self.evaluate_js(r#"
                    (function() {
                        var msgs = document.querySelectorAll('.message.assistant, [data-role="assistant"]');
                        if (msgs.length > 0) {
                            return msgs[msgs.length - 1].innerText || '';
                        }
                        return '';
                    })()
                "#).await?;

                let text = response.get("value")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                if text == last_text && !text.is_empty() {
                    stable_count += 1;
                    if stable_count >= 2 {
                        return Ok(text);
                    }
                } else {
                    last_text = text;
                    stable_count = 0;
                }
            }
        }

        Err(anyhow!("Response timeout after {timeout_secs}s"))
    }

    /// Auto-accept file edits proposed by the AI.
    pub async fn auto_accept_edits(&mut self) -> Result<u32> {
        let js = r#"
            (function() {
                var accepted = 0;
                // Find accept buttons for file edits
                var btns = document.querySelectorAll('button');
                for (var i = 0; i < btns.length; i++) {
                    var text = btns[i].innerText.toLowerCase();
                    var label = (btns[i].getAttribute('aria-label') || '').toLowerCase();
                    if (text.includes('accept') || text.includes('apply') ||
                        label.includes('accept') || label.includes('apply')) {
                        btns[i].click();
                        accepted++;
                    }
                }
                return accepted;
            })()
        "#;

        let result = self.evaluate_js(js).await?;
        let count = result.get("value")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;
        eprintln!("✅ Auto-accepted {count} edits");
        Ok(count)
    }

    /// Switch the IDE model (e.g., to Claude Sonnet, Gemini, etc.)
    pub async fn switch_model(&mut self, model_name: &str) -> Result<()> {
        let escaped = model_name.replace('\'', "\\'");
        let js = format!(r#"
            (function() {{
                // Click model selector dropdown
                var selector = document.querySelector('[data-testid="model-selector"], .model-selector, button[aria-label*="model"]');
                if (selector) {{
                    selector.click();
                    // Wait a bit then click the target model
                    setTimeout(function() {{
                        var options = document.querySelectorAll('[data-testid*="model"], .model-option, [role="option"]');
                        for (var i = 0; i < options.length; i++) {{
                            if (options[i].innerText.toLowerCase().includes('{escaped}'.toLowerCase())) {{
                                options[i].click();
                                return 'switched';
                            }}
                        }}
                    }}, 300);
                    return 'selector_clicked';
                }}
                return 'no_selector';
            }})()
        "#);

        let result = self.evaluate_js(&js).await?;
        eprintln!("🔄 Model switch to '{model_name}': {:?}", result);
        Ok(())
    }

    /// Execute a full autonomous task: inject prompt → wait → auto-accept
    pub async fn execute_task(&mut self, prompt: &str, timeout_secs: u64) -> Result<String> {
        eprintln!("🤖 Executing task: {}...", &prompt[..prompt.len().min(80)]);

        self.inject_prompt(prompt).await?;
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        let response = self.wait_for_response(timeout_secs).await?;

        // Auto-accept any edits
        let edits = self.auto_accept_edits().await.unwrap_or(0);

        eprintln!("✅ Task complete. Response: {} chars, {} edits accepted", response.len(), edits);
        Ok(response)
    }
}

/// Run a batch of tasks autonomously
pub async fn run_task_queue(port: u16, tasks: Vec<String>, timeout_per_task: u64) -> Result<Vec<(String, Result<String>)>> {
    let mut controller = CdpController::discover(port).await?;
    let mut results = Vec::new();

    for (i, task) in tasks.iter().enumerate() {
        eprintln!("\n📋 Task {}/{}: {}", i + 1, tasks.len(), &task[..task.len().min(60)]);

        let result = controller.execute_task(task, timeout_per_task).await;
        results.push((task.clone(), result));

        // Brief pause between tasks
        if i + 1 < tasks.len() {
            tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
        }
    }

    Ok(results)
}
