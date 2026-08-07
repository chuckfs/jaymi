//! Coding Workspace Quick Action Bar — typed Coding Action chrome (Sprint C0.1).
//!
//! Rendering emits [`QuickAction`] clicks only. Mapping those clicks to
//! [`jaymi_core::CodingAction`] lives in [`dispatch_quick_action`] — never
//! inside paint. The app shell submits the action as a normal conversation
//! turn; the Planner owns routing. No direct editor / tool / provider calls.

use eframe::egui;
use jaymi_core::CodingAction;

use crate::theme::{radius, space, type_size, Theme};

/// Height of the Quick Action Bar strip.
pub const QUICK_ACTION_BAR_HEIGHT: f32 = 40.0;

/// Gap between adjacent action buttons (macOS-toolbar density).
pub const QUICK_ACTION_GAP: f32 = 4.0;

/// Horizontal padding inside the bar.
pub const QUICK_ACTION_PAD_X: f32 = space::MD;

/// Approximate width reserved for the trailing "More" overflow control.
pub const MORE_BUTTON_WIDTH: f32 = 52.0;

/// Approximate horizontal chrome reserved beside the action strip
/// (close tile + gaps + optional error label).
pub const QUICK_ACTION_CHROME_RESERVE: f32 = 48.0;

/// One Coding Action button in the Coding Workspace bar.
///
/// Explain resolves to [`CodingAction::ExplainSelection`] or
/// [`CodingAction::ExplainFile`] in Application from Workspace Intelligence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QuickAction {
    /// Explain selection or active file.
    Explain,
    /// Edit selection (conversational).
    Edit,
    /// Refactor selection (proposal only).
    Refactor,
    /// Semantic workspace search.
    Search,
    /// Reviewed project run.
    Run,
    /// Planner-generated Coding Actions menu.
    More,
}

impl QuickAction {
    /// Canonical left-to-right order in the bar.
    pub const ALL: [QuickAction; 6] = [
        QuickAction::Explain,
        QuickAction::Edit,
        QuickAction::Refactor,
        QuickAction::Search,
        QuickAction::Run,
        QuickAction::More,
    ];

    /// Visible button label.
    pub fn label(self) -> &'static str {
        match self {
            QuickAction::Explain => "Explain",
            QuickAction::Edit => "Edit",
            QuickAction::Refactor => "Refactor",
            QuickAction::Search => "Search",
            QuickAction::Run => "Run",
            QuickAction::More => "More",
        }
    }

    /// Hover / accessibility hint.
    pub fn hint(self) -> &'static str {
        match self {
            QuickAction::Explain => "Ask Jaymi to explain the selection or current file",
            QuickAction::Edit => "Ask Jaymi what to change in the selection",
            QuickAction::Refactor => "Ask Jaymi for a refactoring proposal (no edits yet)",
            QuickAction::Search => "Search the workspace (uses selection as query when present)",
            QuickAction::Run => "Propose a reviewed project run command",
            QuickAction::More => "Show Coding Actions menu",
        }
    }

    /// Estimated painted width for layout / overflow (label + pill padding).
    pub fn estimated_width(self) -> f32 {
        let chars = self.label().chars().count() as f32;
        chars * 7.0 + 36.0
    }
}

/// Result of dispatching a quick action — typed Coding Action only.
///
/// Produced by [`dispatch_quick_action`]; Application resolves Explain and
/// submits a conversation turn. Never a dock focus or composer seed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickActionEffect {
    /// Submit this Coding Action through the Planner (Conversation First).
    SubmitCodingAction(CodingAction),
    /// Explain — Application picks selection vs file from CodingState / WI.
    SubmitExplain,
}

/// Map a bar button to a typed Coding Action (pure; no UI side effects).
pub fn dispatch_quick_action(action: QuickAction) -> QuickActionEffect {
    match action {
        QuickAction::Explain => QuickActionEffect::SubmitExplain,
        QuickAction::Edit => QuickActionEffect::SubmitCodingAction(CodingAction::EditSelection),
        QuickAction::Refactor => {
            QuickActionEffect::SubmitCodingAction(CodingAction::RefactorSelection)
        }
        QuickAction::Search => QuickActionEffect::SubmitCodingAction(CodingAction::SearchWorkspace),
        QuickAction::Run => QuickActionEffect::SubmitCodingAction(CodingAction::RunProject),
        QuickAction::More => QuickActionEffect::SubmitCodingAction(CodingAction::OpenCodingActions),
    }
}

/// Which actions fit in the primary strip vs the "More" overflow menu.
///
/// When the overflow control would only contain [`QuickAction::More`], the bar
/// shows More as a primary button that still submits [`CodingAction::OpenCodingActions`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuickActionLayout {
    /// Actions shown as primary text buttons (left → right).
    pub visible: Vec<QuickAction>,
    /// Actions collapsed behind overflow (same relative order).
    pub overflow: Vec<QuickAction>,
}

impl QuickActionLayout {
    /// True when at least one action is behind the overflow control.
    pub fn has_overflow(&self) -> bool {
        !self.overflow.is_empty()
    }
}

/// Compute primary vs overflow actions for an available pixel width.
///
/// Truncates from the **right** so Explain / Edit / Refactor stay visible
/// longest. Reserves space for an overflow control whenever needed.
pub fn layout_quick_actions(available_width: f32) -> QuickActionLayout {
    layout_quick_actions_with(available_width, &QuickAction::ALL)
}

/// Testable layout over an explicit action list.
pub fn layout_quick_actions_with(
    available_width: f32,
    actions: &[QuickAction],
) -> QuickActionLayout {
    if actions.is_empty() {
        return QuickActionLayout {
            visible: Vec::new(),
            overflow: Vec::new(),
        };
    }

    let gaps = |count: usize| -> f32 {
        if count <= 1 {
            0.0
        } else {
            QUICK_ACTION_GAP * (count as f32 - 1.0)
        }
    };

    let full_width: f32 =
        actions.iter().map(|a| a.estimated_width()).sum::<f32>() + gaps(actions.len());

    if full_width <= available_width {
        return QuickActionLayout {
            visible: actions.to_vec(),
            overflow: Vec::new(),
        };
    }

    let budget = (available_width - MORE_BUTTON_WIDTH - QUICK_ACTION_GAP).max(0.0);
    let mut visible = Vec::new();
    let mut used = 0.0;

    for (index, action) in actions.iter().enumerate() {
        let next = action.estimated_width() + if visible.is_empty() { 0.0 } else { QUICK_ACTION_GAP };
        let remaining = actions.len() - index;
        if remaining == 1 && visible.is_empty() {
            break;
        }
        if used + next <= budget {
            used += next;
            visible.push(*action);
        } else {
            break;
        }
    }

    if visible.len() == actions.len() {
        visible.pop();
    }

    let overflow = actions[visible.len()..].to_vec();
    QuickActionLayout { visible, overflow }
}

/// Events the Quick Action Bar can emit (chrome + action clicks).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuickActionBarEvent {
    /// Close the Coding workspace (chrome control, not a Coding Action).
    CloseWorkspace,
    /// User activated a Coding Action button.
    Action(QuickAction),
}

/// Paint the Quick Action Bar. Returns click events — no Planner side effects.
pub fn render_quick_action_bar(
    ui: &mut egui::Ui,
    theme: &Theme,
    open_error: Option<&str>,
) -> Vec<QuickActionBarEvent> {
    let mut events = Vec::new();

    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), QUICK_ACTION_BAR_HEIGHT),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_height(QUICK_ACTION_BAR_HEIGHT);
            ui.set_max_height(QUICK_ACTION_BAR_HEIGHT);

            let close = coding_close_tile(ui, theme);
            if close.clicked() {
                events.push(QuickActionBarEvent::CloseWorkspace);
            }
            ui.add_space(space::SM);

            let error_reserve = if open_error.is_some() { 120.0 } else { 0.0 };
            let action_budget = (ui.available_width() - error_reserve - QUICK_ACTION_CHROME_RESERVE)
                .max(0.0);
            let layout = layout_quick_actions(action_budget);

            for (index, action) in layout.visible.iter().enumerate() {
                if index > 0 {
                    ui.add_space(QUICK_ACTION_GAP);
                }
                if quick_action_button(ui, theme, action.label())
                    .on_hover_text(action.hint())
                    .clicked()
                {
                    events.push(QuickActionBarEvent::Action(*action));
                }
            }

            if layout.has_overflow() {
                ui.add_space(QUICK_ACTION_GAP);
                let more_label = egui::RichText::new("More")
                    .size(type_size::UI)
                    .color(theme.text_primary);
                egui::menu::menu_button(ui, more_label, |ui| {
                    for action in &layout.overflow {
                        let clicked = ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new(action.label())
                                        .size(type_size::UI)
                                        .color(theme.text_primary),
                                )
                                .frame(false)
                                .min_size(egui::vec2(112.0, 24.0)),
                            )
                            .on_hover_text(action.hint())
                            .clicked();
                        if clicked {
                            events.push(QuickActionBarEvent::Action(*action));
                            ui.close_menu();
                        }
                    }
                });
            }

            if let Some(error) = open_error {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(error)
                                .size(type_size::META)
                                .color(theme.error),
                        )
                        .truncate(),
                    );
                });
            }
        },
    );

    events
}

/// A sage-tinted pill chip — these are Jaymi-offered actions (quick actions
/// submit a conversation turn), so they read as proposals, not plain UI.
fn quick_action_button(ui: &mut egui::Ui, theme: &Theme, label: &str) -> egui::Response {
    let font = egui::FontId::proportional(type_size::UI);
    let galley = ui.painter().layout_no_wrap(label.to_string(), font.clone(), egui::Color32::PLACEHOLDER);
    let size = egui::vec2(galley.size().x + space::SM * 2.0, 26.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let hovered = response.hovered();
    ui.painter().rect_filled(
        rect,
        egui::CornerRadius::same(radius::PILL as u8),
        if hovered { theme.accent2_soft } else { theme.accent2_tint },
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        font,
        theme.accent2_deep,
    );
    response
}

/// Compact Coding glyph — accent tile with `{}`; hover flips to × to close.
fn coding_close_tile(ui: &mut egui::Ui, theme: &Theme) -> egui::Response {
    let size = egui::vec2(24.0, 24.0);
    let (rect, mut response) = ui.allocate_exact_size(size, egui::Sense::click());
    response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let hovered = response.hovered();

    let fill = if hovered { theme.error } else { theme.accent };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(radius::PILL as u8), fill);

    if hovered {
        let c = rect.center();
        let arm = 4.5;
        let stroke = egui::Stroke::new(1.75, theme.on_accent());
        ui.painter().line_segment(
            [c + egui::vec2(-arm, -arm), c + egui::vec2(arm, arm)],
            stroke,
        );
        ui.painter().line_segment(
            [c + egui::vec2(arm, -arm), c + egui::vec2(-arm, arm)],
            stroke,
        );
    } else {
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "{}",
            egui::FontId::monospace(type_size::META),
            theme.on_accent(),
        );
    }

    response.on_hover_text("Close Coding")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quick_action_dispatch_is_typed_coding_action() {
        assert_eq!(
            dispatch_quick_action(QuickAction::Explain),
            QuickActionEffect::SubmitExplain
        );
        assert_eq!(
            dispatch_quick_action(QuickAction::Edit),
            QuickActionEffect::SubmitCodingAction(CodingAction::EditSelection)
        );
        assert_eq!(
            dispatch_quick_action(QuickAction::Refactor),
            QuickActionEffect::SubmitCodingAction(CodingAction::RefactorSelection)
        );
        assert_eq!(
            dispatch_quick_action(QuickAction::Search),
            QuickActionEffect::SubmitCodingAction(CodingAction::SearchWorkspace)
        );
        assert_eq!(
            dispatch_quick_action(QuickAction::Run),
            QuickActionEffect::SubmitCodingAction(CodingAction::RunProject)
        );
        assert_eq!(
            dispatch_quick_action(QuickAction::More),
            QuickActionEffect::SubmitCodingAction(CodingAction::OpenCodingActions)
        );
    }

    #[test]
    fn toolbar_layout() {
        let labels: Vec<&str> = QuickAction::ALL.iter().map(|a| a.label()).collect();
        assert_eq!(
            labels,
            vec!["Explain", "Edit", "Refactor", "Search", "Run", "More"]
        );

        let roomy = layout_quick_actions(2000.0);
        assert_eq!(roomy.visible, QuickAction::ALL.to_vec());
        assert!(roomy.overflow.is_empty());
        assert_eq!(roomy.visible[0], QuickAction::Explain);
        assert_eq!(roomy.visible[5], QuickAction::More);
    }

    #[test]
    fn overflow_behavior() {
        let narrow = layout_quick_actions(220.0);
        assert!(narrow.has_overflow());
        assert!(!narrow.visible.is_empty());
        let mut combined = narrow.visible.clone();
        combined.extend(narrow.overflow.iter().copied());
        assert_eq!(combined, QuickAction::ALL.to_vec());
        assert!(narrow.visible.contains(&QuickAction::Explain));

        let tiny = layout_quick_actions(40.0);
        assert!(tiny.visible.is_empty());
        assert_eq!(tiny.overflow, QuickAction::ALL.to_vec());
    }
}
