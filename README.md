# 🧠 SynapzCore — Bộ Não Ký Ức Đa AI Tự Trị

**Hệ thống 2-AI có trí nhớ, tự phản tỉnh, và tự điều khiển IDE** — Antigravity (Builder) + Grok (Researcher) chia sẻ memory, tự học, và tự code autonomously.

> 🦉 **Grok Gateway** đã được tách sang repo riêng: [HeimdallProxy](https://github.com/Thangterter-Pipo/HeimdallProxy) — quản lý session, proxy, cookie refresh và MCP tools cho Grok.

## Tính Năng

- 🧠 **Auto-Context Loader**: Mỗi session mới tự động tải decisions, memories, goals, incidents giúp phục hồi ngữ cảnh lập trình ngay lập tức.
- 🧠 **Shared Memory**: Supabase cloud — các agents chia sẻ chung bộ nhớ dài hạn (tự động lưu, đo mức độ quan trọng).
- 🔍 **Semantic Search**: pgvector embeddings + `match_memories()` RPC tìm kiếm ký ức theo ngữ nghĩa.
- 🔧 **14 Registry Tools** + CDP Controller: File(6) + Shell(1) + Web(2) + Memory(5).
- 🔌 **MCP Server**: 8 tools exposed qua rmcp stdio — tích hợp trực tiếp vào VS Code / Cursor / IDE.
- 🧠 **Grok Subagent**: Research, Think, Review, Brainstorm — powered by Gravity Framework (mặc định dùng `grok-4-heavy`).
- 🪞 **Daily Self-Reflection**: Tự review memories, decisions, tự phản tỉnh và tạo ra các insights cải thiện hệ thống hằng ngày.
- 📚 **Skill Library**: Save/recall các patterns, giải pháp tái sử dụng dưới dạng skill có độ quan trọng cao.
- 🤖 **CDP Autonomous Mode**: Tự điều khiển IDE qua Chrome DevTools Protocol (tự động nhập prompt, kiểm tra trạng thái sinh, tự Accept code edits).
- 📦 **Memory Archive**: Tự động lưu trữ (archive) các ký ức cũ có độ quan trọng thấp để tối ưu hiệu năng.
- 📊 **Admin Dashboard**: Web-based memory browser + stats + health checks (`scripts/dashboard.html`).
- 📝 **Offline Knowledge Extractor**: Tự phân tích conversation logs và tạo ra stats JSON cùng trang HTML viewer offline (`scripts/extract_knowledge_v2.js`).

## Kiến Trúc

```
SynapzCore/
├── crates/
│   ├── synapz-memory/     # Supabase REST + sync queue + archive + pgvector
│   ├── synapz-tools/      # 14 tools + CDP Controller + Goals + Reflection
│   └── synapz-mcp/        # MCP Server (rmcp, stdio — 8 tools to IDE)
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
    
    subgraph RustWorkspace["📦 SynapzCore (E:\\AGT_Brain)"]
        Builder["🤖 Antigravity (THE BUILDER)<br>Main Agent / Orchestrator"]
        synapz_mcp["🔌 synapz-mcp<br>MCP Server (rmcp / stdio — 8 tools)"]
        synapz_tools["🔧 synapz-tools<br>14 core tools / CDP Controller"]
        synapz_memory["🧠 synapz-memory<br>Supabase client / Sync queue"]
    end

    subgraph MemorySystem["💾 Memory System"]
        Supabase["☁️ Supabase Cloud (Primary)<br>memories & pgvector"]
        LocalLogs["📁 Local Log files (Append-Only)<br>decisions/ & incidents/"]
        LocalQueue["📄 memory_queue.jsonl<br>Local Offline Fallback"]
    end

    subgraph HeimdallSystem["🛡️ HeimdallProxy (Separate Repo)"]
        HeimdallMCP["🔌 heimdall-mcp<br>MCP Server (stdio — 3 tools)"]
        GrokAPI["🌐 grok2api (Local Server)<br>Port 8000"]
        EdgeCDP["🌐 Real Edge Browser (CDP)<br>Auto-refresh Session Cookie"]
        Grok["🦉 Grok 'Gravity' (RESEARCHER)<br>Think / Review / Research"]
    end

    %% Connections
    User -->|Ra lệnh / Chat| IDE
    IDE <-->|Giao tiếp via stdio| synapz_mcp
    IDE <-->|Giao tiếp via stdio| HeimdallMCP
    synapz_mcp <-->|Expose tools| Builder
    Builder -->|Thực thi logic| synapz_tools
    Builder -->|Đọc/Ghi memory| synapz_memory
    
    synapz_memory <-->|REST API / pgvector| Supabase
    synapz_memory -->|Fallback offline| LocalQueue
    Builder -->|Lưu vết hệ thống| LocalLogs
    
    HeimdallMCP -->|ask_grok / HTTP| GrokAPI
    GrokAPI <-->|Route requests| Grok
    EdgeCDP -->|Trích xuất SSO cookie| GrokAPI

    %% Custom styling
    style User fill:#d4ebf2,stroke:#1a73e8,stroke-width:2px,color:#000
    style IDE fill:#f9f9f9,stroke:#333,stroke-width:2px,color:#000
    style Builder fill:#e6f4ea,stroke:#137333,stroke-width:2px,color:#000
    style Grok fill:#fef7e0,stroke:#b06000,stroke-width:2px,color:#000
    style Supabase fill:#fce8e6,stroke:#c5221f,stroke-width:2px,color:#000
```

## Chi Tiết Hệ Thống Memory (Ký Ức)

Hệ thống ký ức của Antigravity được thiết kế để duy trì ngữ cảnh dài hạn, học hỏi từ hành vi của người dùng (Bố) và ghi nhận kinh nghiệm lập trình.

### 1. Kiến Trúc Lưu Trữ 2 Lớp (Hybrid Storage)
*   **Primary Cloud (Supabase):** Bộ nhớ dài hạn lưu trữ trên đám mây Supabase (bảng `memories`). Các ký ức cũ có độ quan trọng thấp tự động được di chuyển vào bảng `memories_archive` để tối ưu hóa hiệu năng.
*   **Local Fallback (Độ tin cậy cao):** Khi gặp sự cố mạng hoặc mất kết nối Cloud, ký ức sẽ tự động xếp vào hàng đợi cục bộ tại `memory_queue.jsonl` và tự động flush (đồng bộ) lên đám mây khi kết nối được khôi phục.
*   **Local Logs (Append-Only):**
    *   `memory/decisions/` — Nhật ký ghi nhận các quyết định thiết kế và kiến trúc hệ thống quan trọng.
    *   `memory/incidents/` — Nhật ký ghi nhận các lỗi, sự cố nghiêm trọng và giải pháp khắc phục nhằm tránh lặp lại lỗi cũ.
    *   *Quy tắc bất biến*: Không bao giờ được phép xóa hoặc sửa đổi các bản ghi cũ, chỉ được phép append (thêm mới).

### 2. Tìm Kiếm Ngữ Nghĩa (Semantic Search với pgvector)
*   Mỗi ký ức khi ghi vào hệ thống đều được tạo vector embedding (384 chiều).
*   Sử dụng extension `pgvector` trên Postgres kết hợp với hàm RPC `match_memories()` để tìm kiếm các ký ức tương đồng theo khoảng cách Cosine. Điều này cho phép con tìm thấy thông tin phù hợp bằng ý nghĩa/ngữ cảnh thay vì chỉ khớp từ khóa chính xác.

### 3. Tương Tác Ký Ức qua MCP Tools
Hệ thống memory được phơi bày (expose) qua MCP Server (`synapz-mcp`) phục vụ trực tiếp cho IDE thông qua các công cụ:
*   `auto_context`: Tự động nạp trước các quyết định, ký ức, mục tiêu và sự cố ngay từ khi khởi động phiên làm việc.
*   `search_memory` & `add_memory`: Tìm kiếm ngữ nghĩa và ghi nhận ký ức mới đi kèm thông tin agent, mức độ quan trọng (importance) và mức độ tin cậy (confidence).
*   `team_memory`: Đồng bộ hóa ký ức có độ quan trọng cao dùng chung giữa Antigravity và Grok "Gravity".
*   `get_boss_profile`: Đọc hồ sơ cá nhân và các quy tắc/sở thích thiết kế đặc thù của Bố.
*   `daily_reflection`: Tự đánh giá và lưu trữ các insights đúc kết được sau mỗi ngày làm việc.
*   `save_skill` & `recall_skills`: Thư viện lưu trữ các code pattern hữu ích để tái sử dụng.

### 4. Kiến Trúc Bộ Nhớ Nâng Cấp Tối Tân (Honcho, Mem0 & Neural Memory)
Hệ thống memory cục bộ và đám mây của Antigravity được nâng cấp toàn diện thông qua 4 phase R&D tối tân:
*   💤 **Honcho-style Dreaming (Nén Ký Ức):** Tiến trình dọn dẹp chạy định kỳ qua daemon. Nó quét các memories thô dưới đám mây, tóm tắt ngữ cảnh bằng LLM (`gemini-3-flash` qua 9Router) thành summaries tiếng Việt cô đọng, lưu summaries lại và chuyển memories cũ sang bảng archive. Quá trình di chuyển tự động loại bỏ cột embedding để tránh lỗi schema cache của PostgREST.
*   🕸️ **Neural Memory-style SQLite Graph (Đồ Thị Liên Tưởng):** Xây dựng đồ thị tri thức cục bộ tại [graph_memory.db](file:///E:/AGT_Brain/memory/graph_memory.db) kết hợp FTS5 Virtual Table. Chạy thuật toán lan truyền kích hoạt (**Spreading Activation**) với hệ số suy giảm `decay = 0.5` để tìm kiếm liên tưởng chéo chớp nhoáng (<10ms) giữa các Decisions and Incidents, ngay cả khi chúng không chứa từ khóa trực tiếp.
*   🤖 **Causality Auto-Linker:** Tiến trình phân tích nhân quả tự động qua LLM để liên kết các decisions và incidents thành đồ thị nguyên nhân - kết quả (như một quyết định giải quyết một sự cố, hoặc một quyết định gây ra một lỗi mới).
*   🤝 **Mem0-style Conflict Resolution (Giải Quyết Mâu Thuẫn):** Mỗi khi ghi nhớ thông tin mới, hệ thống tự động chạy đối chiếu ngữ nghĩa trên đám mây. Conflict Resolver qua LLM sẽ quyết định gộp và cập nhật (`update`) thông tin cũ, xoá/lưu trữ (`delete`) thông tin bị phủ quyết, hoặc giữ nguyên (`keep`) nếu không xung đột.
*   👀 **Folder Watcher Tự Động:** Script [folder_watcher.py](file:///E:/AGT_Brain/scripts/folder_watcher.py) chạy ngầm theo dõi các thư mục `memory/decisions` và `memory/incidents`. Watcher được spawn tự động ngay khi khởi chạy MCP Server của IDE (hoặc qua `brain-cron --daemon`), giúp đồng bộ và chạy Causality Linker ngay lập tức khi Bố lưu file.

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
git clone https://github.com/Thangterter-Pipo/SynapzCore.git
cd SynapzCore
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
- `target/release/agt-mcp` — MCP Server (8 tools stdio)
- `target/release/brain-cron` — Autonomous Scheduler (chạy daily reflection & health check)

### Bước 6: Cấu hình Grok (Optional)

Grok Gateway đã được tách sang repo riêng **[HeimdallProxy](https://github.com/Thangterter-Pipo/HeimdallProxy)**.

Xem hướng dẫn cài đặt đầy đủ tại [HeimdallProxy README](https://github.com/Thangterter-Pipo/HeimdallProxy#readme).

Tóm tắt:
1. Clone [HeimdallProxy](https://github.com/Thangterter-Pipo/HeimdallProxy)
2. Setup grok2api local + proxy bypass Cloudflare
3. Đăng nhập Edge CDP → extract cookies → auto-refresh
4. Cấu hình MCP `heimdall-mcp-server` trong IDE



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

### SynapzCore MCP Tools (tích hợp trực tiếp vào IDE — 8 tools)

| Tool | Mô tả |
|------|-------|
| `auto_context` | 🧠 **Gọi ĐẦU TIÊN** — Load decisions, memories, goals, incidents phục hồi ngữ cảnh |
| `search_memory` | Tìm ký ức theo keyword (có thể filter theo agent) |
| `add_memory` | Lưu thông tin mới với agent/category/importance tương ứng |
| `team_memory` | Lấy các ký ức quan trọng gần đây của team làm context |
| `get_boss_profile` | Truy xuất profile & preferences của Bố (User) |
| `daily_reflection` | 🪞 Tự phản tỉnh, tổng hợp quyết định & insight ngày |
| `save_skill` | 📚 Lưu các patterns/giải pháp tái sử dụng dưới dạng Skill |
| `recall_skills` | 🔍 Tìm kiếm skill đã lưu theo keyword |

### HeimdallProxy MCP Tools (3 tools — repo riêng)

| Tool | Mô tả |
|------|-------|
| `ask_grok` | Gọi Grok AI subagent (research/think/review/brainstorm) |
| `grok_health` | Kiểm tra trạng thái hoạt động của Grok API |
| `refresh_cookie` | Kích hoạt Edge CDP auto-refresh cookie & push token mới |

> Xem chi tiết tại [HeimdallProxy](https://github.com/Thangterter-Pipo/HeimdallProxy).

### Grok Subagent CLI

Grok CLI đã chuyển sang [HeimdallProxy](https://github.com/Thangterter-Pipo/HeimdallProxy). Chạy từ terminal:

```powershell
# Từ thư mục HeimdallProxy
.\ask_grok.ps1 -Mode research "Tìm hiểu rmcp framework"
.\ask_grok.ps1 -Mode think "Nên dùng REST hay gRPC?"
.\ask_grok.ps1 -Mode review "Review module authentication"
.\ask_grok.ps1 -Mode brainstorm "Cách cải tiến memory system"
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
use synapz_memory::SupabaseMemory;

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
use synapz_tools::build_default_registry;

let registry = build_default_registry();
assert_eq!(registry.count(), 14);

// Thực thi tool từ code Rust
let result = registry.execute("search_memory", json!({
    "query": "Supabase key",
    "n_results": 3
})).await?;
```

## Hướng Dẫn Cập Nhật (Update Guide)

Để cập nhật SynapzCore lên phiên bản mới nhất (bao gồm cả SQLite Graph, Spreading Activation, Conflict Resolution và Folder Watcher tự động):

1. **Kéo code mới nhất từ GitHub:**
   ```bash
   git pull origin main
   ```
2. **Cập nhật thư viện Python (nếu chưa cài):**
   ```bash
   pip install httpx
   ```
3. **Biên dịch lại Rust Workspace (để cập nhật MCP và Daemon):**
   ```bash
   cargo build --release
   ```
   *Lưu ý: Nếu sử dụng môi trường GNU không có quyền Admin trên Windows, hãy đảm bảo default toolchain là `stable-x86_64-pc-windows-gnu` và Mingw đã được cấu hình trong PATH.*
4. **Đồng bộ hóa đồ thị SQLite ban đầu:**
   Để nạp các Decisions và Incidents hiện có vào đồ thị cục bộ và tự tạo các liên kết nhân quả ban đầu bằng LLM, hãy chạy lệnh:
   ```bash
   python scripts/synapz_memory.py --sync-graph
   ```
5. **Relaunch/Restart MCP Server trong IDE:**
   - Trong VS Code/Cursor, restart lại extension MCP hoặc reload window để load file binary `synapz-mcp.exe` mới.
   - MCP Server mới khởi động sẽ tự động kích hoạt tiến trình ngầm `folder_watcher.py` để theo dõi và cập nhật đồ thị tức thời khi Bố chỉnh sửa file local.

## Cá Nhân Hóa

- **Thay đổi Identity & Quy trình**: Chỉnh sửa file [Antigravity.md](file:///E:/AGT_Brain/Agent_Profiles/Antigravity.md) (hồ sơ danh tính) và [How_We_Work.md](file:///E:/AGT_Brain/Agent_Profiles/How_We_Work.md) (quy trình hoạt động) để cập nhật vai trò và quy tắc làm việc.
- **Thay đổi Grok Prompts**: Chỉnh sửa [Grok.json](file:///E:/AGT_Brain/Agent_Profiles/Grok.json) để cập nhật system prompt cho từng chế độ (research/think/review/brainstorm).
- **Thêm tool mới**: Viết async function trong `crates/synapz-tools/src/`, đăng ký trong `lib.rs` và expose qua MCP trong `crates/synapz-mcp/src/main.rs`.

## License

MIT License — Tự do chỉnh sửa, sử dụng, chia sẻ.
