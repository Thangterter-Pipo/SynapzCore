//! CDP Controller — Autonomous IDE control via Chrome DevTools Protocol.
//!
//! Connects to Antigravity IDE workbench (launched with --remote-debugging-port=9333)
//! and provides programmatic control: inject prompts, monitor responses,
//! switch models, auto-accept edits.

use anyhow::{Result, anyhow};
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

        let pages: Vec<DebugPage> = resp
            .json()
            .await
            .map_err(|e| anyhow!("Invalid CDP response: {e}"))?;

        // Find the workbench page (main IDE window)
        let workbench = pages
            .iter()
            .find(|p| p.url.contains("workbench.html") || p.title.contains("Antigravity"))
            .or_else(|| pages.iter().find(|p| p.page_type == "page"))
            .ok_or_else(|| {
                anyhow!(
                    "No workbench page found. Pages: {:?}",
                    pages.iter().map(|p| &p.title).collect::<Vec<_>>()
                )
            })?;

        let ws_url = workbench
            .web_socket_debugger_url
            .as_ref()
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

        let (mut ws_stream, _) = tokio_tungstenite::connect_async(&self.ws_url)
            .await
            .map_err(|e| anyhow!("WebSocket connect failed: {e}"))?;

        let cmd_json = serde_json::to_string(&cmd)?;
        ws_stream
            .send(Message::Text(cmd_json))
            .await
            .map_err(|e| anyhow!("WebSocket send failed: {e}"))?;

        // Wait for response with matching id
        let timeout = tokio::time::Duration::from_secs(30);
        let deadline = tokio::time::Instant::now() + timeout;

        while tokio::time::Instant::now() < deadline {
            match tokio::time::timeout(timeout, ws_stream.next()).await {
                Ok(Some(Ok(Message::Text(text)))) => {
                    if let Ok(resp) = serde_json::from_str::<CdpResponse>(&text)
                        && resp.id == Some(self.command_id)
                    {
                        if let Some(error) = resp.error {
                            return Err(anyhow!("CDP error: {error}"));
                        }
                        return Ok(resp.result.unwrap_or(serde_json::Value::Null));
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
        let escaped = prompt
            .replace('\\', "\\\\")
            .replace('\'', "\\'")
            .replace('\n', "\\n");

        // Find the chat input textarea and set its value
        let js = format!(
            r#"
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
        "#
        );

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

                let text = response
                    .get("value")
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
    /// Prefers the Composer "Accept all" control (Antigravity renders it as a
    /// clickable <span>), falling back to individual accept/apply buttons.
    pub async fn auto_accept_edits(&mut self) -> Result<u32> {
        let js = r#"
            (function() {
                var accepted = 0;
                // 1. Composer "Accept all" (span-based, Cursor-style widget)
                var spans = document.querySelectorAll('span, button');
                for (var i = 0; i < spans.length; i++) {
                    var t = (spans[i].innerText || '').trim().toLowerCase();
                    if (t === 'accept all') {
                        spans[i].click();
                        return 1;
                    }
                }
                // 2. Fallback: individual accept/apply buttons
                var btns = document.querySelectorAll('button');
                for (var j = 0; j < btns.length; j++) {
                    var text = (btns[j].innerText || '').toLowerCase();
                    var label = (btns[j].getAttribute('aria-label') || '').toLowerCase();
                    if (text.includes('accept') || text.includes('apply') ||
                        label.includes('accept') || label.includes('apply')) {
                        btns[j].click();
                        accepted++;
                    }
                }
                return accepted;
            })()
        "#;

        let result = self.evaluate_js(js).await?;
        let count = result.get("value").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        eprintln!("✅ Auto-accepted {count} edits");
        Ok(count)
    }

    /// Auto-allow permission/approval dialogs (run command, MCP tool calls, etc.).
    /// Technique adapted from LazyGravity's approvalDetector: locate an Allow
    /// button, verify a matching Deny button exists in the same container to
    /// avoid false-positives, then click Allow. Prefers "Allow once".
    /// Returns true if an approval was clicked.
    pub async fn auto_allow(&mut self) -> Result<bool> {
        let js = r#"
            (function() {
                var ALLOW_ONCE = ['allow once', 'allow one time', '今回のみ許可', '1回のみ許可'];
                var ALWAYS_ALLOW = ['allow this conversation', 'allow this chat', 'always allow', '常に許可'];
                var ALLOW = ['allow', 'permit', 'accept', '許可', '承認'];
                var DENY = ['deny', 'reject', 'decline', '拒否'];
                var norm = function(s){ return (s||'').toLowerCase().replace(/\s+/g,' ').trim(); };

                var all = Array.prototype.slice.call(document.querySelectorAll('button'))
                    .filter(function(b){ return b.offsetParent !== null; });

                // Prefer "Allow once"
                var approve = all.find(function(b){
                    var t = norm(b.textContent);
                    return ALLOW_ONCE.some(function(p){ return t.indexOf(p) !== -1; });
                });
                // Then generic Allow (excluding "always allow")
                if (!approve) {
                    approve = all.find(function(b){
                        var t = norm(b.textContent);
                        var isAlways = ALWAYS_ALLOW.some(function(p){ return t.indexOf(p) !== -1; });
                        return !isAlways && ALLOW.some(function(p){ return t.indexOf(p) !== -1; });
                    });
                }
                if (!approve) return 'no_approval';

                var container = approve.closest('[role="dialog"], .modal, .dialog, .monaco-dialog-box')
                    || (approve.parentElement && approve.parentElement.parentElement)
                    || approve.parentElement
                    || document.body;

                var cbtns = Array.prototype.slice.call(container.querySelectorAll('button'))
                    .filter(function(b){ return b.offsetParent !== null; });
                var deny = cbtns.find(function(b){
                    var t = norm(b.textContent);
                    return DENY.some(function(p){ return t.indexOf(p) !== -1; });
                });
                if (!deny) return 'no_deny_guard';

                approve.click();
                return 'allowed';
            })()
        "#;

        let result = self.evaluate_js(js).await?;
        let outcome = result.get("value").and_then(|v| v.as_str()).unwrap_or("");
        if outcome == "allowed" {
            eprintln!("🔓 Auto-allowed an approval dialog");
            return Ok(true);
        }
        Ok(false)
    }

    /// Detect whether the IDE is currently generating a response.
    /// Uses the Cancel button tooltip marker present while streaming.
    pub async fn is_generating(&mut self) -> Result<bool> {
        let js = r#"
            (function() {
                var els = document.querySelectorAll('[data-tooltip-id="input-send-button-cancel-tooltip"]');
                for (var i = 0; i < els.length; i++) {
                    var s = window.getComputedStyle(els[i]);
                    if (s.display !== 'none' && s.visibility !== 'hidden' && parseFloat(s.opacity) > 0) {
                        return true;
                    }
                }
                // Fallback: a visible Cancel button
                return !!document.querySelector('button[aria-label^="Cancel"]');
            })()
        "#;
        let result = self.evaluate_js(js).await?;
        Ok(result
            .get("value")
            .and_then(|v| v.as_bool())
            .unwrap_or(false))
    }

    /// Switch the IDE model (e.g., to Claude Sonnet, Gemini, etc.).
    /// Uses the real Antigravity selector `button[aria-label^="Select model, current:"]`
    /// and the Monaco quick-input list. Returns the outcome string.
    pub async fn switch_model(&mut self, model_name: &str) -> Result<String> {
        let escaped = model_name.replace('\\', "\\\\").replace('\'', "\\'");
        let js = format!(
            r#"
            (function() {{
                return new Promise(function(resolve) {{
                    var target = '{escaped}'.toLowerCase();
                    var btn = document.querySelector('button[aria-label^="Select model, current:"]');
                    if (!btn) {{ resolve('no_model_button'); return; }}
                    btn.click();
                    setTimeout(function() {{
                        // Antigravity model picker = Tailwind popup of full-width
                        // left-aligned <button>, NOT a Monaco quick-input widget.
                        var items = Array.prototype.slice.call(
                            document.querySelectorAll('button.select-none, button[class*="w-full"][class*="text-left"]')
                        ).filter(function(b){{ return b.offsetParent !== null; }});
                        var hit = null;
                        for (var i = 0; i < items.length; i++) {{
                            var t = ((items[i].innerText || '').split('\n')[0] || '').trim().toLowerCase();
                            if (t.indexOf(target) !== -1) {{ hit = items[i]; break; }}
                        }}
                        if (hit) {{ hit.click(); resolve('switched'); }}
                        else {{
                            document.body.dispatchEvent(new KeyboardEvent('keydown', {{ bubbles: true, key: 'Escape', code: 'Escape', keyCode: 27, which: 27 }}));
                            resolve('model_not_found');
                        }}
                    }}, 700);
                }});
            }})()
        "#
        );

        let result = self.evaluate_js(&js).await?;
        let outcome = result
            .get("value")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        eprintln!("🔄 Model switch to '{model_name}': {outcome}");
        Ok(outcome)
    }

    /// Execute a full autonomous task: inject prompt → wait (auto-allow approvals) → auto-accept
    pub async fn execute_task(&mut self, prompt: &str, timeout_secs: u64) -> Result<String> {
        eprintln!("🤖 Executing task: {}...", &prompt[..prompt.len().min(80)]);

        self.inject_prompt(prompt).await?;
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        // While the model works it may surface permission dialogs; clear them.
        let _ = self.auto_allow().await;

        let response = self.wait_for_response(timeout_secs).await?;

        // Clear any trailing approval, then accept edits.
        let _ = self.auto_allow().await;
        let edits = self.auto_accept_edits().await.unwrap_or(0);

        eprintln!(
            "✅ Task complete. Response: {} chars, {} edits accepted",
            response.len(),
            edits
        );
        Ok(response)
    }

    /// Autopilot: continuously poll for approval dialogs and (optionally) file
    /// edits, auto-clicking them. Mirrors LazyGravity's polling detector.
    /// Runs for `duration_secs`, polling every `interval_ms`.
    pub async fn auto_pilot(
        &mut self,
        duration_secs: u64,
        interval_ms: u64,
        accept_edits: bool,
    ) -> Result<(u32, u32)> {
        let deadline =
            tokio::time::Instant::now() + tokio::time::Duration::from_secs(duration_secs);
        let mut allows = 0u32;
        let mut accepts = 0u32;
        eprintln!(
            "🛸 Autopilot started ({duration_secs}s, every {interval_ms}ms, accept_edits={accept_edits})"
        );

        while tokio::time::Instant::now() < deadline {
            if self.auto_allow().await.unwrap_or(false) {
                allows += 1;
            }
            if accept_edits {
                accepts += self.auto_accept_edits().await.unwrap_or(0);
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(interval_ms)).await;
        }

        eprintln!("🛸 Autopilot done: {allows} approvals, {accepts} edits accepted");
        Ok((allows, accepts))
    }
}

/// Run a batch of tasks autonomously
pub async fn run_task_queue(
    port: u16,
    tasks: Vec<String>,
    timeout_per_task: u64,
) -> Result<Vec<(String, Result<String>)>> {
    let mut controller = CdpController::discover(port).await?;
    let mut results = Vec::new();

    for (i, task) in tasks.iter().enumerate() {
        eprintln!(
            "\n📋 Task {}/{}: {}",
            i + 1,
            tasks.len(),
            &task[..task.len().min(60)]
        );

        let result = controller.execute_task(task, timeout_per_task).await;
        results.push((task.clone(), result));

        // Brief pause between tasks
        if i + 1 < tasks.len() {
            tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
        }
    }

    Ok(results)
}
