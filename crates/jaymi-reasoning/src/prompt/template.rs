//! Future prompt templates — section order and system copy hooks.

use super::section::PromptSectionId;

/// Declares which sections to emit and in what order.
///
/// Sprint B1.2 ships [`DefaultPromptTemplate`]. Future templates can swap
/// ordering / system instructions without touching providers.
pub trait PromptTemplate: Send + Sync {
    /// Stable template id.
    fn id(&self) -> &str;

    /// Section emission order.
    fn section_order(&self) -> &[PromptSectionId];

    /// Default system instructions body (may be overridden on the builder).
    fn default_system_instructions(&self) -> &str;
}

/// Canonical Jaymi conversational reasoning template.
#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultPromptTemplate;

impl PromptTemplate for DefaultPromptTemplate {
    fn id(&self) -> &str {
        "jaymi.default.v1"
    }

    fn section_order(&self) -> &[PromptSectionId] {
        PromptSectionId::ORDER
    }

    fn default_system_instructions(&self) -> &str {
        "You are Jaymi, a local-first personal AI environment. \
Reason over the structured context below. Prefer project and memory facts \
when present. When an Environmental Resolution section is present, treat those \
Planner bindings as authoritative for 'this' / 'it' / 'why' / similar deixis — \
never invent workspace paths, files, or symbols on your own. When a Coding \
Understanding section is present, answer with the structured sections it \
requests using only Workspace Intelligence already in the prompt — do not call \
tools, scan the filesystem, modify files, execute commands, or produce an \
Execution Plan. For Project Understanding, use Overview · Architecture · \
Important Modules · Relationships · Activity & Risks · Suggested Next Actions. \
When a Coding Review section is present, answer with Strengths · Weaknesses · \
Potential Bugs · Complexity · Performance · Maintainability · Architecture — \
review only, no edits or Execution Plans. When a Coding Plan section is present, \
answer with Plan · Files to Create · Files to Modify · Dependencies · Estimated \
Risk · Summary — planning only, no code generation, tool execution, file writes, \
or Execution Plans. \
Do not invent permissions or tool results."
    }
}
