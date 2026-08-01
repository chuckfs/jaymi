//! Conversation-first desktop UI for Jaymi.
//!
//! Layout (left → right growth reserved for future workspaces):
//!
//! ```text
//! ┌────────────────────────────────┬──────────┐
//! │ Header                         │          │
//! ├────────────────────────────────┤ (future) │
//! │ Conversation                   │ workspace│
//! ├────────────────────────────────┤          │
//! │ Composer  [⋯]                  │          │
//! └────────────────────────────────┴──────────┘
//! ```

mod action_menu;
mod conversation_view;
mod header;
mod message_bubble;
mod message_composer;
mod settings;
mod theme;

use eframe::egui;

use crate::boot::Application;
use crate::conversation::Conversation;

use action_menu::{ActionMenu, MenuAction};
use conversation_view::ConversationView;
use header::Header;
use message_composer::{ComposerState, MessageComposer};
use settings::SettingsPanel;
use theme::ConversationTheme;

/// Launch Jaymi's permanent conversation shell.
pub fn run_conversation(app: Application) -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([960.0, 720.0])
            .with_min_inner_size([720.0, 520.0])
            .with_title("Jaymi"),
        ..Default::default()
    };

    eframe::run_native(
        "Jaymi",
        options,
        Box::new(move |cc| {
            ConversationTheme::apply(&cc.egui_ctx);
            Ok(Box::new(ConversationApp::new(app)))
        }),
    )
}

struct ConversationApp {
    app: Application,
    conversation: Conversation,
    composer: String,
    composer_state: ComposerState,
    menu_open: bool,
    settings_open: bool,
    status_notice: Option<String>,
    scroll_to_bottom: bool,
    /// Reserved right-rail width for future workspace expansion (currently collapsed).
    workspace_rail_width: f32,
}

impl ConversationApp {
    fn new(app: Application) -> Self {
        Self {
            app,
            conversation: Conversation::with_welcome(),
            composer: String::new(),
            composer_state: ComposerState::Idle,
            menu_open: false,
            settings_open: false,
            status_notice: None,
            scroll_to_bottom: true,
            workspace_rail_width: 0.0,
        }
    }

    fn submit_message(&mut self) {
        let text = self.composer.trim().to_string();
        if text.is_empty() || self.composer_state == ComposerState::Disabled {
            return;
        }

        self.composer.clear();
        self.status_notice = None;
        self.conversation.push_user(text.clone());
        self.composer_state = ComposerState::Sending;
        self.scroll_to_bottom = true;

        match self.app.send_message(&text) {
            Ok(response) => {
                self.conversation
                    .push_assistant(response.assistant_text());
                self.composer_state = ComposerState::Idle;
            }
            Err(error) => {
                self.conversation.push_assistant(format!(
                    "I ran into a problem while handling that:\n{}",
                    error.message()
                ));
                self.composer_state = ComposerState::Idle;
            }
        }
        self.scroll_to_bottom = true;
    }

    fn handle_menu_action(&mut self, action: MenuAction) {
        self.menu_open = false;
        match action {
            MenuAction::Settings => {
                self.settings_open = true;
            }
            MenuAction::StartCodingProject
            | MenuAction::StartCreationWorkspace
            | MenuAction::StartResearchWorkspace
            | MenuAction::SaveConversation => {
                self.status_notice = Some("Coming in a future slice.".to_string());
                self.conversation
                    .push_system("Coming in a future slice.");
                self.scroll_to_bottom = true;
            }
        }
    }
}

impl eframe::App for ConversationApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ConversationTheme::apply(ctx);

        // Future workspace rail — currently collapsed (width 0) so conversation
        // remains the full experience while layout is ready to grow rightward.
        if self.workspace_rail_width > 0.0 {
            egui::SidePanel::right("future_workspace_rail")
                .resizable(false)
                .exact_width(self.workspace_rail_width)
                .show(ctx, |_ui| {});
        }

        egui::TopBottomPanel::top("jaymi_header")
            .exact_height(64.0)
            .frame(
                egui::Frame::NONE
                    .fill(ConversationTheme::HEADER_FILL)
                    .inner_margin(egui::Margin::symmetric(28, 14)),
            )
            .show(ctx, |ui| {
                Header::show(ui);
            });

        egui::TopBottomPanel::bottom("jaymi_composer")
            .exact_height(118.0)
            .frame(
                egui::Frame::NONE
                    .fill(ConversationTheme::COMPOSER_BAND)
                    .inner_margin(egui::Margin::symmetric(24, 16)),
            )
            .show(ctx, |ui| {
                if let Some(notice) = &self.status_notice {
                    ui.label(
                        egui::RichText::new(notice)
                            .size(13.0)
                            .color(ConversationTheme::MUTED_TEXT),
                    );
                    ui.add_space(4.0);
                }

                ui.horizontal(|ui| {
                    ui.set_height(72.0);
                    let menu_reserve = 56.0_f32;
                    let composer_width = (ui.available_width() - menu_reserve).max(240.0);
                    ui.allocate_ui(egui::vec2(composer_width, 72.0), |ui| {
                        let send = MessageComposer::show(
                            ui,
                            &mut self.composer,
                            self.composer_state,
                        );
                        if send {
                            self.submit_message();
                        }
                    });
                    ui.add_space(8.0);
                    let menu_response = ActionMenu::show_button(ui);
                    if menu_response.clicked() {
                        self.menu_open = !self.menu_open;
                    }

                    if self.menu_open {
                        let menu_pos = menu_response.rect.left_top() - egui::vec2(200.0, 210.0);
                        if let Some(action) = ActionMenu::show_popup(ctx, menu_pos) {
                            self.handle_menu_action(action);
                        }
                    }
                });
            });

        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(ConversationTheme::CANVAS)
                    .inner_margin(egui::Margin::symmetric(0, 0)),
            )
            .show(ctx, |ui| {
                paint_atmosphere(ui);
                ConversationView::show(
                    ui,
                    self.conversation.messages(),
                    &mut self.scroll_to_bottom,
                );
            });

        if self.settings_open {
            let snapshot = self.app.diagnostics().ok();
            if let Some(close) = SettingsPanel::show(ctx, snapshot.as_ref()) {
                if close {
                    self.settings_open = false;
                }
            }
        }
    }
}

fn paint_atmosphere(ui: &mut egui::Ui) {
    let rect = ui.max_rect();
    let painter = ui.painter();
    painter.rect_filled(rect, 0.0, ConversationTheme::CANVAS);

    // Soft vertical wash — calm atmosphere without flat single-color fill.
    let top = ConversationTheme::ATMOSPHERE_TOP;
    let bottom = ConversationTheme::ATMOSPHERE_BOTTOM;
    for (index, y) in (0..24).map(|step| {
        (
            step,
            rect.top() + rect.height() * (step as f32 / 23.0),
        )
    }) {
        let t = index as f32 / 23.0;
        let color = lerp_color(top, bottom, t);
        let band = egui::Rect::from_min_max(
            egui::pos2(rect.left(), y),
            egui::pos2(rect.right(), y + rect.height() / 23.0 + 1.0),
        );
        painter.rect_filled(band, 0.0, color);
    }
}

fn lerp_color(a: egui::Color32, b: egui::Color32, t: f32) -> egui::Color32 {
    let t = t.clamp(0.0, 1.0);
    egui::Color32::from_rgba_unmultiplied(
        ((a.r() as f32) + (b.r() as f32 - a.r() as f32) * t) as u8,
        ((a.g() as f32) + (b.g() as f32 - a.g() as f32) * t) as u8,
        ((a.b() as f32) + (b.b() as f32 - a.b() as f32) * t) as u8,
        ((a.a() as f32) + (b.a() as f32 - a.a() as f32) * t) as u8,
    )
}
