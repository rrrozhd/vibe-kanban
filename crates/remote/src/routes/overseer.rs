use api_types::{
    Issue, OverseerBoardColumn, OverseerBoardResponse, OverseerCreateIssueRequest,
    OverseerCreateIssueResponse, OverseerIssueView, OverseerTransitionRequest,
    OverseerTransitionResponse,
};
use axum::{
    Json,
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    routing::{get, post},
};
use serde::Deserialize;
use tracing::instrument;
use uuid::Uuid;

use super::{
    error::ErrorResponse,
    organization_members::ensure_project_access,
};
use crate::{
    AppState,
    auth::RequestContext,
    db::{
        get_txid, issue_followers::IssueFollowerRepository, issues::IssueRepository,
        project_statuses::ProjectStatusRepository,
    },
};

pub fn router() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/overseer/board", get(get_board))
        .route("/overseer/issues", post(create_issue))
        .route("/overseer/issues/{issue_id}/transition", post(transition_issue))
}

// ── Helpers ────────────────────────────────────────────────────────────

fn issue_to_view(issue: &Issue, status_name: String) -> OverseerIssueView {
    OverseerIssueView {
        id: issue.id,
        project_id: issue.project_id,
        simple_id: issue.simple_id.clone(),
        title: issue.title.clone(),
        description: issue.description.clone(),
        status_name,
        status_id: issue.status_id,
        priority: issue.priority,
        parent_issue_id: issue.parent_issue_id,
        sort_order: issue.sort_order,
    }
}

// ── Board View ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct BoardQuery {
    pub project_id: Uuid,
}

#[instrument(
    name = "overseer.get_board",
    skip(state, ctx),
    fields(project_id = %query.project_id, user_id = %ctx.user.id)
)]
async fn get_board(
    State(state): State<AppState>,
    Extension(ctx): Extension<RequestContext>,
    Query(query): Query<BoardQuery>,
) -> Result<Json<OverseerBoardResponse>, ErrorResponse> {
    ensure_project_access(state.pool(), ctx.user.id, query.project_id).await?;

    let statuses = ProjectStatusRepository::list_by_project(state.pool(), query.project_id)
        .await
        .map_err(|e| {
            tracing::error!(?e, "failed to list statuses");
            ErrorResponse::new(StatusCode::INTERNAL_SERVER_ERROR, "failed to list statuses")
        })?;

    let issues = IssueRepository::list_by_project(state.pool(), query.project_id)
        .await
        .map_err(|e| {
            tracing::error!(?e, "failed to list issues");
            ErrorResponse::new(StatusCode::INTERNAL_SERVER_ERROR, "failed to list issues")
        })?;

    // Build a status_id → name lookup
    let status_map: std::collections::HashMap<Uuid, &str> = statuses
        .iter()
        .map(|s| (s.id, s.name.as_str()))
        .collect();

    let mut columns: Vec<OverseerBoardColumn> = statuses
        .iter()
        .map(|s| OverseerBoardColumn {
            status_id: s.id,
            status_name: s.name.clone(),
            hidden: s.hidden,
            issues: Vec::new(),
        })
        .collect();

    // Sort columns by sort_order (already from DB, but be explicit)
    columns.sort_by_key(|c| {
        statuses
            .iter()
            .find(|s| s.id == c.status_id)
            .map(|s| s.sort_order)
            .unwrap_or(0)
    });

    for issue in &issues {
        let status_name = status_map
            .get(&issue.status_id)
            .unwrap_or(&"Unknown")
            .to_string();
        let view = issue_to_view(issue, status_name);
        if let Some(col) = columns.iter_mut().find(|c| c.status_id == issue.status_id) {
            col.issues.push(view);
        }
    }

    // Sort issues within each column by sort_order
    for col in &mut columns {
        col.issues.sort_by(|a, b| a.sort_order.partial_cmp(&b.sort_order).unwrap_or(std::cmp::Ordering::Equal));
    }

    Ok(Json(OverseerBoardResponse {
        project_id: query.project_id,
        columns,
    }))
}

// ── Create Issue ───────────────────────────────────────────────────────

#[instrument(
    name = "overseer.create_issue",
    skip(state, ctx, payload),
    fields(project_id = %payload.project_id, user_id = %ctx.user.id)
)]
async fn create_issue(
    State(state): State<AppState>,
    Extension(ctx): Extension<RequestContext>,
    Json(payload): Json<OverseerCreateIssueRequest>,
) -> Result<Json<OverseerCreateIssueResponse>, ErrorResponse> {
    ensure_project_access(state.pool(), ctx.user.id, payload.project_id).await?;

    // Resolve status
    let statuses = ProjectStatusRepository::list_by_project(state.pool(), payload.project_id)
        .await
        .map_err(|e| {
            tracing::error!(?e, "failed to list statuses");
            ErrorResponse::new(StatusCode::INTERNAL_SERVER_ERROR, "failed to list statuses")
        })?;

    let status = if let Some(ref name) = payload.status_name {
        statuses
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| {
                ErrorResponse::new(
                    StatusCode::BAD_REQUEST,
                    &format!("status '{}' not found", name),
                )
            })?
    } else {
        // Default to first visible status
        statuses
            .iter()
            .filter(|s| !s.hidden)
            .min_by_key(|s| s.sort_order)
            .ok_or_else(|| {
                ErrorResponse::new(StatusCode::BAD_REQUEST, "no visible statuses in project")
            })?
    };

    // Dedupe check
    if let Some(ref dedupe_key) = payload.dedupe_key {
        let existing = find_issue_by_dedupe_key(
            state.pool(),
            payload.project_id,
            dedupe_key,
        )
        .await
        .map_err(|e| {
            tracing::error!(?e, "dedupe lookup failed");
            ErrorResponse::new(StatusCode::INTERNAL_SERVER_ERROR, "dedupe lookup failed")
        })?;

        if let Some(existing_issue) = existing {
            return Ok(Json(OverseerCreateIssueResponse {
                issue: issue_to_view(&existing_issue, status.name.clone()),
                outcome: "existing".to_string(),
                txid: 0,
            }));
        }
    }

    // Compute top-of-column sort order
    let min_sort: Option<f64> = sqlx::query_scalar(
        "SELECT MIN(sort_order) FROM issues WHERE project_id = $1 AND status_id = $2",
    )
    .bind(payload.project_id)
    .bind(status.id)
    .fetch_one(state.pool())
    .await
    .map_err(|e| {
        tracing::error!(?e, "failed to query min sort_order");
        ErrorResponse::new(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
    })?;
    let min_sort = min_sort.unwrap_or(0.0);

    let sort_order = min_sort - 1.0;

    // Build extension_metadata with overseer info
    let mut ext = serde_json::json!({});
    if let Some(ref dk) = payload.dedupe_key {
        ext["overseer"] = serde_json::json!({
            "dedupe_key": dk,
        });
    }

    let response = IssueRepository::create(
        state.pool(),
        None,
        payload.project_id,
        status.id,
        payload.title.clone(),
        payload.description.clone(),
        payload.priority,
        None,
        None,
        None,
        sort_order,
        payload.parent_issue_id,
        None,
        ext,
        ctx.user.id,
    )
    .await
    .map_err(|e| {
        tracing::error!(?e, "failed to create issue");
        ErrorResponse::new(StatusCode::INTERNAL_SERVER_ERROR, "failed to create issue")
    })?;

    // Auto-follow creator
    if let Err(e) =
        IssueFollowerRepository::create(state.pool(), None, response.data.id, ctx.user.id).await
    {
        tracing::warn!(?e, issue_id = %response.data.id, "failed to auto-follow");
    }

    Ok(Json(OverseerCreateIssueResponse {
        issue: issue_to_view(&response.data, status.name.clone()),
        outcome: "created".to_string(),
        txid: response.txid,
    }))
}

// ── Transition Issue ───────────────────────────────────────────────────

#[instrument(
    name = "overseer.transition_issue",
    skip(state, ctx, payload),
    fields(issue_id = %issue_id, user_id = %ctx.user.id)
)]
async fn transition_issue(
    State(state): State<AppState>,
    Extension(ctx): Extension<RequestContext>,
    Path(issue_id): Path<Uuid>,
    Json(payload): Json<OverseerTransitionRequest>,
) -> Result<Json<OverseerTransitionResponse>, ErrorResponse> {
    let issue = IssueRepository::find_by_id(state.pool(), issue_id)
        .await
        .map_err(|e| {
            tracing::error!(?e, %issue_id, "failed to load issue");
            ErrorResponse::new(StatusCode::INTERNAL_SERVER_ERROR, "failed to load issue")
        })?
        .ok_or_else(|| ErrorResponse::new(StatusCode::NOT_FOUND, "issue not found"))?;

    ensure_project_access(state.pool(), ctx.user.id, issue.project_id).await?;

    // Resolve old status name
    let old_status = ProjectStatusRepository::find_by_id(state.pool(), issue.status_id)
        .await
        .map_err(|e| {
            tracing::error!(?e, "failed to find old status");
            ErrorResponse::new(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
        })?
        .ok_or_else(|| ErrorResponse::new(StatusCode::INTERNAL_SERVER_ERROR, "old status missing"))?;

    // Resolve target status by name
    let new_status =
        ProjectStatusRepository::find_by_name(state.pool(), issue.project_id, &payload.status_name)
            .await
            .map_err(|e| {
                tracing::error!(?e, "failed to find status");
                ErrorResponse::new(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
            })?
            .ok_or_else(|| {
                ErrorResponse::new(
                    StatusCode::BAD_REQUEST,
                    &format!("status '{}' not found", payload.status_name),
                )
            })?;

    // Compute sort order (top of target column)
    let min_sort: Option<f64> = sqlx::query_scalar(
        "SELECT MIN(sort_order) FROM issues WHERE project_id = $1 AND status_id = $2",
    )
    .bind(issue.project_id)
    .bind(new_status.id)
    .fetch_one(state.pool())
    .await
    .map_err(|e| {
        tracing::error!(?e, "failed to query min sort_order");
        ErrorResponse::new(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
    })?;
    let min_sort = min_sort.unwrap_or(0.0);

    let sort_order = min_sort - 1.0;

    let mut tx = state.pool().begin().await.map_err(|e| {
        tracing::error!(?e, "failed to begin transaction");
        ErrorResponse::new(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
    })?;

    let updated = IssueRepository::update(
        &mut *tx,
        issue_id,
        Some(new_status.id),
        None,
        None,
        None,
        None,
        None,
        None,
        Some(sort_order),
        None,
        None,
        None,
    )
    .await
    .map_err(|e| {
        tracing::error!(?e, "failed to update issue");
        ErrorResponse::new(StatusCode::INTERNAL_SERVER_ERROR, "failed to update issue")
    })?;

    let txid = get_txid(&mut *tx).await.map_err(|e| {
        tracing::error!(?e, "failed to get txid");
        ErrorResponse::new(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
    })?;

    tx.commit().await.map_err(|e| {
        tracing::error!(?e, "failed to commit");
        ErrorResponse::new(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
    })?;

    Ok(Json(OverseerTransitionResponse {
        issue: issue_to_view(&updated, new_status.name.clone()),
        old_status_name: old_status.name,
        new_status_name: new_status.name,
        txid,
    }))
}

// ── Dedupe Helper ──────────────────────────────────────────────────────

async fn find_issue_by_dedupe_key(
    pool: &sqlx::PgPool,
    project_id: Uuid,
    dedupe_key: &str,
) -> Result<Option<Issue>, crate::db::issues::IssueError> {
    let issues = IssueRepository::list_by_project(pool, project_id).await?;
    Ok(issues.into_iter().find(|issue| {
        if let Some(overseer) = issue.extension_metadata.get("overseer") {
            if let Some(dk) = overseer.get("dedupe_key").and_then(|v| v.as_str()) {
                return dk == dedupe_key && issue.completed_at.is_none();
            }
        }
        false
    }))
}
