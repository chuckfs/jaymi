//! Context Inspector — developer-facing view of the latest assembled context.
//!
//! Pure diagnostics. Building or reading a [`ContextInspectorReport`] never
//! changes Planner / provider / tool execution. The Context Engine records a
//! report after each successful [`crate::ContextEngine::assemble`].

use crate::bundle::{BudgetReport, ContextBundle, ContextSource};
use crate::budget::measure_contribution;
use crate::policy::PolicyReport;
use crate::provider::ContextContribution;

/// Why a provider did not contribute (or how it contributed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderInspectOutcome {
    /// Relevance score was below the engine threshold.
    SkippedRelevance {
        /// Engine threshold in effect.
        threshold: u8,
    },
    /// Context Policy excluded this provider.
    SkippedPolicy {
        /// Policy id that denied (or merged deny).
        policy: String,
        /// Explainability reason.
        reason: String,
        /// Provider sensitivity at decision time.
        sensitivity: String,
    },
    /// Remaining budget could not accept the contribution.
    SkippedBudget {
        /// Characters remaining when skipped.
        remaining_characters: usize,
        /// Provider estimate at skip time.
        estimate_characters: usize,
        /// Short reason (`budget_exhausted`, `estimate_exceeds_budget`, …).
        reason: String,
    },
    /// Provider returned `Ok(None)`.
    Declined,
    /// Contribution was fitted away to empty / dropped.
    Dropped {
        /// Fit summary, when any.
        summary: Option<String>,
    },
    /// Contribution accepted (possibly truncated / summarized).
    Contributed {
        /// Final character size after fitting.
        characters: usize,
        /// Estimated tokens after fitting.
        estimated_tokens: usize,
        /// True when content was truncated to fit.
        truncated: bool,
        /// True when a summary note was produced while fitting.
        summarized: bool,
        /// Fit / truncation summary, when any.
        summary: Option<String>,
        /// Context sources claimed by this contribution.
        sources: Vec<ContextSource>,
    },
}

impl ProviderInspectOutcome {
    /// Stable outcome label for diagnostics grids.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SkippedRelevance { .. } => "skipped_relevance",
            Self::SkippedPolicy { .. } => "skipped_policy",
            Self::SkippedBudget { .. } => "skipped_budget",
            Self::Declined => "declined",
            Self::Dropped { .. } => "dropped",
            Self::Contributed { .. } => "contributed",
        }
    }

    /// True when this provider's data is present in the bundle.
    pub fn contributed(&self) -> bool {
        matches!(self, Self::Contributed { .. })
    }

    /// True when the provider was omitted (not in the final bundle).
    pub fn omitted(&self) -> bool {
        !self.contributed()
    }
}

/// One provider row in the Context Inspector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectedProvider {
    /// Provider id (`memory`, `project`, …).
    pub id: String,
    /// Budget priority (higher first).
    pub priority: u8,
    /// Relevance score for the request (0..=100).
    pub relevance: u8,
    /// Estimated contribution size before contribute (characters).
    pub estimate_characters: usize,
    /// Estimated tokens before contribute.
    pub estimate_tokens: usize,
    /// Assemble outcome for this provider.
    pub outcome: ProviderInspectOutcome,
}

impl InspectedProvider {
    /// Compact one-line detail for dashboards.
    pub fn detail(&self) -> String {
        match &self.outcome {
            ProviderInspectOutcome::Contributed {
                characters,
                truncated,
                summarized,
                summary,
                ..
            } => {
                let mut line = format!(
                    "{} · relevance={} · priority={} · chars={} · truncated={} · summarized={}",
                    self.id,
                    self.relevance,
                    self.priority,
                    characters,
                    truncated,
                    summarized
                );
                if let Some(summary) = summary {
                    line.push_str(" · ");
                    line.push_str(summary);
                }
                line
            }
            ProviderInspectOutcome::SkippedRelevance { threshold } => format!(
                "{} · relevance={} < threshold={} · omitted",
                self.id, self.relevance, threshold
            ),
            ProviderInspectOutcome::SkippedPolicy {
                policy,
                reason,
                sensitivity,
            } => format!(
                "{} · policy={policy} · sensitivity={sensitivity} · excluded · {reason}",
                self.id
            ),
            ProviderInspectOutcome::SkippedBudget {
                remaining_characters,
                estimate_characters,
                reason,
            } => format!(
                "{} · relevance={} · priority={} · omitted ({reason}; estimate={estimate_characters} remaining={remaining_characters})",
                self.id, self.relevance, self.priority
            ),
            ProviderInspectOutcome::Declined => format!(
                "{} · relevance={} · priority={} · declined",
                self.id, self.relevance, self.priority
            ),
            ProviderInspectOutcome::Dropped { summary } => format!(
                "{} · relevance={} · priority={} · dropped{}",
                self.id,
                self.relevance,
                self.priority,
                summary
                    .as_ref()
                    .map(|value| format!(" ({value})"))
                    .unwrap_or_default()
            ),
        }
    }
}

/// Section presence summary for the assembled bundle.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InspectedBundleSection {
    /// Section label.
    pub name: String,
    /// True when the section has meaningful content.
    pub present: bool,
    /// Approximate character size of the section payload.
    pub characters: usize,
    /// Short detail (id / count / preview).
    pub detail: String,
}

/// Developer-facing snapshot of the most recent ContextBundle assemble.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ContextInspectorReport {
    /// Assemble generation counter.
    pub assemble_generation: u64,
    /// Truncated request content preview.
    pub request_preview: String,
    /// Derived request kind label.
    pub request_kind: String,
    /// Active workspace kind, when set.
    pub workspace_kind: Option<String>,
    /// Relevance threshold used for this assemble.
    pub relevance_threshold: u8,
    /// Per-provider decisions (registration / evaluation order preserved for omitted;
    /// contributed rows appear in budget-allocation order among accepted ones).
    pub providers: Vec<InspectedProvider>,
    /// Bundle section summaries derived from the final snapshot.
    pub sections: Vec<InspectedBundleSection>,
    /// Sources claimed on the final bundle.
    pub sources: Vec<ContextSource>,
    /// Budget report from the assemble, when any.
    pub budget: Option<BudgetReport>,
    /// Planner metadata notes from the bundle.
    pub notes: Vec<String>,
    /// True when this report came from a ContextBundle cache hit.
    pub cache_hit: bool,
    /// Context Policy explainability report, when recorded.
    pub policy: Option<PolicyReport>,
}

impl ContextInspectorReport {
    /// Providers that contributed to the bundle.
    pub fn contributed(&self) -> Vec<&InspectedProvider> {
        self.providers
            .iter()
            .filter(|provider| provider.outcome.contributed())
            .collect()
    }

    /// Providers omitted from the bundle (skipped / declined / dropped).
    pub fn omitted(&self) -> Vec<&InspectedProvider> {
        self.providers
            .iter()
            .filter(|provider| provider.outcome.omitted())
            .collect()
    }

    /// Providers whose contribution was truncated or summarized.
    pub fn truncated(&self) -> Vec<&InspectedProvider> {
        self.providers
            .iter()
            .filter(|provider| {
                matches!(
                    provider.outcome,
                    ProviderInspectOutcome::Contributed {
                        truncated: true,
                        ..
                    } | ProviderInspectOutcome::Contributed {
                        summarized: true,
                        ..
                    }
                )
            })
            .collect()
    }

    /// One-line summary for diagnostics headers.
    pub fn summary(&self) -> String {
        let contributed = self.contributed().len();
        let omitted = self.omitted().len();
        let truncated = self.truncated().len();
        let budget = self
            .budget
            .as_ref()
            .map(|report| {
                format!(
                    "{} / {} chars (≈{} tok)",
                    report.used_characters, report.max_characters, report.estimated_tokens
                )
            })
            .unwrap_or_else(|| "n/a".into());
        format!(
            "gen={} · kind={} · workspace={} · contributed={} omitted={} truncated={} · cache_hit={} · budget={}",
            self.assemble_generation,
            self.request_kind,
            self.workspace_kind.as_deref().unwrap_or("-"),
            contributed,
            omitted,
            truncated,
            self.cache_hit,
            budget
        )
    }

    /// Plain-text render for CLI / headless diagnostics.
    pub fn render(&self) -> String {
        let mut lines = Vec::new();
        lines.push("Context Inspector".to_string());
        lines.push(self.summary());
        lines.push(format!("request: {}", self.request_preview));
        lines.push(String::new());
        lines.push(format!(
            "{:<14} {:<10} {:>8} {:>8} {:>8} {}",
            "Provider", "Outcome", "Rel", "Pri", "Chars", "Detail"
        ));
        lines.push("-".repeat(88));
        for provider in &self.providers {
            let chars = match &provider.outcome {
                ProviderInspectOutcome::Contributed { characters, .. } => {
                    characters.to_string()
                }
                _ => "-".into(),
            };
            lines.push(format!(
                "{:<14} {:<10} {:>8} {:>8} {:>8} {}",
                provider.id,
                provider.outcome.as_str(),
                provider.relevance,
                provider.priority,
                chars,
                provider.detail()
            ));
        }
        if !self.sections.is_empty() {
            lines.push(String::new());
            lines.push("Bundle sections".to_string());
            lines.push("-".repeat(72));
            for section in &self.sections {
                lines.push(format!(
                    "{:<18} {:<8} {:>6} chars · {}",
                    section.name,
                    if section.present { "present" } else { "empty" },
                    section.characters,
                    section.detail
                ));
            }
        }
        if let Some(budget) = &self.budget {
            lines.push(String::new());
            lines.push(format!(
                "Budget: used={} max={} tokens≈{} truncated_providers=[{}] skipped_budget=[{}]",
                budget.used_characters,
                budget.max_characters,
                budget.estimated_tokens,
                budget.truncated_providers.join(","),
                budget.skipped_budget.join(",")
            ));
            for summary in &budget.summaries {
                lines.push(format!("  · {summary}"));
            }
        }
        if let Some(policy) = &self.policy {
            lines.push(String::new());
            lines.push(policy.render());
        }
        lines.join("\n")
    }
}

/// Build section summaries from a finished bundle (diagnostics only).
pub fn inspect_bundle_sections(bundle: &ContextBundle, chars_per_token: usize) -> Vec<InspectedBundleSection> {
    let _ = chars_per_token;
    let mut sections = Vec::new();

    let conversation = bundle.conversation();
    sections.push(InspectedBundleSection {
        name: "Conversation".into(),
        present: conversation.id.is_some(),
        characters: opt_len(conversation.id.as_ref())
            + opt_len(conversation.title.as_ref())
            + 24,
        detail: conversation
            .id
            .as_ref()
            .map(|id| {
                format!(
                    "id={id} title={}",
                    conversation.title.as_deref().unwrap_or("-")
                )
            })
            .unwrap_or_else(|| "-".into()),
    });

    let project = bundle.active_project();
    sections.push(InspectedBundleSection {
        name: "Active Project".into(),
        present: project.project_id.is_some(),
        characters: opt_len(project.project_id.as_ref())
            + opt_len(project.name.as_ref())
            + project
                .detail
                .as_ref()
                .map(|ctx| ctx.entry_count().saturating_mul(160))
                .unwrap_or(0),
        detail: project
            .name
            .as_ref()
            .map(|name| {
                format!(
                    "name={name} detail={}",
                    if project.detail.is_some() {
                        "yes"
                    } else {
                        "metadata-only"
                    }
                )
            })
            .unwrap_or_else(|| "-".into()),
    });

    let workspace = bundle.active_workspace();
    sections.push(InspectedBundleSection {
        name: "Active Workspace".into(),
        present: workspace.kind_id.is_some(),
        characters: opt_len(workspace.kind_id.as_ref()) + 8,
        detail: workspace.kind_id.clone().unwrap_or_else(|| "-".into()),
    });

    let file = bundle.current_file();
    sections.push(InspectedBundleSection {
        name: "Current File".into(),
        present: file.path.is_some(),
        characters: opt_len(file.path.as_ref()) + 8,
        detail: file.path.clone().unwrap_or_else(|| "-".into()),
    });

    let selection = bundle.current_selection();
    sections.push(InspectedBundleSection {
        name: "Current Selection".into(),
        present: selection.path.is_some() || selection.text.is_some(),
        characters: opt_len(selection.path.as_ref()) + opt_len(selection.text.as_ref()) + 16,
        detail: selection
            .text
            .as_ref()
            .map(|text| truncate(text, 48))
            .unwrap_or_else(|| "-".into()),
    });

    let open_files = bundle.open_files();
    sections.push(InspectedBundleSection {
        name: "Open Files".into(),
        present: !open_files.files.is_empty(),
        characters: open_files
            .files
            .iter()
            .map(|file| file.path.chars().count() + 8)
            .sum(),
        detail: format!("{} file(s)", open_files.files.len()),
    });

    let search = bundle.search_results();
    sections.push(InspectedBundleSection {
        name: "Search Results".into(),
        present: search.hint.is_some() || !search.hits.is_empty(),
        characters: measure_contribution(
            &ContextContribution {
                search_results: Some(search.clone()),
                ..ContextContribution::default()
            },
            4,
        )
        .characters,
        detail: format!(
            "hits={} hint={}",
            search.hits.len(),
            if search.hint.is_some() { "yes" } else { "no" }
        ),
    });

    let memory = bundle.memory_results();
    sections.push(InspectedBundleSection {
        name: "Memory Results".into(),
        present: !memory.memory.is_empty() || !memory.promotion_suggestions.is_empty(),
        characters: measure_contribution(
            &ContextContribution {
                memory_results: Some(memory.clone()),
                ..ContextContribution::default()
            },
            4,
        )
        .characters,
        detail: format!(
            "memories={} promotions={} truncated={}",
            memory.memory.len(),
            memory.promotion_suggestions.len(),
            memory.memory.truncated
        ),
    });

    let diagnostics = bundle.diagnostics();
    sections.push(InspectedBundleSection {
        name: "Diagnostics".into(),
        present: !diagnostics.diagnostics.is_empty(),
        characters: diagnostics
            .diagnostics
            .iter()
            .map(|diag| diag.message.chars().count() + 16)
            .sum(),
        detail: format!("{} diagnostic(s)", diagnostics.diagnostics.len()),
    });

    let permissions = bundle.permissions();
    sections.push(InspectedBundleSection {
        name: "Permissions".into(),
        present: !permissions.entries.is_empty(),
        characters: permissions
            .entries
            .iter()
            .map(|entry| entry.category.chars().count() + entry.action.chars().count() + 16)
            .sum(),
        detail: format!("{} grant(s)", permissions.entries.len()),
    });

    let caps = bundle.active_capabilities();
    sections.push(InspectedBundleSection {
        name: "Active Capabilities".into(),
        present: !caps.capability_ids.is_empty(),
        characters: caps.capability_ids.iter().map(|id| id.chars().count() + 1).sum(),
        detail: if caps.capability_ids.is_empty() {
            "-".into()
        } else {
            caps.capability_ids.join(",")
        },
    });

    let user = bundle.user_request();
    sections.push(InspectedBundleSection {
        name: "User Request".into(),
        present: !user.content_preview.is_empty(),
        characters: user.content_preview.chars().count() + 32,
        detail: truncate(&user.content_preview, 64),
    });

    sections
}

fn opt_len(value: Option<&String>) -> usize {
    value.map(|s| s.chars().count()).unwrap_or(0)
}

fn truncate(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let truncated: String = value.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{truncated}…")
}

