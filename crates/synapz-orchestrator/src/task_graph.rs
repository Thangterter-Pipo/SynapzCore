//! task_graph — Giai đoạn 1: Bẻ nhánh đồ thị & phân tích phụ thuộc.
//!
//! Một dự án lớn được mô tả thành tập TaskNode, mỗi node khai báo nó phụ thuộc
//! vào những task nào (`depends_on`). Orchestrator chạy thuật toán phân tầng
//! (topological layering / Kahn) để tìm ra các NHÓM task có thể chạy SONG SONG:
//! mọi task trong cùng một tầng không phụ thuộc lẫn nhau → fan-out đồng thời.

use crate::roles::AgentRole;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};

/// Một đơn vị công việc trong đồ thị dự án.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskNode {
    /// Định danh duy nhất (vd "api-login", "ui-frontend").
    pub id: String,
    /// Mô tả/prompt giao cho agent.
    pub prompt: String,
    /// Role phù hợp xử lý task này (None = bất kỳ agent rảnh nào).
    pub role: Option<AgentRole>,
    /// Các task_id phải hoàn thành TRƯỚC khi task này chạy.
    pub depends_on: Vec<String>,
}

impl TaskNode {
    pub fn new(id: impl Into<String>, prompt: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            prompt: prompt.into(),
            role: None,
            depends_on: Vec::new(),
        }
    }

    pub fn with_role(mut self, role: AgentRole) -> Self {
        self.role = Some(role);
        self
    }

    pub fn depends_on(mut self, deps: &[&str]) -> Self {
        self.depends_on = deps.iter().map(|s| s.to_string()).collect();
        self
    }
}

/// Lỗi khi phân tích đồ thị.
#[derive(Debug, PartialEq, Eq)]
pub enum GraphError {
    /// Tồn tại chu trình phụ thuộc (A→B→A) — không thể xếp lịch.
    CycleDetected(Vec<String>),
    /// Một task phụ thuộc vào id không tồn tại.
    UnknownDependency { task: String, missing: String },
    /// Trùng id task.
    DuplicateId(String),
}

impl std::fmt::Display for GraphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GraphError::CycleDetected(ids) => {
                write!(f, "Phát hiện chu trình phụ thuộc giữa: {}", ids.join(" → "))
            }
            GraphError::UnknownDependency { task, missing } => {
                write!(
                    f,
                    "Task '{}' phụ thuộc vào '{}' không tồn tại",
                    task, missing
                )
            }
            GraphError::DuplicateId(id) => write!(f, "Trùng task id: '{}'", id),
        }
    }
}

impl std::error::Error for GraphError {}

/// Đồ thị dự án — tập task + quan hệ phụ thuộc.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskGraph {
    pub nodes: Vec<TaskNode>,
}

impl TaskGraph {
    pub fn new() -> Self {
        Self { nodes: Vec::new() }
    }

    pub fn add(&mut self, node: TaskNode) -> &mut Self {
        self.nodes.push(node);
        self
    }

    /// Kiểm tra tính hợp lệ: không trùng id, không dep lạ, không chu trình.
    pub fn validate(&self) -> Result<(), GraphError> {
        // 1. Trùng id?
        let mut seen = HashSet::new();
        for n in &self.nodes {
            if !seen.insert(n.id.as_str()) {
                return Err(GraphError::DuplicateId(n.id.clone()));
            }
        }
        // 2. Dep lạ?
        for n in &self.nodes {
            for dep in &n.depends_on {
                if !seen.contains(dep.as_str()) {
                    return Err(GraphError::UnknownDependency {
                        task: n.id.clone(),
                        missing: dep.clone(),
                    });
                }
            }
        }
        // 3. Chu trình? (chạy layering thử — nếu còn node chưa xếp được → cycle)
        self.layers().map(|_| ())
    }

    /// PHÂN TÍCH PHỤ THUỘC → trả về các TẦNG song song.
    /// Mỗi Vec<&TaskNode> bên trong là một nhóm task chạy ĐỒNG THỜI.
    /// Thuật toán Kahn (BFS topological): tầng 0 = task không phụ thuộc gì,
    /// tầng kế = task mà mọi dep đã nằm ở tầng trước.
    pub fn layers(&self) -> Result<Vec<Vec<&TaskNode>>, GraphError> {
        let mut index: HashMap<&str, &TaskNode> = HashMap::new();
        let mut indegree: HashMap<&str, usize> = HashMap::new();
        // children: dep → các task phụ thuộc vào nó
        let mut children: HashMap<&str, Vec<&str>> = HashMap::new();

        for n in &self.nodes {
            index.insert(n.id.as_str(), n);
            indegree.entry(n.id.as_str()).or_insert(0);
        }
        for n in &self.nodes {
            for dep in &n.depends_on {
                *indegree.get_mut(n.id.as_str()).unwrap() += 1;
                children
                    .entry(dep.as_str())
                    .or_default()
                    .push(n.id.as_str());
            }
        }

        // Tầng đầu: indegree == 0.
        let mut queue: VecDeque<&str> = indegree
            .iter()
            .filter(|&(_, &d)| d == 0)
            .map(|(&id, _)| id)
            .collect();

        let mut layers: Vec<Vec<&TaskNode>> = Vec::new();
        let mut processed = 0usize;

        while !queue.is_empty() {
            // Toàn bộ phần tử hiện trong queue = 1 tầng song song.
            let mut layer_ids: Vec<&str> = queue.drain(..).collect();
            layer_ids.sort_unstable(); // ổn định thứ tự để test xác định
            let mut next: VecDeque<&str> = VecDeque::new();

            for id in &layer_ids {
                processed += 1;
                if let Some(kids) = children.get(id) {
                    for &child in kids {
                        let d = indegree.get_mut(child).unwrap();
                        *d -= 1;
                        if *d == 0 {
                            next.push_back(child);
                        }
                    }
                }
            }
            layers.push(layer_ids.iter().map(|id| *index.get(id).unwrap()).collect());
            queue = next;
        }

        // Nếu chưa xử lý hết node → có chu trình.
        if processed != self.nodes.len() {
            let stuck: Vec<String> = self
                .nodes
                .iter()
                .filter(|n| *indegree.get(n.id.as_str()).unwrap() > 0)
                .map(|n| n.id.clone())
                .collect();
            return Err(GraphError::CycleDetected(stuck));
        }

        Ok(layers)
    }

    /// Tổng số task.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Độ song song tối đa = kích thước tầng lớn nhất.
    pub fn max_parallelism(&self) -> usize {
        self.layers()
            .map(|ls| ls.iter().map(|l| l.len()).max().unwrap_or(0))
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Website: API login & UI frontend độc lập (tầng 0), integration phụ thuộc cả 2 (tầng 1).
    #[test]
    fn test_website_parallel_layers() {
        let mut g = TaskGraph::new();
        g.add(TaskNode::new("api-login", "code API login").with_role(AgentRole::Coder));
        g.add(TaskNode::new("ui-frontend", "design UI").with_role(AgentRole::Coder));
        g.add(TaskNode::new("integration", "ghép FE+BE").depends_on(&["api-login", "ui-frontend"]));

        g.validate().expect("graph hợp lệ");
        let layers = g.layers().unwrap();

        // Tầng 0: 2 task song song.
        assert_eq!(layers.len(), 2);
        assert_eq!(layers[0].len(), 2);
        assert_eq!(layers[1].len(), 1);
        assert_eq!(layers[1][0].id, "integration");
        assert_eq!(g.max_parallelism(), 2);
    }

    #[test]
    fn test_detect_cycle() {
        let mut g = TaskGraph::new();
        g.add(TaskNode::new("a", "A").depends_on(&["b"]));
        g.add(TaskNode::new("b", "B").depends_on(&["a"]));
        match g.validate() {
            Err(GraphError::CycleDetected(_)) => {}
            other => panic!("kỳ vọng CycleDetected, nhận {:?}", other),
        }
    }

    #[test]
    fn test_unknown_dependency() {
        let mut g = TaskGraph::new();
        g.add(TaskNode::new("a", "A").depends_on(&["ghost"]));
        match g.validate() {
            Err(GraphError::UnknownDependency { task, missing }) => {
                assert_eq!(task, "a");
                assert_eq!(missing, "ghost");
            }
            other => panic!("kỳ vọng UnknownDependency, nhận {:?}", other),
        }
    }

    #[test]
    fn test_duplicate_id() {
        let mut g = TaskGraph::new();
        g.add(TaskNode::new("a", "A"));
        g.add(TaskNode::new("a", "A2"));
        assert_eq!(g.validate(), Err(GraphError::DuplicateId("a".into())));
    }

    /// Chuỗi tuyến tính a→b→c → 3 tầng, mỗi tầng 1 task.
    #[test]
    fn test_linear_chain() {
        let mut g = TaskGraph::new();
        g.add(TaskNode::new("a", "A"));
        g.add(TaskNode::new("b", "B").depends_on(&["a"]));
        g.add(TaskNode::new("c", "C").depends_on(&["b"]));
        let layers = g.layers().unwrap();
        assert_eq!(layers.len(), 3);
        assert_eq!(g.max_parallelism(), 1);
    }
}
