//! Reflection — save decisions and incidents as append-only markdown.

use anyhow::Result;
use chrono::Utc;
use std::fs;
use std::path::Path;

/// Save an architectural/technical decision.
pub fn save_decision(
    base_dir: &str,
    title: &str,
    context: &str,
    decision: &str,
    rationale: &str,
) -> Result<String> {
    let dir = Path::new(base_dir).join("memory").join("decisions");
    fs::create_dir_all(&dir)?;

    let timestamp = Utc::now().format("%Y%m%d_%H%M%S");
    let slug: String = title.to_lowercase().replace(' ', "_").chars().take(30).collect();
    let filename = format!("{timestamp}_{slug}.md");

    let content = format!(
        "# {title}\n\n**Date**: {}\n**Author**: Antigravity\n\n## Context\n{context}\n\n## Decision\n{decision}\n\n## Rationale\n{rationale}\n",
        Utc::now().to_rfc3339()
    );

    let filepath = dir.join(&filename);
    fs::write(&filepath, content)?;
    Ok(format!("✅ Decision saved: {}", filepath.display()))
}

/// Save an incident/bug report.
pub fn save_incident(
    base_dir: &str,
    title: &str,
    what_happened: &str,
    root_cause: &str,
    lesson: &str,
) -> Result<String> {
    let dir = Path::new(base_dir).join("memory").join("incidents");
    fs::create_dir_all(&dir)?;

    let timestamp = Utc::now().format("%Y%m%d_%H%M%S");
    let slug: String = title.to_lowercase().replace(' ', "_").chars().take(30).collect();
    let filename = format!("{timestamp}_{slug}.md");

    let content = format!(
        "# Incident: {title}\n\n**Date**: {}\n**Severity**: Medium\n\n## What Happened\n{what_happened}\n\n## Root Cause\n{root_cause}\n\n## Lesson Learned\n{lesson}\n",
        Utc::now().to_rfc3339()
    );

    let filepath = dir.join(&filename);
    fs::write(&filepath, content)?;
    Ok(format!("✅ Incident saved: {}", filepath.display()))
}
