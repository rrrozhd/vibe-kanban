use std::collections::{HashMap, VecDeque};

use api_types::{NotificationPayload, NotificationType};
use uuid::Uuid;

use super::DigestUser;
use crate::db::digest::NotificationDigestRow;

const MAX_PREVIEW_ITEMS: usize = 5;

pub fn recipient_name(user: &DigestUser) -> String {
    user.first_name
        .clone()
        .or_else(|| user.username.clone())
        .unwrap_or_else(|| "there".to_string())
}

pub fn render_email_body(rows: &[NotificationDigestRow], base_url: &str) -> String {
    let preview_rows = select_preview_rows(rows);
    let mut body = String::new();

    for (index, row) in preview_rows.iter().enumerate() {
        if index > 0 {
            body.push_str("<div style=\"margin: 0 0 26px; border-top: 1px solid #E5E7EB;\"></div>");
        }

        body.push_str(&render_preview_item(row, base_url));
    }

    body
}

pub fn notifications_url(base_url: &str) -> String {
    absolute_url(base_url, "/notifications")
}

fn select_preview_rows(rows: &[NotificationDigestRow]) -> Vec<&NotificationDigestRow> {
    let mut groups = build_preview_groups(rows);
    let mut selected = Vec::with_capacity(MAX_PREVIEW_ITEMS.min(rows.len()));

    while selected.len() < MAX_PREVIEW_ITEMS {
        let mut added_in_pass = false;

        for group in &mut groups {
            if let Some(row) = group.rows.pop_front() {
                selected.push(row);
                added_in_pass = true;

                if selected.len() == MAX_PREVIEW_ITEMS {
                    break;
                }
            }
        }

        if !added_in_pass {
            break;
        }
    }

    selected
}

struct PreviewGroup<'a> {
    rows: VecDeque<&'a NotificationDigestRow>,
}

fn build_preview_groups(rows: &[NotificationDigestRow]) -> Vec<PreviewGroup<'_>> {
    let mut groups: Vec<PreviewGroup<'_>> = Vec::new();
    let mut issue_group_indexes: HashMap<Uuid, usize> = HashMap::new();

    for row in rows {
        if let Some(issue_id) = preview_issue_id(row) {
            if let Some(index) = issue_group_indexes.get(&issue_id).copied() {
                groups[index].rows.push_back(row);
            } else {
                let index = groups.len();
                groups.push(PreviewGroup {
                    rows: VecDeque::from([row]),
                });
                issue_group_indexes.insert(issue_id, index);
            }
        } else {
            groups.push(PreviewGroup {
                rows: VecDeque::from([row]),
            });
        }
    }

    groups
}

fn preview_issue_id(row: &NotificationDigestRow) -> Option<Uuid> {
    row.payload.0.issue_id.or(row.issue_id)
}

fn render_preview_item(row: &NotificationDigestRow, base_url: &str) -> String {
    let payload = &row.payload.0;
    let deeplink = absolute_url(base_url, payload.deeplink_path.as_deref().unwrap_or(""));
    let copy = build_digest_copy(row);
    let title = escape_html(&copy.title);
    let body = copy
        .body
        .map(|value| escape_html(&value))
        .unwrap_or_default();

    let mut html = String::from("<div style=\"margin: 0 0 26px;\">");

    html.push_str(&format!(
        "<a href=\"{}\" style=\"color: #4A86D9; text-decoration: none; font-size: 14px; line-height: 1.4; font-weight: 700;\">{}</a>",
        escape_html(&deeplink),
        title
    ));

    if !body.is_empty() {
        html.push_str(&format!(
            "<div style=\"margin-top: 8px; color: #262626; font-size: 14px; line-height: 1.5;\">{}</div>",
            body
        ));
    }
    html.push_str("</div>");

    html
}

struct DigestCopy {
    title: String,
    body: Option<String>,
}

fn build_digest_copy(row: &NotificationDigestRow) -> DigestCopy {
    let payload = &row.payload.0;
    let actor_name = &row.actor_name;
    let issue_label = issue_label(payload);

    let (title, body) = match row.notification_type {
        NotificationType::IssueCommentAdded => (
            format!("{actor_name} commented on {issue_label}"),
            payload
                .comment_preview
                .as_deref()
                .map(clean_preview_text)
                .filter(|value| !value.is_empty())
                .map(|value| format!("\"{}\"", truncate_text(&value, 180)))
                .or_else(|| issue_context(payload)),
        ),
        NotificationType::IssueStatusChanged => {
            let old_status = clean_optional_text(payload.old_status_name.as_deref());
            let new_status = clean_optional_text(payload.new_status_name.as_deref());

            let title = match (&old_status, &new_status) {
                (Some(old_status), Some(new_status)) => format!(
                    "{actor_name} changed status of {issue_label} from {old_status} to {new_status}"
                ),
                _ => format!("{actor_name} changed status of {issue_label}"),
            };

            let body = match (old_status, new_status) {
                (Some(old_status), Some(new_status)) => {
                    Some(format!("Status changed from {old_status} to {new_status}."))
                }
                (_, Some(new_status)) => Some(format!("Status changed to {new_status}.")),
                _ => None,
            };

            (title, body)
        }
        NotificationType::IssueAssigneeChanged => (
            format!("You were assigned to {issue_label} by {actor_name}"),
            issue_context(payload)
                .map(|issue| format!("You were assigned to {issue}."))
                .or_else(|| Some("You were assigned to an issue.".to_string())),
        ),
        NotificationType::IssuePriorityChanged => {
            let old_priority = payload.old_priority.map(priority_label);
            let new_priority = payload.new_priority.map(priority_label);

            let title = match (&old_priority, &new_priority) {
                (Some(old_priority), Some(new_priority)) => format!(
                    "{actor_name} changed the priority of {issue_label} from {old_priority} to {new_priority}"
                ),
                (None, Some(new_priority)) => {
                    format!("{actor_name} changed the priority of {issue_label} to {new_priority}")
                }
                _ => format!("{actor_name} changed the priority of {issue_label}"),
            };

            let body = match (old_priority, new_priority) {
                (Some(old_priority), Some(new_priority)) => Some(format!(
                    "Priority changed from {old_priority} to {new_priority}."
                )),
                (_, Some(new_priority)) => Some(format!("Priority changed to {new_priority}.")),
                _ => None,
            };

            (title, body)
        }
        NotificationType::IssueUnassigned => (
            format!("{actor_name} unassigned you from {issue_label}"),
            issue_context(payload)
                .map(|issue| format!("You were removed from {issue}."))
                .or_else(|| Some("You were removed from an issue.".to_string())),
        ),
        NotificationType::IssueCommentReaction => {
            let emoji = clean_optional_text(payload.emoji.as_deref());
            let title = match &emoji {
                Some(emoji) => {
                    format!("{actor_name} reacted {emoji} to your comment on {issue_label}")
                }
                None => format!("{actor_name} reacted to your comment on {issue_label}"),
            };
            let body = emoji.map(|emoji| format!("Reacted with {emoji} to your comment."));
            (title, body)
        }
        NotificationType::IssueDeleted => (
            format!("{actor_name} deleted {issue_label}"),
            issue_context(payload)
                .map(|issue| format!("{issue} was deleted."))
                .or_else(|| Some("An issue was deleted.".to_string())),
        ),
        NotificationType::IssueTitleChanged => {
            let new_title = clean_optional_text(payload.new_title.as_deref());
            let title = new_title
                .as_ref()
                .map(|value| format!("{actor_name} changed the title of {issue_label} to {value}"))
                .unwrap_or_else(|| format!("{actor_name} changed the title of {issue_label}"));
            let body = new_title
                .map(|new_title| format!("New title: {new_title}"))
                .or_else(|| issue_context(payload));
            (title, body)
        }
        NotificationType::IssueDescriptionChanged => (
            format!("{actor_name} changed the description on {issue_label}"),
            issue_context(payload).map(|issue| format!("Updated the description on {issue}.")),
        ),
    };

    DigestCopy {
        title,
        body: body.map(|value| truncate_text(&value, 180)),
    }
}

fn issue_context(payload: &NotificationPayload) -> Option<String> {
    clean_optional_text(payload.issue_title.as_deref())
        .or_else(|| clean_optional_text(payload.issue_simple_id.as_deref()))
}

fn issue_label(payload: &NotificationPayload) -> String {
    clean_optional_text(payload.issue_simple_id.as_deref()).unwrap_or_else(|| "issue".to_string())
}

fn priority_label(priority: api_types::IssuePriority) -> &'static str {
    match priority {
        api_types::IssuePriority::Urgent => "Urgent",
        api_types::IssuePriority::High => "High",
        api_types::IssuePriority::Medium => "Medium",
        api_types::IssuePriority::Low => "Low",
    }
}

fn clean_optional_text(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn clean_preview_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_text(value: &str, max_chars: usize) -> String {
    let trimmed = value.trim();
    let char_count = trimmed.chars().count();
    if char_count <= max_chars {
        return trimmed.to_string();
    }

    let truncated = trimmed.chars().take(max_chars).collect::<String>();
    format!("{}...", truncated.trim_end())
}

fn escape_html(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());

    for ch in value.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(ch),
        }
    }

    escaped
}

fn absolute_url(base_url: &str, deeplink_path: &str) -> String {
    let base_url = base_url.trim_end_matches('/');
    let deeplink_path = deeplink_path.trim_start_matches('/');
    format!("{base_url}/{deeplink_path}")
}
