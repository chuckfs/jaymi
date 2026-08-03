//! Conversation-first desktop experience.
//!
//! The conversation stays visible. Capabilities may expand a workspace from
//! the right. Closing the workspace never destroys the conversation.

use eframe::egui;

use crate::boot::Application;
use crate::coding_workspace::{render_coding_shell, CodingShellEvent, MonacoEditorSurface};
use crate::diagnostics::{DiagnosticsSnapshot, OperationalStatus};
use crate::experience::ExperienceSession;
use crate::monaco_host::{
    language_for_path, resolve_monaco_assets, MonacoDocument, MonacoHost, MonacoIpcMessage,
};
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
                monaco: None,
                monaco_minimap: true,
                monaco_last_error: None,
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
    /// Child WebView hosting Monaco (rehydrated from CodingState on Ready).
    monaco: Option<MonacoHost>,
    /// Minimap preference for the Monaco overlay.
    monaco_minimap: bool,
    /// Last Monaco host error (assets / webview create).
    monaco_last_error: Option<String>,
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
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if let Ok(session) = self.app.experience() {
            self.experience = session;
        }

        let coding_open = self.experience.active_workspace_kind() == Some(WorkspaceKind::Coding);
        let mut monaco_surface: Option<MonacoEditorSurface> = None;

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
                Some(WorkspaceKind::Coding) => 560.0,
                _ => 420.0,
            };
            egui::SidePanel::right("jaymi_workspace")
                .default_width(width)
                .resizable(true)
                .show(ctx, |ui| {
                    monaco_surface = self.render_workspace(ui);
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

        self.sync_monaco(ctx, frame, coding_open, monaco_surface.as_ref());
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
                egui::RichText::new("HI, I'm Jaymi")
                    .size(42.0)
                    .strong()
                    .color(ui.visuals().strong_text_color()),
            );
            ui.add_space(10.0);
            ui.label(
                egui::RichText::new("Ask anything, or open ⋯ → Open Project… / Start Coding Project.")
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
        enum ConversationAction {
            OpenProjectFolder,
            OpenProjectId(String),
            StartCoding,
            StartResearch,
            StartCreation,
            Settings,
        }

        let known_projects = self.app.list_projects().unwrap_or_default();
        let mut action = None;
        ui.menu_button("⋯", |ui| {
            ui.set_min_width(220.0);
            if ui.button("Open Project…").clicked() {
                action = Some(ConversationAction::OpenProjectFolder);
                ui.close_menu();
            }
            if !known_projects.is_empty() {
                ui.menu_button("Recent Projects", |ui| {
                    ui.set_min_width(220.0);
                    for project in &known_projects {
                        let label = match project.root_directory.as_ref() {
                            Some(root) => format!("{} — {}", project.name, root.display()),
                            None => project.name.clone(),
                        };
                        if ui.button(label).clicked() {
                            action =
                                Some(ConversationAction::OpenProjectId(project.id.to_string()));
                            ui.close_menu();
                        }
                    }
                });
            }
            ui.separator();
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
            Some(ConversationAction::OpenProjectFolder) => self.open_project_folder(),
            Some(ConversationAction::OpenProjectId(project_id)) => {
                self.open_project_by_id(&project_id)
            }
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

    fn open_project_folder(&mut self) {
        let picked = rfd::FileDialog::new()
            .set_title("Open Project")
            .pick_folder();
        let Some(path) = picked else {
            return;
        };
        match self.app.open_project_from_path(&path) {
            Ok(_) => {
                self.error = None;
                self.start_coding_project();
            }
            Err(error) => self.error = Some(error.message().to_string()),
        }
    }

    fn open_project_by_id(&mut self, project_id: &str) {
        match self.app.open_project(project_id) {
            Ok(_) => {
                self.error = None;
                self.start_coding_project();
            }
            Err(error) => self.error = Some(error.message().to_string()),
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

    fn handle_coding_events(&mut self, events: Vec<CodingShellEvent>) {
        for event in events {
            let result = match event {
                CodingShellEvent::OpenProject => {
                    self.open_project_folder();
                    Ok(())
                }
                CodingShellEvent::ToggleExpand(path) => self.app.toggle_coding_expand(&path),
                CodingShellEvent::SelectPath { path, is_dir } => {
                    self.app.select_coding_path(&path, is_dir)
                }
                CodingShellEvent::ActivateTab(path) => self.app.activate_coding_tab(&path),
                CodingShellEvent::CloseTab(path) => self.app.close_coding_tab(&path),
                CodingShellEvent::EditContent { path, content } => {
                    self.app.set_coding_tab_content(&path, content)
                }
                CodingShellEvent::Scroll { path, offset } => {
                    self.app.set_coding_tab_scroll(&path, offset)
                }
                CodingShellEvent::SaveActive => self.app.save_active_coding_file(),
                CodingShellEvent::SaveTab(path) => self.app.save_coding_file(&path),
                CodingShellEvent::SetMinimap(enabled) => {
                    self.monaco_minimap = enabled;
                    if let Some(host) = &self.monaco {
                        let _ = host.set_minimap(enabled);
                    }
                    Ok(())
                }
                CodingShellEvent::TerminalInput { session_id, input } => {
                    self.app.set_coding_terminal_input(&session_id, input)
                }
                CodingShellEvent::TerminalRun {
                    session_id,
                    command,
                } => self.app.run_coding_terminal_command(&session_id, &command),
                CodingShellEvent::TerminalHistory {
                    session_id,
                    direction,
                } => self
                    .app
                    .navigate_coding_terminal_history(&session_id, direction),
                CodingShellEvent::TerminalScroll { session_id, offset } => {
                    self.app.set_coding_terminal_scroll(&session_id, offset)
                }
                CodingShellEvent::GitRefresh => self.app.refresh_coding_git(),
                CodingShellEvent::GitStage { paths } => self.app.coding_git_stage(&paths),
                CodingShellEvent::GitUnstage { paths } => self.app.coding_git_unstage(&paths),
                CodingShellEvent::GitDiscard { paths } => self.app.coding_git_discard(&paths),
                CodingShellEvent::GitCommitMessage(message) => {
                    self.app.set_coding_git_commit_message(message)
                }
                CodingShellEvent::GitCommit => self.app.coding_git_commit_active(),
            };
            if let Err(error) = result {
                self.error = Some(error.message().to_string());
            }
        }
        if let Ok(session) = self.app.experience() {
            self.experience = session;
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

    fn render_workspace(&mut self, ui: &mut egui::Ui) -> Option<MonacoEditorSurface> {
        let Some(workspace) = self.experience.active_workspace().cloned() else {
            return None;
        };

        if workspace.kind == WorkspaceKind::Coding {
            let coding = self
                .experience
                .capability_state()
                .and_then(|state| state.coding())
                .cloned();
            let diagnostics = self.app.coding_diagnostics_view().ok();
            let mut events = Vec::new();
            let mut monaco_surface = None;
            render_coding_shell(
                ui,
                &workspace,
                coding.as_ref(),
                diagnostics.as_ref(),
                &mut events,
                self.monaco_minimap,
                &mut monaco_surface,
            );
            self.handle_coding_events(events);
            if let Some(error) = &self.monaco_last_error {
                ui.colored_label(egui::Color32::from_rgb(200, 80, 80), error);
            }
            ui.add_space(12.0);
            if ui.button("Close workspace").clicked() {
                self.close_workspace();
            }
            ui.weak("Closing returns to conversation without losing chat history.");
            return monaco_surface;
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
        None
    }

    fn sync_monaco(
        &mut self,
        ctx: &egui::Context,
        frame: &mut eframe::Frame,
        coding_open: bool,
        surface: Option<&MonacoEditorSurface>,
    ) {
        if !coding_open {
            if let Some(host) = &self.monaco {
                let _ = host.set_viewport(None, 0.0, 1.0);
            }
            self.monaco = None;
            return;
        }

        if surface.is_some() && self.monaco.is_none() {
            match MonacoHost::new(frame, resolve_monaco_assets()) {
                Ok(host) => {
                    self.monaco = Some(host);
                    self.monaco_last_error = None;
                }
                Err(error) => {
                    self.monaco_last_error = Some(error);
                }
            }
        }

        if self.monaco.is_none() {
            return;
        }

        // Keep egui pumping so Monaco IPC (edits / save) reaches CodingState.
        ctx.request_repaint();

        let messages = self
            .monaco
            .as_mut()
            .map(|host| host.poll())
            .unwrap_or_default();
        let mut events = Vec::new();
        let mut lsp_requests = Vec::new();
        for message in messages {
            match message {
                MonacoIpcMessage::Ready => {}
                MonacoIpcMessage::Change { path, content } => {
                    if let Some(host) = self.monaco.as_mut() {
                        host.note_external_edit(&path, &content);
                    }
                    events.push(CodingShellEvent::EditContent { path, content });
                }
                MonacoIpcMessage::Scroll { path, offset } => {
                    events.push(CodingShellEvent::Scroll { path, offset });
                }
                MonacoIpcMessage::Save { .. } => {
                    events.push(CodingShellEvent::SaveActive);
                }
                MonacoIpcMessage::Lsp {
                    id,
                    method,
                    path,
                    line,
                    character,
                    new_name,
                } => {
                    lsp_requests.push((id, method, path, line, character, new_name));
                }
            }
        }
        if !events.is_empty() {
            self.handle_coding_events(events);
        }
        for (id, method, path, line, character, new_name) in lsp_requests {
            let payload = self.handle_monaco_lsp(&method, &path, line, character, new_name.as_deref());
            if let Some(host) = &self.monaco {
                let _ = host.resolve_lsp(id, &payload);
            }
        }

        // Push CodingState diagnostics into Monaco markers for the active file.
        if let Some(path) = surface.map(|surface| surface.document.path.clone()) {
            let markers = self.monaco_diagnostic_markers(&path);
            if let Some(host) = &self.monaco {
                let _ = host.set_diagnostics(&markers);
            }
        }

        let screen_height = ctx.screen_rect().height();
        let zoom = ctx.pixels_per_point();
        let document = surface.map(|surface| self.monaco_document_from_state(surface));
        let Some(host) = self.monaco.as_mut() else {
            return;
        };
        if let (Some(surface), Some(document)) = (surface, document) {
            if let Err(error) = host.set_viewport(Some(surface.viewport), screen_height, zoom) {
                self.monaco_last_error = Some(error);
            } else if let Err(error) = host.sync_document(&document) {
                self.monaco_last_error = Some(error);
            }
        } else if let Err(error) = host.set_viewport(None, screen_height, zoom) {
            self.monaco_last_error = Some(error);
        }
    }

    fn handle_monaco_lsp(
        &mut self,
        method: &str,
        path: &str,
        line: u32,
        character: u32,
        new_name: Option<&str>,
    ) -> String {
        let result = match method {
            "hover" => self.app.coding_lsp_hover(path, line, character),
            "completion" => self.app.coding_lsp_completion(path, line, character),
            "definition" => self.app.coding_lsp_definition(path, line, character),
            "references" => self.app.coding_lsp_references(path, line, character),
            "rename" => self
                .app
                .coding_lsp_rename(path, line, character, new_name.unwrap_or("")),
            _ => return "null".to_string(),
        };
        match result {
            Ok(response) => match method {
                "hover" => match response.lsp_hover {
                    Some(hover) => serde_json::json!({
                        "contents": hover.contents,
                        "range": hover.range.map(|range| serde_json::json!({
                            "start": { "line": range.start.line, "character": range.start.character },
                            "end": { "line": range.end.line, "character": range.end.character },
                        })),
                    })
                    .to_string(),
                    None => "null".to_string(),
                },
                "completion" => serde_json::json!({
                    "items": response.lsp_completions.iter().map(|item| serde_json::json!({
                        "label": item.label,
                        "detail": item.detail,
                        "insertText": item.insert_text,
                    })).collect::<Vec<_>>(),
                })
                .to_string(),
                "definition" | "references" => {
                    let locations = if method == "definition" {
                        &response.lsp_definitions
                    } else {
                        &response.lsp_references
                    };
                    serde_json::json!({
                        "locations": locations.iter().map(|loc| serde_json::json!({
                            "path": loc.path,
                            "range": {
                                "start": { "line": loc.range.start.line, "character": loc.range.start.character },
                                "end": { "line": loc.range.end.line, "character": loc.range.end.character },
                            },
                        })).collect::<Vec<_>>(),
                    })
                    .to_string()
                }
                "rename" => serde_json::json!({
                    "edits": response.lsp_edits.iter().map(|edit| serde_json::json!({
                        "path": edit.path,
                        "newText": edit.new_text,
                        "range": {
                            "start": { "line": edit.range.start.line, "character": edit.range.start.character },
                            "end": { "line": edit.range.end.line, "character": edit.range.end.character },
                        },
                    })).collect::<Vec<_>>(),
                })
                .to_string(),
                _ => "null".to_string(),
            },
            Err(error) => {
                self.monaco_last_error = Some(error.message().to_string());
                "null".to_string()
            }
        }
    }

    fn monaco_diagnostic_markers(&self, path: &str) -> String {
        let markers = self
            .experience
            .capability_state()
            .and_then(|state| state.coding())
            .map(|coding| {
                coding
                    .diagnostics
                    .iter()
                    .filter(|diag| diag.path.as_deref() == Some(path))
                    .map(|diag| {
                        serde_json::json!({
                            "message": diag.message,
                            "severity": diag.severity,
                            "line": diag.line.unwrap_or(0),
                            "character": diag.character.unwrap_or(0),
                            "endLine": diag.end_line.unwrap_or(diag.line.unwrap_or(0)),
                            "endCharacter": diag.end_character.unwrap_or(diag.character.unwrap_or(0) + 1),
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        serde_json::to_string(&markers).unwrap_or_else(|_| "[]".to_string())
    }

    /// Prefer live CodingState so Monaco IPC edits aren't overwritten by a stale surface.
    fn monaco_document_from_state(&self, surface: &MonacoEditorSurface) -> MonacoDocument {
        let Some(tab) = self
            .experience
            .capability_state()
            .and_then(|state| state.coding())
            .and_then(|coding| {
                coding.open_tabs.iter().find(|tab| {
                    Some(tab.path.as_str()) == coding.active_tab_path.as_deref()
                })
            })
        else {
            return surface.document.clone();
        };

        MonacoDocument {
            path: tab.path.clone(),
            content: tab.content.clone(),
            language: language_for_path(&tab.path).to_string(),
            scroll_top: tab.scroll_offset,
            minimap: self.monaco_minimap,
        }
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
        if let Some(host) = &self.monaco {
            let _ = host.set_viewport(None, 0.0, 1.0);
        }
        self.monaco = None;
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
