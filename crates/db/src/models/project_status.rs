use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use uuid::Uuid;

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct ProjectStatus {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub color: String,
    pub sort_order: i32,
    pub hidden: bool,
    pub created_at: DateTime<Utc>,
}

pub const DEFAULT_STATUSES: &[(&str, &str)] = &[
    ("Backlog", "#6B7280"),
    ("Todo", "#3B82F6"),
    ("In Progress", "#F59E0B"),
    ("In Review", "#8B5CF6"),
    ("Done", "#10B981"),
    ("Cancelled", "#EF4444"),
];

impl ProjectStatus {
    pub async fn list_by_project(
        pool: &SqlitePool,
        project_id: Uuid,
    ) -> Result<Vec<Self>, sqlx::Error> {
        sqlx::query_as!(
            ProjectStatus,
            r#"SELECT
                id as "id!: Uuid",
                project_id as "project_id!: Uuid",
                name,
                color,
                sort_order as "sort_order!: i32",
                hidden as "hidden!: bool",
                created_at as "created_at!: DateTime<Utc>"
               FROM project_statuses
               WHERE project_id = $1
               ORDER BY sort_order ASC"#,
            project_id
        )
        .fetch_all(pool)
        .await
    }

    pub async fn find_by_name(
        pool: &SqlitePool,
        project_id: Uuid,
        name: &str,
    ) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as!(
            ProjectStatus,
            r#"SELECT
                id as "id!: Uuid",
                project_id as "project_id!: Uuid",
                name,
                color,
                sort_order as "sort_order!: i32",
                hidden as "hidden!: bool",
                created_at as "created_at!: DateTime<Utc>"
               FROM project_statuses
               WHERE project_id = $1 AND LOWER(name) = LOWER($2)"#,
            project_id,
            name
        )
        .fetch_optional(pool)
        .await
    }

    pub async fn find_by_id(pool: &SqlitePool, id: Uuid) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as!(
            ProjectStatus,
            r#"SELECT
                id as "id!: Uuid",
                project_id as "project_id!: Uuid",
                name,
                color,
                sort_order as "sort_order!: i32",
                hidden as "hidden!: bool",
                created_at as "created_at!: DateTime<Utc>"
               FROM project_statuses
               WHERE id = $1"#,
            id
        )
        .fetch_optional(pool)
        .await
    }

    pub async fn ensure_defaults_exist(
        pool: &SqlitePool,
        project_id: Uuid,
    ) -> Result<Vec<Self>, sqlx::Error> {
        let existing = Self::list_by_project(pool, project_id).await?;
        if !existing.is_empty() {
            return Ok(existing);
        }

        for (i, (name, color)) in DEFAULT_STATUSES.iter().enumerate() {
            let id = Uuid::new_v4();
            let sort_order = i as i32;
            sqlx::query!(
                r#"INSERT INTO project_statuses (id, project_id, name, color, sort_order)
                   VALUES ($1, $2, $3, $4, $5)"#,
                id,
                project_id,
                name,
                color,
                sort_order
            )
            .execute(pool)
            .await?;
        }

        Self::list_by_project(pool, project_id).await
    }
}
