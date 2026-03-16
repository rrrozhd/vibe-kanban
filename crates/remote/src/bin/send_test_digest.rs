use std::{env, process::ExitCode};

use api_types::{NotificationPayload, NotificationType};
use chrono::{Duration, Utc};
use remote::{
    db::digest::NotificationDigestRow,
    digest::email,
    mail::{LoopsMailer, Mailer},
};
use sqlx::types::Json;
use uuid::Uuid;

#[tokio::main]
async fn main() -> ExitCode {
    remote::init_tracing();

    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> anyhow::Result<()> {
    let to_email = required_env("TEST_DIGEST_TO_EMAIL")?;
    let base_url = env::var("TEST_DIGEST_BASE_URL")
        .or_else(|_| env::var("SERVER_PUBLIC_BASE_URL"))
        .unwrap_or_else(|_| "http://localhost:5173".to_string());
    let name = env::var("TEST_DIGEST_NAME").unwrap_or_else(|_| "Gabriel".to_string());
    let notification_count = env::var("TEST_DIGEST_NOTIFICATION_COUNT")
        .ok()
        .and_then(|value| value.parse::<i32>().ok())
        .unwrap_or(12);
    let api_key = required_env("LOOPS_EMAIL_API_KEY")?;

    let rows = sample_rows();
    let email_body = email::render_email_body(&rows, &base_url);
    let deeplink = email::notifications_url(&base_url);

    println!("Sending test digest to {to_email}");
    println!("Name: {name}");
    println!("Notification count: {notification_count}");
    println!("Base URL: {base_url}");
    println!("Notifications deeplink: {deeplink}");
    println!("Rendered email body:\n{email_body}");

    let mailer = LoopsMailer::new(api_key);
    mailer
        .send_notification_digest(&to_email, &name, notification_count, &email_body, &deeplink)
        .await?;

    println!("Test digest sent.");

    Ok(())
}

fn required_env(name: &str) -> anyhow::Result<String> {
    env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("{name} must be set"))
}

fn sample_rows() -> Vec<NotificationDigestRow> {
    let now = Utc::now();
    let issue_one = Uuid::new_v4();
    let issue_two = Uuid::new_v4();
    let issue_three = Uuid::new_v4();
    let project_id = Uuid::new_v4();

    vec![
        NotificationDigestRow {
            id: Uuid::new_v4(),
            notification_type: NotificationType::IssueCommentAdded,
            payload: Json(NotificationPayload {
                deeplink_path: Some(format!("/projects/{project_id}/issues/{issue_one}")),
                issue_id: Some(issue_one),
                issue_simple_id: Some("VK-214".to_string()),
                issue_title: Some("Finalize notification digest email design".to_string()),
                actor_user_id: Some(Uuid::new_v4()),
                comment_preview: Some(
                    "I simplified the template on the Loops side. We should push the full card layout from the server so placeholder rendering stops leaking into the email."
                        .to_string(),
                ),
                ..NotificationPayload::default()
            }),
            issue_id: Some(issue_one),
            created_at: now,
            actor_name: "Shaun Lane".to_string(),
        },
        NotificationDigestRow {
            id: Uuid::new_v4(),
            notification_type: NotificationType::IssueStatusChanged,
            payload: Json(NotificationPayload {
                deeplink_path: Some(format!("/projects/{project_id}/issues/{issue_two}")),
                issue_id: Some(issue_two),
                issue_simple_id: Some("VK-198".to_string()),
                issue_title: Some("Ship remote notification settings".to_string()),
                actor_user_id: Some(Uuid::new_v4()),
                ..NotificationPayload::default()
            }),
            issue_id: Some(issue_two),
            created_at: now - Duration::minutes(7),
            actor_name: "Warren Weissbluth".to_string(),
        },
        NotificationDigestRow {
            id: Uuid::new_v4(),
            notification_type: NotificationType::IssuePriorityChanged,
            payload: Json(NotificationPayload {
                deeplink_path: Some(format!("/projects/{project_id}/issues/{issue_three}")),
                issue_id: Some(issue_three),
                issue_simple_id: Some("VK-173".to_string()),
                issue_title: Some("Backfill delivery metrics for digests".to_string()),
                actor_user_id: Some(Uuid::new_v4()),
                ..NotificationPayload::default()
            }),
            issue_id: Some(issue_three),
            created_at: now - Duration::minutes(14),
            actor_name: "Kent Goodman".to_string(),
        },
        NotificationDigestRow {
            id: Uuid::new_v4(),
            notification_type: NotificationType::IssueTitleChanged,
            payload: Json(NotificationPayload {
                deeplink_path: Some(format!("/projects/{project_id}/issues/{issue_one}")),
                issue_id: Some(issue_one),
                issue_simple_id: Some("VK-214".to_string()),
                issue_title: Some("Finalize notification digest email design".to_string()),
                actor_user_id: Some(Uuid::new_v4()),
                new_title: Some("Finalize server-rendered digest email body".to_string()),
                ..NotificationPayload::default()
            }),
            issue_id: Some(issue_one),
            created_at: now - Duration::minutes(19),
            actor_name: "Olly Wilson".to_string(),
        },
        NotificationDigestRow {
            id: Uuid::new_v4(),
            notification_type: NotificationType::IssueCommentReaction,
            payload: Json(NotificationPayload {
                deeplink_path: Some(format!("/projects/{project_id}/issues/{issue_two}")),
                issue_id: Some(issue_two),
                issue_simple_id: Some("VK-198".to_string()),
                issue_title: Some("Ship remote notification settings".to_string()),
                actor_user_id: Some(Uuid::new_v4()),
                ..NotificationPayload::default()
            }),
            issue_id: Some(issue_two),
            created_at: now - Duration::minutes(27),
            actor_name: "SimpleHash Ops".to_string(),
        },
        NotificationDigestRow {
            id: Uuid::new_v4(),
            notification_type: NotificationType::IssueDescriptionChanged,
            payload: Json(NotificationPayload {
                deeplink_path: Some(format!("/projects/{project_id}/issues/{issue_three}")),
                issue_id: Some(issue_three),
                issue_simple_id: Some("VK-173".to_string()),
                issue_title: Some("Backfill delivery metrics for digests".to_string()),
                actor_user_id: Some(Uuid::new_v4()),
                ..NotificationPayload::default()
            }),
            issue_id: Some(issue_three),
            created_at: now - Duration::minutes(35),
            actor_name: "Riviera Team".to_string(),
        },
    ]
}
