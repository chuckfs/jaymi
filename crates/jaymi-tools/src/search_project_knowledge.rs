//! Search Project Knowledge Tool — project-scoped retrieval through the Project Engine.

use std::sync::Arc;

use jaymi_capabilities::Capability;
use jaymi_core::{JaymiError, JaymiResult, ProjectKnowledgeRequest};
use jaymi_project_engine::{
    ProjectEngineApi, ProjectKnowledgeHit, ProjectKnowledgeQuery,
};
use jaymi_providers::FILESYSTEM_PROVIDER_ID;

use crate::metadata::{
    EstimatedRuntime, ExecutionMode, GpuRequirements, InternetRequirement, MemoryUsage,
    PrivacyMode, Reliability, ResourceCost, ResultType, ToolMetadata,
};
use crate::tool::{Tool, ToolInput, ToolOutput};

/// Stable tool identifier used by the Planner and registries.
pub const SEARCH_PROJECT_KNOWLEDGE_TOOL_ID: &str = "search_project_knowledge";

/// Tool that searches knowledge belonging to one project.
///
/// Architecture path:
/// Planner → Search capability → this tool → Project Engine → Search / Memory
pub struct SearchProjectKnowledgeTool {
    metadata: ToolMetadata,
    projects: Arc<dyn ProjectEngineApi>,
}

impl SearchProjectKnowledgeTool {
    /// Create a project-knowledge tool bound to the Project Engine.
    pub fn new(projects: Arc<dyn ProjectEngineApi>) -> Self {
        Self {
            metadata: ToolMetadata {
                id: SEARCH_PROJECT_KNOWLEDGE_TOOL_ID.to_string(),
                name: "Search Project Knowledge".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                description:
                    "Search files, memories, tasks, and decisions inside one project boundary"
                        .to_string(),
                provider: FILESYSTEM_PROVIDER_ID.to_string(),
                capabilities: vec![Capability::Search],
                execution_mode: ExecutionMode::Synchronous,
                estimated_runtime: EstimatedRuntime::Fast,
                resource_cost: ResourceCost::VeryLow,
                memory_usage: MemoryUsage::Tiny,
                gpu_requirements: GpuRequirements::None,
                privacy: PrivacyMode::LocalOnly,
                internet: InternetRequirement::Never,
                reliability: Reliability::Stable,
                result_type: ResultType::SearchResults,
            },
            projects,
        }
    }
}

impl Tool for SearchProjectKnowledgeTool {
    fn metadata(&self) -> &ToolMetadata {
        &self.metadata
    }

    fn validate(&self, input: &ToolInput) -> JaymiResult<()> {
        let Some(request) = &input.project_knowledge else {
            return Err(JaymiError::new(
                "search_project_knowledge requires project_knowledge input",
            ));
        };
        if request.project_id.trim().is_empty() {
            return Err(JaymiError::new(
                "search_project_knowledge requires project_id",
            ));
        }
        if request.text.trim().is_empty() {
            return Err(JaymiError::new(
                "search_project_knowledge requires non-empty text",
            ));
        }
        Ok(())
    }

    fn execute(&self, input: &ToolInput) -> JaymiResult<ToolOutput> {
        self.validate(input)?;
        let request = input
            .project_knowledge
            .as_ref()
            .expect("validated project_knowledge");
        let hits = self.projects.search_knowledge(&ProjectKnowledgeQuery {
            project_id: request.project_id.clone(),
            text: request.text.clone(),
            limit: request.limit,
        })?;
        Ok(project_knowledge_output(request, hits))
    }
}

fn project_knowledge_output(
    request: &ProjectKnowledgeRequest,
    hits: Vec<ProjectKnowledgeHit>,
) -> ToolOutput {
    let count = hits.len();
    let message = if count == 0 {
        format!(
            "No project knowledge matched \"{}\" in project {} via search → search_project_knowledge.",
            request.text, request.project_id
        )
    } else {
        format!(
            "Found {count} project knowledge hit(s) for \"{}\" in project {} via search → search_project_knowledge.",
            request.text, request.project_id
        )
    };
    ToolOutput {
        success: true,
        entries: Vec::new(),
        citations: Vec::new(),
        document: None,
        parser_id: None,
        message: Some(message),
        listed_path: None,
        project_knowledge: hits,
                ..Default::default()
        }
}
