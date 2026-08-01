//! Temporary diagnostics desktop UI.
//!
//! Displays runtime health and directory listing results from the Planner
//! pipeline. Chat is intentionally out of scope.

use eframe::egui;

use crate::boot::Application;
use crate::diagnostics::DiagnosticsSnapshot;

/// Launch the diagnostics window with an interactive directory listing control.
pub fn run_diagnostics(
    app: Application,
    initial_path: String,
    initial_snapshot: DiagnosticsSnapshot,
) -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([720.0, 560.0])
            .with_title("Jaymi"),
        ..Default::default()
    };

    eframe::run_native(
        "Jaymi",
        options,
        Box::new(move |_cc| {
            Ok(Box::new(DiagnosticsApp {
                app,
                snapshot: initial_snapshot,
                path_input: initial_path,
                error: None,
            }))
        }),
    )
}

struct DiagnosticsApp {
    app: Application,
    snapshot: DiagnosticsSnapshot,
    path_input: String,
    error: Option<String>,
}

impl eframe::App for DiagnosticsApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(12.0);
            ui.vertical_centered(|ui| {
                ui.heading("Jaymi");
                ui.add_space(8.0);
                ui.label(format!("Status: {}", self.snapshot.app_state.label()));
                ui.label(format!("Planner: {}", self.snapshot.planner_label()));
                ui.label(format!("Providers: {}", self.snapshot.provider_count));
                ui.label(format!("Tools: {}", self.snapshot.tool_count));
                ui.label(format!("Capabilities: {}", self.snapshot.capability_count));
                ui.label(format!("Database: {}", self.snapshot.database_label()));
            });

            ui.add_space(16.0);
            ui.separator();
            ui.heading("List Directory");
            ui.horizontal(|ui| {
                ui.label("Path:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.path_input)
                        .desired_width(420.0)
                        .hint_text("/path/to/directory"),
                );
                if ui.button("List").clicked() {
                    self.run_listing();
                }
            });

            if let Some(error) = &self.error {
                ui.colored_label(egui::Color32::from_rgb(200, 80, 80), error);
            }

            if let Some(summary) = &self.snapshot.listing_summary {
                ui.add_space(8.0);
                ui.label(summary);
            }

            ui.add_space(8.0);
            egui::ScrollArea::vertical().show(ui, |ui| {
                egui::Grid::new("listing_grid")
                    .striped(true)
                    .num_columns(5)
                    .spacing([12.0, 4.0])
                    .show(ui, |ui| {
                        ui.strong("Name");
                        ui.strong("Type");
                        ui.strong("Path");
                        ui.strong("Size");
                        ui.strong("Modified");
                        ui.end_row();

                        for entry in &self.snapshot.entries {
                            ui.label(&entry.name);
                            ui.label(entry.entry_type.label());
                            ui.label(entry.path.display().to_string());
                            ui.label(entry.size.to_string());
                            ui.label(
                                entry
                                    .modified
                                    .map(|value| value.to_string())
                                    .unwrap_or_else(|| "-".to_string()),
                            );
                            ui.end_row();
                        }
                    });
            });
        });
    }
}

impl DiagnosticsApp {
    fn run_listing(&mut self) {
        match self.app.list_directory(self.path_input.trim()) {
            Ok(response) => {
                self.error = None;
                match self.app.diagnostics_with_listing(Some(response)) {
                    Ok(snapshot) => self.snapshot = snapshot,
                    Err(error) => self.error = Some(error.message().to_string()),
                }
            }
            Err(error) => {
                self.error = Some(error.message().to_string());
            }
        }
    }
}
