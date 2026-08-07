//! In-conversation Review Card (egui) — the Organic system's "signature card".
//!
//! Renders a [`ReviewCardModel`] inside the conversation scroll area — never
//! as a modal. Sage kicker = this is Jaymi's proposal; terracotta buttons =
//! the user's decision. Buttons emit [`ReviewIntent`] only.

use eframe::egui;

use crate::theme::{inset, radius, space, type_size, Theme};
use crate::ui::components::{pill_button, tag, ButtonStyle, TagStyle};
use jaymi_planner::{ReviewCardModel, ReviewCardState, ReviewIntent};

/// Paint a Review Card and return any newly chosen intent.
///
/// Returns `None` when no button was clicked, the card is already resolved,
/// or the click did not produce a valid intent. Callers must map intents
/// outside paint — never execute from here.
///
/// `modify_note` is the draft free-text guidance for Modify; the UI owns the
/// buffer so it survives frames. Modify is enabled once the note is non-empty.
/// `preview_expanded` toggles full vs truncated Preview Before Action bodies.
pub fn render_review_card(
    ui: &mut egui::Ui,
    theme: &Theme,
    model: &ReviewCardModel,
    modify_note: &mut String,
    preview_expanded: &mut bool,
) -> Option<ReviewIntent> {
    let mut chosen: Option<ReviewIntent> = None;
    let pending = model.state.is_pending();

    egui::Frame::new()
        .corner_radius(radius::XL)
        .inner_margin(inset(space::MD + space::XS, space::MD))
        .fill(theme.surface)
        .shadow(theme.shadow_md())
        .show(ui, |ui| {
            ui.set_max_width(ui.available_width());

            ui.horizontal(|ui| {
                tag(ui, theme, "Review before action", TagStyle::Accent2);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let revision_label = if model.revision > 1 {
                        format!("{} · rev {}", model.plan_id.as_str(), model.revision)
                    } else {
                        model.plan_id.as_str().to_string()
                    };
                    ui.label(
                        egui::RichText::new(revision_label)
                            .size(type_size::META)
                            .color(theme.text_faint),
                    );
                });
            });

            ui.add_space(space::SM + space::XS);
            ui.label(
                egui::RichText::new(&model.opening)
                    .font(crate::theme::display_font(type_size::TITLE))
                    .color(theme.text_primary),
            );

            if model.revision > 1 || !model.revision_changes.is_empty() {
                ui.add_space(space::MD);
                ui.label(
                    egui::RichText::new(format!("Changes in revision {}", model.revision))
                        .size(type_size::UI)
                        .strong()
                        .color(theme.text_primary),
                );
                ui.add_space(space::XS);
                if let Some(parent) = &model.parent_plan_id {
                    ui.label(
                        egui::RichText::new(format!("Supersedes plan {}", parent.as_str()))
                            .size(type_size::META)
                            .color(theme.text_secondary),
                    );
                }
                for change in &model.revision_changes {
                    ui.label(
                        egui::RichText::new(format!("• {change}"))
                            .size(type_size::BODY)
                            .color(theme.text_primary),
                    );
                }
            }

            ui.add_space(space::MD);
            if model.plan_items.is_empty() {
                ui.label(
                    egui::RichText::new("(no concrete steps)")
                        .size(type_size::BODY)
                        .color(theme.text_secondary),
                );
            } else {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = space::SM;
                    for (index, item) in model.plan_items.iter().enumerate() {
                        step_row(ui, theme, index + 1, item);
                    }
                });
            }

            if !model.affected_resources.is_empty() {
                ui.add_space(space::MD);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(space::XS, space::XS);
                    for resource in &model.affected_resources {
                        tag(ui, theme, resource, TagStyle::Neutral);
                    }
                });
            }

            if let Some(preview) = &model.action_preview {
                ui.add_space(space::MD);
                ui.label(
                    egui::RichText::new(preview.kind.review_section_title())
                        .size(type_size::UI)
                        .strong()
                        .color(theme.text_primary),
                );
                ui.add_space(space::XS);
                let display = if *preview_expanded {
                    preview.clone()
                } else {
                    preview.clone().truncate_for_display(
                        jaymi_core::PREVIEW_MAX_BODY_LINES,
                        jaymi_core::PREVIEW_MAX_BODY_CHARS,
                    )
                };
                for line in display.summary_lines {
                    ui.label(
                        egui::RichText::new(format!("• {line}"))
                            .size(type_size::BODY)
                            .color(theme.text_primary),
                    );
                }
                if let Some(body) = display.body {
                    ui.add_space(space::XS);
                    ui.label(
                        egui::RichText::new(body)
                            .size(type_size::META)
                            .color(theme.text_secondary)
                            .monospace(),
                    );
                }
                if display.truncated || preview.truncated {
                    ui.add_space(space::XS);
                    let label = if *preview_expanded {
                        "Show less"
                    } else {
                        "Expand preview"
                    };
                    if pill_button(ui, theme, label, ButtonStyle::Ghost).clicked() {
                        *preview_expanded = !*preview_expanded;
                    }
                }
            }

            ui.add_space(space::MD);
            ui.label(
                egui::RichText::new(&model.approval_notice)
                    .size(type_size::BODY)
                    .color(theme.warning),
            );

            ui.add_space(space::SM);
            meta_row(ui, theme, "Risk", model.risk_level.as_str());
            if let Some(method) = model.deletion_method {
                meta_row(ui, theme, "Deletion Method", method.as_str());
            }
            meta_row(ui, theme, "Permissions", &display_list(&model.permissions));
            meta_row(
                ui,
                theme,
                "Duration",
                model.estimated_duration.as_str(),
            );
            meta_row(ui, theme, "Reversibility", model.reversibility.as_str());

            ui.add_space(space::MD);
            match &model.state {
                ReviewCardState::Pending => {
                    if !model.modify_examples.is_empty() {
                        ui.label(
                            egui::RichText::new("For example:")
                                .size(type_size::META)
                                .color(theme.text_faint),
                        );
                        for example in &model.modify_examples {
                            ui.label(
                                egui::RichText::new(format!("“{example}”"))
                                    .size(type_size::META)
                                    .italics()
                                    .color(theme.text_faint),
                            );
                        }
                        ui.add_space(space::SM);
                    }

                    ui.add(
                        egui::TextEdit::multiline(modify_note)
                            .desired_width(ui.available_width())
                            .desired_rows(2)
                            .hint_text(modify_hint(model))
                            .frame(true),
                    );
                    ui.add_space(space::SM + space::XS);

                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = space::SM;
                        if ui
                            .add_enabled_ui(pending, |ui| {
                                pill_button(ui, theme, "Approve", ButtonStyle::Primary)
                            })
                            .inner
                            .clicked()
                        {
                            chosen = Some(ReviewIntent::Approve {
                                plan_id: model.plan_id.clone(),
                            });
                        }

                        let note = modify_note.trim();
                        let can_modify = pending && !note.is_empty();
                        if ui
                            .add_enabled_ui(can_modify, |ui| {
                                pill_button(ui, theme, "Modify", ButtonStyle::Secondary)
                            })
                            .inner
                            .clicked()
                        {
                            chosen = Some(ReviewIntent::Modify {
                                plan_id: model.plan_id.clone(),
                                note: Some(note.to_string()),
                            });
                        }

                        if ui
                            .add_enabled_ui(pending, |ui| {
                                pill_button(ui, theme, "Cancel", ButtonStyle::Ghost)
                            })
                            .inner
                            .clicked()
                        {
                            chosen = Some(ReviewIntent::Cancel {
                                plan_id: model.plan_id.clone(),
                            });
                        }
                    });
                    ui.add_space(space::XS);
                    ui.label(
                        egui::RichText::new(
                            "Approve runs the plan. Modify revises it. Cancel drops it. Nothing runs until then.",
                        )
                        .size(type_size::META)
                        .color(theme.text_faint),
                    );
                }
                ReviewCardState::Resolved { intent } => {
                    ui.label(
                        egui::RichText::new(format!("Recorded: {}", intent.acknowledgement()))
                            .size(type_size::UI)
                            .color(theme.text_secondary),
                    );
                }
            }
        });

    chosen
}

/// One numbered plan step — a small terracotta-tint badge, matching the
/// signature card spec (sage kicker = proposal, terracotta = the user acting
/// on it; the step numbers sit in the terracotta ramp since Approve/Modify
/// are the user's calls to make).
fn step_row(ui: &mut egui::Ui, theme: &Theme, index: usize, text: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::SM;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::hover());
        ui.painter()
            .circle_filled(rect.center(), 10.0, theme.accent_tint);
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            index.to_string(),
            egui::FontId::proportional(type_size::META),
            theme.accent_deep,
        );
        ui.label(
            egui::RichText::new(text)
                .size(type_size::BODY)
                .color(theme.text_primary),
        );
    });
}

fn modify_hint(model: &ReviewCardModel) -> String {
    model
        .modify_examples
        .first()
        .cloned()
        .unwrap_or_else(|| "Describe how to change the plan…".into())
}

fn meta_row(ui: &mut egui::Ui, theme: &Theme, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("{label}:"))
                .size(type_size::META)
                .strong()
                .color(theme.text_secondary),
        );
        ui.label(
            egui::RichText::new(value)
                .size(type_size::META)
                .color(theme.text_primary),
        );
    });
}

fn display_list(items: &[String]) -> String {
    if items.is_empty() {
        "none".to_string()
    } else {
        items.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_planner::{
        EstimatedReversibility, EstimatedRisk, ExecutionPlan, ExecutionPlanParams, ExecutionStep,
        PlanPermissionRequirement, ReviewRequirement,
    };
    use jaymi_capabilities::Capability;
    use jaymi_core::IntentId;

    #[test]
    fn pending_card_model_exposes_conversational_contract() {
        let mut plan = ExecutionPlan::create(ExecutionPlanParams {
            originating_request: "Delete draft".into(),
            planner_intent: IntentId::ManagePath,
            capability: Capability::FileManagement,
            proposed_tools: vec!["manage_path".into()],
            steps: vec![ExecutionStep {
                order: 1,
                description: "Delete draft".into(),
                tool_id: Some("manage_path".into()),
                resource: Some("/tmp/draft".into()),
            }],
            estimated_risk: EstimatedRisk::High,
            affected_resources: vec!["/tmp/draft".into()],
            permissions_required: vec![PlanPermissionRequirement {
                category: "filesystem".into(),
                action: "delete".into(),
            }],
            review_requirement: ReviewRequirement::Required,
            estimated_reversibility: EstimatedReversibility::Irreversible,
            expected_outputs: vec!["managed path".into()],
            deletion_method: None,
            action_preview: None,
            lineage: Default::default(),
        });
        plan.mark_ready().unwrap();
        plan.mark_awaiting_review().unwrap();
        let model = ReviewCardModel::from_plan(&plan, None);
        assert!(model.state.is_pending());
        assert_eq!(model.opening, "I can do that.");
        assert!(model.render_text().contains("You can:"));
        assert!(!model.modify_examples.is_empty());
    }
}
