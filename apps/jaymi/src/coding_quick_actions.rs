//! Coding Workspace Quick Action Bar — Planner-intent chrome (not a VS Code toolbar).
//!
//! Rendering emits [`QuickAction`] clicks only. Mapping those clicks to Planner
//! prompts or panel focus lives in [`dispatch_quick_action`] — never inside paint.

use eframe::egui;

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

/// One Planner-oriented quick action in the Coding Workspace bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QuickAction {
    /// Ask the Planner to explain the active file.
    Explain,
    /// Ask the Planner to edit the active file.
    Edit,
    /// Ask the Planner to refactor the current selection.
    Refactor,
    /// Open Find in Files (Search dock).
    Search,
    /// Open the Terminal dock (run / execute context).
    Run,
    /// Focus the Terminal dock.
    Terminal,
    /// Focus the Git dock.
    Git,
}

impl QuickAction {
    /// Canonical left-to-right order in the bar.
    pub const ALL: [QuickAction; 7] = [
        QuickAction::Explain,
        QuickAction::Edit,
        QuickAction::Refactor,
        QuickAction::Search,
        QuickAction::Run,
        QuickAction::Terminal,
        QuickAction::Git,
    ];

    /// Visible button label.
    pub fn label(self) -> &'static str {
        match self {
            QuickAction::Explain => "Explain",
            QuickAction::Edit => "Edit",
            QuickAction::Refactor => "Refactor",
            QuickAction::Search => "Search",
            QuickAction::Run => "Run",
            QuickAction::Terminal => "Terminal",
            QuickAction::Git => "Git",
        }
    }

    /// Hover / accessibility hint.
    pub fn hint(self) -> &'static str {
        match self {
            QuickAction::Explain => "Ask Jaymi to explain the current file",
            QuickAction::Edit => "Ask Jaymi to edit the current file",
            QuickAction::Refactor => "Ask Jaymi to refactor the selected code",
            QuickAction::Search => "Open Find in Files",
            QuickAction::Run => "Open Terminal to run commands",
            QuickAction::Terminal => "Focus the Terminal panel",
            QuickAction::Git => "Focus the Git panel",
        }
    }

    /// Estimated painted width for layout / overflow (label + padding).
    pub fn estimated_width(self) -> f32 {
        let chars = self.label().chars().count() as f32;
        // META/UI proportional ~7px + horizontal padding inside the button.
        chars * 7.0 + 20.0
    }
}

/// Result of dispatching a quick action — composer seed or panel focus.
///
/// This is a **UI effect**, not Planner [`jaymi_core::IntentId`]. Prompts inserted
/// into the composer are later classified by the Planner Decision Engine.
///
/// Produced by [`dispatch_quick_action`]; applied by the app shell, never by paint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuickActionEffect {
    /// Insert a Planner prompt into the conversation composer.
    InsertPlannerPrompt(&'static str),
    /// Open / focus the Search (Find in Files) dock page.
    OpenSearchPanel,
    /// Open the Terminal dock page (Run).
    OpenTerminalPanel,
    /// Focus the Terminal dock page.
    FocusTerminalPanel,
    /// Focus the Git dock page.
    FocusGitPanel,
}

/// Map a bar button to a composer seed or dock focus (pure; no UI side effects).
pub fn dispatch_quick_action(action: QuickAction) -> QuickActionEffect {
    match action {
        QuickAction::Explain => {
            QuickActionEffect::InsertPlannerPrompt("Explain the current file")
        }
        QuickAction::Edit => QuickActionEffect::InsertPlannerPrompt("Edit the current file"),
        QuickAction::Refactor => {
            QuickActionEffect::InsertPlannerPrompt("Refactor the selected code")
        }
        QuickAction::Search => QuickActionEffect::OpenSearchPanel,
        QuickAction::Run => QuickActionEffect::OpenTerminalPanel,
        QuickAction::Terminal => QuickActionEffect::FocusTerminalPanel,
        QuickAction::Git => QuickActionEffect::FocusGitPanel,
    }
}

/// Which actions fit in the primary strip vs the "More" overflow menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuickActionLayout {
    /// Actions shown as primary text buttons (left → right).
    pub visible: Vec<QuickAction>,
    /// Actions collapsed behind "More" (same relative order).
    pub overflow: Vec<QuickAction>,
}

impl QuickActionLayout {
    /// True when at least one action is behind "More".
    pub fn has_overflow(&self) -> bool {
        !self.overflow.is_empty()
    }
}

/// Compute primary vs overflow actions for an available pixel width.
///
/// Truncates from the **right** (trailing intents move into More first) so
/// Explain / Edit / Refactor stay visible longest. Reserves space for a "More"
/// control whenever any action would overflow.
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

    let full_width: f32 = actions.iter().map(|a| a.estimated_width()).sum::<f32>()
        + gaps(actions.len());

    if full_width <= available_width {
        return QuickActionLayout {
            visible: actions.to_vec(),
            overflow: Vec::new(),
        };
    }

    // Need a More button — reserve it, then pack as many leading actions as fit.
    let budget = (available_width - MORE_BUTTON_WIDTH - QUICK_ACTION_GAP).max(0.0);
    let mut visible = Vec::new();
    let mut used = 0.0;

    for (index, action) in actions.iter().enumerate() {
        let next = action.estimated_width() + if visible.is_empty() { 0.0 } else { QUICK_ACTION_GAP };
        let remaining = actions.len() - index;
        // Always leave at least one action for overflow when we already know
        // not everything fits (otherwise More would be empty).
        if remaining == 1 && visible.is_empty() {
            // Extremely narrow: show nothing primary; everything in More.
            break;
        }
        if used + next <= budget {
            used += next;
            visible.push(*action);
        } else {
            break;
        }
    }

    // Ensure More is never empty when the full set doesn't fit.
    if visible.len() == actions.len() {
        visible.pop();
    }

    let overflow = actions[visible.len()..].to_vec();
    QuickActionLayout { visible, overflow }
}

/// Events the Quick Action Bar can emit (chrome + action clicks).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuickActionBarEvent {
    /// Close the Coding workspace (chrome control, not a Planner intent).
    CloseWorkspace,
    /// User activated a Planner / dock quick action.
    Action(QuickAction),
}

/// Paint the Quick Action Bar. Returns click events — no Planner/panel side effects.
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
            let action_budget = (ui.available_width()
                - error_reserve
                - QUICK_ACTION_CHROME_RESERVE)
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

fn quick_action_button(ui: &mut egui::Ui, theme: &Theme, label: &str) -> egui::Response {
    let text = egui::RichText::new(label.to_owned())
        .size(type_size::UI)
        .color(theme.text_primary);
    ui.add(
        egui::Button::new(text)
            .frame(false)
            .min_size(egui::vec2(0.0, 26.0)),
    )
    .on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Compact Coding glyph — accent tile with `{}`; hover flips to × to close.
fn coding_close_tile(ui: &mut egui::Ui, theme: &Theme) -> egui::Response {
    let size = egui::vec2(24.0, 24.0);
    let (rect, mut response) = ui.allocate_exact_size(size, egui::Sense::click());
    response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let hovered = response.hovered();

    let fill = if hovered { theme.error } else { theme.accent };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(radius::SM as u8), fill);

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
    fn quick_action_dispatch() {
        assert_eq!(
            dispatch_quick_action(QuickAction::Explain),
            QuickActionEffect::InsertPlannerPrompt("Explain the current file")
        );
        assert_eq!(
            dispatch_quick_action(QuickAction::Edit),
            QuickActionEffect::InsertPlannerPrompt("Edit the current file")
        );
        assert_eq!(
            dispatch_quick_action(QuickAction::Refactor),
            QuickActionEffect::InsertPlannerPrompt("Refactor the selected code")
        );
        assert_eq!(
            dispatch_quick_action(QuickAction::Search),
            QuickActionEffect::OpenSearchPanel
        );
        assert_eq!(
            dispatch_quick_action(QuickAction::Run),
            QuickActionEffect::OpenTerminalPanel
        );
        assert_eq!(
            dispatch_quick_action(QuickAction::Terminal),
            QuickActionEffect::FocusTerminalPanel
        );
        assert_eq!(
            dispatch_quick_action(QuickAction::Git),
            QuickActionEffect::FocusGitPanel
        );
    }

    #[test]
    fn toolbar_layout() {
        let labels: Vec<&str> = QuickAction::ALL.iter().map(|a| a.label()).collect();
        assert_eq!(
            labels,
            vec![
                "Explain",
                "Edit",
                "Refactor",
                "Search",
                "Run",
                "Terminal",
                "Git"
            ]
        );

        let roomy = layout_quick_actions(2000.0);
        assert_eq!(roomy.visible, QuickAction::ALL.to_vec());
        assert!(roomy.overflow.is_empty());
        assert!(!roomy.has_overflow());

        // Order is stable and left-biased: Planner intents lead the strip.
        assert_eq!(roomy.visible[0], QuickAction::Explain);
        assert_eq!(roomy.visible[1], QuickAction::Edit);
        assert_eq!(roomy.visible[2], QuickAction::Refactor);
    }

    #[test]
    fn overflow_behavior() {
        let full = layout_quick_actions(2000.0);
        assert!(full.overflow.is_empty());

        // Narrow enough that trailing actions collapse into More.
        let narrow = layout_quick_actions(220.0);
        assert!(narrow.has_overflow());
        assert!(!narrow.visible.is_empty());
        assert!(!narrow.overflow.is_empty());
        // Visible + overflow partition the full set without duplicates.
        let mut combined = narrow.visible.clone();
        combined.extend(narrow.overflow.iter().copied());
        assert_eq!(combined, QuickAction::ALL.to_vec());
        // Trailing actions overflow first.
        assert!(narrow.overflow.contains(&QuickAction::Git));
        assert!(narrow.visible.contains(&QuickAction::Explain));

        // Extremely narrow — everything behind More.
        let tiny = layout_quick_actions(40.0);
        assert!(tiny.visible.is_empty());
        assert_eq!(tiny.overflow, QuickAction::ALL.to_vec());

        // Mid width: More appears and primary count shrinks vs full.
        let mid = layout_quick_actions(280.0);
        assert!(mid.has_overflow());
        assert!(mid.visible.len() < QuickAction::ALL.len());
        assert_eq!(
            mid.visible.len() + mid.overflow.len(),
            QuickAction::ALL.len()
        );
    }
}
