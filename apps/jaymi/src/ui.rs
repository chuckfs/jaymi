//! Conversation-first desktop experience.
//!
//! The conversation stays visible. Capabilities may expand a workspace from
//! the right. Closing the workspace never destroys the conversation.

use eframe::egui;

use crate::boot::Application;
use crate::coding_workspace::render_coding_shell;
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
            let width = match self.experience.active_workspace_kind() {
                Some(WorkspaceKind::Coding) => 480.0,
                _ => 420.0,
            };
            egui::SidePanel::right("jaymi_workspace")
                .default_width(width)
                .resizable(true)
                .show(ctx, |ui| {
                    self.render_workspace(ui);
                });
        }

        // Composer stays pinned to the bottom of the conversation column.
        egui::TopBottomPanel::bottom("chat_composer")
            .show_separator_line(false)
            .frame(
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(16, 12))
                    .fill(ctx.style().visuals.panel_fill),
            )
            .show(ctx, |ui| {
                self.render_chat_composer(ui);
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            self.render_chat(ui);
            if self.show_diagnostics {
                ui.add_space(12.0);
                ui.separator();
                self.render_diagnostics(ui);
            }
        });
    }
}

impl JaymiApp {
    fn render_chat(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Conversation");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                self.render_conversation_actions(ui);
            });
        });
        ui.add_space(4.0);

        let available = ui.available_height();
        egui::ScrollArea::vertical()
            .id_salt("conversation_scroll")
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .max_height(available)
            .show(ui, |ui| {
                ui.set_min_height(available.max(240.0));
                if self.experience.conversation().is_empty() {
                    self.render_welcome(ui);
                } else {
                    ui.add_space(8.0);
                    for turn in self.experience.conversation() {
                        self.render_chat_bubble(ui, turn.role, &turn.content);
                        ui.add_space(10.0);
                    }
                }
            });
    }

    fn render_welcome(&self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            let height = ui.available_height().max(200.0);
            ui.add_space((height * 0.32).clamp(48.0, 180.0));
            ui.label(
                egui::RichText::new("Hi! Its Jaymi")
                    .size(42.0)
                    .strong()
                    .color(ui.visuals().strong_text_color()),
            );
            ui.add_space(10.0);
            ui.label(
                egui::RichText::new("Ask anything, or open ⋯ → Start Coding Project.")
                    .size(15.0)
                    .color(ui.visuals().weak_text_color()),
            );
        });
    }

    fn render_chat_bubble(&self, ui: &mut egui::Ui, role: MessageRole, content: &str) {
        let is_user = matches!(role, MessageRole::User);
        let (label, fill, text_color, meta_color) = match role {
            MessageRole::User => (
                "You",
                egui::Color32::from_rgb(36, 99, 235),
                egui::Color32::WHITE,
                egui::Color32::from_rgb(200, 220, 255),
            ),
            MessageRole::Assistant => (
                "Jaymi",
                ui.visuals().faint_bg_color,
                ui.visuals().text_color(),
                ui.visuals().weak_text_color(),
            ),
            MessageRole::System => (
                "System",
                ui.visuals().faint_bg_color,
                ui.visuals().text_color(),
                ui.visuals().weak_text_color(),
            ),
        };

        let max_bubble = ui.available_width() * 0.78;
        ui.horizontal(|ui| {
            if is_user {
                ui.add_space((ui.available_width() - max_bubble).max(0.0));
            }
            egui::Frame::new()
                .corner_radius(14.0)
                .inner_margin(egui::Margin::symmetric(14, 10))
                .fill(fill)
                .show(ui, |ui| {
                    ui.set_max_width(max_bubble);
                    ui.label(egui::RichText::new(label).small().strong().color(meta_color));
                    ui.label(egui::RichText::new(content).color(text_color));
                });
        });
    }

    fn render_chat_composer(&mut self, ui: &mut egui::Ui) {
        if let Some(error) = &self.error {
            ui.colored_label(egui::Color32::from_rgb(200, 80, 80), error);
            ui.add_space(6.0);
        }

        let send_clicked = egui::Frame::new()
            .corner_radius(24.0)
            .inner_margin(egui::Margin::symmetric(14, 8))
            .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
            .fill(ui.visuals().extreme_bg_color)
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.prompt)
                            .desired_width(ui.available_width() - 48.0)
                            .hint_text("Message Jaymi…")
                            .frame(false),
                    );
                    let enter_send = response.lost_focus()
                        && ui.input(|input| input.key_pressed(egui::Key::Enter));

                    let send = ui
                        .add_sized(
                            [36.0, 36.0],
                            egui::Button::new(egui::RichText::new("↑").size(20.0).strong())
                                .corner_radius(18.0)
                                .fill(egui::Color32::from_rgb(36, 99, 235)),
                        )
                        .on_hover_text("Send");

                    if enter_send {
                        response.request_focus();
                    }
                    send.clicked() || enter_send
                })
                .inner
            })
            .inner;

        if send_clicked {
            self.send_prompt();
        }
    }

    /// Three-dot action menu in the conversation header (discoverable activation).
    fn render_conversation_actions(&mut self, ui: &mut egui::Ui) {
        #[derive(Clone, Copy)]
        enum ConversationAction {
            StartCoding,
            StartResearch,
            StartCreation,
            Settings,
        }

        let mut action = None;
        ui.menu_button("⋯", |ui| {
            ui.set_min_width(180.0);
            if ui.button("Start Coding Project").clicked() {
                action = Some(ConversationAction::StartCoding);
                ui.close_menu();
            }
            if ui.button("Start Research").clicked() {
                action = Some(ConversationAction::StartResearch);
                ui.close_menu();
            }
            if ui.button("Start Creation").clicked() {
                action = Some(ConversationAction::StartCreation);
                ui.close_menu();
            }
            ui.separator();
            if ui.button("Settings").clicked() {
                action = Some(ConversationAction::Settings);
                ui.close_menu();
            }
        })
        .response
        .on_hover_text("Conversation actions");

        match action {
            Some(ConversationAction::StartCoding) => self.start_coding_project(),
            Some(ConversationAction::StartResearch) => self.start_research_workspace(),
            Some(ConversationAction::StartCreation) => self.start_creation_workspace(),
            Some(ConversationAction::Settings) => {
                self.show_diagnostics = true;
                self.error = None;
            }
            None => {}
        }
    }

    fn start_coding_project(&mut self) {
        match self.app.start_coding_project() {
            Ok(()) => {
                self.error = None;
                if let Ok(session) = self.app.experience() {
                    self.experience = session;
                }
            }
            Err(error) => self.error = Some(error.message().to_string()),
        }
    }

    fn start_research_workspace(&mut self) {
        match self.app.start_research_workspace() {
            Ok(()) => {
                self.error = None;
                if let Ok(session) = self.app.experience() {
                    self.experience = session;
                }
            }
            Err(error) => self.error = Some(error.message().to_string()),
        }
    }

    fn start_creation_workspace(&mut self) {
        match self.app.start_creation_workspace() {
            Ok(()) => {
                self.error = None;
                if let Ok(session) = self.app.experience() {
                    self.experience = session;
                }
            }
            Err(error) => self.error = Some(error.message().to_string()),
        }
    }

    fn render_workspace(&mut self, ui: &mut egui::Ui) {
        let Some(workspace) = self.experience.active_workspace().cloned() else {
            return;
        };

        if workspace.kind == WorkspaceKind::Coding {
            let coding = self
                .experience
                .capability_state()
                .and_then(|state| state.coding())
                .cloned();
            render_coding_shell(ui, &workspace, coding.as_ref());
            ui.add_space(12.0);
            if ui.button("Close workspace").clicked() {
                self.close_workspace();
            }
            ui.weak("Closing returns to conversation without losing chat history.");
            return;
        }

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
                    WorkspaceKind::Creation => match panel {
                        WorkspacePanel::Canvas => ui.weak("visual canvas"),
                        _ => ui.weak(panel.id()),
                    },
                    WorkspaceKind::Research => match panel {
                        WorkspacePanel::Citations => ui.weak("source-backed notes"),
                        _ => ui.weak(panel.id()),
                    },
                    WorkspaceKind::Conversation | WorkspaceKind::Coding => ui.weak(""),
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
