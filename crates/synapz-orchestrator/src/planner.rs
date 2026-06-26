//! planner — #3: Tự phân rã mục tiêu ngôn ngữ tự nhiên thành TaskGraph.
//!
//! Orchestrator (Kiro) nhận một mục tiêu cấp cao của Bố ("xây API login + UI"),
//! gọi LLM (qua CLI agent đã verify, vd Claude qua 9router) để sinh ra một TaskGraph
//! JSON, rồi validate. Đây là mảnh ĐÓNG VÒNG tự trị: con người ra lệnh cấp cao →
//! hệ tự phân rã thành các task song song có phụ thuộc → giao builder chạy.
//!
//! Tách 2 phần để test được không cần LLM:
//!
//! - build_planning_prompt(goal): dựng prompt (thuần, test nội dung).
//! - extract_graph_json(llm_output): trích JSON từ output LLM (kèm fence/prose), parse thành TaskGraph + validate (thuần, test bằng chuỗi mẫu).
//!
//! Phần gọi LLM (plan_with_llm) dùng CliInvocation.run đã verify.

use crate::runner::CliInvocation;
use crate::task_graph::TaskGraph;

/// Dựng prompt yêu cầu LLM sinh TaskGraph JSON.
/// Ràng buộc rõ: chỉ trả JSON, role thuộc {Coder, Builder, Tester, Researcher},
/// depends_on tham chiếu id hợp lệ, không chu trình.
pub fn build_planning_prompt(goal: &str) -> String {
    format!(
        r#"Bạn là bộ phân rã kế hoạch (planner) của một hệ điều phối đa tác nhân.
Nhiệm vụ: chia mục tiêu sau thành các task nhỏ chạy được, có quan hệ phụ thuộc.

MỤC TIÊU: {goal}

QUY TẮC ĐẦU RA — TUYỆT ĐỐI TUÂN THỦ:
1. CHỈ in ra MỘT khối JSON, KHÔNG giải thích, KHÔNG văn bản thừa.
2. Cấu trúc: {{"nodes": [{{"id","prompt","role","depends_on"}}]}}
   - id: chuỗi định danh ngắn, duy nhất (vd "api-login").
   - prompt: mô tả việc cụ thể giao cho agent.
   - role: MỘT trong {{"Coder","Builder","Tester","Researcher"}} (đúng hoa/thường).
   - depends_on: mảng id các task phải xong TRƯỚC (mảng rỗng nếu không phụ thuộc).
3. Task độc lập để depends_on=[] (chúng sẽ chạy SONG SONG).
4. KHÔNG tạo chu trình phụ thuộc. Mọi id trong depends_on phải tồn tại.
5. Gợi ý vai trò: Coder=viết code, Builder=ghép/dựng/triển khai, Tester=kiểm thử, Researcher=phân tích/lên kế hoạch.

Ví dụ định dạng:
{{"nodes":[{{"id":"a","prompt":"...","role":"Coder","depends_on":[]}},{{"id":"b","prompt":"...","role":"Tester","depends_on":["a"]}}]}}"#,
        goal = goal
    )
}

/// Trích khối JSON TaskGraph từ output LLM (chịu được ```json fence và prose xung quanh),
/// parse + validate. Trả Err mô tả nếu không tìm được JSON hợp lệ hoặc graph sai.
pub fn extract_graph_json(llm_output: &str) -> Result<TaskGraph, String> {
    let json_str = slice_json_object(llm_output)
        .ok_or_else(|| "Không tìm thấy khối JSON {...} trong output LLM".to_string())?;
    let graph: TaskGraph = serde_json::from_str(&json_str)
        .map_err(|e| format!("JSON không parse được thành TaskGraph: {e}"))?;
    if graph.is_empty() {
        return Err("TaskGraph rỗng (không có node nào)".to_string());
    }
    graph
        .validate()
        .map_err(|e| format!("TaskGraph không hợp lệ: {e}"))?;
    Ok(graph)
}

/// Lấy chuỗi từ dấu '{' đầu tiên tới '}' khớp ngoặc cuối cùng (cân bằng ngoặc,
/// bỏ qua ngoặc trong chuỗi "..."). Chịu được ```json fence và văn bản thừa.
fn slice_json_object(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let start = s.find('{')?;
    let mut depth = 0i32;
    let mut in_str = false;
    let mut escaped = false;
    for i in start..bytes.len() {
        let c = bytes[i] as char;
        if in_str {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(s[start..=i].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

/// Gọi LLM qua CLI agent để sinh kế hoạch, rồi trích + validate thành TaskGraph.
pub async fn plan_with_llm(
    goal: &str,
    inv: &CliInvocation,
    timeout_secs: u64,
) -> Result<TaskGraph, String> {
    let prompt = build_planning_prompt(goal);
    let output = inv.run(&prompt, timeout_secs).await?;
    extract_graph_json(&output)
}

/// Gọi 9router (OpenAI-compatible /v1/chat/completions) TRỰC TIẾP để lập kế hoạch.
/// Đáng tin hơn shell ra CLI agent (vốn dễ lỗi config/auth). Response 9router có đuôi
/// "data: [DONE]" sau JSON → ta trích JSON object đầu tiên rồi lấy message.content.
pub async fn plan_with_9router(
    goal: &str,
    base_url: &str,
    key: &str,
    model: &str,
    timeout_secs: u64,
) -> Result<TaskGraph, String> {
    let prompt = build_planning_prompt(goal);
    let url = format!("{}/v1/chat/completions", base_url.trim_end_matches('/'));
    let body = serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": prompt}],
        "max_tokens": 2000,
    });
    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {key}"))
        .header("Content-Type", "application/json")
        .json(&body)
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .send()
        .await
        .map_err(|e| format!("9router request lỗi: {e}"))?;
    if !resp.status().is_success() {
        let code = resp.status();
        let t = resp.text().await.unwrap_or_default();
        let snippet: String = t.chars().take(200).collect();
        return Err(format!("9router {code}: {snippet}"));
    }
    let raw = resp
        .text()
        .await
        .map_err(|e| format!("đọc body lỗi: {e}"))?;
    let obj = slice_json_object(&raw)
        .ok_or_else(|| "không tìm thấy JSON trong response 9router".to_string())?;
    let v: serde_json::Value =
        serde_json::from_str(&obj).map_err(|e| format!("parse response 9router lỗi: {e}"))?;
    let content = v
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .ok_or_else(|| "response 9router thiếu choices[0].message.content".to_string())?;
    extract_graph_json(content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prompt_contains_goal_and_rules() {
        let p = build_planning_prompt("xây website bán hàng");
        assert!(p.contains("xây website bán hàng"));
        assert!(p.contains("Coder") && p.contains("Builder") && p.contains("Tester"));
        assert!(p.contains("depends_on"));
    }

    #[test]
    fn test_extract_bare_json() {
        let out = r#"{"nodes":[{"id":"a","prompt":"code","role":"Coder","depends_on":[]}]}"#;
        let g = extract_graph_json(out).unwrap();
        assert_eq!(g.len(), 1);
        assert_eq!(g.nodes[0].id, "a");
    }

    #[test]
    fn test_extract_fenced_json_with_prose() {
        let out = "Đây là kế hoạch của tôi:\n```json\n{\"nodes\":[\
            {\"id\":\"api\",\"prompt\":\"code api\",\"role\":\"Coder\",\"depends_on\":[]},\
            {\"id\":\"test\",\"prompt\":\"kiểm thử\",\"role\":\"Tester\",\"depends_on\":[\"api\"]}\
            ]}\n```\nHy vọng giúp được bạn!";
        let g = extract_graph_json(out).unwrap();
        assert_eq!(g.len(), 2);
        let layers = g.layers().unwrap();
        assert_eq!(layers.len(), 2); // api tầng 0, test tầng 1
    }

    #[test]
    fn test_extract_rejects_no_json() {
        assert!(extract_graph_json("xin lỗi tôi không thể").is_err());
    }

    #[test]
    fn test_extract_rejects_cycle() {
        let out = r#"{"nodes":[
            {"id":"a","prompt":"x","role":"Coder","depends_on":["b"]},
            {"id":"b","prompt":"y","role":"Coder","depends_on":["a"]}
        ]}"#;
        let err = extract_graph_json(out).unwrap_err();
        assert!(err.contains("không hợp lệ") || err.to_lowercase().contains("chu trình"));
    }

    #[test]
    fn test_extract_rejects_unknown_dep() {
        let out = r#"{"nodes":[{"id":"a","prompt":"x","role":"Coder","depends_on":["ghost"]}]}"#;
        assert!(extract_graph_json(out).is_err());
    }

    #[test]
    fn test_slice_json_ignores_braces_in_strings() {
        let out = r#"prefix {"nodes":[{"id":"a","prompt":"dùng {placeholder}","role":"Coder","depends_on":[]}]} suffix"#;
        let g = extract_graph_json(out).unwrap();
        assert_eq!(g.nodes[0].prompt, "dùng {placeholder}");
    }
}
