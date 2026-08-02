//! Project registry persistence for project-scoped memory.

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
    /// Unix seconds created.
    pub created_at: i64,
    /// Unix seconds updated.
    pub updated_at: i64,
    /// Status label (`active` / `archived`).
    pub status: String,
}

impl Database {
    /// Insert or update a project.
    pub fn upsert_project(&self, record: &ProjectRecord) -> JaymiResult<()> {
        self.with_connection(|conn| {
            conn.execute(
                "INSERT INTO projects (
                    project_id, name, slug, root_path, created_at, updated_at, status
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(project_id) DO UPDATE SET
                    name = excluded.name,
                    slug = excluded.slug,
                    root_path = excluded.root_path,
                    updated_at = excluded.updated_at,
                    status = excluded.status",
                params![
                    record.project_id,
                    record.name,
                    record.slug,
                    record.root_path,
                    record.created_at,
                    record.updated_at,
                    record.status,
                ],
            )
            .map_err(db_error)?;
            Ok(())
        })
    }

    /// Load a project by id.
    pub fn get_project(&self, project_id: &str) -> JaymiResult<Option<ProjectRecord>> {
        self.with_connection(|conn| {
            conn.query_row(
                "SELECT project_id, name, slug, root_path, created_at, updated_at, status
                 FROM projects WHERE project_id = ?1",
                params![project_id],
                map_project_row,
            )
            .optional()
            .map_err(db_error)
        })
    }

    /// Find a project by display name or slug (case-insensitive).
    pub fn find_project_by_name(&self, name: &str) -> JaymiResult<Option<ProjectRecord>> {
        let needle = name.trim().to_ascii_lowercase();
        if needle.is_empty() {
            return Ok(None);
        }
        self.with_connection(|conn| {
            conn.query_row(
                "SELECT project_id, name, slug, root_path, created_at, updated_at, status
                 FROM projects
                 WHERE lower(name) = ?1 OR lower(slug) = ?1
                 ORDER BY updated_at DESC, project_id ASC
                 LIMIT 1",
                params![needle],
                map_project_row,
            )
            .optional()
            .map_err(db_error)
        })
    }

    /// List conversation ids attached to a project (newest first).
    pub fn list_conversation_ids_for_project(
        &self,
        project_id: &str,
    ) -> JaymiResult<Vec<String>> {
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

    /// Count registered projects.
    pub fn project_count(&self) -> JaymiResult<u64> {
        self.with_connection(|conn| {
            let count: i64 = conn
                .query_row("SELECT COUNT(*) FROM projects", [], |row| row.get(0))
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
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
        status: row.get(6)?,
    })
}

fn db_error(error: rusqlite::Error) -> JaymiError {
    JaymiError::new(format!("database error: {error}"))
}
