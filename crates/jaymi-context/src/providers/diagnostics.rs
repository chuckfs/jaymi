//! Diagnostics context provider.
//!
//! Sprint **B2.3:** prefers diagnostics from the host [`crate::EditorSnapshot`]
//! when present. Never calls LSP during assemble.
//! Sprint **B2.13.1:** proposes one candidate per diagnostic finding.

use jaymi_core::JaymiResult;

use crate::bundle::DiagnosticsSection;
use crate::candidate::{CandidatePayload, ContextCandidate, ContextCandidateKind};
use crate::provider::{ContextProvider, ProviderRequest};
use crate::budget::{BudgetEstimate, BudgetUnits, ProviderPriority};
use crate::relevance::{IntentTag, RelevanceScore, RequestKind};
use crate::ContextSource;

/// Contributes attached diagnostics when the session carries any.
pub struct DiagnosticsProvider;

impl DiagnosticsProvider {
    fn diagnostics_section(request: &ProviderRequest<'_>) -> DiagnosticsSection {
        if let Some(snapshot) = request.session.editor_snapshot.as_ref() {
            if !snapshot.diagnostics.is_empty() {
                return DiagnosticsSection {
                    diagnostics: snapshot.diagnostics.clone(),
                };
            }
        }
        request.session.diagnostics.clone()
    }
}

impl ContextProvider for DiagnosticsProvider {
    fn id(&self) -> &'static str {
        "diagnostics"
    }

    fn priority(&self) -> ProviderPriority {
        ProviderPriority::DIAGNOSTICS
    }

    fn relevance(&self, request: &ProviderRequest<'_>) -> RelevanceScore {
        let signals = request.relevance;
        let has_diagnostics = !Self::diagnostics_section(request).diagnostics.is_empty();
        RelevanceScore::from_parts([
            if has_diagnostics { 45 } else { 0 },
            if matches!(signals.request_kind, RequestKind::Lsp) { 50 } else { 0 },
            if signals.has_intent(IntentTag::Lsp) || signals.has_intent(IntentTag::Code) {
                25
            } else {
                0
            },
            if signals.coding_workspace() { 30 } else { 0 },
            if signals.has_capability("code") { 20 } else { 0 },
            if matches!(
                signals.request_kind,
                RequestKind::FileRead | RequestKind::FileWrite
            ) {
                15
            } else {
                0
            },
        ])
    }

    fn estimate_size(&self, request: &ProviderRequest<'_>) -> BudgetEstimate {
        let chars = Self::diagnostics_section(request)
            .diagnostics
            .iter()
            .map(|diag| {
                diag.message.chars().count()
                    + diag.severity.chars().count()
                    + diag.path.as_ref().map(|p| p.chars().count()).unwrap_or(0)
                    + 16
            })
            .sum::<usize>()
            .max(32);
        BudgetEstimate::flexible(BudgetUnits::from_characters(chars, 4))
    }

    fn propose_candidates(
        &self,
        request: &ProviderRequest<'_>,
    ) -> JaymiResult<Vec<ContextCandidate>> {
        let section = Self::diagnostics_section(request);
        if section.diagnostics.is_empty() {
            return Ok(Vec::new());
        }
        let relevance = self.relevance(request).value();
        let sensitivity = self.sensitivity();
        let priority = self.priority();
        let mut out = Vec::new();
        for (idx, diag) in section.diagnostics.into_iter().take(48).enumerate() {
            let key = format!(
                "{}:{}:{}",
                diag.path.as_deref().unwrap_or("-"),
                diag.line.unwrap_or(0),
                idx
            );
            let importance = match diag.severity.as_str() {
                "error" => 85u8,
                "warning" => 70,
                _ => 55,
            }
            .max(relevance / 2);
            out.push(ContextCandidate::new(
                self.id(),
                ContextCandidateKind::Diagnostic,
                ContextSource::Diagnostics,
                key,
                CandidatePayload::Diagnostic(diag),
                sensitivity,
                importance,
                priority,
                false,
            ));
        }
        Ok(out)
    }
}
