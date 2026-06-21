//! File system tools — read, write, append, list, search, exists.

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::fs;
use std::path::Path;

pub async fn read_file(params: Value) -> Result<Value> {
    let path = params.get("path").and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'path' parameter"))?;
    let content = fs::read_to_string(path)?;
    Ok(json!(content))
}

pub async fn write_file(params: Value) -> Result<Value> {
    let path = params.get("path").and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'path'"))?;
    let content = params.get("content").and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'content'"))?;

    if let Some(parent) = Path::new(path).parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)?;
    Ok(json!(format!("✅ Wrote {} bytes to {}", content.len(), path)))
}

pub async fn append_file(params: Value) -> Result<Value> {
    let path = params.get("path").and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'path'"))?;
    let content = params.get("content").and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'content'"))?;

    if let Some(parent) = Path::new(path).parent() {
        fs::create_dir_all(parent)?;
    }
    use std::io::Write;
    let mut file = fs::OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(content.as_bytes())?;
    Ok(json!(format!("✅ Appended {} bytes to {}", content.len(), path)))
}

pub async fn list_dir(params: Value) -> Result<Value> {
    let path = params.get("path").and_then(|v| v.as_str()).unwrap_or(".");
    let entries: Vec<Value> = fs::read_dir(path)?
        .filter_map(|e| e.ok())
        .map(|e| {
            let meta = e.metadata().ok();
            json!({
                "name": e.file_name().to_string_lossy(),
                "is_dir": meta.as_ref().map(|m| m.is_dir()).unwrap_or(false),
                "size": meta.as_ref().and_then(|m| if m.is_file() { Some(m.len()) } else { None }),
            })
        })
        .collect();
    Ok(json!(entries))
}

pub async fn search_files(params: Value) -> Result<Value> {
    let pattern = params.get("pattern").and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'pattern'"))?;
    let matches: Vec<String> = glob::glob(pattern)?
        .filter_map(|p| p.ok())
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    Ok(json!(matches))
}

pub async fn file_exists(params: Value) -> Result<Value> {
    let path = params.get("path").and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'path'"))?;
    Ok(json!(Path::new(path).exists()))
}
