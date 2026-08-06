//! In-conversation Review Card (egui).
//!
//! Renders a [`ReviewCardModel`] inside the conversation scroll area — never
//! as a modal. The card is conversational: opening, Plan, approval notice,
//! and Approve / Modify / Cancel. Buttons emit [`ReviewIntent`] only.

use eframe::egui;

use crate::theme::{inset, radius, space, type_size, Theme};
use jaymi_planner::{ReviewCardModel, ReviewCardState, ReviewIntent};

/// Paint a Review Card and return any newly chosen intent.
///
/// Returns `None` when no button was clicked, the card is already resolved,
/// or the click did not produce a valid intent. Callers must map intents
/// outside paint — never execute from here.
///
/// `modify_note` is the draft free-text guidance for Modify; the UI owns the
/// buffer so it survives frames. Modify is enabled once the note is non-empty.
pub fn render_review_card(
    ui: &mut egui::Ui,
    theme: &Theme,
    model: &ReviewCardModel,
    modify_note: &mut String,
) -> Option<ReviewIntent> {
    let mut chosen: Option<ReviewIntent> = None;
    let pending = model.state.is_pending();

    egui::Frame::new()
        .corner_radius(radius::LG)
        .inner_margin(inset(space::MD, space::MD))
        .fill(theme.surface)
        .stroke(egui::Stroke::new(1.0, theme.border))
        .shadow(theme.elevation_shadow())
        .show(ui, |ui| {
            ui.set_max_width(ui.available_width());

            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(&model.opening)
                        .size(type_size::TITLE)
                        .strong()
                        .color(theme.text_primary),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let revision_label = if model.revision > 1 {
                        format!("{} · rev {}", model.plan_id.as_str(), model.revision)
                    } else {
                        model.plan_id.as_str().to_string()
                    };
                    ui.label(
                        egui::RichText::new(revision_label)
                            .size(type_size::META)
                            .color(theme.text_secondary),
                    );
                });
            });

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
            ui.label(
                egui::RichText::new("Plan")
                    .size(type_size::UI)
                    .strong()
                    .color(theme.text_primary),
            );
            ui.add_space(space::XS);
            if model.plan_items.is_empty() {
                ui.label(
                    egui::RichText::new("• (no concrete steps)")
                        .size(type_size::BODY)
                        .color(theme.text_secondary),
                );
            } else {
                for item in &model.plan_items {
                    ui.label(
                        egui::RichText::new(format!("• {item}"))
                            .size(type_size::BODY)
                            .color(theme.text_primary),
                    );
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
                    ui.label(
                        egui::RichText::new("You can:")
                            .size(type_size::UI)
                            .strong()
                            .color(theme.text_primary),
                    );
                    ui.add_space(space::XS);
                    ui.label(
                        egui::RichText::new("Approve · Cancel · Modify the plan")
                            .size(type_size::META)
                            .color(theme.text_secondary),
                    );

                    if !model.modify_examples.is_empty() {
                        ui.add_space(space::SM);
                        ui.label(
                            egui::RichText::new("For example:")
                                .size(type_size::META)
                                .color(theme.text_secondary),
                        );
                        for example in &model.modify_examples {
                            ui.label(
                                egui::RichText::new(format!("“{example}”"))
                                    .size(type_size::META)
                                    .italics()
                                    .color(theme.text_secondary),
                            );
                        }
                    }

                    ui.add_space(space::SM);
                    ui.add(
                        egui::TextEdit::multiline(modify_note)
                            .desired_width(ui.available_width())
                            .desired_rows(2)
                            .hint_text(modify_hint(model))
                            .frame(true),
                    );
                    ui.add_space(space::SM);

                    ui.horizontal(|ui| {
                        let approve = egui::Button::new(
                            egui::RichText::new("Approve")
                                .size(type_size::UI)
                                .color(theme.on_accent()),
                        )
                        .fill(theme.accent)
                        .corner_radius(radius::SM);
                        if ui.add_enabled(pending, approve).clicked() {
                            chosen = Some(ReviewIntent::Approve {
                                plan_id: model.plan_id.clone(),
                            });
                        }

                        ui.add_space(space::SM);
                        let note = modify_note.trim();
                        let can_modify = pending && !note.is_empty();
                        let modify = egui::Button::new(
                            egui::RichText::new("Modify")
                                .size(type_size::UI)
                                .color(theme.text_primary),
                        )
                        .fill(theme.surface_alt)
                        .corner_radius(radius::SM);
                        if ui.add_enabled(can_modify, modify).clicked() {
                            chosen = Some(ReviewIntent::Modify {
                                plan_id: model.plan_id.clone(),
                                note: Some(note.to_string()),
                            });
                        }

                        ui.add_space(space::SM);
                        let cancel = egui::Button::new(
                            egui::RichText::new("Cancel")
                                .size(type_size::UI)
                                .color(theme.text_secondary),
                        )
                        .fill(theme.surface_alt)
                        .corner_radius(radius::SM);
                        if ui.add_enabled(pending, cancel).clicked() {
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
                        .italics()
                        .color(theme.text_secondary),
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
