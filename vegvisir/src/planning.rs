use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    InProgress,
    Blocked,
    Passed,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlanItem {
    pub description: String,
    pub id: String,
    pub status: TaskStatus,
    #[serde(default)]
    pub evidence: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Plan {
    pub goal: String,
    pub items: Vec<PlanItem>,
}

impl Plan {
    pub fn from_goal(goal: impl Into<String>) -> Self {
        let goal = goal.into();
        Self {
            goal: goal.clone(),
            items: vec![PlanItem {
                description: goal,
                id: Uuid::new_v4().simple().to_string(),
                status: TaskStatus::InProgress,
                evidence: Vec::new(),
            }],
        }
    }
}
