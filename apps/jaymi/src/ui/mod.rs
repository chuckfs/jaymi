//! Conversation-first desktop experience.
//!
//! The conversation stays visible. Capabilities may expand a workspace from
//! the right. Closing the workspace never destroys the conversation.

pub mod explorer;
pub mod nav_rail;
pub mod review_card;

use std::collections::HashMap;
use std::time::SystemTime;

use eframe::egui;

use crate::boot::Application;
use crate::coding_quick_actions::{dispatch_quick_action, QuickActionEffect};
use crate::coding_workspace::{render_coding_shell, CodingShellEvent, MonacoEditorSurface};
use crate::command_dispatch::{dispatch_command, CommandDispatchEffect};
use crate::command_palette::{
    gather_palette_items, render_command_palette, CommandPaletteOutcome, CommandPaletteState,
    PaletteAction, PaletteCommandRef, PaletteGatherInput,
};
use crate::diagnostics::{DiagnosticsSnapshot, OperationalStatus};
use crate::experience::ExperienceSession;
use crate::monaco_host::{
    language_for_path, resolve_monaco_assets, MonacoDocument, MonacoHost, MonacoIpcMessage,
};
use crate::theme::{inset, radius, space, stroke, type_size, Theme};
use crate::ui::explorer::ExplorerEvent;
use crate::ui::nav_rail::{
    render_nav_rail, NavRailContext, NavRailEvent, NavTab, DEFAULT_NAV_WIDTH, MAX_NAV_WIDTH,
    MIN_NAV_WIDTH,
};
use crate::ui::review_card::render_review_card;
use jaymi_capabilities::{
    CodingBottomTab, EditorSettings, FoldedRegion, SplitDirection, WorkspaceKind,
    DEFAULT_CONVERSATION_FRACTION, DEFAULT_WORKSPACE_PANEL_WIDTH, MAX_CONVERSATION_FRACTION,
    MAX_WORKSPACE_PANEL_WIDTH, MIN_CONVERSATION_WIDTH, MIN_WORKSPACE_PANEL_WIDTH,
};
use jaymi_config::{Config, Theme as ThemePreference};
use jaymi_core::UserRequest;
use jaymi_memory::{ConversationMeta, MessageRole};
use jaymi_planner::ReviewIntent;

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
        Box::new(move |cc| {
            configure_fonts(&cc.egui_ctx);
            let preference = app
                .container()
                .resolve::<Config>()
                .map(|config| config.settings().theme)
                .unwrap_or(ThemePreference::System);
            let system_dark = cc
                .egui_ctx
                .system_theme()
                .map(|theme| matches!(theme, egui::Theme::Dark))
                .unwrap_or(false);
            let theme = Theme::resolve(preference, system_dark);
            theme.apply_egui(&cc.egui_ctx);
            Ok(Box::new(JaymiApp {
                app,
                snapshot: initial_snapshot,
                list_path_input: initial_list_path,
                read_path_input: initial_read_path,
                prompt: String::new(),
                focus_composer: false,
                review_modify_notes: HashMap::new(),
                experience,
                show_diagnostics: false,
                error: None,
                status: None,
                monaco: None,
                monaco_last_error: None,
                command_palette: CommandPaletteState::default(),
                workspace_was_expanded: false,
                workspace_anim_start: None,
                workspace_anim_from: MIN_WORKSPACE_PANEL_WIDTH,
                workspace_anim_target: DEFAULT_WORKSPACE_PANEL_WIDTH,
                awaiting_reply: false,
                theme,
                nav_open: false,
                nav_tab: NavTab::Conversations,
                nav_was_open: false,
                nav_anim_start: None,
                nav_anim_from: 0.0,
                nav_anim_target: DEFAULT_NAV_WIDTH,
                nav_width: DEFAULT_NAV_WIDTH,
            }))
        }),
    )
}

/// Painted hamburger (three bars) — no Unicode menu glyph.
fn paint_hamburger(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let c = rect.center();
    let half_w = 7.0;
    let stroke = egui::Stroke::new(1.75, color);
    for dy in [-5.0_f32, 0.0, 5.0] {
        painter.line_segment(
            [
                egui::pos2(c.x - half_w, c.y + dy),
                egui::pos2(c.x + half_w, c.y + dy),
            ],
            stroke,
        );
    }
}

/// Accent send control with a painted up-chevron (no Unicode icon glyphs).
fn paint_send_button(ui: &mut egui::Ui, theme: &Theme) -> egui::Response {
    let size = egui::vec2(32.0, 32.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let fill = if response.hovered() {
        theme.accent.gamma_multiply(0.92)
    } else {
        theme.accent
    };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(radius::MD as u8), fill);
    let c = rect.center();
    let arrow = [
        c + egui::vec2(0.0, -5.0),
        c + egui::vec2(5.0, 2.5),
        c + egui::vec2(-5.0, 2.5),
    ];
    ui.painter().add(egui::Shape::convex_polygon(
        arrow.to_vec(),
        theme.on_accent(),
        egui::Stroke::NONE,
    ));
    response
}

/// Compact composer toolbar control — monochrome icon tile.
fn composer_icon_button(
    ui: &mut egui::Ui,
    theme: &Theme,
    paint: impl FnOnce(&egui::Painter, egui::Pos2, egui::Color32),
) -> egui::Response {
    let size = egui::vec2(28.0, 28.0);
    let (rect, mut response) = ui.allocate_exact_size(size, egui::Sense::click());
    response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let color = if response.hovered() {
        theme.text_primary
    } else {
        theme.text_secondary
    };
    if response.hovered() {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(radius::SM as u8),
            theme.selection(),
        );
    }
    paint(ui.painter(), rect.center(), color);
    response
}

/// Text chip in the composer toolbar (`@Project`, `⌘P`).
fn composer_chip(ui: &mut egui::Ui, theme: &Theme, label: &str) -> egui::Response {
    let galley = ui.fonts(|f| {
        f.layout_no_wrap(
            label.to_string(),
            egui::FontId::proportional(type_size::META),
            theme.text_secondary,
        )
    });
    let pad_x = space::SM;
    let size = egui::vec2(galley.size().x + pad_x * 2.0, 28.0);
    let (rect, mut response) = ui.allocate_exact_size(size, egui::Sense::click());
    response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let hovered = response.hovered();
    ui.painter().rect_filled(
        rect,
        egui::CornerRadius::same(radius::SM as u8),
        if hovered {
            theme.selection()
        } else {
            theme.surface_alt
        },
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(type_size::META),
        if hovered {
            theme.text_primary
        } else {
            theme.text_secondary
        },
    );
    response
}

fn paint_composer_plus(painter: &egui::Painter, center: egui::Pos2, color: egui::Color32) {
    let stroke = egui::Stroke::new(1.6, color);
    let arm = 5.0;
    painter.line_segment(
        [center + egui::vec2(-arm, 0.0), center + egui::vec2(arm, 0.0)],
        stroke,
    );
    painter.line_segment(
        [center + egui::vec2(0.0, -arm), center + egui::vec2(0.0, arm)],
        stroke,
    );
}

/// Prefer system proportional fonts so UI text never falls back to tofu squares.
fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    // Keep egui defaults as fallbacks; prepend OS UI fonts when available.
    #[cfg(target_os = "macos")]
    {
        for (path, name) in [
            ("/System/Library/Fonts/SFNS.ttf", "sf_pro"),
            ("/System/Library/Fonts/SFNSText.ttf", "sf_pro_text"),
            ("/System/Library/Fonts/Helvetica.ttc", "helvetica"),
        ] {
            if let Ok(bytes) = std::fs::read(path) {
                fonts
                    .font_data
                    .insert(name.into(), egui::FontData::from_owned(bytes).into());
                fonts
                    .families
                    .entry(egui::FontFamily::Proportional)
                    .or_default()
                    .insert(0, name.into());
                break;
            }
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(bytes) = std::fs::read("C:\\Windows\\Fonts\\segoeui.ttf") {
            fonts
                .font_data
                .insert("segoe_ui".into(), egui::FontData::from_owned(bytes).into());
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .insert(0, "segoe_ui".into());
        }
    }
    #[cfg(target_os = "linux")]
    {
        for path in [
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/TTF/DejaVuSans.ttf",
        ] {
            if let Ok(bytes) = std::fs::read(path) {
                fonts
                    .font_data
                    .insert("dejavu".into(), egui::FontData::from_owned(bytes).into());
                fonts
                    .families
                    .entry(egui::FontFamily::Proportional)
                    .or_default()
                    .insert(0, "dejavu".into());
                break;
            }
        }
    }
    ctx.set_fonts(fonts);
}

struct JaymiApp {
    app: Application,
    snapshot: DiagnosticsSnapshot,
    list_path_input: String,
    read_path_input: String,
    prompt: String,
    /// Request focus on the conversation composer after a Quick Action insert.
    focus_composer: bool,
    /// Per-plan draft notes for Review Card Modify guidance.
    review_modify_notes: HashMap<String, String>,
    experience: ExperienceSession,
    show_diagnostics: bool,
    /// Hard failure message (composer + recoverable actions).
    error: Option<String>,
    /// Non-error status (search summaries, confirmations) — never painted as error.
    status: Option<String>,
    /// Child WebView hosting Monaco (rehydrated from CodingState on Ready).
    monaco: Option<MonacoHost>,
    /// Last Monaco host error (assets / webview create).
    monaco_last_error: Option<String>,
    /// VS Code–style Command Palette (⌘P).
    command_palette: CommandPaletteState,
    /// Whether the workspace SidePanel was expanded on the previous frame
    /// (drives the expand-in animation on the false → true transition).
    workspace_was_expanded: bool,
    /// Wall-clock start of the current expand animation, when running.
    workspace_anim_start: Option<std::time::Instant>,
    /// Width the expand animation starts from (typically the min width).
    workspace_anim_from: f32,
    /// Width the expand animation eases toward the remembered/default width.
    workspace_anim_target: f32,
    /// True while waiting for Jaymi to respond (typing indicator).
    awaiting_reply: bool,
    /// Active application theme (drives egui visuals + Monaco Jaymi themes).
    theme: Theme,
    /// Whether the left navigation rail is open (or animating open).
    nav_open: bool,
    /// Active page in the left navigation rail.
    nav_tab: NavTab,
    /// Previous-frame nav open flag (drives open/close animation).
    nav_was_open: bool,
    /// Wall-clock start of the nav rail animation, when running.
    nav_anim_start: Option<std::time::Instant>,
    /// Width the nav animation starts from.
    nav_anim_from: f32,
    /// Width the nav animation eases toward.
    nav_anim_target: f32,
    /// Remembered open width for the left nav rail.
    nav_width: f32,
}

/// Duration of the workspace expand-in animation.
const WORKSPACE_EXPAND_ANIM_SECS: f32 = 0.18;

/// Ease-out cubic — quick start, gentle settle (used for the expand animation).
fn ease_out_cubic(t: f32) -> f32 {
    let inv = 1.0 - t.clamp(0.0, 1.0);
    1.0 - inv * inv * inv
}

fn status_color(theme: &Theme, status: OperationalStatus) -> egui::Color32 {
    match status {
        OperationalStatus::Operational => theme.success,
        OperationalStatus::Experimental => theme.warning,
        OperationalStatus::Stub => theme.text_secondary,
        OperationalStatus::Disabled => theme.error,
    }
}

/// Day bucket label for conversation timestamp separators (UTC calendar day).
fn format_day_separator(created_at: i64) -> String {
    let today = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64 / 86_400)
        .unwrap_or(0);
    let day = created_at.div_euclid(86_400);
    match today - day {
        0 => "Today".to_string(),
        1 => "Yesterday".to_string(),
        delta if delta > 1 && delta < 7 => format!("{delta} days ago"),
        _ => {
            // YYYY-MM-DD from UTC seconds (stable, no extra deps).
            let days = created_at.div_euclid(86_400);
            let (year, month, day) = civil_from_days(days);
            format!("{year:04}-{month:02}-{day:02}")
        }
    }
}

/// Clock time on a message bubble (UTC HH:MM — presentation stamp).
fn format_message_time(created_at: i64) -> String {
    let secs = created_at.rem_euclid(86_400) as u32;
    let hours = secs / 3600;
    let minutes = (secs % 3600) / 60;
    format!("{hours:02}:{minutes:02}")
}

/// Howard Hinnant civil_from_days — days since Unix epoch → Y-M-D.
fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m as u32, d as u32)
}

impl eframe::App for JaymiApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.sync_theme(ctx);

        if let Ok(session) = self.app.experience() {
            self.experience = session;
        }

        // ⌘P / ⌘⇧P Command Palette · ⌘⇧F Find in Files
        let (open_palette, find_in_files) = ctx.input(|input| {
            let command = input.modifiers.command || input.modifiers.mac_cmd;
            let shift = input.modifiers.shift;
            let p_pressed = input.key_pressed(egui::Key::P);
            let f_pressed = input.key_pressed(egui::Key::F);
            (
                command && p_pressed, // ⌘P and ⌘⇧P both open the global palette
                command && shift && f_pressed,
            )
        });
        if open_palette {
            self.command_palette.open();
            self.refresh_command_palette();
        }
        if find_in_files {
            self.open_find_in_files();
        }

        let coding_open = self.experience.active_workspace_kind() == Some(WorkspaceKind::Coding);
        let mut monaco_surface: Option<MonacoEditorSurface> = None;

        egui::TopBottomPanel::top("jaymi_top")
            .exact_height(48.0)
            .show_separator_line(false)
            .frame(
                egui::Frame::new()
                    .fill(self.theme.background)
                    .inner_margin(inset(space::LG, space::SM))
                    .stroke(egui::Stroke::NONE),
            )
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    self.render_nav_toggle(ui);
                });
            });

        self.render_nav_side_panel(ctx);

        if self.experience.workspace_expanded() {
            // Chat-forward: Coding expands beside conversation, never full-window.
            let is_coding = self.experience.active_workspace_kind() == Some(WorkspaceKind::Coding);
            let remembered_coding_width = self
                .experience
                .capability_state()
                .and_then(|state| state.coding())
                .map(|coding| coding.workspace_panel_width)
                .unwrap_or(DEFAULT_WORKSPACE_PANEL_WIDTH);

            let (default_w, min_w, max_w) = if is_coding {
                // Conversation defaults to ~30% of the window, never below
                // MIN_CONVERSATION_WIDTH, and never above MAX_CONVERSATION_FRACTION.
                // Reserve left-nav width when the rail is open/animating.
                let window_w = ctx.screen_rect().width();
                let nav_reserve = self.current_nav_width();
                let max_w = (window_w - MIN_CONVERSATION_WIDTH - nav_reserve)
                    .clamp(0.0, MAX_WORKSPACE_PANEL_WIDTH);
                let min_from_conversation_cap =
                    (window_w * (1.0 - MAX_CONVERSATION_FRACTION)).max(0.0);
                let min_w = min_from_conversation_cap
                    .max(MIN_WORKSPACE_PANEL_WIDTH.min(max_w))
                    .min(max_w);
                let preferred = if remembered_coding_width > 0.0 {
                    remembered_coding_width
                } else {
                    window_w * (1.0 - DEFAULT_CONVERSATION_FRACTION)
                };
                let default_w = preferred.clamp(min_w, max_w.max(min_w));
                (default_w, min_w, max_w.max(min_w))
            } else {
                (420.0, 320.0, 560.0)
            };

            // Smooth expansion: the first frame a workspace opens, animate from a
            // compact width up to the remembered/default width.
            let just_expanded = !self.workspace_was_expanded;
            if just_expanded {
                self.workspace_anim_start = Some(std::time::Instant::now());
                self.workspace_anim_from = min_w;
                self.workspace_anim_target = default_w;
            }
            self.workspace_was_expanded = true;

            let animating = self
                .workspace_anim_start
                .is_some_and(|start| start.elapsed().as_secs_f32() < WORKSPACE_EXPAND_ANIM_SECS);

            let mut panel = egui::SidePanel::right("jaymi_workspace")
                .min_width(min_w)
                .max_width(max_w)
                .resizable(true);
            if animating {
                let elapsed = self
                    .workspace_anim_start
                    .expect("animating implies start")
                    .elapsed()
                    .as_secs_f32();
                let t = ease_out_cubic(elapsed / WORKSPACE_EXPAND_ANIM_SECS);
                let width = self.workspace_anim_from
                    + (self.workspace_anim_target - self.workspace_anim_from) * t;
                panel = panel.exact_width(width.clamp(min_w, max_w));
                ctx.request_repaint();
            } else {
                panel = panel.default_width(default_w);
            }

            let panel_response = panel.show(ctx, |ui| {
                monaco_surface = self.render_workspace(ui);
            });

            // Write back user resizes of the Coding side panel so they survive
            // workspace close/reopen (persisted in `.jaymi/workspace.json`).
            if is_coding && !animating {
                let rendered_width = panel_response.response.rect.width();
                if (rendered_width - remembered_coding_width).abs() > 1.0 {
                    let updated = self
                        .app
                        .with_coding_state(|coding| {
                            coding.set_workspace_panel_width(rendered_width)
                        })
                        .is_ok();
                    if updated {
                        // Avoid a disk write on every dragged frame — persist once
                        // the mouse button is released.
                        if !ctx.input(|input| input.pointer.primary_down()) {
                            let _ = self.app.persist_coding_editor_workspace();
                        }
                        if let Ok(session) = self.app.experience() {
                            self.experience = session;
                        }
                    }
                }
            }
        } else {
            self.workspace_was_expanded = false;
            self.workspace_anim_start = None;
        }

        // Conversation column — always alive beside Coding (never replaced).
        // Open window background only — no nested conversation frame.
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(self.theme.background)
                    .inner_margin(egui::Margin::ZERO),
            )
            .show(ctx, |ui| {
                // Floating composer — anchored bottom, inset from edges, no chrome bar.
                egui::TopBottomPanel::bottom("chat_composer")
                    .show_separator_line(false)
                    .frame(
                        egui::Frame::new()
                            .inner_margin(egui::Margin {
                                left: space::XL as i8,
                                right: space::XL as i8,
                                top: space::MD as i8,
                                bottom: space::LG as i8,
                            })
                            .fill(self.theme.background)
                            .stroke(egui::Stroke::NONE),
                    )
                    .show_inside(ui, |ui| {
                        self.render_chat_composer(ui);
                    });

                self.render_conversation_surface(ui);
                if self.show_diagnostics {
                    ui.add_space(space::MD);
                    self.render_diagnostics(ui);
                }
            });

        if let Ok(registry) = self.app.command_registry() {
            let _ = registry;
            let outcome = render_command_palette(ctx, &mut self.command_palette, &self.theme);
            self.handle_command_palette_outcome(outcome);
        }

        self.sync_monaco(ctx, frame, coding_open, monaco_surface.as_ref());
    }
}

impl JaymiApp {
    /// Resolve config + OS appearance into [`Theme`] and push to egui / Monaco.
    fn sync_theme(&mut self, ctx: &egui::Context) {
        let preference = self
            .app
            .container()
            .resolve::<Config>()
            .map(|config| config.settings().theme)
            .unwrap_or(ThemePreference::System);
        let system_dark = ctx
            .system_theme()
            .map(|theme| matches!(theme, egui::Theme::Dark))
            .unwrap_or(self.theme.mode.is_dark());
        let next = Theme::resolve(preference, system_dark);
        if next != self.theme {
            next.apply_egui(ctx);
            if let Some(host) = self.monaco.as_mut() {
                host.clear_theme_cache();
            }
            self.theme = next;
        }
    }

    fn handle_command_palette_outcome(&mut self, outcome: CommandPaletteOutcome) {
        match outcome {
            CommandPaletteOutcome::None => {}
            CommandPaletteOutcome::QueryChanged(_) => self.refresh_command_palette(),
            CommandPaletteOutcome::Execute(action) => self.dispatch_palette_action(action),
        }
    }

    fn refresh_command_palette(&mut self) {
        let query = self.command_palette.query().to_string();
        let Ok(registry) = self.app.command_registry() else {
            self.command_palette.set_results(Vec::new());
            return;
        };
        let commands = match registry.list() {
            Ok(list) => list
                .into_iter()
                .map(|command| PaletteCommandRef {
                    id: command.id.clone(),
                    title: command.title.clone(),
                    category: command.category.label().to_string(),
                    keywords: command.keywords.clone(),
                    keybinding: command.keybinding.clone(),
                    argument_prompt: command.argument_prompt.clone(),
                })
                .collect(),
            Err(_) => Vec::new(),
        };

        let projects: Vec<(String, String)> = self
            .app
            .list_projects()
            .unwrap_or_default()
            .into_iter()
            .map(|project| {
                let label = if !project.name.trim().is_empty() {
                    project.name.clone()
                } else {
                    project
                        .root_directory
                        .as_ref()
                        .and_then(|root| {
                            root.file_name()
                                .map(|name| name.to_string_lossy().into_owned())
                        })
                        .unwrap_or_else(|| project.id.to_string())
                };
                (project.id.to_string(), label)
            })
            .collect();

        let conversations: Vec<(String, String)> = self
            .project_conversations()
            .into_iter()
            .map(|meta| {
                let title = meta
                    .title
                    .clone()
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| "Conversation".to_string());
                (meta.id.to_string(), title)
            })
            .collect();

        let capabilities: Vec<(String, String)> = jaymi_capabilities::Capability::all()
            .iter()
            .map(|capability| {
                let descriptor = jaymi_capabilities::capability_descriptor(*capability);
                (capability.id().to_string(), descriptor.name.to_string())
            })
            .collect();

        let mut files = Vec::new();
        let mut knowledge = Vec::new();
        if !query.trim().is_empty() {
            let mut request = jaymi_core::SearchRequest::filename(&query);
            if let Some(root) = self.app.active_project_root_path() {
                request.folder = Some(root);
            }
            if let Ok(results) = self.app.project_search(request) {
                files = results
                    .into_iter()
                    .take(24)
                    .enumerate()
                    .map(|(index, result)| {
                        let title = if result.title.is_empty() {
                            std::path::Path::new(&result.path)
                                .file_name()
                                .map(|name| name.to_string_lossy().into_owned())
                                .unwrap_or_else(|| result.path.clone())
                        } else {
                            result.title
                        };
                        // Prefer earlier Search Engine hits.
                        let score = 200u32.saturating_sub(index as u32 * 5);
                        (result.path, title, score)
                    })
                    .collect();
            }
            if let Some(project_id) = self.app.active_project_id() {
                if let Ok(hits) = self
                    .app
                    .search_project_knowledge(&project_id, &query, Some(16))
                {
                    knowledge = hits
                        .into_iter()
                        .map(|hit| {
                            (
                                hit.title,
                                hit.detail,
                                hit.path.map(|path| path.to_string_lossy().into_owned()),
                                hit.score,
                            )
                        })
                        .collect();
                }
            }
        }

        let items = gather_palette_items(&PaletteGatherInput {
            commands,
            projects,
            conversations,
            capabilities,
            files,
            knowledge,
            query,
        });
        self.command_palette.set_results(items);
    }

    fn dispatch_palette_action(&mut self, action: PaletteAction) {
        match action {
            PaletteAction::RunCommand { id, argument } => {
                if argument.is_none() {
                    if let Ok(registry) = self.app.command_registry() {
                        if let Ok(Some(command)) = registry.get(&id) {
                            if let Some(prompt) = command.argument_prompt.clone() {
                                self.command_palette.prompt_argument(
                                    command.id.clone(),
                                    command.title.clone(),
                                    prompt,
                                );
                                return;
                            }
                        }
                    }
                }
                self.command_palette.close();
                self.run_dispatched_command(&id, argument.as_deref());
            }
            PaletteAction::OpenProject { project_id } => {
                self.command_palette.close();
                self.open_project_by_id(&project_id);
            }
            PaletteAction::OpenFile { path } => {
                self.command_palette.close();
                match self.app.open_search_result(&path, None, None) {
                    Ok(()) => {
                        self.error = None;
                        if let Ok(session) = self.app.experience() {
                            self.experience = session;
                        }
                    }
                    Err(error) => self.error = Some(error.message().to_string()),
                }
            }
            PaletteAction::OpenConversation { conversation_id } => {
                self.command_palette.close();
                match self.app.switch_to_conversation(&conversation_id) {
                    Ok(()) => {
                        self.error = None;
                        if let Ok(session) = self.app.experience() {
                            self.experience = session;
                        }
                    }
                    Err(error) => self.error = Some(error.message().to_string()),
                }
            }
            PaletteAction::ContinueConversation { prompt } => {
                self.command_palette.close();
                if let Some(text) = prompt {
                    self.prompt = text;
                }
                self.focus_composer = true;
                self.status = Some("Continue the conversation…".into());
            }
            PaletteAction::OpenCapability { capability_id } => {
                self.command_palette.close();
                match self.app.resolve_capability(&capability_id) {
                    Ok(Some(descriptor)) => {
                        self.error = None;
                        self.status = Some(format!(
                            "{} — {}",
                            descriptor.name, descriptor.description
                        ));
                    }
                    Ok(None) => {
                        self.status = Some(format!("Capability: {capability_id}"));
                    }
                    Err(error) => self.error = Some(error.message().to_string()),
                }
            }
            PaletteAction::OpenKnowledge { title, path, query } => {
                self.command_palette.close();
                if let Some(path) = path {
                    match self.app.open_search_result(&path, None, None) {
                        Ok(()) => {
                            self.error = None;
                            if let Ok(session) = self.app.experience() {
                                self.experience = session;
                            }
                        }
                        Err(error) => self.error = Some(error.message().to_string()),
                    }
                } else {
                    self.prompt = format!("Search knowledge for “{query}” ({title})");
                    self.focus_composer = true;
                    self.status = Some(format!("Knowledge: {title}"));
                }
            }
        }
    }

    fn run_dispatched_command(&mut self, id: &str, argument: Option<&str>) {
        match dispatch_command(&self.app, id, argument) {
            Ok(CommandDispatchEffect::None) => {
                self.error = None;
                self.status = None;
            }
            Ok(CommandDispatchEffect::RefreshExperience) => {
                self.error = None;
                self.status = None;
                if let Ok(session) = self.app.experience() {
                    self.experience = session;
                }
            }
            Ok(CommandDispatchEffect::CloseWorkspace) => {
                self.close_workspace();
            }
            Ok(CommandDispatchEffect::PickAndOpenFile) => {
                self.palette_open_file();
            }
            Ok(CommandDispatchEffect::PickAndOpenFolder) => {
                self.open_project_folder();
            }
            Ok(CommandDispatchEffect::Status(message)) => {
                self.error = None;
                if let Ok(session) = self.app.experience() {
                    self.experience = session;
                }
                self.status = Some(message);
            }
            Ok(CommandDispatchEffect::OpenCommandPalette) => {
                self.command_palette.open();
                self.refresh_command_palette();
            }
            Ok(CommandDispatchEffect::ContinueConversation) => {
                self.focus_composer = true;
                self.status = Some("Continue the conversation…".into());
            }
            Err(error) => {
                self.status = None;
                self.error = Some(error.message().to_string());
            }
        }
    }

    /// Open Find in Files (⌘⇧F) — ensures Coding is up and shows the Search dock.
    fn open_find_in_files(&mut self) {
        if let Err(error) = self.app.start_coding_project() {
            self.status = None;
            self.error = Some(error.message().to_string());
            return;
        }
        match self.app.with_coding_state(|coding| {
            coding.show_bottom_tab(CodingBottomTab::Search);
        }) {
            Ok(()) => {
                self.error = None;
                let _ = self.app.persist_coding_editor_workspace();
                if let Ok(session) = self.app.experience() {
                    self.experience = session;
                }
            }
            Err(error) => {
                self.status = None;
                self.error = Some(error.message().to_string());
            }
        }
    }

    fn palette_open_file(&mut self) {
        let picked = rfd::FileDialog::new().set_title("Open File").pick_file();
        let Some(path) = picked else {
            return;
        };
        if let Err(error) = self.app.start_coding_project() {
            self.error = Some(error.message().to_string());
            return;
        }
        match self.app.open_coding_file(&path.to_string_lossy()) {
            Ok(()) => {
                self.error = None;
                if let Ok(session) = self.app.experience() {
                    self.experience = session;
                }
            }
            Err(error) => self.error = Some(error.message().to_string()),
        }
    }

    fn render_conversation_surface(&mut self, ui: &mut egui::Ui) {
        // Open history on the window background — no title, no surrounding frame.
        let available = ui.available_height();
        let turns: Vec<_> = self.experience.conversation().to_vec();
        let empty = turns.is_empty() && !self.awaiting_reply;
        let mut review_intent: Option<ReviewIntent> = None;
        egui::ScrollArea::vertical()
            .id_salt("conversation_scroll")
            .animated(true)
            .auto_shrink([false, false])
            .stick_to_bottom(!empty)
            .max_height(available)
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.set_min_height(available.max(200.0));
                if empty {
                    // Full-bleed welcome — centered in the conversation column.
                    self.render_conversation_empty_state(ui);
                } else {
                    ui.add_space(space::MD);
                    ui.horizontal(|ui| {
                        ui.add_space(space::LG);
                        ui.vertical(|ui| {
                            ui.set_max_width((ui.available_width() - space::LG).max(120.0));
                            let mut last_day: Option<i64> = None;
                            for turn in &turns {
                                let day = turn.created_at.div_euclid(86_400);
                                if last_day != Some(day) {
                                    self.render_timestamp_separator(ui, turn.created_at);
                                    last_day = Some(day);
                                }
                                self.render_chat_bubble(
                                    ui,
                                    turn.role,
                                    &turn.content,
                                    turn.created_at,
                                );
                                if let Some(review) = &turn.review {
                                    ui.add_space(space::SM);
                                    let plan_key = review.plan_id.as_str().to_string();
                                    let note = self
                                        .review_modify_notes
                                        .entry(plan_key)
                                        .or_default();
                                    if let Some(intent) =
                                        render_review_card(ui, &self.theme, review, note)
                                    {
                                        review_intent = Some(intent);
                                    }
                                }
                                ui.add_space(space::LG);
                            }
                            if self.awaiting_reply {
                                self.render_typing_indicator(ui);
                            }
                            // Breathing room above the floating composer.
                            ui.add_space(space::XL);
                        });
                        ui.add_space(space::LG);
                    });
                }
            });
        if let Some(intent) = review_intent {
            self.handle_review_intent(intent);
        }
    }

    fn render_conversation_empty_state(&mut self, ui: &mut egui::Ui) {
        let available = ui.available_size();
        ui.allocate_ui_with_layout(
            available,
            egui::Layout::top_down(egui::Align::Center),
            |ui| {
                // True vertical center of the conversation surface.
                let block_height = 140.0;
                let top = ((ui.available_height() - block_height) * 0.5).max(space::XL);
                ui.add_space(top);

                ui.label(
                    egui::RichText::new("Hi, I'm Jaymi")
                        .size(type_size::WELCOME)
                        .strong()
                        .color(self.theme.text_primary),
                );
                ui.add_space(space::MD + space::XS);
                ui.set_max_width((ui.available_width() * 0.7).clamp(260.0, 480.0));
                ui.label(
                    egui::RichText::new("Ask anything,\nor open a Coding Workspace.")
                        .size(type_size::BODY + 2.0)
                        .color(self.theme.text_secondary),
                );
            },
        );
    }

    fn render_timestamp_separator(&self, ui: &mut egui::Ui, created_at: i64) {
        ui.add_space(space::MD);
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new(format_day_separator(created_at))
                    .size(type_size::META)
                    .color(self.theme.text_secondary),
            );
        });
        ui.add_space(space::MD);
    }

    fn render_typing_indicator(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.add_space(space::SM);
            ui.label(
                egui::RichText::new("Jaymi is typing…")
                    .italics()
                    .size(type_size::BODY)
                    .color(self.theme.text_secondary),
            );
        });
        ui.add_space(space::MD);
    }

    fn render_chat_bubble(
        &self,
        ui: &mut egui::Ui,
        role: MessageRole,
        content: &str,
        created_at: i64,
    ) {
        let max_bubble = (ui.available_width() * 0.78).clamp(240.0, 720.0);

        match role {
            MessageRole::User => {
                ui.horizontal(|ui| {
                    ui.add_space((ui.available_width() - max_bubble).max(0.0));
                    egui::Frame::new()
                        .corner_radius(radius::XL)
                        .inner_margin(inset(space::MD, space::SM + space::XS))
                        .fill(self.theme.accent)
                        .show(ui, |ui| {
                            ui.set_max_width(max_bubble);
                            ui.label(
                                egui::RichText::new(format_message_time(created_at))
                                    .size(type_size::META)
                                    .color(self.theme.on_accent()),
                            );
                            ui.add_space(space::XS);
                            ui.label(
                                egui::RichText::new(content)
                                    .size(type_size::BODY)
                                    .color(self.theme.on_accent()),
                            );
                        });
                });
            }
            MessageRole::Assistant | MessageRole::System => {
                // Assistant / system sit on the open background — no nested card.
                let label = if matches!(role, MessageRole::System) {
                    "System"
                } else {
                    "Jaymi"
                };
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(label)
                            .size(type_size::META)
                            .strong()
                            .color(self.theme.text_secondary),
                    );
                    ui.label(
                        egui::RichText::new(format_message_time(created_at))
                            .size(type_size::META)
                            .color(self.theme.text_secondary),
                    );
                });
                ui.add_space(space::XS);
                ui.set_max_width(max_bubble);
                ui.label(
                    egui::RichText::new(content)
                        .size(type_size::BODY)
                        .color(self.theme.text_primary),
                );
            }
        }
    }

    fn render_chat_composer(&mut self, ui: &mut egui::Ui) {
        if let Some(error) = &self.error {
            ui.label(
                egui::RichText::new(error)
                    .size(type_size::UI)
                    .color(self.theme.error),
            );
            ui.add_space(space::SM);
        } else if let Some(status) = &self.status {
            ui.label(
                egui::RichText::new(status)
                    .size(type_size::UI)
                    .color(self.theme.text_secondary),
            );
            ui.add_space(space::SM);
        }

        let mut attach_clicked = false;
        let mut quick_open_clicked = false;
        let mut send_clicked = false;

        const MAX_COMPOSER_ROWS: usize = 8;
        // Rough width of ⌘P chip + send + gaps on the trailing edge.
        const TRAILING_CONTROLS_W: f32 = 108.0;
        const LEADING_PLUS_W: f32 = 28.0;

        egui::Frame::new()
            .corner_radius(radius::XL)
            .inner_margin(egui::Margin::symmetric(space::MD as i8, space::SM as i8))
            .fill(self.theme.surface)
            .shadow(self.theme.elevation_shadow())
            .stroke(egui::Stroke::NONE)
            .show(ui, |ui| {
                let font_id = egui::FontId::proportional(type_size::BODY);
                let line_h = ui.fonts(|f| f.row_height(&font_id)).max(type_size::BODY + 4.0);
                let available = ui.available_width();
                let inline_edit_w =
                    (available - LEADING_PLUS_W - space::SM - TRAILING_CONTROLS_W).max(96.0);

                // Measure wrapped height at the inline width (first-row layout).
                let measure_rows = |text: &str, wrap_w: f32| -> usize {
                    if text.is_empty() {
                        return 1;
                    }
                    let galley = ui.fonts(|f| {
                        f.layout(
                            text.to_string(),
                            font_id.clone(),
                            self.theme.text_primary,
                            wrap_w,
                        )
                    });
                    ((galley.size().y / line_h).ceil() as usize).max(1)
                };

                let inline_rows = measure_rows(&self.prompt, inline_edit_w);
                let full_rows = measure_rows(&self.prompt, available);
                // Stack once the first row would wrap / contain a newline — text
                // sits above a locked + / ⌘P / send bar.
                let stacked = self.prompt.contains('\n') || inline_rows > 1 || full_rows > 1;

                let edit_id = egui::Id::new("jaymi_composer_input");
                // Shift+Enter inserts a newline; plain Enter sends.
                let newline_key = egui::KeyboardShortcut::new(
                    egui::Modifiers::SHIFT,
                    egui::Key::Enter,
                );
                let mut edit_response = None;

                if stacked {
                    let content_rows = full_rows.max(self.prompt.lines().count().max(1));
                    let visible_rows = content_rows.min(MAX_COMPOSER_ROWS);
                    let max_h = line_h * MAX_COMPOSER_ROWS as f32 + space::XS;

                    egui::ScrollArea::vertical()
                        .id_salt("jaymi_composer_scroll")
                        .max_height(max_h)
                        .auto_shrink([false, true])
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            let response = ui.add(
                                egui::TextEdit::multiline(&mut self.prompt)
                                    .id(edit_id)
                                    .desired_width(ui.available_width())
                                    .desired_rows(visible_rows)
                                    .return_key(Some(newline_key))
                                    .hint_text(
                                        egui::RichText::new("Message Jaymi…")
                                            .color(self.theme.text_secondary),
                                    )
                                    .text_color(self.theme.text_primary)
                                    .frame(false),
                            );
                            edit_response = Some(response);
                        });

                    ui.add_space(space::XS);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = space::SM;
                        let attach = composer_icon_button(ui, &self.theme, paint_composer_plus)
                            .on_hover_text("Attach files (soon)");
                        if attach.clicked() {
                            attach_clicked = true;
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = space::SM;
                            let send =
                                paint_send_button(ui, &self.theme).on_hover_text("Send (Enter)");
                            if send.clicked() {
                                send_clicked = true;
                            }
                            let quick = composer_chip(ui, &self.theme, "⌘P")
                                .on_hover_text("Command Palette (⌘P)");
                            if quick.clicked() {
                                quick_open_clicked = true;
                            }
                        });
                    });
                } else {
                    // First row: [+] Message Jaymi… …… [⌘P] [↑]
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = space::SM;
                        ui.set_min_height(32.0);

                        let attach = composer_icon_button(ui, &self.theme, paint_composer_plus)
                            .on_hover_text("Attach files (soon)");
                        if attach.clicked() {
                            attach_clicked = true;
                        }

                        let response = ui.add(
                            egui::TextEdit::multiline(&mut self.prompt)
                                .id(edit_id)
                                .desired_width(inline_edit_w)
                                .desired_rows(1)
                                .return_key(Some(newline_key))
                                .hint_text(
                                    egui::RichText::new("Message Jaymi…")
                                        .color(self.theme.text_secondary),
                                )
                                .text_color(self.theme.text_primary)
                                .frame(false),
                        );
                        edit_response = Some(response);

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = space::SM;
                            let send =
                                paint_send_button(ui, &self.theme).on_hover_text("Send (Enter)");
                            if send.clicked() {
                                send_clicked = true;
                            }
                            let quick = composer_chip(ui, &self.theme, "⌘P")
                                .on_hover_text("Command Palette (⌘P)");
                            if quick.clicked() {
                                quick_open_clicked = true;
                            }
                        });
                    });
                }

                if let Some(response) = edit_response {
                    if self.focus_composer {
                        response.request_focus();
                        self.focus_composer = false;
                    }
                    let enter_send = response.has_focus()
                        && ui.input(|input| {
                            input.key_pressed(egui::Key::Enter) && !input.modifiers.shift
                        });
                    if enter_send {
                        send_clicked = true;
                    }
                }
            });

        if attach_clicked {
            self.status = Some("Attach files coming soon.".to_string());
            self.error = None;
        }
        if quick_open_clicked {
            self.command_palette.open();
            self.refresh_command_palette();
        }
        if send_clicked {
            self.send_prompt();
        }
    }

    /// Hamburger toggle for the left navigation rail.
    fn render_nav_toggle(&mut self, ui: &mut egui::Ui) {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(34.0, 28.0), egui::Sense::click());
        paint_hamburger(
            ui.painter(),
            rect,
            if response.hovered() || self.nav_open {
                self.theme.text_primary
            } else {
                self.theme.text_secondary
            },
        );
        response.clone().on_hover_text(if self.nav_open {
            "Hide navigation"
        } else {
            "Show navigation"
        });
        if response.clicked() {
            self.nav_open = !self.nav_open;
        }
    }

    /// Animated left rail (History / Images / Code).
    fn render_nav_side_panel(&mut self, ctx: &egui::Context) {
        let just_opened = self.nav_open && !self.nav_was_open;
        let just_closed = !self.nav_open && self.nav_was_open;
        if just_opened {
            self.nav_anim_start = Some(std::time::Instant::now());
            self.nav_anim_from = 0.0;
            self.nav_anim_target = self.nav_width.clamp(MIN_NAV_WIDTH, MAX_NAV_WIDTH);
        } else if just_closed {
            self.nav_anim_start = Some(std::time::Instant::now());
            self.nav_anim_from = self.current_nav_width().max(MIN_NAV_WIDTH * 0.25);
            self.nav_anim_target = 0.0;
        }
        self.nav_was_open = self.nav_open;

        let animating = self
            .nav_anim_start
            .is_some_and(|start| start.elapsed().as_secs_f32() < WORKSPACE_EXPAND_ANIM_SECS);
        if !self.nav_open && !animating {
            self.nav_anim_start = None;
            return;
        }

        let width = if animating {
            let elapsed = self
                .nav_anim_start
                .expect("animating implies start")
                .elapsed()
                .as_secs_f32();
            let t = ease_out_cubic(elapsed / WORKSPACE_EXPAND_ANIM_SECS);
            ctx.request_repaint();
            self.nav_anim_from + (self.nav_anim_target - self.nav_anim_from) * t
        } else {
            self.nav_width.clamp(MIN_NAV_WIDTH, MAX_NAV_WIDTH)
        };

        if width < 1.0 {
            return;
        }

        let conversations = self.project_conversations();
        let recent_projects: Vec<(String, String)> = self
            .app
            .list_projects()
            .unwrap_or_default()
            .into_iter()
            .map(|project| {
                let label = if !project.name.trim().is_empty() {
                    project.name.clone()
                } else {
                    project
                        .root_directory
                        .as_ref()
                        .and_then(|root| {
                            root.file_name()
                                .map(|name| name.to_string_lossy().into_owned())
                        })
                        .unwrap_or_else(|| project.id.to_string())
                };
                (project.id.to_string(), label)
            })
            .collect();
        let active_conversation_id = self.experience.conversation_id().map(str::to_string);
        let has_project = self.app.active_project_id().is_some();
        let coding_open = self.experience.active_workspace_kind() == Some(WorkspaceKind::Coding);

        let mut events = Vec::new();
        let panel = egui::SidePanel::left("jaymi_nav")
            .exact_width(width)
            .resizable(self.nav_open && !animating)
            .min_width(MIN_NAV_WIDTH)
            .max_width(MAX_NAV_WIDTH)
            .show_separator_line(false)
            .frame(
                egui::Frame::new()
                    .fill(self.theme.background)
                    .inner_margin(egui::Margin {
                        left: space::MD as i8,
                        right: space::MD as i8,
                        top: space::MD as i8,
                        bottom: space::SM as i8,
                    })
                    .stroke(egui::Stroke::NONE),
            );

        let response = panel.show(ctx, |ui| {
            // Hairline on the rail's trailing edge.
            let rect = ui.max_rect();
            ui.painter().vline(
                rect.right(),
                rect.y_range(),
                egui::Stroke::new(stroke::HAIRLINE, self.theme.border),
            );
            let nav_ctx = NavRailContext {
                theme: &self.theme,
                tab: self.nav_tab,
                conversations: &conversations,
                active_conversation_id: active_conversation_id.as_deref(),
                has_project,
                coding_open,
                recent_projects: &recent_projects,
                show_diagnostics: self.show_diagnostics,
            };
            render_nav_rail(ui, &nav_ctx, &mut events);
        });

        if self.nav_open && !animating {
            let rendered = response.response.rect.width();
            if (rendered - self.nav_width).abs() > 1.0 {
                self.nav_width = rendered.clamp(MIN_NAV_WIDTH, MAX_NAV_WIDTH);
            }
        }

        self.handle_nav_events(events);
    }

    fn current_nav_width(&self) -> f32 {
        let animating = self
            .nav_anim_start
            .is_some_and(|start| start.elapsed().as_secs_f32() < WORKSPACE_EXPAND_ANIM_SECS);
        if animating {
            let elapsed = self
                .nav_anim_start
                .map(|start| start.elapsed().as_secs_f32())
                .unwrap_or(0.0);
            let t = ease_out_cubic(elapsed / WORKSPACE_EXPAND_ANIM_SECS);
            self.nav_anim_from + (self.nav_anim_target - self.nav_anim_from) * t
        } else if self.nav_open {
            self.nav_width.clamp(MIN_NAV_WIDTH, MAX_NAV_WIDTH)
        } else {
            0.0
        }
    }

    fn project_conversations(&self) -> Vec<ConversationMeta> {
        let Some(project_id) = self.app.active_project_id() else {
            return Vec::new();
        };
        self.app
            .list_project_conversations(&project_id)
            .unwrap_or_default()
    }

    fn handle_nav_events(&mut self, events: Vec<NavRailEvent>) {
        for event in events {
            match event {
                NavRailEvent::SelectTab(tab) => {
                    self.nav_tab = tab;
                }
                NavRailEvent::OpenProject => self.open_project_folder(),
                NavRailEvent::OpenProjectId(project_id) => self.open_project_by_id(&project_id),
                NavRailEvent::ToggleDiagnostics => {
                    self.show_diagnostics = !self.show_diagnostics;
                    self.error = None;
                    self.status = None;
                }
                NavRailEvent::OpenConversation(conversation_id) => {
                    match self.app.switch_to_conversation(&conversation_id) {
                        Ok(()) => {
                            self.error = None;
                            if let Ok(session) = self.app.experience() {
                                self.experience = session;
                            }
                        }
                        Err(error) => self.error = Some(error.message().to_string()),
                    }
                }
                NavRailEvent::OpenCoding => {
                    self.nav_tab = NavTab::Projects;
                    if self.app.active_project_id().is_some() {
                        self.start_coding_project();
                    } else {
                        self.open_project_folder();
                    }
                }
            }
        }
    }

    fn open_project_folder(&mut self) {
        let picked = rfd::FileDialog::new()
            .set_title("Open Project")
            .pick_folder();
        let Some(path) = picked else {
            return;
        };
        let _ = self.app.persist_coding_editor_workspace();
        match self.app.open_project_from_path(&path) {
            Ok(_) => {
                self.error = None;
                self.start_coding_project();
            }
            Err(error) => self.error = Some(error.message().to_string()),
        }
    }

    fn open_project_by_id(&mut self, project_id: &str) {
        let _ = self.app.persist_coding_editor_workspace();
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
                CodingShellEvent::ActivateTab { pane, path } => {
                    self.app.activate_coding_tab_in_pane(&pane, &path)
                }
                CodingShellEvent::CloseTab { pane, path } => {
                    self.app.close_coding_tab_in_pane(&pane, &path)
                }
                CodingShellEvent::EditContent {
                    pane,
                    path,
                    content,
                } => self
                    .app
                    .set_coding_tab_content_in_pane(&pane, &path, content),
                CodingShellEvent::Scroll { pane, path, offset } => {
                    self.app.set_coding_tab_scroll_in_pane(&pane, &path, offset)
                }
                CodingShellEvent::SetCursor {
                    pane,
                    path,
                    line,
                    column,
                } => self
                    .app
                    .set_coding_tab_cursor_in_pane(&pane, &path, line, column),
                CodingShellEvent::SetFolds {
                    pane,
                    path,
                    regions,
                } => {
                    let folds = regions
                        .into_iter()
                        .map(|(start_line, end_line)| FoldedRegion {
                            start_line,
                            end_line,
                        })
                        .collect();
                    self.app.set_coding_tab_folds_in_pane(&pane, &path, folds)
                }
                CodingShellEvent::SaveActive => self.app.save_active_coding_file(),
                CodingShellEvent::SaveTab(path) => self.app.save_coding_file(&path),
                CodingShellEvent::OpenCommandPalette => {
                    self.command_palette.open();
                    Ok(())
                }
                CodingShellEvent::OpenQuickOpen => {
                    self.command_palette.open();
                    self.refresh_command_palette();
                    Ok(())
                }
                CodingShellEvent::CloseWorkspace => {
                    self.close_workspace();
                    Ok(())
                }
                CodingShellEvent::OpenSearch => {
                    let result = self.app.with_coding_state(|coding| {
                        coding.show_bottom_tab(CodingBottomTab::Search);
                    });
                    if result.is_ok() {
                        let _ = self.app.persist_coding_editor_workspace();
                    }
                    result
                }
                CodingShellEvent::OpenSettings => {
                    // Until a real Settings surface exists, open the workspace
                    // Diagnostics dock page (not the developer dashboard).
                    let result = self.app.with_coding_state(|coding| {
                        coding.show_bottom_tab(CodingBottomTab::Diagnostics);
                    });
                    if result.is_ok() {
                        let _ = self.app.persist_coding_editor_workspace();
                    }
                    result
                }
                CodingShellEvent::SetMinimap(enabled) => {
                    self.update_editor_setting(|settings| settings.minimap = enabled)
                }
                CodingShellEvent::SetWordWrap(enabled) => {
                    self.update_editor_setting(|settings| settings.word_wrap = enabled)
                }
                CodingShellEvent::SetFontSize(size) => {
                    self.update_editor_setting(|settings| settings.font_size = size.max(8))
                }
                CodingShellEvent::SetBottomTab(tab) => {
                    let result = self.app.with_coding_state(|coding| {
                        if tab.is_page() {
                            coding.show_bottom_tab(tab);
                        } else {
                            coding.hide_bottom_dock();
                        }
                    });
                    if result.is_ok() {
                        let _ = self.app.persist_coding_editor_workspace();
                    }
                    result
                }
                CodingShellEvent::ToggleBottomDock => {
                    let result = self
                        .app
                        .with_coding_state(|coding| coding.toggle_bottom_dock());
                    if result.is_ok() {
                        let _ = self.app.persist_coding_editor_workspace();
                    }
                    result
                }
                CodingShellEvent::SplitVertical => self
                    .app
                    .split_coding_editor(SplitDirection::Vertical)
                    .map(|_| ()),
                CodingShellEvent::SplitHorizontal => self
                    .app
                    .split_coding_editor(SplitDirection::Horizontal)
                    .map(|_| ()),
                CodingShellEvent::ClosePane(pane_id) => self.app.close_coding_editor_pane(&pane_id),
                CodingShellEvent::FocusPane(pane_id) => self.app.focus_coding_editor_pane(&pane_id),
                CodingShellEvent::MoveTab {
                    from_pane,
                    path,
                    to_pane,
                    index,
                } => self
                    .app
                    .move_coding_editor_tab(&from_pane, &path, &to_pane, index),
                CodingShellEvent::ResizeSplit { node_path, sizes } => {
                    self.app.resize_coding_editor_split(&node_path, sizes)
                }
                CodingShellEvent::SetExplorerWidth { width, commit } => {
                    let result = self
                        .app
                        .with_coding_state(|coding| coding.set_explorer_width(width));
                    if commit && result.is_ok() {
                        let _ = self.app.persist_coding_editor_workspace();
                    }
                    result
                }
                CodingShellEvent::SetExplorerVisible { visible, commit } => {
                    let result = self
                        .app
                        .with_coding_state(|coding| coding.set_explorer_visible(visible));
                    if commit && result.is_ok() {
                        let _ = self.app.persist_coding_editor_workspace();
                    }
                    result
                }
                CodingShellEvent::SetBottomPanelHeight { height, commit } => {
                    let result = self
                        .app
                        .with_coding_state(|coding| coding.set_bottom_panel_height(height));
                    if commit && result.is_ok() {
                        let _ = self.app.persist_coding_editor_workspace();
                    }
                    result
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
                CodingShellEvent::TerminalCreate { title } => {
                    self.app.create_coding_terminal(title)
                }
                CodingShellEvent::TerminalSelect { session_id } => {
                    self.app.select_coding_terminal(&session_id)
                }
                CodingShellEvent::TerminalRename { session_id, title } => {
                    self.app.rename_coding_terminal(&session_id, &title)
                }
                CodingShellEvent::TerminalKill { session_id } => {
                    self.app.kill_coding_terminal(&session_id)
                }
                CodingShellEvent::GitRefresh => self.app.refresh_coding_git(),
                CodingShellEvent::GitStage { paths } => self.app.coding_git_stage(&paths),
                CodingShellEvent::GitUnstage { paths } => self.app.coding_git_unstage(&paths),
                CodingShellEvent::GitDiscardRequest { paths } => {
                    self.app.coding_git_request_discard(&paths)
                }
                CodingShellEvent::GitDiscardConfirm => self.app.coding_git_confirm_discard(None),
                CodingShellEvent::GitDiscardCancel => self.app.coding_git_cancel_discard(),
                CodingShellEvent::GitCommitMessage(message) => {
                    self.app.set_coding_git_commit_message(message)
                }
                CodingShellEvent::GitCommit => self.app.coding_git_commit_active(),
                CodingShellEvent::UpdateSearchPanel {
                    query,
                    replace_text,
                    use_regex,
                    case_sensitive,
                    whole_word,
                    filename_only,
                } => self.app.with_coding_state(|coding| {
                    if let Some(query) = query {
                        coding.search.query = query;
                    }
                    if let Some(replace_text) = replace_text {
                        coding.search.replace_text = replace_text;
                    }
                    if let Some(use_regex) = use_regex {
                        coding.search.use_regex = use_regex;
                    }
                    if let Some(case_sensitive) = case_sensitive {
                        coding.search.case_sensitive = case_sensitive;
                    }
                    if let Some(whole_word) = whole_word {
                        coding.search.whole_word = whole_word;
                    }
                    if let Some(filename_only) = filename_only {
                        coding.search.filename_only = filename_only;
                    }
                }),
                CodingShellEvent::RunSearch => self.app.run_coding_search_from_panel(),
                CodingShellEvent::ReplaceAll => {
                    let panel = self.app.with_coding_state(|coding| coding.search.clone());
                    match panel {
                        Ok(panel) => {
                            let query = panel.query.trim().to_string();
                            if query.is_empty() {
                                Ok(())
                            } else {
                                let mut request = jaymi_core::SearchRequest::free_text(query)
                                    .with_case_sensitive(panel.case_sensitive)
                                    .with_whole_word(panel.whole_word)
                                    .with_regex(panel.use_regex);
                                request.limit = Some(500);
                                if let Ok(Some(root)) = self.app.with_coding_state(|coding| {
                                    coding.explorer.project_root.clone()
                                }) {
                                    request.folder = Some(std::path::PathBuf::from(root));
                                }
                                self.app
                                    .replace_in_search_results(request, &panel.replace_text)
                                    .and_then(|count| {
                                        self.app.with_coding_state(|coding| {
                                            coding.search.status =
                                                format!("Replaced {count} match(es)");
                                        })
                                    })
                                    .and_then(|_| self.app.run_coding_search_from_panel())
                            }
                        }
                        Err(error) => Err(error),
                    }
                }
                CodingShellEvent::OpenSearchResult { path, line, column } => {
                    self.app.open_search_result(&path, line, column)
                }
                CodingShellEvent::OpenProblem { path, line, column } => {
                    self.app.open_search_result(&path, line, column)
                }
                CodingShellEvent::ProblemsRefresh => self.app.refresh_coding_problems(),
                CodingShellEvent::RevealInExplorer { path, is_dir } => {
                    self.app.with_coding_state(|coding| {
                        crate::apply_breadcrumb_reveal(coding, &path, is_dir);
                    })
                }
                CodingShellEvent::QuickAction(action) => {
                    match dispatch_quick_action(action) {
                        QuickActionEffect::InsertPlannerPrompt(text) => {
                            self.prompt = text.to_string();
                            self.focus_composer = true;
                            Ok(())
                        }
                        QuickActionEffect::OpenSearchPanel => {
                            let result = self.app.with_coding_state(|coding| {
                                coding.show_bottom_tab(CodingBottomTab::Search);
                            });
                            if result.is_ok() {
                                let _ = self.app.persist_coding_editor_workspace();
                            }
                            result
                        }
                        QuickActionEffect::OpenTerminalPanel
                        | QuickActionEffect::FocusTerminalPanel => {
                            let result = self.app.with_coding_state(|coding| {
                                coding.show_bottom_tab(CodingBottomTab::Terminal);
                            });
                            if result.is_ok() {
                                let _ = self.app.persist_coding_editor_workspace();
                            }
                            result
                        }
                        QuickActionEffect::FocusGitPanel => {
                            let result = self.app.with_coding_state(|coding| {
                                coding.show_bottom_tab(CodingBottomTab::Git);
                            });
                            if result.is_ok() {
                                let _ = self.app.persist_coding_editor_workspace();
                            }
                            result
                        }
                    }
                }
            };
            if let Err(error) = result {
                self.error = Some(error.message().to_string());
            }
        }
        if let Ok(session) = self.app.experience() {
            self.experience = session;
        }
    }

    fn handle_explorer_events(&mut self, events: Vec<ExplorerEvent>) {
        for event in events {
            let result = match event {
                ExplorerEvent::OpenProject => {
                    self.open_project_folder();
                    Ok(())
                }
                ExplorerEvent::ToggleExpand(path) => self.app.toggle_coding_expand(&path),
                ExplorerEvent::Select { path, is_dir } => {
                    self.app.select_coding_path(&path, is_dir).and_then(|_| {
                        if is_dir {
                            Ok(())
                        } else {
                            // Open as a permanent tab so each file stays switchable.
                            self.app.open_coding_file(&path)
                        }
                    })
                }
                ExplorerEvent::Open(path) => self.app.open_coding_file(&path),
                ExplorerEvent::BeginNewFile { parent } => self.app.begin_coding_new_file(&parent),
                ExplorerEvent::BeginNewFolder { parent } => {
                    self.app.begin_coding_new_folder(&parent)
                }
                ExplorerEvent::BeginRename { path, name } => {
                    self.app.begin_coding_rename(&path, &name)
                }
                ExplorerEvent::PendingNameChanged(name) => {
                    self.app.set_coding_explorer_pending_name(name)
                }
                ExplorerEvent::ConfirmPending => self.app.confirm_coding_explorer_pending(),
                ExplorerEvent::CancelPending => self.app.cancel_coding_explorer_pending(),
                ExplorerEvent::Delete(path) => self.app.delete_coding_path(&path),
                ExplorerEvent::Reveal(path) => self.app.reveal_in_file_manager(&path),
                ExplorerEvent::Refresh => self.app.refresh_coding_explorer(),
            };
            if let Err(error) = result {
                self.error = Some(error.message().to_string());
            }
        }
        if let Ok(session) = self.app.experience() {
            self.experience = session;
        }
    }

    fn render_workspace(&mut self, ui: &mut egui::Ui) -> Option<MonacoEditorSurface> {
        let workspace = self.experience.active_workspace().cloned()?;

        if workspace.kind == WorkspaceKind::Coding {
            let coding = self
                .experience
                .capability_state()
                .and_then(|state| state.coding())
                .cloned();
            let diagnostics = self.app.coding_diagnostics_view().ok();
            let mut events = Vec::new();
            let mut explorer_events = Vec::new();
            let mut monaco_surface = None;
            let open_error = self.monaco_last_error.clone();
            render_coding_shell(
                ui,
                &self.theme,
                &workspace,
                coding.as_ref(),
                diagnostics.as_ref(),
                &mut events,
                &mut monaco_surface,
                open_error.as_deref(),
                |ui, state| {
                    let dirty_paths: std::collections::BTreeSet<String> = state
                        .editors
                        .sessions()
                        .into_iter()
                        .filter(|session| session.dirty)
                        .map(|session| session.path)
                        .collect();
                    explorer::render_explorer(
                        ui,
                        &self.theme,
                        &state.explorer,
                        state.active_tab_path(),
                        &dirty_paths,
                        &mut explorer_events,
                    );
                },
            );
            let had_surface = monaco_surface.is_some();
            self.handle_coding_events(events);
            self.handle_explorer_events(explorer_events);
            // Selecting a file updates CodingState after the shell paints; repaint
            // so tabs and Monaco mount against the newly opened file next frame.
            if !had_surface
                && self
                    .experience
                    .capability_state()
                    .and_then(|state| state.coding())
                    .is_some_and(|coding| !coding.editors.is_empty())
            {
                ui.ctx().request_repaint();
            }
            // Close workspace lives in the app top bar; do not stack widgets under
            // the Coding CentralPanel (it already fills the remaining region).
            return monaco_surface;
        }

        ui.label(
            egui::RichText::new(workspace.title())
                .strong()
                .size(type_size::TITLE)
                .color(self.theme.text_primary),
        );
        ui.add_space(space::XS);
        ui.label(
            egui::RichText::new("This workspace is not ready yet.")
                .size(type_size::BODY)
                .color(self.theme.text_secondary),
        );
        ui.add_space(space::SM);
        ui.label(
            egui::RichText::new(
                "Closing returns you to the conversation without losing chat history.",
            )
            .size(type_size::META)
            .color(self.theme.text_secondary),
        );
        ui.add_space(space::MD);
        if ui
            .add(
                egui::Button::new(
                    egui::RichText::new("Close Workspace")
                        .size(type_size::UI)
                        .color(self.theme.on_accent()),
                )
                .fill(self.theme.accent)
                .corner_radius(radius::MD)
                .stroke(egui::Stroke::NONE),
            )
            .clicked()
        {
            self.close_workspace();
        }
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
            if let Some(host) = self.monaco.as_mut() {
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
        // Monaco only ever overlays the focused pane's active tab (see
        // `monaco_document_from_state`), so every IPC message it sends
        // originates from that pane.
        let focused_pane = self
            .experience
            .capability_state()
            .and_then(|state| state.coding())
            .map(|coding| coding.editors.focused_pane.as_str().to_string())
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
                    events.push(CodingShellEvent::EditContent {
                        pane: focused_pane.clone(),
                        path,
                        content,
                    });
                }
                MonacoIpcMessage::Scroll { path, offset } => {
                    if let Some(host) = self.monaco.as_mut() {
                        host.note_external_scroll(&path, offset);
                    }
                    events.push(CodingShellEvent::Scroll {
                        pane: focused_pane.clone(),
                        path,
                        offset,
                    });
                }
                MonacoIpcMessage::Cursor { path, line, column } => {
                    if let Some(host) = self.monaco.as_mut() {
                        host.note_external_cursor(&path, line, column);
                    }
                    events.push(CodingShellEvent::SetCursor {
                        pane: focused_pane.clone(),
                        path,
                        line,
                        column,
                    });
                }
                MonacoIpcMessage::Folds { path, regions } => {
                    if let Some(host) = self.monaco.as_mut() {
                        host.note_external_folds(&path, &regions);
                    }
                    events.push(CodingShellEvent::SetFolds {
                        pane: focused_pane.clone(),
                        path,
                        regions,
                    });
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
            let payload =
                self.handle_monaco_lsp(&method, &path, line, character, new_name.as_deref());
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
        // Native WKWebView paints above egui — hide it while modal overlays own
        // the foreground so Quick Open / Command Palette are not covered by code.
        let overlay_blocking = self.command_palette.is_open();
        let document = surface
            .filter(|_| !overlay_blocking)
            .map(|surface| self.monaco_document_from_state(surface));
        let surface = surface.filter(|_| !overlay_blocking);
        let theme_id = self.theme.monaco_theme_id().to_string();
        let definition = self.theme.monaco_definition_json();
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
        if let Err(error) = host.set_theme(&theme_id, &definition) {
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

    /// Markers for Monaco, sourced from the aggregated Problems panel when it
    /// has been populated (via `refresh_coding_problems`), falling back to the
    /// raw LSP working set otherwise.
    fn monaco_diagnostic_markers(&self, path: &str) -> String {
        let markers = self
            .experience
            .capability_state()
            .and_then(|state| state.coding())
            .map(|coding| {
                if !coding.problems.is_empty() {
                    coding
                        .problems
                        .iter()
                        .filter(|issue| issue.path.as_deref() == Some(path))
                        .map(|issue| {
                            serde_json::json!({
                                "message": issue.message,
                                "severity": issue.severity.as_str(),
                                "line": issue.line.unwrap_or(0),
                                "character": issue.column.unwrap_or(0),
                                "endLine": issue.end_line.unwrap_or(issue.line.unwrap_or(0)),
                                "endCharacter": issue.end_column.unwrap_or(issue.column.unwrap_or(0) + 1),
                            })
                        })
                        .collect::<Vec<_>>()
                } else {
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
                }
            })
            .unwrap_or_default();
        serde_json::to_string(&markers).unwrap_or_else(|_| "[]".to_string())
    }

    /// Prefer live CodingState so Monaco IPC edits aren't overwritten by a stale surface.
    fn monaco_document_from_state(&self, surface: &MonacoEditorSurface) -> MonacoDocument {
        let Some(coding) = self
            .experience
            .capability_state()
            .and_then(|state| state.coding())
        else {
            return surface.document.clone();
        };
        let Some(session) = coding.editors.active_session() else {
            return surface.document.clone();
        };
        let settings = &coding.editor_settings;

        MonacoDocument {
            path: session.path.clone(),
            content: session.content.clone(),
            language: language_for_path(&session.path).to_string(),
            scroll_top: session.view.scroll_top,
            cursor_line: session.view.cursor.line,
            cursor_column: session.view.cursor.column,
            folded_regions: session
                .view
                .folded_regions
                .iter()
                .map(|region| (region.start_line, region.end_line))
                .collect(),
            minimap: settings.minimap,
            word_wrap: settings.word_wrap,
            font_size: settings.font_size,
        }
    }

    fn update_editor_setting(
        &mut self,
        update: impl FnOnce(&mut EditorSettings),
    ) -> Result<(), jaymi_core::JaymiError> {
        let settings = self.app.with_coding_state(|coding| {
            update(&mut coding.editor_settings);
            coding.editor_settings.clone()
        })?;
        let _ = self.app.persist_coding_editor_workspace();
        if let Some(host) = &self.monaco {
            let _ = host.set_editor_options(
                Some(settings.minimap),
                Some(settings.word_wrap),
                Some(settings.font_size),
            );
        }
        Ok(())
    }

    fn send_prompt(&mut self) {
        let prompt = self.prompt.trim().to_string();
        if prompt.is_empty() {
            return;
        }
        self.prompt.clear();
        self.awaiting_reply = true;
        match self.app.handle_with_workspace(UserRequest::new(prompt)) {
            Ok(_) => {
                self.error = None;
                self.status = None;
                if let Ok(session) = self.app.experience() {
                    self.experience = session;
                }
            }
            Err(error) => {
                self.status = None;
                self.error = Some(error.message().to_string());
            }
        }
        self.awaiting_reply = false;
    }

    /// Apply a Review Card button: record intent, then Planner pause/resume.
    ///
    /// Approve resumes the paused plan without replanning. Modify regenerates
    /// affected steps into a child plan (re-paused for approval). Cancel drops
    /// the pause. The card itself never executes tools — the Planner does.
    fn handle_review_intent(&mut self, intent: ReviewIntent) {
        if let ReviewIntent::Modify { plan_id, .. } = &intent {
            self.review_modify_notes.remove(plan_id.as_str());
        }
        match self.app.communicate_review_intent(intent) {
            Ok(response) => {
                self.error = None;
                self.status = Some(if response.awaiting_review {
                    if response
                        .execution_plan
                        .as_ref()
                        .map(|plan| plan.revision() > 1)
                        .unwrap_or(false)
                    {
                        "Plan revised — review the changes before approval.".into()
                    } else {
                        "Still awaiting review.".into()
                    }
                } else if response
                    .execution_plan
                    .as_ref()
                    .map(|plan| plan.status() == jaymi_planner::ExecutionStatus::Completed)
                    .unwrap_or(false)
                {
                    "Resumed paused plan — execution completed.".into()
                } else if response
                    .execution_plan
                    .as_ref()
                    .map(|plan| plan.status() == jaymi_planner::ExecutionStatus::Cancelled)
                    .unwrap_or(false)
                {
                    "Paused plan cancelled.".into()
                } else {
                    response.content.chars().take(120).collect()
                });
                if let Ok(session) = self.app.experience() {
                    self.experience = session;
                }
            }
            Err(error) => {
                self.status = None;
                self.error = Some(error.message().to_string());
            }
        }
    }

    fn close_workspace(&mut self) {
        if let Some(host) = self.monaco.as_mut() {
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
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Developer Diagnostics")
                    .strong()
                    .size(type_size::TITLE)
                    .color(self.theme.text_primary),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new("Hide")
                                .size(type_size::UI)
                                .color(self.theme.text_secondary),
                        )
                        .frame(false),
                    )
                    .clicked()
                {
                    self.show_diagnostics = false;
                }
            });
        });
        ui.add_space(space::SM);
        ui.label(
            egui::RichText::new(format!("App state: {}", self.snapshot.app_state.label()))
                .size(type_size::UI)
                .color(self.theme.text_secondary),
        );

        // Execution inspection — why plans are paused / resumed.
        if let Ok(view) = self.app.coding_diagnostics_view() {
            let execution_titles: std::collections::HashSet<&str> =
                crate::execution_diagnostics::EXECUTION_INSPECTION_SECTION_TITLES
                    .iter()
                    .copied()
                    .collect();
            let execution_sections: Vec<_> = view
                .sections
                .iter()
                .filter(|section| execution_titles.contains(section.title.as_str()))
                .collect();
            if !execution_sections.is_empty() {
                ui.add_space(space::MD);
                ui.label(
                    egui::RichText::new("Execution inspection")
                        .strong()
                        .size(type_size::UI)
                        .color(self.theme.text_primary),
                );
                ui.add_space(space::XS);
                ui.label(
                    egui::RichText::new(
                        "Developer view of Execution Plans, review gates, pause/resume, and approvals.",
                    )
                    .size(type_size::META)
                    .color(self.theme.text_secondary),
                );
                for section in execution_sections {
                    ui.add_space(space::SM);
                    ui.label(
                        egui::RichText::new(&section.title)
                            .strong()
                            .size(type_size::META)
                            .color(self.theme.text_primary),
                    );
                    for line in &section.lines {
                        ui.label(
                            egui::RichText::new(line)
                                .size(type_size::META)
                                .color(self.theme.text_secondary),
                        );
                    }
                }
            }
        }

        ui.add_space(space::SM);
        egui::Grid::new("subsystem_statuses")
            .striped(true)
            .num_columns(3)
            .spacing([space::MD, space::SM])
            .min_col_width(120.0)
            .show(ui, |ui| {
                ui.strong("Subsystem");
                ui.strong("Status");
                ui.strong("Detail");
                ui.end_row();
                for row in &self.snapshot.subsystems {
                    ui.label(&row.name);
                    ui.colored_label(status_color(&self.theme, row.status), row.status.label());
                    ui.label(&row.detail);
                    ui.end_row();
                }
            });

        if let Some(inspector) = &self.snapshot.capability_inspector {
            ui.add_space(space::MD);
            ui.label(
                egui::RichText::new("Capability Inspector")
                    .strong()
                    .size(type_size::UI)
                    .color(self.theme.text_primary),
            );
            ui.add_space(space::XS);
            ui.label(
                egui::RichText::new(inspector.summary())
                    .size(type_size::META)
                    .color(self.theme.text_secondary),
            );
            ui.add_space(space::SM);
            egui::Grid::new("capability_inspector")
                .striped(true)
                .num_columns(5)
                .spacing([space::MD, space::XS])
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

        if let Some(inspector) = &self.snapshot.context_inspector {
            ui.add_space(space::MD);
            ui.label(
                egui::RichText::new("Context Inspector")
                    .strong()
                    .size(type_size::UI)
                    .color(self.theme.text_primary),
            );
            ui.add_space(space::XS);
            ui.label(
                egui::RichText::new(
                    "Pipeline (Current): Intent → Capability → Context Policy → Providers → Bundle → Action Policy → Permission → Tool",
                )
                .size(type_size::META)
                .color(self.theme.text_secondary),
            );
            ui.add_space(space::XS);
            ui.label(
                egui::RichText::new(inspector.summary())
                    .size(type_size::META)
                    .color(self.theme.text_secondary),
            );
            ui.add_space(space::XS);
            ui.label(
                egui::RichText::new(format!("request: {}", inspector.request_preview))
                    .size(type_size::META)
                    .color(self.theme.text_secondary),
            );
            ui.add_space(space::XS);
            ui.label(
                egui::RichText::new(format!(
                    "cache={} · duration_ms={} · final_bundle={} chars (≈{} tok) · order=[{}]",
                    inspector.cache_status(),
                    inspector.duration_ms,
                    inspector.bundle_size_characters,
                    inspector.bundle_size_estimated_tokens,
                    inspector.contributor_order.join(", ")
                ))
                .size(type_size::META)
                .color(self.theme.text_secondary),
            );
            ui.add_space(space::SM);
            egui::Grid::new("context_inspector_providers")
                .striped(true)
                .num_columns(9)
                .spacing([space::MD, space::XS])
                .min_col_width(48.0)
                .show(ui, |ui| {
                    ui.strong("Eval");
                    ui.strong("Alloc");
                    ui.strong("Provider");
                    ui.strong("Outcome");
                    ui.strong("Rel");
                    ui.strong("Sens");
                    ui.strong("Approval");
                    ui.strong("Size");
                    ui.strong("Detail");
                    ui.end_row();
                    for provider in &inspector.providers {
                        let size = match &provider.outcome {
                            jaymi_context::ProviderInspectOutcome::Contributed {
                                characters,
                                truncated,
                                summarized,
                                ..
                            } => {
                                let mut label = format!("{characters} ch");
                                if *truncated {
                                    label.push_str(" trunc");
                                }
                                if *summarized {
                                    label.push_str(" sum");
                                }
                                label
                            }
                            _ => "—".into(),
                        };
                        ui.label(provider.evaluation_order.to_string());
                        ui.label(
                            provider
                                .allocation_order
                                .map(|order| order.to_string())
                                .unwrap_or_else(|| "—".into()),
                        );
                        ui.label(&provider.id);
                        ui.label(provider.outcome.as_str());
                        ui.label(provider.relevance.to_string());
                        ui.label(&provider.sensitivity);
                        ui.label(&provider.approval_status);
                        ui.label(size);
                        ui.label(provider.detail());
                        ui.end_row();
                    }
                });
            if let Some(budget) = &inspector.budget {
                ui.add_space(space::SM);
                ui.label(
                    egui::RichText::new(format!(
                        "Budget allocation: {} / {} chars (≈{} tok) · truncated=[{}] · skipped=[{}]",
                        budget.used_characters,
                        budget.max_characters,
                        budget.estimated_tokens,
                        budget.truncated_providers.join(", "),
                        budget.skipped_budget.join(", ")
                    ))
                    .size(type_size::META)
                    .color(self.theme.text_secondary),
                );
                for summary in &budget.summaries {
                    ui.label(
                        egui::RichText::new(format!("· {summary}"))
                            .size(type_size::META)
                            .color(self.theme.text_secondary),
                    );
                }
            }
            if let Some(policy) = &inspector.policy {
                ui.add_space(space::SM);
                ui.label(
                    egui::RichText::new("Context Policy")
                        .strong()
                        .size(type_size::UI)
                        .color(self.theme.text_primary),
                );
                ui.add_space(space::XS);
                ui.label(
                    egui::RichText::new(format!(
                        "active=[{}] · before={} · after={} · assembled={} chars",
                        policy.active_policies.join(","),
                        policy.size_before_characters,
                        policy.size_after_characters,
                        policy.size_assembled_characters
                    ))
                    .size(type_size::META)
                    .color(self.theme.text_secondary),
                );
                ui.label(
                    egui::RichText::new(format!(
                        "included=[{}] · excluded=[{}]",
                        policy.included_providers().join(","),
                        policy.excluded_providers().join(",")
                    ))
                    .size(type_size::META)
                    .color(self.theme.text_secondary),
                );
                ui.add_space(space::SM);
                egui::Grid::new("context_policy_decisions")
                    .striped(true)
                    .num_columns(6)
                    .spacing([space::MD, space::XS])
                    .min_col_width(60.0)
                    .show(ui, |ui| {
                        ui.strong("Provider");
                        ui.strong("Status");
                        ui.strong("Approval");
                        ui.strong("Sensitivity");
                        ui.strong("Constraints");
                        ui.strong("Reason");
                        ui.end_row();
                        for decision in &policy.decisions {
                            let status = if decision.included {
                                "Included"
                            } else if decision.approval_status == "pending" {
                                "Pending"
                            } else {
                                "Excluded"
                            };
                            ui.label(&decision.provider_id);
                            ui.label(status);
                            ui.label(&decision.approval_status);
                            ui.label(&decision.sensitivity);
                            ui.label(if decision.constraints.is_empty() {
                                "-".to_string()
                            } else {
                                decision.constraints.join(",")
                            });
                            let mut reason = decision.reason.clone();
                            if let Some(truncation) = &decision.truncation_reason {
                                reason.push_str(" · trunc=");
                                reason.push_str(truncation);
                            }
                            ui.label(reason);
                            ui.end_row();
                        }
                    });
            }
            if !inspector.sections.is_empty() {
                ui.add_space(space::SM);
                egui::Grid::new("context_inspector_sections")
                    .striped(true)
                    .num_columns(4)
                    .spacing([space::MD, space::XS])
                    .min_col_width(80.0)
                    .show(ui, |ui| {
                        ui.strong("Section");
                        ui.strong("Present");
                        ui.strong("Chars");
                        ui.strong("Detail");
                        ui.end_row();
                        for section in &inspector.sections {
                            ui.label(&section.name);
                            ui.label(if section.present { "yes" } else { "no" });
                            ui.label(section.characters.to_string());
                            ui.label(&section.detail);
                            ui.end_row();
                        }
                    });
            }
        }

        if !self.snapshot.context_history.is_empty() {
            ui.add_space(space::MD);
            ui.label(
                egui::RichText::new("Context History")
                    .strong()
                    .size(type_size::UI)
                    .color(self.theme.text_primary),
            );
            ui.add_space(space::XS);
            ui.label(
                egui::RichText::new(format!(
                    "{} recent assembles retained",
                    self.snapshot.context_history.len()
                ))
                .size(type_size::META)
                .color(self.theme.text_secondary),
            );
            ui.add_space(space::SM);
            egui::Grid::new("context_history_entries")
                .striped(true)
                .num_columns(6)
                .spacing([space::MD, space::XS])
                .min_col_width(60.0)
                .show(ui, |ui| {
                    ui.strong("Gen");
                    ui.strong("ms");
                    ui.strong("Size");
                    ui.strong("Providers");
                    ui.strong("Cache");
                    ui.strong("Request");
                    ui.end_row();
                    for entry in self.snapshot.context_history.iter().take(12) {
                        ui.label(entry.assemble_generation.to_string());
                        ui.label(entry.duration_ms.to_string());
                        ui.label(format!("{} ch", entry.bundle_size_characters));
                        ui.label(entry.providers_used.join(","));
                        ui.label(if entry.cache_hit { "hit" } else { "miss" });
                        ui.label(&entry.request);
                        ui.end_row();
                    }
                });
        }

        ui.add_space(space::MD);
        ui.label(
            egui::RichText::new("List Directory")
                .strong()
                .size(type_size::UI)
                .color(self.theme.text_primary),
        );
        ui.add_space(space::SM);
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

        ui.add_space(space::SM);
        ui.label(
            egui::RichText::new("Read File")
                .strong()
                .size(type_size::UI)
                .color(self.theme.text_primary),
        );
        ui.add_space(space::SM);
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
