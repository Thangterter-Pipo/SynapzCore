//! Web tools — HTTP GET/POST via reqwest.

use anyhow::{Result, anyhow};
use serde_json::{Value, json};

pub async fn http_get(params: Value) -> Result<Value> {
    let url = params
        .get("url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'url'"))?;
    let timeout = params.get("timeout").and_then(|v| v.as_u64()).unwrap_or(15);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout))
        .build()?;

    let resp = client.get(url).send().await?;
    let status = resp.status().as_u16();
    let body = resp.text().await?;

    Ok(json!({
        "status": status,
        "body": &body[..body.len().min(5000)],
    }))
}

pub async fn http_post(params: Value) -> Result<Value> {
    let url = params
        .get("url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'url'"))?;
    let json_body = params.get("json_body");
    let timeout = params.get("timeout").and_then(|v| v.as_u64()).unwrap_or(15);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout))
        .build()?;

    let mut req = client.post(url);
    if let Some(body) = json_body {
        req = req.json(body);
    }

    let resp = req.send().await?;
    let status = resp.status().as_u16();
    let body = resp.text().await?;

    Ok(json!({
        "status": status,
        "body": &body[..body.len().min(5000)],
    }))
}
