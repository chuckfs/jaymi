//! Temporary diagnostics desktop UI.
//!
//! Displays runtime health, directory listing results, and unified document
//! reads from the Planner pipeline. Chat is intentionally out of scope.

use eframe::egui;

use crate::boot::Application;
use crate::diagnostics::DiagnosticsSnapshot;

/// Launch the diagnostics window with listing and read controls.
pub fn run_diagnostics(
    app: Application,
    initial_list_path: String,
    initial_read_path: String,
    initial_snapshot: DiagnosticsSnapshot,
) -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([820.0, 720.0])
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
                list_path_input: initial_list_path,
                read_path_input: initial_read_path,
                error: None,
            }))
        }),
    )
}

struct DiagnosticsApp {
    app: Application,
    snapshot: DiagnosticsSnapshot,
    list_path_input: String,
    read_path_input: String,
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

            ui.add_space(12.0);
            ui.separator();
            ui.heading("List Directory");
            ui.horizontal(|ui| {
                ui.label("Path:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.list_path_input)
                        .desired_width(420.0)
                        .hint_text("/path/to/directory"),
                );
                if ui.button("List").clicked() {
                    self.run_listing();
                }
            });

            if let Some(summary) = &self.snapshot.listing_summary {
                ui.label(summary);
            }

            if !self.snapshot.entries.is_empty() {
                egui::ScrollArea::vertical()
                    .id_salt("listing_scroll")
                    .max_height(140.0)
                    .show(ui, |ui| {
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
            }

            ui.add_space(12.0);
            ui.separator();
            ui.heading("Read File");
            ui.horizontal(|ui| {
                ui.label("Path:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.read_path_input)
                        .desired_width(420.0)
                        .hint_text("/path/to/file.md"),
                );
                if ui.button("Read").clicked() {
                    self.run_read();
                }
            });

            if let Some(error) = &self.error {
                ui.colored_label(egui::Color32::from_rgb(200, 80, 80), error);
            }

            if let Some(summary) = &self.snapshot.read_summary {
                ui.label(summary);
            }

            ui.label(format!(
                "Read file path: {}",
                self.snapshot
                    .read_path
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "-".to_string())
            ));
            ui.label(format!(
                "File type: {}",
                self.snapshot
                    .read_file_type
                    .clone()
                    .unwrap_or_else(|| "-".to_string())
            ));
            ui.label(format!(
                "Parser selected: {}",
                self.snapshot
                    .read_parser
                    .clone()
                    .unwrap_or_else(|| "-".to_string())
            ));
            ui.label(format!(
                "Parsed successfully: {}",
                self.snapshot.read_success_label()
            ));
            ui.label(format!(
                "Character count: {}",
                self.snapshot
                    .read_character_count
                    .map(|count| count.to_string())
                    .unwrap_or_else(|| "-".to_string())
            ));

            ui.add_space(8.0);
            ui.label("Parsed text:");
            egui::ScrollArea::vertical()
                .id_salt("read_text_scroll")
                .max_height(220.0)
                .show(ui, |ui| {
                    let text = self.snapshot.read_text.as_deref().unwrap_or("(no document loaded)");
                    ui.monospace(text);
                });
        });
    }
}

impl DiagnosticsApp {
    fn run_listing(&mut self) {
        match self.app.list_directory(self.list_path_input.trim()) {
            Ok(response) => {
                self.error = None;
                match self.app.diagnostics_from_response(Some(response)) {
                    Ok(snapshot) => self.snapshot = snapshot,
                    Err(error) => self.error = Some(error.message().to_string()),
                }
            }
            Err(error) => self.error = Some(error.message().to_string()),
        }
    }

    fn run_read(&mut self) {
        match self.app.read_file(self.read_path_input.trim()) {
            Ok(response) => {
                self.error = None;
                match self.app.diagnostics_from_response(Some(response)) {
                    Ok(snapshot) => self.snapshot = snapshot,
                    Err(error) => self.error = Some(error.message().to_string()),
                }
            }
            Err(error) => self.error = Some(error.message().to_string()),
        }
    }
}
