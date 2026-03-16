use std::{sync::Arc, time::Duration};

use chrono::{DateTime, Days, Timelike, Utc};
use sqlx::PgPool;
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

use crate::{db::digest::DigestRepository, digest::run_email_digest, mail::Mailer};

const DEFAULT_WINDOW: Duration = Duration::from_secs(86400);
const DEFAULT_RUN_HOUR_UTC: u32 = 8;

pub fn spawn_digest_task(
    pool: PgPool,
    mailer: Arc<dyn Mailer>,
    base_url: String,
) -> JoinHandle<()> {
    let interval_override = std::env::var("DIGEST_INTERVAL_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map(Duration::from_secs);
    let run_hour_utc = std::env::var("DIGEST_RUN_HOUR_UTC")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .filter(|hour| *hour < 24)
        .unwrap_or(DEFAULT_RUN_HOUR_UTC);
    let window = std::env::var("DIGEST_WINDOW_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_WINDOW);

    match interval_override {
        Some(interval) => info!(
            interval_secs = interval.as_secs(),
            window_secs = window.as_secs(),
            "Starting notification digest background task with interval override"
        ),
        None => info!(
            run_hour_utc,
            window_secs = window.as_secs(),
            "Starting notification digest background task"
        ),
    }

    tokio::spawn(async move {
        loop {
            if let Some(interval) = interval_override {
                tokio::time::sleep(interval).await;
            } else {
                let now = Utc::now();
                let next_run = next_run_at(now, run_hour_utc);
                let sleep_duration = (next_run - now)
                    .to_std()
                    .unwrap_or_else(|_| Duration::from_secs(0));

                info!(next_run = %next_run, sleep_secs = sleep_duration.as_secs(), "Next notification digest scheduled");
                tokio::time::sleep(sleep_duration).await;
            }

            let Some(lock) = acquire_run_lock(&pool).await else {
                continue;
            };

            match run_email_digest(&pool, mailer.as_ref(), &base_url, Utc::now(), window).await {
                Ok(stats) => {
                    info!(
                        users_processed = stats.users_processed,
                        emails_sent = stats.emails_sent,
                        errors = stats.errors,
                        "Notification digest cycle complete"
                    );
                }
                Err(e) => {
                    error!(error = %e, "Notification digest cycle failed");
                }
            }

            if let Err(error) = lock.release().await {
                warn!(error = %error, "Failed to release notification digest lock");
            }
        }
    })
}

async fn acquire_run_lock(pool: &PgPool) -> Option<crate::db::digest::DigestRunLock> {
    match DigestRepository::try_acquire_run_lock(pool).await {
        Ok(Some(lock)) => Some(lock),
        Ok(None) => {
            info!("Skipping notification digest cycle because another instance is running it");
            None
        }
        Err(error) => {
            error!(error = %error, "Failed to acquire notification digest lock");
            None
        }
    }
}

fn next_run_at(now: DateTime<Utc>, run_hour_utc: u32) -> DateTime<Utc> {
    let today = now.date_naive();
    let today_run = today
        .and_hms_opt(run_hour_utc, 0, 0)
        .expect("validated digest hour");

    let next_naive = if now.hour() < run_hour_utc {
        today_run
    } else {
        today
            .checked_add_days(Days::new(1))
            .expect("date overflow for digest schedule")
            .and_hms_opt(run_hour_utc, 0, 0)
            .expect("validated digest hour")
    };

    DateTime::from_naive_utc_and_offset(next_naive, Utc)
}
