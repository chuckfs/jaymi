//! Editor context provider (current file / selection / open tabs).

use jaymi_core::JaymiResult;

use crate::provider::{ContextContribution, ContextProvider, ProviderRequest};
use crate::budget::{BudgetEstimate, BudgetUnits, ProviderPriority};
use crate::relevance::{IntentTag, RelevanceScore, RequestKind};
use crate::ContextSource;

/// Contributes editor session state when any file / selection / tab is present.
pub struct EditorProvider;

impl ContextProvider for EditorProvider {
    fn id(&self) -> &'static str {
        "editor"
    }

    fn priority(&self) -> ProviderPriority {
        ProviderPriority::EDITOR
    }

    fn relevance(&self, request: &ProviderRequest<'_>) -> RelevanceScore {
        let signals = request.relevance;
        let has_editor_data = request.session.current_file.path.is_some()
            || request.session.current_selection.path.is_some()
            || !request.session.open_files.files.is_empty();
        RelevanceScore::from_parts([
            if has_editor_data { 35 } else { 0 },
            if signals.coding_workspace() { 40 } else { 0 },
            if signals.has_capability("code") { 25 } else { 0 },
            if matches!(
                signals.request_kind,
                RequestKind::FileRead
                    | RequestKind::FileWrite
                    | RequestKind::Lsp
                    | RequestKind::Git
            ) {
                35
            } else {
                0
            },
            if signals.has_intent(IntentTag::Code) || signals.has_intent(IntentTag::Lsp) {
                20
            } else {
                0
            },
            if matches!(signals.request_kind, RequestKind::Chat) && !signals.coding_workspace() {
                0
            } else {
                0
            },
        ])
    }

    fn estimate_size(&self, request: &ProviderRequest<'_>) -> BudgetEstimate {
        let session = request.session;
        let mut chars = 0usize;
        if let Some(path) = &session.current_file.path {
            chars += path.chars().count() + 16;
        }
        if let Some(text) = &session.current_selection.text {
            chars += text.chars().count() + 32;
        } else if session.current_selection.path.is_some() {
            chars += 48;
        }
        chars += session
            .open_files
            .files
            .iter()
            .map(|file| file.path.chars().count() + 8)
            .sum::<usize>();
        BudgetEstimate::flexible(BudgetUnits::from_characters(chars.max(64), 4))
    }

    fn contribute(
        &self,
        request: &ProviderRequest<'_>,
    ) -> JaymiResult<Option<ContextContribution>> {
        let session = request.session;
        let present = session.current_file.path.is_some()
            || session.current_selection.path.is_some()
            || !session.open_files.files.is_empty();
        if !present {
            return Ok(None);
        }

        Ok(Some(ContextContribution {
            sources: vec![ContextSource::EditorState],
            current_file: Some(session.current_file.clone()),
            current_selection: Some(session.current_selection.clone()),
            open_files: Some(session.open_files.clone()),
            ..ContextContribution::default()
        }))
    }
}
