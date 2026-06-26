//! git_isolation — Giai đoạn 2: Cấp phát Workspace riêng (Sandbox Isolation).
//!
//! Mỗi agent chạy song song được cấp một NHÁNH GIT riêng (branch-feature-<task>)
//! để không dẫm chân nhau khi cùng sửa code. Orchestrator (kết hợp Git Automation)
//! tạo branch từ một base, agent commit vào branch của mình, rồi GĐ3 merge về base.
//!
//! Toàn bộ dùng `git` qua tokio::process (async, không block). An toàn: KHÔNG ép
//! buộc (--force) trừ khi gọi rõ; tạo branch tách biệt nên không đụng nhánh chính.

use std::path::PathBuf;
use tokio::process::Command;

/// Lỗi thao tác git.
#[derive(Debug)]
pub enum GitError {
    NotARepo(String),
    CommandFailed { cmd: String, stderr: String },
    Spawn(String),
}

impl std::fmt::Display for GitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GitError::NotARepo(p) => write!(f, "Không phải git repo: {}", p),
            GitError::CommandFailed { cmd, stderr } => {
                write!(f, "git thất bại [{}]: {}", cmd, stderr)
            }
            GitError::Spawn(e) => write!(f, "Không spawn được git: {}", e),
        }
    }
}

impl std::error::Error for GitError {}

/// Quản lý cô lập nhánh cho các agent trong một repo.
#[derive(Debug, Clone)]
pub struct GitBranchManager {
    /// Thư mục gốc repo.
    pub repo: PathBuf,
    /// Prefix branch agent (mặc định "agent/").
    pub prefix: String,
}

impl GitBranchManager {
    pub fn new(repo: impl Into<PathBuf>) -> Self {
        Self {
            repo: repo.into(),
            prefix: "agent/".to_string(),
        }
    }

    pub fn with_prefix(mut self, prefix: &str) -> Self {
        self.prefix = prefix.to_string();
        self
    }

    /// Tên branch cô lập cho một task.
    pub fn branch_name(&self, task_id: &str) -> String {
        // chuẩn hóa: thay ký tự không an toàn cho ref git.
        let safe: String = task_id
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '-'
                }
            })
            .collect();
        format!("{}{}", self.prefix, safe)
    }

    /// Chạy một lệnh git trong repo, trả stdout hoặc GitError.
    async fn git(&self, args: &[&str]) -> Result<String, GitError> {
        let mut cmd = Command::new("git");
        cmd.current_dir(&self.repo);
        cmd.args(args);
        let out = cmd
            .output()
            .await
            .map_err(|e| GitError::Spawn(e.to_string()))?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
        } else {
            Err(GitError::CommandFailed {
                cmd: format!("git {}", args.join(" ")),
                stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
            })
        }
    }

    /// Kiểm tra thư mục có phải git repo.
    pub async fn is_repo(&self) -> bool {
        self.git(&["rev-parse", "--is-inside-work-tree"])
            .await
            .is_ok()
    }

    /// Branch hiện tại (để biết base quay về).
    pub async fn current_branch(&self) -> Result<String, GitError> {
        self.git(&["rev-parse", "--abbrev-ref", "HEAD"]).await
    }

    /// CẤP PHÁT: tạo branch cô lập cho task từ `base` rồi checkout sang nó.
    /// Nếu branch đã tồn tại → checkout thẳng (idempotent).
    pub async fn create_isolated(&self, task_id: &str, base: &str) -> Result<String, GitError> {
        if !self.is_repo().await {
            return Err(GitError::NotARepo(self.repo.display().to_string()));
        }
        let branch = self.branch_name(task_id);
        // Đã tồn tại? checkout luôn.
        if self.git(&["rev-parse", "--verify", &branch]).await.is_ok() {
            self.git(&["checkout", &branch]).await?;
        } else {
            // tạo mới từ base.
            self.git(&["checkout", "-b", &branch, base]).await?;
        }
        Ok(branch)
    }

    /// Agent commit toàn bộ thay đổi vào branch của mình.
    /// Trả về true nếu có commit (false nếu working tree sạch, không có gì để commit).
    pub async fn commit_all(&self, message: &str) -> Result<bool, GitError> {
        self.git(&["add", "-A"]).await?;
        // Có gì để commit không?
        if self.git(&["diff", "--cached", "--quiet"]).await.is_ok() {
            return Ok(false); // không có staged change
        }
        self.git(&["commit", "-m", message]).await?;
        Ok(true)
    }

    /// Quay về branch base (sau khi agent xong, trước khi merge).
    pub async fn checkout(&self, branch: &str) -> Result<(), GitError> {
        self.git(&["checkout", branch]).await.map(|_| ())
    }

    /// MERGE branch agent về base hiện tại (no-ff để giữ vết).
    /// Trả Ok(true) nếu merge sạch, Ok(false) nếu có conflict (cần SmartMerge GĐ3).
    pub async fn merge_into_current(&self, branch: &str) -> Result<bool, GitError> {
        match self.git(&["merge", "--no-ff", "--no-edit", branch]).await {
            Ok(_) => Ok(true),
            Err(GitError::CommandFailed { stderr, .. })
                if stderr.contains("CONFLICT") || stderr.contains("conflict") =>
            {
                Ok(false)
            }
            // git merge in conflict ra stdout chứ không phải stderr ở vài version → kiểm trạng thái.
            Err(_) => {
                // Có file conflict không?
                let unmerged = self
                    .git(&["diff", "--name-only", "--diff-filter=U"])
                    .await
                    .unwrap_or_default();
                if !unmerged.is_empty() {
                    Ok(false)
                } else {
                    Ok(true)
                }
            }
        }
    }

    /// Hủy merge đang dang dở (khi conflict không giải được).
    pub async fn abort_merge(&self) -> Result<(), GitError> {
        self.git(&["merge", "--abort"]).await.map(|_| ())
    }

    /// Liệt kê file đang conflict (cho SmartMerge biết phải xử lý gì).
    pub async fn conflicted_files(&self) -> Vec<String> {
        self.git(&["diff", "--name-only", "--diff-filter=U"])
            .await
            .map(|s| {
                s.lines()
                    .map(|l| l.to_string())
                    .filter(|l| !l.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Dọn branch agent sau khi merge xong.
    pub async fn delete_branch(&self, branch: &str, force: bool) -> Result<(), GitError> {
        let flag = if force { "-D" } else { "-d" };
        self.git(&["branch", flag, branch]).await.map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_branch_name_sanitize() {
        let m = GitBranchManager::new(".");
        assert_eq!(m.branch_name("api-login"), "agent/api-login");
        assert_eq!(m.branch_name("feat/pay ment"), "agent/feat-pay-ment");
        assert_eq!(m.branch_name("a.b@c"), "agent/a-b-c");
    }

    #[test]
    fn test_custom_prefix() {
        let m = GitBranchManager::new(".").with_prefix("wip/");
        assert_eq!(m.branch_name("x"), "wip/x");
    }

    /// Test THẬT trên repo tạm: tạo branch cô lập, commit, merge về base.
    #[tokio::test]
    async fn test_real_isolated_branch_lifecycle() {
        let tmp = std::env::temp_dir().join(format!("synapz_git_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let m = GitBranchManager::new(&tmp);
        // init repo + config + commit đầu.
        for args in [
            vec!["init", "-b", "main"],
            vec!["config", "user.email", "test@synapz.local"],
            vec!["config", "user.name", "synapz-test"],
        ] {
            m.git(&args).await.expect("git setup");
        }
        std::fs::write(tmp.join("README.md"), "base\n").unwrap();
        m.git(&["add", "-A"]).await.unwrap();
        m.git(&["commit", "-m", "init"]).await.unwrap();

        assert!(m.is_repo().await);
        let base = m.current_branch().await.unwrap();
        assert_eq!(base, "main");

        // Cấp branch cô lập cho task.
        let branch = m.create_isolated("api-login", "main").await.unwrap();
        assert_eq!(branch, "agent/api-login");

        // Agent ghi file riêng + commit.
        std::fs::write(tmp.join("login.rs"), "fn login() {}\n").unwrap();
        let committed = m.commit_all("feat: login").await.unwrap();
        assert!(committed, "phải có commit");

        // Quay về main + merge.
        m.checkout("main").await.unwrap();
        let clean = m.merge_into_current("agent/api-login").await.unwrap();
        assert!(clean, "merge phải sạch");
        // File từ branch giờ có ở main.
        assert!(tmp.join("login.rs").exists());

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
