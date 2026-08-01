//! Temporary diagnostics desktop UI for Milestone 1.
//!
//! Displays runtime health only. Chat is intentionally out of scope.

use eframe::egui;

use crate::diagnostics::DiagnosticsSnapshot;

/// Launch the diagnostics window and block until it closes.
pub fn run_diagnostics(snapshot: DiagnosticsSnapshot) -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([420.0, 320.0])
            .with_title("Jaymi"),
        ..Default::default()
    };

    eframe::run_native(
        "Jaymi",
        options,
        Box::new(|_cc| Ok(Box::new(DiagnosticsApp { snapshot }))),
    )
}

struct DiagnosticsApp {
    snapshot: DiagnosticsSnapshot,
}

impl eframe::App for DiagnosticsApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(24.0);
            ui.vertical_centered(|ui| {
                ui.heading("Jaymi");
                ui.add_space(16.0);
                ui.label(format!("Status: {}", self.snapshot.app_state.label()));
                ui.label(format!("Planner: {}", self.snapshot.planner_label()));
                ui.label(format!("Providers: {}", self.snapshot.provider_count));
                ui.label(format!("Tools: {}", self.snapshot.tool_count));
                ui.label(format!("Capabilities: {}", self.snapshot.capability_count));
                ui.label(format!("Database: {}", self.snapshot.database_label()));
            });
        });
    }
}
