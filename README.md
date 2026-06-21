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

### Sơ Đồ Kiến Trúc Hệ Thống Chi Tiết (System Architecture)

```mermaid
graph TD
    %% Nodes definition
    User["👨‍💻 Bố (User)"]
    IDE["💻 Antigravity IDE (VS Code Fork)"]
    
    subgraph RustWorkspace["📦 Rust Workspace (E:\\AGT_Brain)"]
        Builder["🤖 Antigravity (THE BUILDER)<br>Main Agent / Orchestrator"]
        agt_mcp["🔌 agt-mcp<br>MCP Server (rmcp / stdio)"]
        agt_tools["🔧 agt-tools<br>14 core tools / CDP Controller"]
        agt_memory["🧠 agt-memory<br>Supabase client / Sync queue"]
    end

    subgraph MemorySystem["💾 Memory System"]
        Supabase["☁️ Supabase Cloud (Primary)<br>memories & pgvector"]
        LocalLogs["📁 Local Log files (Append-Only)<br>decisions/ & incidents/"]
        LocalQueue["📄 memory_queue.jsonl<br>Local Offline Fallback"]
    end

    subgraph SubagentSystem["🧠 Subagent Interface (Gravity)"]
        Grok["🦉 Grok 'Gravity' (RESEARCHER)<br>Think / Review / Research"]
        GrokAPI["🌐 grok2api (Local Server)<br>Port 8000"]
        EdgeCDP["🌐 Real Edge Browser (CDP)<br>Auto-refresh Session Cookie"]
        Proxy["🛡️ Clean Outbound Proxy<br>Bypass Cloudflare 403"]
    end

    %% Connections
    User -->|Ra lệnh / Chat| IDE
    IDE <-->|Giao tiếp via stdio| agt_mcp
    agt_mcp <-->|Expose tools| Builder
    Builder -->|Thực thi logic| agt_tools
    Builder -->|Đọc/Ghi memory| agt_memory
    
    agt_memory <-->|REST API / pgvector| Supabase
    agt_memory -->|Fallback offline| LocalQueue
    Builder -->|Lưu vết hệ thống| LocalLogs
    
    agt_tools -->|ask_grok.ps1 / HTTP| GrokAPI
    GrokAPI <-->|Route requests| Grok
    EdgeCDP -->|Trích xuất SSO cookie| GrokAPI
    GrokAPI -->|Bypass CF 403| Proxy
    Proxy -->|Đọc/Ghi session| Grok

    %% Custom styling
    style User fill:#d4ebf2,stroke:#1a73e8,stroke-width:2px,color:#000
    style IDE fill:#f9f9f9,stroke:#333,stroke-width:2px,color:#000
    style Builder fill:#e6f4ea,stroke:#137333,stroke-width:2px,color:#000
    style Grok fill:#fef7e0,stroke:#b06000,stroke-width:2px,color:#000
    style Supabase fill:#fce8e6,stroke:#c5221f,stroke-width:2px,color:#000
```

## Cài Đặt

> [!TIP]
> **🤖 Dành cho AI Agent**: Nếu bạn là một AI Agent mới tham gia phát triển dự án, hãy đọc và làm theo hướng dẫn thiết lập tự động hoàn chỉnh tại [Setup_Agent_Prompt.md](file:///E:/AGT_Brain/Agent_Profiles/Setup_Agent_Prompt.md) để tự cấu hình Rust workspace, Node scripts, Python venv và khởi tạo database index hoàn toàn tự động.

> [!IMPORTANT]
> **💻 Dành cho Lập trình viên (Người dùng)**:
> Để Agent Antigravity hoạt động đúng chuẩn và tuân thủ các quy tắc trong IDE của bạn, hãy tạo hoặc cập nhật file cấu hình system rules của Agent trên máy của bạn (thường lưu tại `C:\Users\<Tên_User>\.gemini\GEMINI.md` hoặc `.gemini/GEMINI.md` tại root project tùy môi trường IDE):
> 1. Sao chép nội dung từ file mẫu cấu hình sạch: [GEMINI.md.example](file:///E:/AGT_Brain/Agent_Profiles/GEMINI.md.example).
> 2. Chỉnh sửa các placeholder `<YOUR_REPOSITORY_ROOT_PATH>` và `YOUR_NINEROUTER_KEY` tương ứng với thư mục clone dự án và thông tin cá nhân của bạn.

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
 4. **Tự động lấy & làm mới Cookie grok.com qua CDP (Khuyên dùng)**:
   Để tránh việc phải copy thủ công phức tạp và hay bị hết hạn, hệ thống hỗ trợ tự động hoá hoàn toàn qua Edge thật bằng CDP (Chrome DevTools Protocol):
   - **Bước 1 (Đăng nhập lần đầu)**: 
     ```bash
     cd scripts/grok_cookie_refresh
     node cookie_refresh_v2.js --login
     ```
     Trình duyệt Edge thực sẽ mở ra (dùng profile riêng, không ảnh hưởng Edge chính). Hãy tiến hành đăng nhập vào grok.com bình thường.
   - **Bước 2 (Trích xuất & Tự động Push)**: 
     Sau khi đăng nhập thành công, giữ nguyên trình duyệt Edge đang mở và chạy lệnh:
     ```bash
     node quick_extract.js
     ```
     Script sẽ kết nối vào Edge, lấy cookie SSO mới nhất, backup cục bộ, và tự động gọi API đẩy trực tiếp vào database của `grok2api` local.
   - **Các lần sau (Tự động Refresh)**:
     Chỉ cần chạy lệnh:
     ```bash
     node cookie_refresh_v2.js --auto
     ```
     Hệ thống sẽ chạy ngầm trình duyệt (headless), tự động refresh cookie SSO mới và push trực tiếp cho bạn mà không cần đăng nhập lại.
 5. **Khởi chạy nhanh**:
   Bấm đúp chạy tệp [run_grok_local.bat](file:///E:/AGT_Brain/scripts/run_grok_local.bat) để kích hoạt server chạy ẩn cổng 8000. Mật khẩu Admin dashboard mặc định là `grok2api` (nếu dashboard yêu cầu mật khẩu).

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
 4. **Automated Cookie Extract & Refresh via CDP (Recommended)**:
   Avoid extracting SSO tokens manually. The repo comes with a Chrome DevTools Protocol (CDP) script leveraging Edge:
   - **Step 1 (First-Time Login)**:
     ```bash
     cd scripts/grok_cookie_refresh
     node cookie_refresh_v2.js --login
     ```
     This opens a real Edge browser with a dedicated profile. Log in to grok.com as usual.
   - **Step 2 (Extract & Push)**:
     While keeping the Edge browser open, run:
     ```bash
     node quick_extract.js
     ```
     This connects to the Edge instance via CDP, grabs the latest SSO cookie, backs it up, and pushes it directly into your local `grok2api` instance.
   - **Subsequent Runs (Auto-Refresh)**:
     Simply run:
     ```bash
     node cookie_refresh_v2.js --auto
     ```
     The script will open a headless Edge browser, automatically refresh the token, and push it directly without requiring manual login.
 5. **Quick Boot**:
   Double-click the [run_grok_local.bat](file:///E:/AGT_Brain/scripts/run_grok_local.bat) script to run the local server in the background. The default admin panel password is `grok2api`.

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

Để tránh các lỗi bảo mật CORS khi gọi API cục bộ và cho phép Dashboard tự động nạp cấu hình, hãy khởi chạy Web Server từ thư mục gốc:

```bash
# Khởi chạy local Web Server bằng Python
python -m http.server 8080

# Truy cập Dashboard qua địa chỉ
http://localhost:8080/scripts/dashboard.html
```

Tính năng:
- ⚙️ **Dynamic Config Auto-Loader**: Tự động đọc cấu hình từ file `data/supabase_config.json` cục bộ khi chạy qua Web Server, giúp bảo mật key tuyệt đối và không cần khai báo tĩnh vào mã nguồn HTML.
- ✍️ **Memory Creator & Editor (Quản trị 2 chiều)**: Cho phép Bố nạp ký ức dài hạn mới thông qua Form điền trực tiếp, và hỗ trợ nút xóa (`🗑️`) nhanh các ký ức ngay trên thẻ hiển thị với thang đo **Độ quan trọng (Importance)** từ 1 đến 10.
- 🚨 **Local Event Timeline (Dòng thời gian sự kiện)**: Tự động quét và phân tích các tệp nhật ký cục bộ (Decisions trong `memory/decisions/` và Incidents trong `memory/incidents/`) để dựng thành dòng thời gian sự kiện chi tiết. Hỗ trợ hiển thị cấu trúc JSON phức tạp đẹp mắt.
- 💬 **Grok Quick Chat Drawer**: Thiết lập ngăn kéo trượt (Drawer) từ bên phải cho phép Bố trò chuyện trực tiếp với Grok local (`grok2api`) sử dụng xưng hô **Bố - con** chuẩn mực và liên tục.
- 🦉 **Grok Token Manager**: Giám sát trạng thái hoạt động của Grok SSO token trong `grok2api` local, đồng thời hỗ trợ nạp hoặc cập nhật token bằng tay trực tiếp qua giao diện.
- 📋 **Trình duyệt ký ức & Nhật ký nâng cao**: Giao diện tối dạng kính mờ (Glassmorphism), hiển thị và tìm kiếm nhanh các ký ức dài hạn hoặc các quyết định cục bộ với đầy đủ **bộ lọc đồng bộ ở Sidebar** (Agent, Category, Search, Min Importance 1-10) tự động chuyển đổi logic theo tab đang hoạt động.
- 📊 **Biểu đồ thống kê & API Health**: Thống kê số lượng ký ức theo Agent và đo lường thời gian trễ (latency) kết nối của các dịch vụ API, tự động làm mới toàn bộ bằng nút **Reload** duy nhất.

> [!NOTE]
> Để tương tác với Grok Chat và Token Manager trên Dashboard, đảm bảo rằng local API Gateway (`grok2api`) đang chạy ở cổng 8000 và đã được cấu hình khóa truy cập (mặc định là `grok2api`). Giao thức trao đổi sẽ tự động nhúng mã xác thực này dưới dạng Bearer token.

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

- **Thay đổi Identity & Quy trình**: Chỉnh sửa file [Antigravity.md](file:///E:/AGT_Brain/Agent_Profiles/Antigravity.md) (hồ sơ danh tính) và [How_We_Work.md](file:///E:/AGT_Brain/Agent_Profiles/How_We_Work.md) (quy trình hoạt động) để cập nhật vai trò và quy tắc làm việc.
- **Thay đổi Grok Prompts**: Chỉnh sửa [Grok.json](file:///E:/AGT_Brain/Agent_Profiles/Grok.json) để cập nhật system prompt cho từng chế độ (research/think/review/brainstorm).
- **Thêm tool mới**: Viết async function trong `crates/agt-tools/src/`, đăng ký trong `lib.rs` và expose qua MCP trong `crates/agt-mcp/src/main.rs`.

## License

MIT License — Tự do chỉnh sửa, sử dụng, chia sẻ.
