//! Conversation-first desktop experience.
//!
//! The conversation stays visible. Capabilities may expand a workspace from
//! the right. Closing the workspace never destroys the conversation.

use eframe::egui;

use crate::boot::Application;
use crate::diagnostics::{DiagnosticsSnapshot, OperationalStatus};
use crate::experience::ExperienceSession;
use jaymi_capabilities::{WorkspaceKind, WorkspacePanel};
use jaymi_core::UserRequest;
use jaymi_memory::MessageRole;

/// Launch the conversation-first desktop window.
pub fn run_diagnostics(
    app: Application,
    initial_list_path: String,
    initial_read_path: String,
    initial_snapshot: DiagnosticsSnapshot,
) -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 820.0])
            .with_title("Jaymi"),
        ..Default::default()
    };

    let experience = app.experience().unwrap_or_default();
    eframe::run_native(
        "Jaymi",
        options,
        Box::new(move |_cc| {
            Ok(Box::new(JaymiApp {
                app,
                snapshot: initial_snapshot,
                list_path_input: initial_list_path,
                read_path_input: initial_read_path,
                prompt: String::new(),
                experience,
                show_diagnostics: false,
                error: None,
            }))
        }),
    )
}

struct JaymiApp {
    app: Application,
    snapshot: DiagnosticsSnapshot,
    list_path_input: String,
    read_path_input: String,
    prompt: String,
    experience: ExperienceSession,
    show_diagnostics: bool,
    error: Option<String>,
}

fn status_color(status: OperationalStatus) -> egui::Color32 {
    match status {
        OperationalStatus::Operational => egui::Color32::from_rgb(56, 142, 60),
        OperationalStatus::Experimental => egui::Color32::from_rgb(194, 140, 0),
        OperationalStatus::Stub => egui::Color32::from_rgb(120, 120, 120),
        OperationalStatus::Disabled => egui::Color32::from_rgb(180, 60, 60),
    }
}

impl eframe::App for JaymiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Ok(session) = self.app.experience() {
            self.experience = session;
        }

        egui::TopBottomPanel::top("jaymi_top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Jaymi");
                ui.separator();
                ui.label("Conversation is permanent · workspaces expand from the right");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.checkbox(&mut self.show_diagnostics, "Diagnostics");
                    if self.experience.workspace_expanded() {
                        if ui.button("Close workspace").clicked() {
                            self.close_workspace();
                        }
                    }
                });
            });
        });

        if self.experience.workspace_expanded() {
            egui::SidePanel::right("jaymi_workspace")
                .default_width(420.0)
                .resizable(true)
                .show(ctx, |ui| {
                    self.render_workspace(ui);
                });
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            self.render_conversation(ui);
            if self.show_diagnostics {
                ui.add_space(12.0);
                ui.separator();
                self.render_diagnostics(ui);
            }
        });
    }
}

impl JaymiApp {
    fn render_conversation(&mut self, ui: &mut egui::Ui) {
        ui.heading("Conversation");
        ui.label("Capabilities may expand a workspace; closing it keeps this chat.");
        ui.add_space(8.0);

        egui::ScrollArea::vertical()
            .id_salt("conversation_scroll")
            .max_height(420.0)
            .stick_to_bottom(true)
            .show(ui, |ui| {
                if self.experience.conversation().is_empty() {
                    ui.weak("Say something — for example: Help me build an app.");
                }
                for turn in self.experience.conversation() {
                    let label = match turn.role {
                        MessageRole::User => "You",
                        MessageRole::Assistant => "Jaymi",
                        MessageRole::System => "System",
                    };
                    ui.group(|ui| {
                        ui.strong(label);
                        ui.label(&turn.content);
                    });
                    ui.add_space(6.0);
                }
            });

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.prompt)
                    .desired_width(520.0)
                    .hint_text("Ask Jaymi…"),
            );
            if ui.button("Send").clicked() {
                self.send_prompt();
            }
        });

        if let Some(error) = &self.error {
            ui.colored_label(egui::Color32::from_rgb(200, 80, 80), error);
        }

        if let Some(workspace) = self.experience.active_workspace() {
            ui.add_space(8.0);
            ui.label(format!(
                "Expanded workspace: {} ({})",
                workspace.title(),
                workspace.summary()
            ));
        } else {
            ui.add_space(8.0);
            ui.weak("No workspace expanded — conversation only.");
        }
    }

    fn render_workspace(&mut self, ui: &mut egui::Ui) {
        let Some(workspace) = self.experience.active_workspace().cloned() else {
            return;
        };
        ui.heading(workspace.title());
        ui.label(format!(
            "Requested by capability · {}",
            workspace.capability.id()
        ));
        ui.label(format!("Expands from: {}", workspace.expands_from.as_str()));
        ui.add_space(8.0);
        ui.separator();
        ui.strong("Panels");
        for panel in &workspace.panels {
            ui.horizontal(|ui| {
                ui.label("•");
                ui.label(panel.label());
                match workspace.kind {
                    WorkspaceKind::Coding => match panel {
                        WorkspacePanel::Editor => ui.weak("source editing surface"),
                        WorkspacePanel::Terminal => ui.weak("command surface"),
                        _ => ui.weak(panel.id()),
                    },
                    WorkspaceKind::Creation => match panel {
                        WorkspacePanel::Canvas => ui.weak("visual canvas"),
                        _ => ui.weak(panel.id()),
                    },
                    WorkspaceKind::Research => match panel {
                        WorkspacePanel::Citations => ui.weak("source-backed notes"),
                        _ => ui.weak(panel.id()),
                    },
                    WorkspaceKind::Conversation => ui.weak(""),
                };
            });
        }
        ui.add_space(12.0);
        if ui.button("Close workspace").clicked() {
            self.close_workspace();
        }
        ui.weak("Closing returns to conversation without losing chat history.");
    }

    fn send_prompt(&mut self) {
        let prompt = self.prompt.trim().to_string();
        if prompt.is_empty() {
            return;
        }
        self.prompt.clear();
        match self
            .app
            .handle_with_workspace(UserRequest::new(prompt))
        {
            Ok(_) => {
                self.error = None;
                if let Ok(session) = self.app.experience() {
                    self.experience = session;
                }
            }
            Err(error) => self.error = Some(error.message().to_string()),
        }
    }

    fn close_workspace(&mut self) {
        match self.app.close_ui_workspace() {
            Ok(_) => {
                self.error = None;
                if let Ok(session) = self.app.experience() {
                    self.experience = session;
                }
            }
            Err(error) => self.error = Some(error.message().to_string()),
        }
    }

    fn render_diagnostics(&mut self, ui: &mut egui::Ui) {
        ui.heading("Diagnostics");
        ui.label(format!("App state: {}", self.snapshot.app_state.label()));
        egui::Grid::new("subsystem_statuses")
            .striped(true)
            .num_columns(3)
            .spacing([16.0, 6.0])
            .min_col_width(120.0)
            .show(ui, |ui| {
                ui.strong("Subsystem");
                ui.strong("Status");
                ui.strong("Detail");
                ui.end_row();
                for row in &self.snapshot.subsystems {
                    ui.label(&row.name);
                    ui.colored_label(status_color(row.status), row.status.label());
                    ui.label(&row.detail);
                    ui.end_row();
                }
            });

        if let Some(inspector) = &self.snapshot.capability_inspector {
            ui.add_space(12.0);
            ui.heading("Capability Inspector");
            ui.label(inspector.summary());
            egui::Grid::new("capability_inspector")
                .striped(true)
                .num_columns(5)
                .spacing([12.0, 4.0])
                .min_col_width(80.0)
                .show(ui, |ui| {
                    ui.strong("Capability");
                    ui.strong("Availability");
                    ui.strong("Workspace");
                    ui.strong("Required tools");
                    ui.strong("Required providers");
                    ui.end_row();
                    for entry in &inspector.entries {
                        // Show the full registered catalog, including Planned.
                        if !entry.registered {
                            continue;
                        }
                        ui.label(&entry.id);
                        ui.label(entry.availability.as_str());
                        ui.label(
                            entry
                                .workspace
                                .map(|kind| kind.id())
                                .unwrap_or("conversation"),
                        );
                        ui.label(if entry.required_tools.is_empty() {
                            "-".to_string()
                        } else {
                            entry.required_tools.join(", ")
                        });
                        ui.label(if entry.required_providers.is_empty() {
                            "-".to_string()
                        } else {
                            entry.required_providers.join(", ")
                        });
                        ui.end_row();
                    }
                });
        }

        ui.add_space(12.0);
        ui.heading("List Directory");
        ui.horizontal(|ui| {
            ui.label("Path:");
            ui.add(
                egui::TextEdit::singleline(&mut self.list_path_input)
                    .desired_width(360.0)
                    .hint_text("/path/to/directory"),
            );
            if ui.button("List").clicked() {
                self.run_listing();
            }
        });
        if let Some(summary) = &self.snapshot.listing_summary {
            ui.label(summary);
        }

        ui.add_space(8.0);
        ui.heading("Read File");
        ui.horizontal(|ui| {
            ui.label("Path:");
            ui.add(
                egui::TextEdit::singleline(&mut self.read_path_input)
                    .desired_width(360.0)
                    .hint_text("/path/to/file.md"),
            );
            if ui.button("Read").clicked() {
                self.run_read();
            }
        });
    }

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
