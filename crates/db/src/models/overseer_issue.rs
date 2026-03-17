use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use uuid::Uuid;

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct OverseerIssue {
    pub id: Uuid,
    pub project_id: Uuid,
    pub issue_number: i32,
    pub simple_id: String,
    pub status_id: Uuid,
    pub title: String,
    pub description: Option<String>,
    pub priority: Option<String>,
    pub sort_order: f64,
    pub parent_issue_id: Option<Uuid>,
    pub extension_metadata: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl OverseerIssue {
    pub async fn find_by_id(pool: &SqlitePool, id: Uuid) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as!(
            OverseerIssue,
            r#"SELECT
                id as "id!: Uuid",
                project_id as "project_id!: Uuid",
                issue_number as "issue_number!: i32",
                simple_id,
                status_id as "status_id!: Uuid",
                title,
                description,
                priority,
                sort_order as "sort_order!: f64",
                parent_issue_id as "parent_issue_id: Uuid",
                extension_metadata,
                created_at as "created_at!: DateTime<Utc>",
                updated_at as "updated_at!: DateTime<Utc>"
               FROM overseer_issues
               WHERE id = $1"#,
            id
        )
        .fetch_optional(pool)
        .await
    }

    pub async fn find_by_project_and_status(
        pool: &SqlitePool,
        project_id: Uuid,
        status_id: Uuid,
    ) -> Result<Vec<Self>, sqlx::Error> {
        sqlx::query_as!(
            OverseerIssue,
            r#"SELECT
                id as "id!: Uuid",
                project_id as "project_id!: Uuid",
                issue_number as "issue_number!: i32",
                simple_id,
                status_id as "status_id!: Uuid",
                title,
                description,
                priority,
                sort_order as "sort_order!: f64",
                parent_issue_id as "parent_issue_id: Uuid",
                extension_metadata,
                created_at as "created_at!: DateTime<Utc>",
                updated_at as "updated_at!: DateTime<Utc>"
               FROM overseer_issues
               WHERE project_id = $1 AND status_id = $2
               ORDER BY sort_order ASC"#,
            project_id,
            status_id
        )
        .fetch_all(pool)
        .await
    }

    pub async fn find_by_dedupe_key(
        pool: &SqlitePool,
        project_id: Uuid,
        dedupe_key: &str,
    ) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as!(
            OverseerIssue,
            r#"SELECT
                id as "id!: Uuid",
                project_id as "project_id!: Uuid",
                issue_number as "issue_number!: i32",
                simple_id,
                status_id as "status_id!: Uuid",
                title,
                description,
                priority,
                sort_order as "sort_order!: f64",
                parent_issue_id as "parent_issue_id: Uuid",
                extension_metadata,
                created_at as "created_at!: DateTime<Utc>",
                updated_at as "updated_at!: DateTime<Utc>"
               FROM overseer_issues
               WHERE project_id = $1
                 AND json_extract(extension_metadata, '$.overseer.dedupe_key') = $2"#,
            project_id,
            dedupe_key
        )
        .fetch_optional(pool)
        .await
    }

    pub async fn next_issue_number(
        pool: &SqlitePool,
        project_id: Uuid,
    ) -> Result<i32, sqlx::Error> {
        let row = sqlx::query_scalar!(
            r#"SELECT COALESCE(MAX(issue_number), 0) as "n!" FROM overseer_issues WHERE project_id = $1"#,
            project_id
        )
        .fetch_one(pool)
        .await?;
        Ok((row as i32) + 1)
    }

    pub async fn min_sort_order(
        pool: &SqlitePool,
        project_id: Uuid,
        status_id: Uuid,
    ) -> Result<f64, sqlx::Error> {
        let row = sqlx::query_scalar!(
            r#"SELECT COALESCE(MIN(sort_order), 1000.0) as "n!" FROM overseer_issues WHERE project_id = $1 AND status_id = $2"#,
            project_id,
            status_id
        )
        .fetch_one(pool)
        .await?;
        Ok(row - 100.0)
    }

    pub async fn create(
        pool: &SqlitePool,
        id: Uuid,
        project_id: Uuid,
        issue_number: i32,
        simple_id: &str,
        status_id: Uuid,
        title: &str,
        description: Option<&str>,
        priority: Option<&str>,
        sort_order: f64,
        parent_issue_id: Option<Uuid>,
        extension_metadata: &str,
    ) -> Result<Self, sqlx::Error> {
        sqlx::query_as!(
            OverseerIssue,
            r#"INSERT INTO overseer_issues
                (id, project_id, issue_number, simple_id, status_id, title, description,
                 priority, sort_order, parent_issue_id, extension_metadata)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
               RETURNING
                id as "id!: Uuid",
                project_id as "project_id!: Uuid",
                issue_number as "issue_number!: i32",
                simple_id,
                status_id as "status_id!: Uuid",
                title,
                description,
                priority,
                sort_order as "sort_order!: f64",
                parent_issue_id as "parent_issue_id: Uuid",
                extension_metadata,
                created_at as "created_at!: DateTime<Utc>",
                updated_at as "updated_at!: DateTime<Utc>""#,
            id,
            project_id,
            issue_number,
            simple_id,
            status_id,
            title,
            description,
            priority,
            sort_order,
            parent_issue_id,
            extension_metadata
        )
        .fetch_one(pool)
        .await
    }

    pub async fn update_status(
        pool: &SqlitePool,
        id: Uuid,
        status_id: Uuid,
        sort_order: f64,
    ) -> Result<Self, sqlx::Error> {
        sqlx::query_as!(
            OverseerIssue,
            r#"UPDATE overseer_issues
               SET status_id = $2, sort_order = $3, updated_at = datetime('now', 'subsec')
               WHERE id = $1
               RETURNING
                id as "id!: Uuid",
                project_id as "project_id!: Uuid",
                issue_number as "issue_number!: i32",
                simple_id,
                status_id as "status_id!: Uuid",
                title,
                description,
                priority,
                sort_order as "sort_order!: f64",
                parent_issue_id as "parent_issue_id: Uuid",
                extension_metadata,
                created_at as "created_at!: DateTime<Utc>",
                updated_at as "updated_at!: DateTime<Utc>""#,
            id,
            status_id,
            sort_order
        )
        .fetch_one(pool)
        .await
    }
}
