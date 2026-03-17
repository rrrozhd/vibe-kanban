use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

use crate::IssuePriority;

/// Request to create an issue via the overseer API.
/// Accepts `status_name` instead of `status_id` — the server resolves it.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct OverseerCreateIssueRequest {
    pub project_id: Uuid,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Human-readable status name (e.g. "To do", "In progress").
    /// Resolved server-side. Defaults to the first visible status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<IssuePriority>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_issue_id: Option<Uuid>,
    /// Idempotency key — if an open issue with this key already exists, it is returned instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedupe_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct OverseerCreateIssueResponse {
    pub issue: OverseerIssueView,
    /// `"created"` or `"existing"` (when deduped).
    pub outcome: String,
    pub txid: i64,
}

/// Transition an issue to a new status by name.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct OverseerTransitionRequest {
    pub status_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct OverseerTransitionResponse {
    pub issue: OverseerIssueView,
    pub old_status_name: String,
    pub new_status_name: String,
    pub txid: i64,
}

/// Enriched issue view returned by overseer endpoints.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct OverseerIssueView {
    pub id: Uuid,
    pub project_id: Uuid,
    pub simple_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub status_name: String,
    pub status_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<IssuePriority>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_issue_id: Option<Uuid>,
    pub sort_order: f64,
}

/// Full board view: statuses with their issues.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct OverseerBoardColumn {
    pub status_id: Uuid,
    pub status_name: String,
    pub hidden: bool,
    pub issues: Vec<OverseerIssueView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct OverseerBoardResponse {
    pub project_id: Uuid,
    pub columns: Vec<OverseerBoardColumn>,
}
