//! Project Store — persistence behind the Project Engine.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use jaymi_core::{EntityId, JaymiError, JaymiResult};
use jaymi_database::{Database, ProjectRecord};

use crate::types::{slugify_project_name, Project, ProjectStatus, ProjectType};

/// Persistence API for first-class projects.
pub trait ProjectStore: Send + Sync {
    /// Insert or replace a project row.
    fn upsert(&self, project: &Project) -> JaymiResult<()>;

    /// Load by id (including deleted).
    fn get(&self, project_id: &str) -> JaymiResult<Option<Project>>;

    /// Find an active project by name or slug.
    fn find_by_name(&self, name: &str) -> JaymiResult<Option<Project>>;

    /// List active projects.
    fn list_active(&self) -> JaymiResult<Vec<Project>>;

    /// Soft-delete a project.
    fn delete(&self, project_id: &str, now: i64) -> JaymiResult<bool>;

    /// Count projects by status.
    fn count_by_status(&self, status: ProjectStatus) -> JaymiResult<u64>;
}

/// In-memory Project Store for tests.
#[derive(Default)]
pub struct InMemoryProjectStore {
    inner: Mutex<HashMap<String, Project>>,
}

impl InMemoryProjectStore {
    /// Create an empty store.
    pub fn new() -> Self {
        Self::default()
    }
}

impl ProjectStore for InMemoryProjectStore {
    fn upsert(&self, project: &Project) -> JaymiResult<()> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| JaymiError::new("project store lock"))?;
        guard.insert(project.id.as_str().to_string(), project.clone());
        Ok(())
    }

    fn get(&self, project_id: &str) -> JaymiResult<Option<Project>> {
        let guard = self
            .inner
            .lock()
            .map_err(|_| JaymiError::new("project store lock"))?;
        Ok(guard.get(project_id).cloned())
    }

    fn find_by_name(&self, name: &str) -> JaymiResult<Option<Project>> {
        let needle = name.trim().to_ascii_lowercase();
        if needle.is_empty() {
            return Ok(None);
        }
        let guard = self
            .inner
            .lock()
            .map_err(|_| JaymiError::new("project store lock"))?;
        let mut matches: Vec<_> = guard
            .values()
            .filter(|project| {
                project.status == ProjectStatus::Active
                    && (project.name.to_ascii_lowercase() == needle
                        || slugify_project_name(&project.name) == needle)
            })
            .cloned()
            .collect();
        matches.sort_by(|left, right| {
            right
                .last_opened_at
                .unwrap_or(0)
                .cmp(&left.last_opened_at.unwrap_or(0))
                .then(right.updated_at.cmp(&left.updated_at))
                .then(left.id.as_str().cmp(right.id.as_str()))
        });
        Ok(matches.into_iter().next())
    }

    fn list_active(&self) -> JaymiResult<Vec<Project>> {
        let guard = self
            .inner
            .lock()
            .map_err(|_| JaymiError::new("project store lock"))?;
        let mut out: Vec<_> = guard
            .values()
            .filter(|project| project.status == ProjectStatus::Active)
            .cloned()
            .collect();
        out.sort_by(|left, right| {
            right
                .last_opened_at
                .unwrap_or(0)
                .cmp(&left.last_opened_at.unwrap_or(0))
                .then(right.updated_at.cmp(&left.updated_at))
                .then(left.name.cmp(&right.name))
        });
        Ok(out)
    }

    fn delete(&self, project_id: &str, now: i64) -> JaymiResult<bool> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| JaymiError::new("project store lock"))?;
        let Some(project) = guard.get_mut(project_id) else {
            return Ok(false);
        };
        if project.status == ProjectStatus::Deleted {
            return Ok(false);
        }
        project.status = ProjectStatus::Deleted;
        project.updated_at = now;
        Ok(true)
    }

    fn count_by_status(&self, status: ProjectStatus) -> JaymiResult<u64> {
        let guard = self
            .inner
            .lock()
            .map_err(|_| JaymiError::new("project store lock"))?;
        Ok(guard
            .values()
            .filter(|project| project.status == status)
            .count() as u64)
    }
}

/// SQLite-backed Project Store sharing Jaymi's database.
pub struct SqliteProjectStore {
    database: Arc<Database>,
}

impl SqliteProjectStore {
    /// Create a store over the shared database.
    pub fn new(database: Arc<Database>) -> Self {
        Self { database }
    }
}

impl ProjectStore for SqliteProjectStore {
    fn upsert(&self, project: &Project) -> JaymiResult<()> {
        self.database.upsert_project(&to_record(project))
    }

    fn get(&self, project_id: &str) -> JaymiResult<Option<Project>> {
        Ok(self.database.get_project(project_id)?.map(from_record))
    }

    fn find_by_name(&self, name: &str) -> JaymiResult<Option<Project>> {
        Ok(self.database.find_project_by_name(name)?.map(from_record))
    }

    fn list_active(&self) -> JaymiResult<Vec<Project>> {
        Ok(self
            .database
            .list_projects()?
            .into_iter()
            .map(from_record)
            .collect())
    }

    fn delete(&self, project_id: &str, now: i64) -> JaymiResult<bool> {
        self.database.delete_project(project_id, now)
    }

    fn count_by_status(&self, status: ProjectStatus) -> JaymiResult<u64> {
        self.database.count_projects_with_status(status.as_str())
    }
}

fn to_record(project: &Project) -> ProjectRecord {
    ProjectRecord {
        project_id: project.id.as_str().to_string(),
        name: project.name.clone(),
        slug: slugify_project_name(&project.name),
        root_path: project
            .root_directory
            .as_ref()
            .map(|path| path.display().to_string()),
        description: project.description.clone(),
        project_type: project.project_type.as_str().to_string(),
        created_at: project.created_at,
        updated_at: project.updated_at,
        last_opened_at: project.last_opened_at,
        status: project.status.as_str().to_string(),
    }
}

fn from_record(record: ProjectRecord) -> Project {
    Project {
        id: EntityId::new(record.project_id),
        name: record.name,
        description: record.description,
        root_directory: record.root_path.map(PathBuf::from),
        created_at: record.created_at,
        updated_at: record.updated_at,
        last_opened_at: record.last_opened_at,
        project_type: ProjectType::parse(&record.project_type).unwrap_or(ProjectType::General),
        status: ProjectStatus::parse(&record.status).unwrap_or(ProjectStatus::Active),
    }
}
