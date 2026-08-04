//! Quick Open — fuzzy filename jump (⌘P).
//!
//! Mirrors the Command Palette modal shell but queries files through
//! [`crate::boot::Application::project_search`] (Planner → Search Engine)
//! instead of the command registry. Selecting a result opens the file in
//! Monaco without seeking to a specific location.

use eframe::egui;

use jaymi_capabilities::SearchResultEntry;

/// Quick Open modal state owned by the desktop UI.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct QuickOpenState {
    open: bool,
    query: String,
    selected: usize,
    results: Vec<SearchResultEntry>,
}

impl QuickOpenState {
    /// Whether the Quick Open overlay should capture input.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Open the modal with an empty query.
    pub fn open(&mut self) {
        self.open = true;
        self.query.clear();
        self.selected = 0;
        self.results.clear();
    }

    /// Close the modal.
    pub fn close(&mut self) {
        self.open = false;
    }

    /// Current filter query.
    pub fn query(&self) -> &str {
        &self.query
    }

    /// Replace the result list from a fresh Quick Open search.
    pub fn set_results(&mut self, results: Vec<SearchResultEntry>) {
        self.results = results;
        self.selected = self.selected.min(self.results.len().saturating_sub(1));
    }
}

/// Result of interacting with Quick Open for one frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuickOpenOutcome {
    /// No action.
    None,
    /// The query text changed — caller should re-run the search.
    QueryChanged(String),
    /// The user picked a file to open.
    Open(String),
}

/// Render the Quick Open overlay and return any query change / open request.
pub fn render_quick_open(
    ctx: &egui::Context,
    state: &mut QuickOpenState,
    overlay_scrim: egui::Color32,
) -> QuickOpenOutcome {
    if !state.open {
        return QuickOpenOutcome::None;
    }

    let mut outcome = QuickOpenOutcome::None;
    let mut dismiss = false;

    let screen = ctx.screen_rect();
    egui::Area::new(egui::Id::new("jaymi_quick_open_backdrop"))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            let response = ui.allocate_response(screen.size(), egui::Sense::click());
            ui.painter().rect_filled(screen, 0.0, overlay_scrim);
            if response.clicked() {
                dismiss = true;
            }
        });

    let panel_width = (screen.width() * 0.55).clamp(420.0, 640.0);
    let panel_height = 360.0_f32;

    egui::Window::new("Quick Open")
        .id(egui::Id::new("jaymi_quick_open"))
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 72.0))
        .fixed_size(egui::vec2(panel_width, panel_height))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                dismiss = true;
            }

            ui.horizontal(|ui| {
                ui.weak("Go to file");
                let response = ui.add(
                    egui::TextEdit::singleline(&mut state.query)
                        .hint_text("Type a filename…")
                        .desired_width(panel_width - 80.0),
                );
                response.request_focus();
                if response.changed() {
                    state.selected = 0;
                    outcome = QuickOpenOutcome::QueryChanged(state.query.clone());
                }
            });
            ui.add_space(6.0);
            ui.separator();

            if state.results.is_empty() {
                ui.weak(if state.query.trim().is_empty() {
                    "Type to search project files"
                } else {
                    "No matching files"
                });
            } else {
                state.selected = state.selected.min(state.results.len().saturating_sub(1));
                egui::ScrollArea::vertical()
                    .max_height(panel_height - 64.0)
                    .show(ui, |ui| {
                        for (index, result) in state.results.iter().enumerate() {
                            let selected_row = index == state.selected;
                            let label = format!("{}  ·  {}", result.title, result.path);
                            let mut text = egui::RichText::new(label);
                            if selected_row {
                                text = text.strong();
                            }
                            if ui.selectable_label(selected_row, text).clicked() {
                                outcome = QuickOpenOutcome::Open(result.path.clone());
                            }
                        }
                    });

                let enter = ui.input(|input| input.key_pressed(egui::Key::Enter));
                let up = ui.input(|input| input.key_pressed(egui::Key::ArrowUp));
                let down = ui.input(|input| input.key_pressed(egui::Key::ArrowDown));
                if up {
                    state.selected = state.selected.saturating_sub(1);
                }
                if down {
                    state.selected =
                        (state.selected + 1).min(state.results.len().saturating_sub(1));
                }
                if enter {
                    if let Some(result) = state.results.get(state.selected) {
                        outcome = QuickOpenOutcome::Open(result.path.clone());
                    }
                }
            }
        });

    if matches!(outcome, QuickOpenOutcome::Open(_)) || dismiss {
        state.close();
    }
    outcome
}
