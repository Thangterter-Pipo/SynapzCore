# SynapzCore — Open-Source Roadmap

> Tài liệu này là **hướng phát triển đo được** để đưa SynapzCore từ "bộ não AI cá nhân"
> thành một dự án Coding Agents công khai mà người lạ clone–build–chạy được.
>
> Nền tảng triết lý (dbs-goal / Wittgenstein): *"mạnh mẽ nhất" và "được nhiều người quan tâm"*
> KHÔNG phải mục tiêu — đó là **điểm số tự xuất hiện SAU KHI** người lạ build & chạy được repo.
> Việc thật bây giờ: gỡ rào cản onboarding + chứng minh vì sao đáng theo dõi.

---

## 0. Mục tiêu đã được audit (thay cho khẩu hiệu)

| Khẩu hiệu (空转) | Mục tiêu đo được (làm việc) |
|---|---|
| "mạnh mẽ nhất" | Người lạ `git clone` → `cargo build` xanh → chạy MCP server trong **≤ 5 phút** |
| "được nhiều người quan tâm" | Đếm SAU: GitHub stars / issues / forks — chỉ là hệ quả, không phải task |
| "hướng phát triển" | Roadmap 4 giai đoạn dưới đây, mỗi mốc có tiêu chí tick được |

**Một câu:** Repo đáng quan tâm khi người lạ chạy được nó mà không cần hỏi tác giả.

---

## 1. Đánh giá hiện trạng (đa góc, 2026-06)

**Kỹ thuật**
- 4 crate Rust (edition 2024), ~5.9k LOC, `cargo build` **xanh** (đã verify).
- Kiến trúc rõ: `memory` (Supabase) · `tools` (14 tool + CDP) · `mcp` (12 tool stdio) · `orchestrator` (multi-agent: task graph, git isolation, smart merge, self-correct).
- Điểm mạnh hiếm: orchestrator có git-isolation + smart-merge + self-correct viết bằng Rust — đây là USP.

**Bảo mật (góc opensource quan trọng nhất)**
- ✅ Không có API key/secret hardcode trong source (chỉ tham chiếu path config).
- ✅ `.gitignore` chặn `.env`, `/data/`, `*_config.json`, `/memory/` — secrets không bị track.
- ⚠️ Còn vài đường dẫn tuyệt đối `E:\AGT_Brain` (đã sửa `search_code` → dùng `SYNAPZ_ROOT`/cwd; `brain_cron.rs` còn default cứng nhưng có env override).

**Onboarding / cộng đồng (rào cản cho người lạ)**
- ✅ README, LICENSE (MIT), remote GitHub public.
- ❌ README viết 100% tiếng Việt → giới hạn người quan tâm quốc tế.
- ❌ Thiếu: CONTRIBUTING.md, CODE_OF_CONDUCT, issue/PR template, CI (GitHub Actions), `.env.example` cho Supabase, hướng dẫn "chạy thử không cần Supabase".
- ❌ Phụ thuộc cứng Supabase cloud → người lạ không có config sẽ không chạy được memory.

**Định vị**
- Hiện mô tả là "bộ não AI cá nhân tự trị" (1 user). Để opensource cần đóng gói lại thành
  "framework multi-agent coding orchestrator có memory" — phần người khác tái dùng được.

---

## 2. Roadmap 4 giai đoạn (mỗi mốc tick được)

### Phase 1 — Runnable by strangers (rào cản onboarding) ⭐ ưu tiên cao nhất
- [x] `cargo build --workspace` + `cargo test --workspace` xanh, không cần secret (cloud test tự skip).
- [ ] `.env.example` đầy đủ biến (`SYNAPZ_ROOT`, Supabase URL/key) + README mục "5-minute start".
- [x] Offline/local memory: `recall`/`fetch_recent` fallback đọc JSONL queue khi Supabase lỗi (có test).
- [x] Xóa hết đường dẫn tuyệt đối: `search_code` + `brain_cron.rs` dùng `synapz_root()` (env→exe→cwd).
- [x] README có mục **English Quickstart (5-minute)** ở đầu file.
- **Done khi:** một người chưa từng thấy repo chạy được MCP server theo README, không nhắn tác giả.

### Phase 2 — Trust signals (tín hiệu cộng đồng tin được)
- [x] CI GitHub Actions (`.github/workflows/ci.yml`): **4 gate cứng** fmt + clippy `-D warnings` + build + test.
- [x] `CONTRIBUTING.md` (EN) + issue templates (bug/feature) + PR template + `SECURITY.md`.
- [x] Badge: CI status + license trong README.
- [x] `cargo test` xanh (62 tests; orchestrator 46) — clippy `-D warnings` sạch toàn workspace.
- **Done khi:** PR từ người ngoài chạy qua CI tự động, có hướng dẫn đóng góp rõ.

### Phase 3 — Reusable framework (biến thành thứ người khác dùng)
- [x] Roster orchestrator tách khỏi config cá nhân: `default_roster()` + override `SYNAPZ_AGENTS` (model generic, không hardcode tên model riêng), có test.
- [ ] Tài liệu: kiến trúc MCP, cách thêm tool mới, cách cắm IDE khác (Cursor/VS Code/Claude).
- [x] Demo e2e offline: `examples/demo_graph.json` + `--pipeline --echo` (fan-out→merge→self-correct), không cần cloud/LLM. Có `examples/README.md`.
- [x] Fix bug crash: UTF-8 byte-slice panic (mcp/orchestrator/brain_cron) → `truncate_chars()` char-safe, có regression test.
- [ ] Quickstart cho 2-3 IDE phổ biến.
- **Done khi:** người khác cắm SynapzCore vào dự án của họ mà không fork-sửa-cứng.

### Phase 4 — Discoverability (để "nhiều người" tìm thấy — hệ quả, không phải gốc)
- [ ] Trang README có GIF/demo 30s + "why SynapzCore vs X".
- [ ] Bài viết kỹ thuật về USP (Rust orchestrator + git isolation + memory).
- [x] `CHANGELOG.md` (Keep a Changelog) + crates đã ở version 0.1.0. Tag/push remote chờ Bố duyệt.
- **Đo:** stars/issues/forks theo dõi — KPI quan sát, không phải việc để "làm".

---

## 3. USP — vì sao đáng quan tâm (1 dòng mỗi cái)

- **Rust orchestrator** thật (không phải script Python) cho multi-agent coding.
- **Git isolation + smart merge**: nhiều agent code song song không đạp nhau.
- **Memory dài hạn** pgvector + spreading-activation, không chỉ context window.
- **MCP-native**: cắm thẳng vào IDE qua 12 tool stdio.
- **Self-correct loop** có giới hạn retry — chống agent lặp vô hạn.

---

## 4. Việc đã triển khai trong vòng này

- Audit mục tiêu bằng dbs-goal → thay khẩu hiệu bằng tiêu chí đo được.
- Phân tích đa góc: kỹ thuật / bảo mật / onboarding / định vị.
- Verify `cargo build` xanh trên toàn workspace.
- Fix rào cản portability: `search_code` bỏ hardcode `E:\AGT_Brain`, dùng `SYNAPZ_ROOT`/cwd.
- Đồng bộ `structure.md` (workflow SDAF-8) với hạ tầng thật.

## 5. Việc tiếp theo gần nhất (theo Phase 1)

1. Viết `.env.example` + mục "5-minute start" trong README (EN).
2. (done) Phase 2: repo fmt/clippy-clean, CI enforce `-D warnings`, badges.
3. Phase 4: CHANGELOG xong; còn `git tag v0.1.0` + push (cần Bố duyệt) + demo GIF (cosmetic).


