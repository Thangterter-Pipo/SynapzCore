//! Goal Manager — persistent JSON-based goal tracking.

use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Goal {
    pub id: String,
    pub title: String,
    pub description: String,
    pub priority: u8,
    pub status: String, // pending, active, completed, failed
    pub steps: Vec<GoalStep>,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub reflection: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalStep {
    pub text: String,
    pub done: bool,
    pub at: String,
}

pub struct GoalManager {
    goals: Vec<Goal>,
    path: PathBuf,
}

impl GoalManager {
    pub fn new(path: &str) -> Self {
        let path = PathBuf::from(path);
        let goals = if path.exists() {
            fs::read_to_string(&path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        Self { goals, path }
    }

    fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&self.goals)?;
        fs::write(&self.path, json)?;
        Ok(())
    }

    pub fn add(&mut self, title: &str, description: &str, priority: u8) -> Result<&Goal> {
        let goal = Goal {
            id: Uuid::new_v4().to_string()[..8].to_string(),
            title: title.to_string(),
            description: description.to_string(),
            priority,
            status: "pending".to_string(),
            steps: Vec::new(),
            created_at: Utc::now().to_rfc3339(),
            completed_at: None,
            reflection: None,
        };
        self.goals.push(goal);
        self.save()?;
        Ok(self.goals.last().unwrap())
    }

    pub fn get_active(&self) -> Vec<&Goal> {
        let mut active: Vec<&Goal> = self
            .goals
            .iter()
            .filter(|g| g.status == "pending" || g.status == "active")
            .collect();
        active.sort_by(|a, b| b.priority.cmp(&a.priority));
        active
    }

    pub fn update_status(
        &mut self,
        id: &str,
        status: &str,
        reflection: Option<&str>,
    ) -> Result<bool> {
        if let Some(goal) = self.goals.iter_mut().find(|g| g.id == id) {
            goal.status = status.to_string();
            if status == "completed" || status == "failed" {
                goal.completed_at = Some(Utc::now().to_rfc3339());
            }
            if let Some(r) = reflection {
                goal.reflection = Some(r.to_string());
            }
            self.save()?;
            return Ok(true);
        }
        Ok(false)
    }

    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({
            "total": self.goals.len(),
            "pending": self.goals.iter().filter(|g| g.status == "pending").count(),
            "active": self.goals.iter().filter(|g| g.status == "active").count(),
            "completed": self.goals.iter().filter(|g| g.status == "completed").count(),
            "failed": self.goals.iter().filter(|g| g.status == "failed").count(),
        })
    }
}
