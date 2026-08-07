//! Editor context provider (current file / selection / open tabs / intelligence).
//!
//! Sprint **B2.3:** prefers the host [`crate::EditorSnapshot`] on session inputs.
//! Never calls LSP — Planner and Reasoning also never talk to LSP for assemble.
//! Sprint **B2.13.1:** proposes fine-grained [`ContextCandidate`] nodes.

use jaymi_core::JaymiResult;

use crate::candidate::{CandidatePayload, ContextCandidate, ContextCandidateKind};
use crate::provider::{ContextProvider, ProviderRequest};
use crate::budget::{BudgetEstimate, BudgetUnits, ProviderPriority};
use crate::relevance::{IntentTag, RelevanceScore, RequestKind};
use crate::ContextSource;

/// Contributes editor session state when any file / selection / tab is present.
pub struct EditorProvider;

impl EditorProvider {
    fn has_editor_data(request: &ProviderRequest<'_>) -> bool {
        if let Some(snapshot) = request.session.editor_snapshot.as_ref() {
            return snapshot.has_editor_state();
        }
        request.session.current_file.path.is_some()
            || request.session.current_selection.path.is_some()
            || !request.session.open_files.files.is_empty()
    }
}

impl ContextProvider for EditorProvider {
    fn id(&self) -> &'static str {
        "editor"
    }

    fn priority(&self) -> ProviderPriority {
        ProviderPriority::EDITOR
    }

    fn relevance(&self, request: &ProviderRequest<'_>) -> RelevanceScore {
        let signals = request.relevance;
        let has_editor_data = Self::has_editor_data(request);
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
        if let Some(snapshot) = session.editor_snapshot.as_ref() {
            if let Some(path) = &snapshot.active_file.path {
                chars += path.chars().count() + 16;
            }
            if let Some(text) = &snapshot.selection.text {
                chars += text.chars().count() + 32;
            } else if snapshot.selection.path.is_some() {
                chars += 48;
            }
            chars += snapshot
                .open_editors
                .files
                .iter()
                .map(|file| file.path.chars().count() + 8)
                .sum::<usize>();
            if let Some(hover) = &snapshot.hover {
                chars += hover.contents.chars().count().min(2_048) + 24;
            }
            if let Some(symbol) = &snapshot.symbol {
                chars += symbol.name.chars().count() + 16;
            }
            chars += snapshot.references.len().saturating_mul(48);
            chars += snapshot.semantic_tokens.len().saturating_mul(12);
            chars += snapshot.code_lens.len().saturating_mul(24);
        } else {
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
        }
        BudgetEstimate::flexible(BudgetUnits::from_characters(chars.max(64), 4))
    }

    fn propose_candidates(
        &self,
        request: &ProviderRequest<'_>,
    ) -> JaymiResult<Vec<ContextCandidate>> {
        let session = request.session;
        let mut out = Vec::new();
        let sensitivity = self.sensitivity();
        let priority = self.priority();

        if let Some(snapshot) = session.editor_snapshot.as_ref() {
            if !snapshot.has_editor_state() && !snapshot.has_intelligence() {
                return Ok(Vec::new());
            }
            if snapshot.active_file.path.is_some() {
                let key = snapshot
                    .active_file
                    .path
                    .clone()
                    .unwrap_or_else(|| "file".into());
                out.push(ContextCandidate::new(
                    self.id(),
                    ContextCandidateKind::CurrentFile,
                    ContextSource::EditorState,
                    key,
                    CandidatePayload::CurrentFile(snapshot.active_file.clone()),
                    sensitivity,
                    85,
                    priority,
                    true,
                ));
            }
            if snapshot.selection.path.is_some() || snapshot.selection.text.is_some() {
                out.push(ContextCandidate::new(
                    self.id(),
                    ContextCandidateKind::Selection,
                    ContextSource::EditorState,
                    "selection",
                    CandidatePayload::CurrentSelection(snapshot.selection.clone()),
                    sensitivity,
                    80,
                    priority,
                    false,
                ));
            }
            for entry in snapshot.open_editors.files.iter().take(48) {
                out.push(ContextCandidate::new(
                    self.id(),
                    ContextCandidateKind::OpenFile,
                    ContextSource::EditorState,
                    &entry.path,
                    CandidatePayload::OpenFile(entry.clone()),
                    sensitivity,
                    60,
                    priority,
                    false,
                ));
            }
            if snapshot.has_intelligence() {
                out.push(ContextCandidate::new(
                    self.id(),
                    ContextCandidateKind::EditorIntelligence,
                    ContextSource::EditorIntelligence,
                    "intel",
                    CandidatePayload::EditorIntelligence(snapshot.intelligence_section()),
                    sensitivity,
                    75,
                    priority,
                    false,
                ));
            }
            return Ok(out);
        }

        let present = session.current_file.path.is_some()
            || session.current_selection.path.is_some()
            || !session.open_files.files.is_empty();
        if !present {
            return Ok(Vec::new());
        }
        if session.current_file.path.is_some() {
            let key = session
                .current_file
                .path
                .clone()
                .unwrap_or_else(|| "file".into());
            out.push(ContextCandidate::new(
                self.id(),
                ContextCandidateKind::CurrentFile,
                ContextSource::EditorState,
                key,
                CandidatePayload::CurrentFile(session.current_file.clone()),
                sensitivity,
                85,
                priority,
                true,
            ));
        }
        if session.current_selection.path.is_some() || session.current_selection.text.is_some() {
            out.push(ContextCandidate::new(
                self.id(),
                ContextCandidateKind::Selection,
                ContextSource::EditorState,
                "selection",
                CandidatePayload::CurrentSelection(session.current_selection.clone()),
                sensitivity,
                80,
                priority,
                false,
            ));
        }
        for entry in session.open_files.files.iter().take(48) {
            out.push(ContextCandidate::new(
                self.id(),
                ContextCandidateKind::OpenFile,
                ContextSource::EditorState,
                &entry.path,
                CandidatePayload::OpenFile(entry.clone()),
                sensitivity,
                60,
                priority,
                false,
            ));
        }
        Ok(out)
    }
}
