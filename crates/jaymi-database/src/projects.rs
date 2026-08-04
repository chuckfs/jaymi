//! Project registry persistence for first-class Jaymi projects.

use rusqlite::{params, OptionalExtension};

use jaymi_core::{JaymiError, JaymiResult};

use crate::Database;

/// Persisted project row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRecord {
    /// Stable project identity.
    pub project_id: String,
    /// Display name.
    pub name: String,
    /// Normalized slug for lookup.
    pub slug: String,
    /// Optional workspace root.
    pub root_path: Option<String>,
    /// Human-readable description.
    pub description: String,
    /// Project type label (`general`, `code`, `documents`, `mixed`).
    pub project_type: String,
    /// Unix seconds created.
    pub created_at: i64,
    /// Unix seconds updated.
    pub updated_at: i64,
    /// Unix seconds last opened, when any.
    pub last_opened_at: Option<i64>,
    /// Status label (`active` / `deleted`).
    pub status: String,
}

impl Database {
    /// Insert or update a project.
    pub fn upsert_project(&self, record: &ProjectRecord) -> JaymiResult<()> {
        self.with_connection(|conn| {
            conn.execute(
                "INSERT INTO projects (
                    project_id, name, slug, root_path, description, project_type,
                    created_at, updated_at, last_opened_at, status
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT(project_id) DO UPDATE SET
                    name = excluded.name,
                    slug = excluded.slug,
                    root_path = excluded.root_path,
                    description = excluded.description,
                    project_type = excluded.project_type,
                    updated_at = excluded.updated_at,
                    last_opened_at = excluded.last_opened_at,
                    status = excluded.status",
                params![
                    record.project_id,
                    record.name,
                    record.slug,
                    record.root_path,
                    record.description,
                    record.project_type,
                    record.created_at,
                    record.updated_at,
                    record.last_opened_at,
                    record.status,
                ],
            )
            .map_err(db_error)?;
            Ok(())
        })
    }

    /// Load a project by id (including soft-deleted).
    pub fn get_project(&self, project_id: &str) -> JaymiResult<Option<ProjectRecord>> {
        self.with_connection(|conn| {
            conn.query_row(
                "SELECT project_id, name, slug, root_path, description, project_type,
                        created_at, updated_at, last_opened_at, status
                 FROM projects WHERE project_id = ?1",
                params![project_id],
                map_project_row,
            )
            .optional()
            .map_err(db_error)
        })
    }

    /// Find an active project by display name or slug (case-insensitive).
    pub fn find_project_by_name(&self, name: &str) -> JaymiResult<Option<ProjectRecord>> {
        let needle = name.trim().to_ascii_lowercase();
        if needle.is_empty() {
            return Ok(None);
        }
        self.with_connection(|conn| {
            conn.query_row(
                "SELECT project_id, name, slug, root_path, description, project_type,
                        created_at, updated_at, last_opened_at, status
                 FROM projects
                 WHERE status = 'active'
                   AND (lower(name) = ?1 OR lower(slug) = ?1)
                 ORDER BY updated_at DESC, project_id ASC
                 LIMIT 1",
                params![needle],
                map_project_row,
            )
            .optional()
            .map_err(db_error)
        })
    }

    /// List active projects (newest opened / updated first).
    pub fn list_projects(&self) -> JaymiResult<Vec<ProjectRecord>> {
        self.with_connection(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT project_id, name, slug, root_path, description, project_type,
                            created_at, updated_at, last_opened_at, status
                     FROM projects
                     WHERE status = 'active'
                     ORDER BY COALESCE(last_opened_at, 0) DESC, updated_at DESC, name ASC",
                )
                .map_err(db_error)?;
            let rows = stmt.query_map([], map_project_row).map_err(db_error)?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row.map_err(db_error)?);
            }
            Ok(out)
        })
    }

    /// Soft-delete a project.
    pub fn delete_project(&self, project_id: &str, now: i64) -> JaymiResult<bool> {
        self.with_connection(|conn| {
            let changed = conn
                .execute(
                    "UPDATE projects
                     SET status = 'deleted', updated_at = ?2
                     WHERE project_id = ?1 AND status != 'deleted'",
                    params![project_id, now],
                )
                .map_err(db_error)?;
            Ok(changed > 0)
        })
    }

    /// Count projects with an exact status label.
    pub fn count_projects_with_status(&self, status: &str) -> JaymiResult<u64> {
        self.with_connection(|conn| {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM projects WHERE status = ?1",
                    params![status],
                    |row| row.get(0),
                )
                .map_err(db_error)?;
            Ok(count as u64)
        })
    }

    /// List conversation ids attached to a project (newest first).
    pub fn list_conversation_ids_for_project(&self, project_id: &str) -> JaymiResult<Vec<String>> {
        self.with_connection(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT conversation_id FROM conversations
                     WHERE project_id = ?1
                     ORDER BY updated_at DESC, conversation_id ASC",
                )
                .map_err(db_error)?;
            let rows = stmt
                .query_map(params![project_id], |row| row.get(0))
                .map_err(db_error)?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row.map_err(db_error)?);
            }
            Ok(out)
        })
    }

    /// Count active projects.
    pub fn project_count(&self) -> JaymiResult<u64> {
        self.with_connection(|conn| {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM projects WHERE status = 'active'",
                    [],
                    |row| row.get(0),
                )
                .map_err(db_error)?;
            Ok(count as u64)
        })
    }
}

fn map_project_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProjectRecord> {
    Ok(ProjectRecord {
        project_id: row.get(0)?,
        name: row.get(1)?,
        slug: row.get(2)?,
        root_path: row.get(3)?,
        description: row.get(4)?,
        project_type: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
        last_opened_at: row.get(8)?,
        status: row.get(9)?,
    })
}

fn db_error(error: rusqlite::Error) -> JaymiError {
    JaymiError::new(format!("database error: {error}"))
}
