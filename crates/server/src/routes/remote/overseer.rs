use api_types::{
    IssuePriority, OverseerBoardColumn, OverseerBoardResponse, OverseerCreateIssueRequest,
    OverseerCreateIssueResponse, OverseerIssueView, OverseerTransitionRequest,
    OverseerTransitionResponse,
};
use axum::{
    Router,
    extract::{Json, Path, Query, State},
    response::Json as ResponseJson,
    routing::{get, post},
};
use db::models::{overseer_issue::OverseerIssue, project_status::ProjectStatus};
use deployment::Deployment;
use serde::Deserialize;
use utils::response::ApiResponse;
use uuid::Uuid;

use crate::{DeploymentImpl, error::ApiError};

#[derive(Debug, Deserialize)]
pub struct BoardQuery {
    pub project_id: Uuid,
}

pub fn router() -> Router<DeploymentImpl> {
    Router::new()
        .route("/overseer/board", get(get_board))
        .route("/overseer/issues", post(create_issue))
        .route(
            "/overseer/issues/{issue_id}/transition",
            post(transition_issue),
        )
}

fn issue_to_view(issue: &OverseerIssue, status_name: &str) -> OverseerIssueView {
    let priority = issue.priority.as_deref().and_then(|p| match p {
        "Urgent" => Some(IssuePriority::Urgent),
        "High" => Some(IssuePriority::High),
        "Medium" => Some(IssuePriority::Medium),
        "Low" => Some(IssuePriority::Low),
        _ => None,
    });

    OverseerIssueView {
        id: issue.id,
        project_id: issue.project_id,
        simple_id: issue.simple_id.clone(),
        title: issue.title.clone(),
        description: issue.description.clone(),
        status_name: status_name.to_string(),
        status_id: issue.status_id,
        priority,
        parent_issue_id: issue.parent_issue_id,
        sort_order: issue.sort_order,
    }
}

async fn get_board(
    State(deployment): State<DeploymentImpl>,
    Query(query): Query<BoardQuery>,
) -> Result<ResponseJson<ApiResponse<OverseerBoardResponse>>, ApiError> {
    let pool = &deployment.db().pool;
    let statuses = ProjectStatus::ensure_defaults_exist(pool, query.project_id).await?;

    let mut columns = Vec::new();
    for status in &statuses {
        let issues =
            OverseerIssue::find_by_project_and_status(pool, query.project_id, status.id).await?;
        let issue_views: Vec<OverseerIssueView> = issues
            .iter()
            .map(|i| issue_to_view(i, &status.name))
            .collect();

        columns.push(OverseerBoardColumn {
            status_id: status.id,
            status_name: status.name.clone(),
            hidden: status.hidden,
            issues: issue_views,
        });
    }

    Ok(ResponseJson(ApiResponse::success(OverseerBoardResponse {
        project_id: query.project_id,
        columns,
    })))
}

async fn create_issue(
    State(deployment): State<DeploymentImpl>,
    Json(payload): Json<OverseerCreateIssueRequest>,
) -> Result<ResponseJson<ApiResponse<OverseerCreateIssueResponse>>, ApiError> {
    let pool = &deployment.db().pool;

    // Ensure default statuses exist
    let statuses = ProjectStatus::ensure_defaults_exist(pool, payload.project_id).await?;

    // Check dedupe
    if let Some(ref dedupe_key) = payload.dedupe_key {
        if let Some(existing) =
            OverseerIssue::find_by_dedupe_key(pool, payload.project_id, dedupe_key).await?
        {
            let status_name = statuses
                .iter()
                .find(|s| s.id == existing.status_id)
                .map(|s| s.name.as_str())
                .unwrap_or("Unknown");

            return Ok(ResponseJson(ApiResponse::success(
                OverseerCreateIssueResponse {
                    issue: issue_to_view(&existing, status_name),
                    outcome: "existing".to_string(),
                    txid: 0,
                },
            )));
        }
    }

    // Resolve status
    let status = if let Some(ref status_name) = payload.status_name {
        ProjectStatus::find_by_name(pool, payload.project_id, status_name)
            .await?
            .ok_or_else(|| ApiError::BadRequest(format!("Status '{}' not found", status_name)))?
    } else {
        // Default to first status (Backlog)
        statuses
            .first()
            .cloned()
            .ok_or_else(|| ApiError::BadRequest("No statuses found for project".to_string()))?
    };

    // Compute sort order (top of column)
    let sort_order = OverseerIssue::min_sort_order(pool, payload.project_id, status.id).await?;

    // Generate issue number and simple_id
    let issue_number = OverseerIssue::next_issue_number(pool, payload.project_id).await?;
    let simple_id = format!("OVR-{}", issue_number);

    // Build extension_metadata
    let ext_meta = if let Some(ref dedupe_key) = payload.dedupe_key {
        serde_json::json!({"overseer": {"dedupe_key": dedupe_key}}).to_string()
    } else {
        "{}".to_string()
    };

    let priority_str = payload.priority.as_ref().map(|p| match p {
        IssuePriority::Urgent => "Urgent",
        IssuePriority::High => "High",
        IssuePriority::Medium => "Medium",
        IssuePriority::Low => "Low",
    });

    let id = Uuid::new_v4();
    let issue = OverseerIssue::create(
        pool,
        id,
        payload.project_id,
        issue_number,
        &simple_id,
        status.id,
        &payload.title,
        payload.description.as_deref(),
        priority_str,
        sort_order,
        payload.parent_issue_id,
        &ext_meta,
    )
    .await?;

    Ok(ResponseJson(ApiResponse::success(
        OverseerCreateIssueResponse {
            issue: issue_to_view(&issue, &status.name),
            outcome: "created".to_string(),
            txid: 0,
        },
    )))
}

async fn transition_issue(
    State(deployment): State<DeploymentImpl>,
    Path(issue_id): Path<Uuid>,
    Json(request): Json<OverseerTransitionRequest>,
) -> Result<ResponseJson<ApiResponse<OverseerTransitionResponse>>, ApiError> {
    let pool = &deployment.db().pool;

    let issue = OverseerIssue::find_by_id(pool, issue_id)
        .await?
        .ok_or_else(|| ApiError::BadRequest(format!("Issue {} not found", issue_id)))?;

    // Ensure defaults exist and find current + target statuses
    let statuses = ProjectStatus::ensure_defaults_exist(pool, issue.project_id).await?;

    let old_status = statuses
        .iter()
        .find(|s| s.id == issue.status_id)
        .ok_or_else(|| ApiError::BadRequest("Current status not found".to_string()))?;

    let new_status = ProjectStatus::find_by_name(pool, issue.project_id, &request.status_name)
        .await?
        .ok_or_else(|| {
            ApiError::BadRequest(format!("Status '{}' not found", request.status_name))
        })?;

    let sort_order = OverseerIssue::min_sort_order(pool, issue.project_id, new_status.id).await?;

    let updated = OverseerIssue::update_status(pool, issue_id, new_status.id, sort_order).await?;

    Ok(ResponseJson(ApiResponse::success(
        OverseerTransitionResponse {
            issue: issue_to_view(&updated, &new_status.name),
            old_status_name: old_status.name.clone(),
            new_status_name: new_status.name.clone(),
            txid: 0,
        },
    )))
}
