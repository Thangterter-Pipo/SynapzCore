# 🧠 AGT_Brain — Bộ Não AI Tự Trị 2-Agent

**Hệ thống 2-AI có trí nhớ, tự phản tỉnh, và tự điều khiển IDE** — Antigravity (Builder) + Grok (Researcher) chia sẻ memory, tự học, và tự code autonomously.

## Tính Năng

- 🧠 **Auto-Context Loader**: Mỗi session mới tự động tải decisions, memories, goals, incidents giúp phục hồi ngữ cảnh lập trình ngay lập tức.
- 🧠 **Shared Memory**: Supabase cloud — các agents chia sẻ chung bộ nhớ dài hạn (tự động lưu, đo mức độ quan trọng).
- 🔍 **Semantic Search**: pgvector embeddings + `match_memories()` RPC tìm kiếm ký ức theo ngữ nghĩa.
- 🔧 **14 Registry Tools** + CDP Controller: File(6) + Shell(1) + Web(2) + Memory(5) + Grok(2).
- 🔌 **MCP Server**: 10 tools exposed qua rmcp stdio — tích hợp trực tiếp vào VS Code / Cursor / IDE.
- 🧠 **Grok Subagent**: Research, Think, Review, Brainstorm — powered by Gravity Framework (mặc định dùng `grok-4-heavy`).
- 🪞 **Daily Self-Reflection**: Tự review memories, decisions, tự phản tỉnh và tạo ra các insights cải thiện hệ thống hằng ngày.
- 📚 **Skill Library**: Save/recall các patterns, giải pháp tái sử dụng dưới dạng skill có độ quan trọng cao.
- 🤖 **CDP Autonomous Mode**: Tự điều khiển IDE qua Chrome DevTools Protocol (tự động nhập prompt, kiểm tra trạng thái sinh, tự Accept code edits).
- 📦 **Memory Archive**: Tự động lưu trữ (archive) các ký ức cũ có độ quan trọng thấp để tối ưu hiệu năng.
- 📊 **Admin Dashboard**: Web-based memory browser + stats + health checks (`scripts/dashboard.html`).
- 📝 **Offline Knowledge Extractor**: Tự phân tích conversation logs và tạo ra stats JSON cùng trang HTML viewer offline (`scripts/extract_knowledge_v2.js`).

## Kiến Trúc

```
AGT_Brain/
├── crates/
│   ├── agt-memory/     # Supabase REST + sync queue + archive + pgvector
│   ├── agt-tools/      # 14 tools + CDP Controller + Goals + Reflection
│   └── agt-mcp/        # MCP Server (rmcp, stdio — 10 tools to IDE)
├── memory/             # decisions/ & incidents/ (append-only local logs)
├── data/               # goals.json (Supabase config được ignore bảo mật)
├── Agent_Profiles/     # Agent identity, workflow docs, Grok/Gravity config
├── scripts/            # Scripts tự động hóa + dashboard.html + extractors
└── Cargo.toml          # Rust workspace (resolver 2, edition 2024)
```

### 2-AI Team Architecture

```
┌──────────────────────────────────────────────────┐
│                   Bố (User)                      │
│                      │                            │
│             ┌────────┴────────┐                  │
│             │   Antigravity   │ ← THE BUILDER    │
│             │  (Main Brain)   │   Orchestrator   │
│             └───────┬────────┘                   │
│                     │                             │
│             ┌───────┴───────┐                    │
│             │     Grok      │ ← RESEARCHER       │
│             │  "Gravity"    │   Think / Review   │
│             └───────────────┘                    │
└──────────────────────────────────────────────────┘
```

### Shared Memory Flow

```
Antigravity ─┐
             ├──→ Supabase Cloud (memories table)
Grok ────────┘        │
                       ├── keyword search (ilike)
                       ├── semantic search (pgvector)
                       ├── team recall (importance ≥ 3)
                       └── archive (→ memories_archive)
```

## Cài Đặt

### Yêu cầu
- [Rust](https://rustup.rs/) (1.85+ / Edition 2024)
- Tài khoản [Supabase](https://supabase.com/) miễn phí

### Bước 1: Clone

```bash
git clone https://github.com/Thangterter-Pipo/AGT_Brain.git
cd AGT_Brain
```

### Bước 2: Tạo Supabase Project

1. Vào [supabase.com/dashboard](https://supabase.com/dashboard)
2. Tạo project mới.
3. Vào **Settings → API** → copy:
   - `Project URL` (ví dụ: `https://xxxxx.supabase.co`)
   - `service_role` key (secret role key để đọc/ghi)

### Bước 3: Tạo bảng `memories`

Vào **SQL Editor** trên Supabase Dashboard, chạy:

```sql
-- Enable pgvector
CREATE EXTENSION IF NOT EXISTS vector;

-- Bảng memories chính (10 columns)
CREATE TABLE IF NOT EXISTS memories (
    id BIGSERIAL PRIMARY KEY,
    content TEXT NOT NULL,
    role TEXT NOT NULL DEFAULT 'user',
    agent TEXT NOT NULL DEFAULT 'antigravity',
    session_id TEXT,
    category TEXT NOT NULL DEFAULT 'general',
    importance SMALLINT NOT NULL DEFAULT 3,
    confidence SMALLINT NOT NULL DEFAULT 3,
    metadata JSONB DEFAULT '{}',
    embedding vector(384),
    created_at TIMESTAMPTZ DEFAULT NOW()
);

-- Indexes
CREATE INDEX IF NOT EXISTS idx_memories_content ON memories USING GIN (to_tsvector('simple', content));
CREATE INDEX IF NOT EXISTS idx_memories_agent ON memories (agent);
CREATE INDEX IF NOT EXISTS idx_memories_category ON memories (category);
CREATE INDEX IF NOT EXISTS idx_memories_importance ON memories (importance DESC);
CREATE INDEX IF NOT EXISTS idx_memories_created ON memories (created_at DESC);
CREATE INDEX IF NOT EXISTS idx_memories_embedding ON memories USING ivfflat (embedding vector_cosine_ops) WITH (lists = 50);

-- Archive table
CREATE TABLE IF NOT EXISTS memories_archive (LIKE memories INCLUDING ALL);

-- RLS
ALTER TABLE memories ENABLE ROW LEVEL SECURITY;
CREATE POLICY "Service role full access" ON memories FOR ALL USING (true) WITH CHECK (true);
```

### Bước 4: Cấu hình

Tạo file cấu hình tại `data/supabase_config.json`:

```json
{
    "supabase_url": "https://YOUR_PROJECT_ID.supabase.co",
    "supabase_key": "YOUR_SERVICE_ROLE_KEY"
}
```

> ⚠️ **QUAN TRỌNG**: Sử dụng `service_role` key — **KHÔNG** dùng `anon` key.

### Bước 5: Build

```bash
cargo build --release
```

Binaries output:
- `target/release/agt-mcp` — MCP Server (10 tools stdio)
- `target/release/ask-grok` — Grok Subagent CLI
- `target/release/brain-cron` — Autonomous Scheduler (chạy daily reflection & health check)

### Bước 6: Cấu hình Grok Local / Grok Local Configuration (Optional)

#### 🇻🇳 Tiếng Việt: Hướng dẫn cấu hình Grok Local
Để tối ưu hiệu năng và tránh bị Cloudflare chặn (lỗi 403) khi gọi Grok API từ môi trường code, khuyến nghị cài đặt dịch vụ `grok2api` chạy local kết hợp với proxy sạch.

1. **Tải và Cài đặt `grok2api`**:
   ```bash
   git clone https://github.com/chenyme/grok2api.git E:\AGT_Brain\grok2api_local
   cd E:\AGT_Brain\grok2api_local
   python -m venv venv
   .\venv\Scripts\pip install .
   ```
2. **Thiết lập `.env`**:
   Sao chép `.env.example` thành `.env` và cấu hình:
   ```env
   TZ=Asia/Ho_Chi_Minh
   SERVER_HOST=127.0.0.1
   SERVER_PORT=8000
   ACCOUNT_STORAGE=local
   DATA_DIR=./data
   ```
3. **Cấu hình Proxy để bypass Cloudflare 403**:
   Khởi chạy dịch vụ một lần để sinh file `data/config.toml` (hoặc tạo thủ công) và cấu hình proxy sạch (ví dụ proxy IPv6 của bạn) ở mục `[proxy.egress]`:
   ```toml
   [proxy.egress]
   mode = "single_proxy"
   proxy_url = "http://username:password@your_proxy_ip:port/"
   ```
4. **Lấy Cookie `sso` từ grok.com**:
   - Truy cập **https://grok.com** và đăng nhập.
   - Nhấn **F12** -> chọn tab **Console** -> gõ `document.cookie` -> nhấn **Enter**.
   - Copy toàn bộ chuỗi text kết quả bên trong dấu nháy kép (chứa biến `sso=eyJ...`).
5. **Nạp token vào hệ thống**:
   - Truy cập trang quản trị Admin local: `http://127.0.0.1:8000/admin` (mật khẩu mặc định: `grok2api`).
   - Vào mục **Account / Tokens**, dán chuỗi cookie vừa copy vào và nhấn **Thêm tài khoản**.
   - Hệ thống sẽ tự động đồng bộ hạn mức (quota) qua proxy và chuyển tài khoản sang trạng thái `active`.
6. **Khởi chạy nhanh**:
   Bấm đúp chạy tệp [run_grok_local.bat](file:///E:/AGT_Brain/scripts/run_grok_local.bat) để kích hoạt server chạy ẩn cổng 8000.

---

#### 🇺🇸 English: Grok Local Configuration Guide
To optimize API latency and prevent Cloudflare blocking (403 errors) when calling the Grok API from your agent workflows, it is highly recommended to host a local `grok2api` instance mapped to a clean outbound proxy.

1. **Clone & Install `grok2api`**:
   ```bash
   git clone https://github.com/chenyme/grok2api.git E:\AGT_Brain\grok2api_local
   cd E:\AGT_Brain\grok2api_local
   python -m venv venv
   .\venv\Scripts\pip install .
   ```
2. **Set up `.env`**:
   Copy `.env.example` to `.env` and set the following parameters:
   ```env
   TZ=Asia/Ho_Chi_Minh
   SERVER_HOST=127.0.0.1
   SERVER_PORT=8000
   ACCOUNT_STORAGE=local
   DATA_DIR=./data
   ```
3. **Configure Proxy to Bypass Cloudflare 403**:
   Launch the service once to generate `data/config.toml` (or create it manually) and edit the `[proxy.egress]` section to route traffic through a clean proxy (e.g., your IPv6 proxy):
   ```toml
   [proxy.egress]
   mode = "single_proxy"
   proxy_url = "http://username:password@your_proxy_ip:port/"
   ```
4. **Extract the `sso` Cookie**:
   - Access **https://grok.com** on your browser and sign in.
   - Press **F12** -> go to **Console** tab -> execute `document.cookie` -> press **Enter**.
   - Copy the output string inside the quotation marks (which contains `sso=eyJ...`).
5. **Add the Token to Local DB**:
   - Access the local Admin panel at `http://127.0.0.1:8000/admin` (default password is `grok2api`).
   - Go to the **Account / Tokens** section, paste the full cookie string, and click **Add Account**.
   - The gateway will automatically query and synchronize account quotas via your proxy to change the status to `active`.
6. **Quick Boot**:
   Double-click the [run_grok_local.bat](file:///E:/AGT_Brain/scripts/run_grok_local.bat) script to run the local server in the background.

---

#### 📚 References / Nguồn tham khảo:
- [chenyme/grok2api](https://github.com/chenyme/grok2api) — Original FastAPI gateway converting Grok Web client capabilities into OpenAI-compatible API endpoints.
- [xAI API Documentation](https://docs.x.ai/) — Official references for models, features, and specs.



### Autonomous Scheduler

```bash
# Phản tỉnh một lần (one-shot)
brain-cron

# Chạy kiểm tra sức khỏe hệ thống
brain-cron --health-only

# Chạy ngầm định kỳ daemon (ví dụ: mỗi 12 giờ)
brain-cron --daemon --interval 12
```

Tự động chạy trên Windows: Sử dụng file batch `scripts/run_brain_cron.bat` kết hợp Windows Task Scheduler.

## Sử Dụng

### MCP Tools (tích hợp trực tiếp vào IDE — 10 tools)

| Tool | Mô tả |
|------|-------|
| `auto_context` | 🧠 **Gọi ĐẦU TIÊN** — Load decisions, memories, goals, incidents phục hồi ngữ cảnh |
| `search_memory` | Tìm ký ức theo keyword (có thể filter theo agent) |
| `add_memory` | Lưu thông tin mới với agent/category/importance tương ứng |
| `team_memory` | Lấy các ký ức quan trọng gần đây của team làm context |
| `get_boss_profile` | Truy xuất profile & preferences của Bố (User) |
| `ask_grok` | Gọi Grok AI subagent (research/think/review/brainstorm) |
| `grok_health` | Kiểm tra trạng thái hoạt động của Grok API |
| `daily_reflection` | 🪞 Tự phản tỉnh, tổng hợp quyết định & insight ngày |
| `save_skill` | 📚 Lưu các patterns/giải pháp tái sử dụng dưới dạng Skill |
| `recall_skills` | 🔍 Tìm kiếm skill đã lưu theo keyword |

### Grok Subagent CLI

Chạy trực tiếp từ terminal:

```bash
# Research công nghệ mới
ask-grok --mode research "Tìm hiểu rmcp framework"

# Quyết định kiến trúc (Grok Heavy suy nghĩ sâu)
ask-grok --mode think "Nên dùng REST hay gRPC cho microservices?"

# Review code phức tạp
ask-grok --mode review --code "fn main() { panic!(); }" "Kiểm tra an toàn"

# Brainstorm ý tưởng mới
ask-grok --mode brainstorm "Cách thiết kế memory system"
```

Bố cũng có thể sử dụng script PowerShell nhanh:
```powershell
.\scripts\ask_grok.ps1 -Mode research "So sánh các cơ sở dữ liệu Rust"
```

### Offline Viewer & Extractor

Để phân tích dữ liệu hội thoại offline không cần LLM:
```bash
node scripts/extract_knowledge_v2.js
```
Script sẽ quét thư mục conversation logs và tạo ra:
- `data/knowledge/knowledge_base.json`: Dữ liệu kiến trúc & quyết định dạng JSON.
- `data/knowledge/knowledge_viewer.html`: Giao diện Web tối giản hiển thị stats và toàn bộ lịch sử hội thoại được phân tích tự động.

### Web Admin Dashboard

```bash
# Mở dashboard trực tiếp trong trình duyệt
start scripts/dashboard.html
```

Tính năng:
- 📋 Trình duyệt ký ức Supabase đầy đủ bộ lọc (agent, category, importance).
- 📊 Biểu đồ stats thời gian thực (số lượng memory theo agent, archive).
- 🏥 Kiểm tra sức khỏe kết nối API (Supabase, Grok).
- 🔍 Tìm kiếm keyword nhanh.

### Rust API Example

```rust
use agt_memory::SupabaseMemory;

let mem = SupabaseMemory::from_config("data/supabase_config.json")?;

// Ghi nhớ ký ức mới
mem.remember_as("Quyết định dùng Supabase", "antigravity", "antigravity", "decision", 5, 4, &json!({})).await?;

// Recall tìm kiếm keyword
let results = mem.recall("Supabase", 5).await?;

// Fetch recent team memories (importance >= 3)
let team = mem.recall_team(10).await?;
```

### Tool Registry API (14 tools)

```rust
use agt_tools::build_default_registry;

let registry = build_default_registry();
assert_eq!(registry.count(), 14);

// Thực thi tool từ code Rust
let result = registry.execute("search_memory", json!({
    "query": "Supabase key",
    "n_results": 3
})).await?;
```

## Cá Nhân Hóa

- **Thay đổi Identity Agent**: Chỉnh sửa file [Antigravity.md](file:///E:/AGT_Brain/Agent_Profiles/Antigravity.md) để cập nhật vai trò, quy tắc hoạt động.
- **Thay đổi Grok Prompts**: Chỉnh sửa [Grok.json](file:///E:/AGT_Brain/Agent_Profiles/Grok.json) để cập nhật system prompt cho từng chế độ (research/think/review/brainstorm).
- **Thêm tool mới**: Viết async function trong `crates/agt-tools/src/`, đăng ký trong `lib.rs` và expose qua MCP trong `crates/agt-mcp/src/main.rs`.

## License

MIT License — Tự do chỉnh sửa, sử dụng, chia sẻ.
