//! 🧠 ask-grok — Gravity Subagent CLI (Rust)
//!
//! Antigravity's second brain, powered by Grok AI + Gravity Framework.
//!
//! Usage:
//!   ask-grok "câu hỏi ở đây"
//!   ask-grok --mode research "chủ đề cần research"
//!   ask-grok --mode think "vấn đề cần quyết định"
//!   ask-grok --mode review --code "fn main() {}" "review Rust code"
//!   ask-grok --mode brainstorm "chủ đề brainstorm"
//!   ask-grok --model grok-4 "câu hỏi cho grok-4"

use anyhow::Result;
use clap::Parser;

use agt_tools::grok::{ChatMessage, GrokModel, GrokSubagent};

/// 🧠 Gravity Subagent CLI — Antigravity's second brain
#[derive(Parser, Debug)]
#[command(name = "ask-grok", version, about = "🧠 Gravity Subagent — Research, Think, Review, Brainstorm")]
struct Args {
    /// Prompt / câu hỏi cho Gravity
    prompt: String,

    /// Mode: chat, research, think, review, brainstorm
    #[arg(short, long, default_value = "chat")]
    mode: String,

    /// Model: grok-3, grok-3-mini, grok-3-thinking, grok-4, grok-4-heavy
    #[arg(long, default_value = "grok-4-heavy")]
    model: String,

    /// Code block (dùng cho mode review)
    #[arg(long)]
    code: Option<String>,

    /// Context bổ sung
    #[arg(long)]
    context: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let grok = GrokSubagent::new();

    let result = match args.mode.as_str() {
        "research" => {
            eprintln!("🔬 Gravity Research Engine activating...");
            grok.research(&args.prompt).await?
        }
        "think" => {
            eprintln!("🧠 Gravity Thinking Engine activating...");
            grok.think(&args.prompt).await?
        }
        "review" => {
            eprintln!("💻 Gravity Code Review Engine activating...");
            let code = args.code.as_deref().unwrap_or(&args.prompt);
            let context = args.context.as_deref().unwrap_or("Code review request");
            grok.review_code(code, context).await?
        }
        "brainstorm" => {
            eprintln!("💡 Gravity Brainstorm Engine activating...");
            grok.brainstorm(&args.prompt).await?
        }
        _ => {
            // Chat mode — custom model
            eprintln!("💬 Gravity Chat (model: {})...", args.model);
            let model = GrokModel::from_str(&args.model);
            let messages = vec![ChatMessage {
                role: "user".to_string(),
                content: args.prompt,
            }];
            grok.chat(messages, model).await?
        }
    };

    println!("{result}");
    Ok(())
}
