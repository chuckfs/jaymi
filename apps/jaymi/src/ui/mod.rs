//! Conversation-first desktop experience.
//!
//! The conversation stays visible. Capabilities may expand a workspace from
//! the right. Closing the workspace never destroys the conversation.

pub mod components;
pub mod explorer;
pub mod icons;
pub mod nav_rail;
pub mod review_card;

use std::collections::HashMap;
use std::time::SystemTime;

use eframe::egui;

use crate::boot::{Application, BeginGeneration, PumpGeneration};
use crate::coding_quick_actions::{dispatch_quick_action, QuickActionEffect};
use crate::coding_workspace::{render_coding_shell, CodingShellEvent, MonacoEditorSurface};
use crate::command_dispatch::{dispatch_command, CommandDispatchEffect};
use crate::command_palette::{
    gather_palette_items, render_command_palette, CommandPaletteOutcome, CommandPaletteState,
    PaletteAction, PaletteCommandRef, PaletteGatherInput,
};
use crate::conversation_ux::{
    action_accessibility_label, caret_blink_on, display_content, loading_opacity,
    progress_accessibility_label, show_typing_indicator, turn_actions,
};
use crate::diagnostics::{DiagnosticsSnapshot, OperationalStatus};
use crate::creation_workspace::render_creation_workspace;
use crate::experience::ExperienceSession;
use crate::knowledge_workspace::{
    render_knowledge_workspace, KnowledgeWorkspaceContext, KnowledgeWorkspaceEvent,
    KnowledgeWorkspaceState,
};
use crate::research_workspace::render_research_workspace;
use crate::monaco_host::{
    language_for_path, resolve_monaco_assets, MonacoDocument, MonacoHost, MonacoIpcMessage,
};
use crate::settings_workspace::{
    render_settings_workspace, ReasoningSettingsSnapshot, SettingsCategory, SettingsWorkspaceContext,
    SettingsWorkspaceEvent, SettingsWorkspaceState,
};
use crate::theme::{inset, radius, space, stroke, type_size, Theme};
use crate::ui::components::{icon_pill_button, pulse_alpha, suggestion_chip};
use crate::ui::explorer::ExplorerEvent;
use crate::ui::icons::Icon;
use crate::ui::nav_rail::{
    render_nav_rail, NavRailContext, NavRailEvent, DEFAULT_NAV_WIDTH, MAX_NAV_WIDTH, MIN_NAV_WIDTH,
};
use crate::ui::review_card::render_review_card;
use jaymi_capabilities::{
    workspace_expansion_for, Capability, CodingBottomTab, EditorSelection, EditorSettings,
    FoldedRegion, SplitDirection, WorkspaceKind, DEFAULT_CONVERSATION_FRACTION,
    DEFAULT_WORKSPACE_PANEL_WIDTH, MAX_CONVERSATION_FRACTION, MAX_WORKSPACE_PANEL_WIDTH,
    MIN_CONVERSATION_WIDTH, MIN_WORKSPACE_PANEL_WIDTH,
};
use jaymi_config::{Config, Theme as ThemePreference};
use std::sync::{Arc, Mutex};
use jaymi_memory::{ConversationMeta, CreateConversationRequest, MessageRole};
use jaymi_planner::ReviewIntent;

/// Launch the conversation-first desktop window.
pub fn run_diagnostics(
    app: Application,
    initial_list_path: String,
    initial_read_path: String,
    initial_snapshot: DiagnosticsSnapshot,
) -> eframe::Result<()> {
    let app = Arc::new(app);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 820.0])
            .with_title("Jaymi")
            // Unified title bar: content paints behind the (now transparent)
            // title bar and the app's own top bar takes over its space, but
            // the native traffic-light buttons stay put — real close/
            // minimize/zoom, not hand-painted. `render_top_bar` reserves
            // `TRAFFIC_LIGHT_INSET` of left padding so nothing sits under them.
            .with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_title_shown(false),
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
                .resolve::<Arc<Mutex<Config>>>()
                .ok()
                .and_then(|config| {
                    config
                        .lock()
                        .ok()
                        .map(|guard| guard.settings().theme)
                })
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
                review_preview_expanded: HashMap::new(),
                experience,
                show_diagnostics: false,
                error: None,
                status: None,
                monaco: None,
                monaco_last_error: None,
                egui_wants_keyboard: false,
                pending_terminal_focus: None,
                command_palette: CommandPaletteState::default(),
                workspace_was_expanded: false,
                workspace_anim_start: None,
                workspace_anim_from: MIN_WORKSPACE_PANEL_WIDTH,
                workspace_anim_target: DEFAULT_WORKSPACE_PANEL_WIDTH,
                awaiting_reply: false,
                loading_started_at: None,
                last_clipboard: None,
                theme,
                nav_open: false,
                settings_open: false,
                nav_was_open: false,
                nav_anim_start: None,
                nav_anim_from: 0.0,
                nav_anim_target: DEFAULT_NAV_WIDTH,
                nav_width: DEFAULT_NAV_WIDTH,
                settings: SettingsWorkspaceState {
                    category: SettingsCategory::Reasoning,
                },
                reasoning_settings: ReasoningSettingsSnapshot::default(),
                settings_busy: false,
                knowledge: KnowledgeWorkspaceState::default(),
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

/// Stop generation control — a small square on the accent pill (mirrors the
/// send affordance's shape, distinguished by glyph only).
fn paint_stop_button(ui: &mut egui::Ui, theme: &Theme) -> egui::Response {
    let size = egui::vec2(32.0, 32.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let fill = if response.hovered() {
        theme.accent.gamma_multiply(0.92)
    } else {
        theme.accent
    };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(radius::PILL as u8), fill);
    let inset = 10.0;
    let stop = egui::Rect::from_center_size(rect.center(), egui::vec2(inset, inset));
    ui.painter()
        .rect_filled(stop, egui::CornerRadius::same(2), theme.on_accent());
    response
}

/// Text chip in the composer toolbar (`⌘P`).
fn composer_chip(ui: &mut egui::Ui, theme: &Theme, label: &str) -> egui::Response {
    let galley = ui.fonts(|f| {
        f.layout_no_wrap(
            label.to_string(),
            egui::FontId::proportional(type_size::META),
            theme.text_secondary,
        )
    });
    let pad_x = space::SM + 2.0;
    let size = egui::vec2(galley.size().x + pad_x * 2.0, 28.0);
    let (rect, mut response) = ui.allocate_exact_size(size, egui::Sense::click());
    response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let hovered = response.hovered();
    ui.painter().rect_filled(
        rect,
        egui::CornerRadius::same(radius::PILL as u8),
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

/// Organic system type: Figtree is the default proportional body/UI font;
/// Caprasimo is a separate named family reserved for display headings (see
/// [`crate::theme::display_font`]). Both are embedded at compile time so the
/// shell never depends on what happens to be installed on the machine.
///
/// System UI fonts are still registered as a fallback *after* Figtree, purely
/// for glyph coverage Figtree doesn't carry (CJK, extended symbols) — they
/// never win priority over the brand type.
fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    fonts.font_data.insert(
        "figtree".into(),
        egui::FontData::from_static(include_bytes!("../../assets/fonts/Figtree-Regular.ttf"))
            .into(),
    );
    fonts.font_data.insert(
        "figtree-semibold".into(),
        egui::FontData::from_static(include_bytes!("../../assets/fonts/Figtree-SemiBold.ttf"))
            .into(),
    );
    fonts.font_data.insert(
        "figtree-bold".into(),
        egui::FontData::from_static(include_bytes!("../../assets/fonts/Figtree-Bold.ttf")).into(),
    );
    fonts.font_data.insert(
        crate::theme::font::DISPLAY.into(),
        egui::FontData::from_static(include_bytes!("../../assets/fonts/Caprasimo-Regular.ttf"))
            .into(),
    );

    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, "figtree".into());
    fonts.families.insert(
        egui::FontFamily::Name("figtree-semibold".into()),
        vec!["figtree-semibold".into(), "figtree".into()],
    );
    fonts.families.insert(
        egui::FontFamily::Name("figtree-bold".into()),
        vec!["figtree-bold".into(), "figtree".into()],
    );
    fonts.families.insert(
        egui::FontFamily::Name(crate::theme::font::DISPLAY.into()),
        vec![crate::theme::font::DISPLAY.into(), "figtree".into()],
    );

    // Fallback OS fonts, appended after Figtree for extra glyph coverage only.
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
                    .push(name.into());
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
                .push("segoe_ui".into());
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
                    .push("dejavu".into());
                break;
            }
        }
    }
    ctx.set_fonts(fonts);
}

struct JaymiApp {
    app: Arc<Application>,
    snapshot: DiagnosticsSnapshot,
    list_path_input: String,
    read_path_input: String,
    prompt: String,
    /// Request focus on the conversation composer after a Quick Action insert.
    focus_composer: bool,
    /// Per-plan draft notes for Review Card Modify guidance.
    review_modify_notes: HashMap<String, String>,
    /// Per-plan Preview Before Action expansion state.
    review_preview_expanded: HashMap<String, bool>,
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
    /// When set, Monaco must resign first-responder so egui TextEdit receives keys.
    egui_wants_keyboard: bool,
    /// Focus the terminal command field for this session id on the next paint.
    pending_terminal_focus: Option<String>,
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
    /// True while waiting for Jaymi to respond (typing indicator / Stop affordance).
    awaiting_reply: bool,
    /// When the current loading indicator started (opacity transition).
    loading_started_at: Option<std::time::Instant>,
    /// Last text copied to the clipboard (tests / status).
    last_clipboard: Option<String>,
    /// Active application theme (drives egui visuals + Monaco Jaymi themes).
    theme: Theme,
    /// Whether the left navigation rail is open (or animating open).
    nav_open: bool,
    /// Whether the Settings panel is the active right-side panel (mutually
    /// exclusive with an expanded capability workspace — both use the same
    /// slide-out `SidePanel`).
    settings_open: bool,
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
    /// Settings Workspace UI selection (preferences only).
    settings: SettingsWorkspaceState,
    /// Cached Reasoning settings snapshot from Application.
    reasoning_settings: ReasoningSettingsSnapshot,
    /// True while Settings refresh / test is in flight.
    settings_busy: bool,
    /// Knowledge Workspace UI selection (search box, selected vault/hit).
    knowledge: KnowledgeWorkspaceState,
}

/// Duration of the workspace expand-in / nav-rail slide animation — 320ms,
/// matching the design system's panel-motion spec (paired with
/// [`ease_out_cubic`], which approximates its cubic-bezier(.32,.72,0,1)).
const WORKSPACE_EXPAND_ANIM_SECS: f32 = 0.32;

/// Left padding so the top bar's own content never sits under macOS's
/// native traffic-light buttons (the window uses `with_fullsize_content_view`
/// so the (now-transparent) title bar area is ours to paint into, but the
/// traffic lights themselves stay put at their standard inset).
const TRAFFIC_LIGHT_INSET: f32 = 56.0;

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

/// Day + clock label for conversation timestamp separators (UTC).
fn format_day_separator(created_at: i64) -> String {
    let today = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64 / 86_400)
        .unwrap_or(0);
    let day = created_at.div_euclid(86_400);
    let day_label = match today - day {
        0 => "Today".to_string(),
        1 => "Yesterday".to_string(),
        delta if delta > 1 && delta < 7 => format!("{delta} days ago"),
        _ => {
            // YYYY-MM-DD from UTC seconds (stable, no extra deps).
            let days = created_at.div_euclid(86_400);
            let (year, month, day) = civil_from_days(days);
            format!("{year:04}-{month:02}-{day:02}")
        }
    };
    format!("{day_label} · {}", format_message_time(created_at))
}

/// Clock time for the day separator (UTC HH:MM — presentation stamp).
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
        self.pump_active_generation(ctx);
        let _ = self.app.pump_context_maintenance();

        if let Ok(session) = self.app.experience() {
            self.experience = session;
        }

        // Escape cancels an in-flight generation (Conversation First).
        if self.awaiting_reply
            && ctx.input(|input| input.key_pressed(egui::Key::Escape))
        {
            let _ = self.app.cancel_generation();
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
            .exact_height(52.0)
            .show_separator_line(false)
            .frame(
                egui::Frame::new()
                    .fill(self.theme.background)
                    .inner_margin(inset(space::LG, space::SM))
                    .stroke(egui::Stroke::NONE),
            )
            .show(ctx, |ui| {
                self.render_top_bar(ui);
            });

        self.render_nav_side_panel(ctx);

        let right_panel_open = self.experience.workspace_expanded() || self.settings_open;
        if right_panel_open {
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
                if self.settings_open {
                    self.render_settings_surface(ui);
                } else {
                    monaco_surface = self.render_workspace(ui);
                }
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

        // Conversation column — always present; Settings/Coding/Research/
        // Knowledge/Creation all expand beside it from the right, never
        // replace it (Conversation First).
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
            .theme_preference()
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
            Ok(CommandDispatchEffect::OpenSettings) => {
                self.open_settings_workspace();
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
        let conversation_state = self.experience.conversation_state();
        let has_streaming = self.experience.has_streaming_turn();
        let show_progress = self.awaiting_reply
            || show_typing_indicator(conversation_state, has_streaming)
            || matches!(
                conversation_state,
                jaymi_planner::ConversationState::WaitingForReview
            );
        let empty = turns.is_empty() && !show_progress;
        let mut review_intent: Option<ReviewIntent> = None;
        let mut copy_index: Option<usize> = None;
        let mut retry_index: Option<usize> = None;
        let mut regenerate_index: Option<usize> = None;
        let caret_on = caret_blink_on(ui.input(|input| input.time));
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
                            // Fixed conversation column width so user bubbles can
                            // Align::Max against the right edge; right LG inset
                            // tracks workspace growth (CentralPanel shrinks).
                            let column_width = (ui.available_width() - space::LG).max(120.0);
                            ui.set_min_width(column_width);
                            ui.set_max_width(column_width);
                            let mut last_day: Option<i64> = None;
                            for (index, turn) in turns.iter().enumerate() {
                                let day = turn.created_at.div_euclid(86_400);
                                if last_day != Some(day) {
                                    self.render_timestamp_separator(ui, turn.created_at);
                                    last_day = Some(day);
                                }
                                let body = display_content(turn, caret_on);
                                self.render_chat_bubble(
                                    ui,
                                    turn.role,
                                    &body,
                                    turn.is_streaming(),
                                );
                                if let Some(review) = &turn.review {
                                    ui.add_space(space::SM);
                                    let plan_key = review.plan_id.as_str().to_string();
                                    let note = self
                                        .review_modify_notes
                                        .entry(plan_key.clone())
                                        .or_default();
                                    let expanded = self
                                        .review_preview_expanded
                                        .entry(plan_key)
                                        .or_insert(false);
                                    if let Some(intent) = render_review_card(
                                        ui,
                                        &self.theme,
                                        review,
                                        note,
                                        expanded,
                                    ) {
                                        review_intent = Some(intent);
                                    }
                                }
                                let actions = turn_actions(turn);
                                if actions.any() {
                                    ui.add_space(space::XS);
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = space::SM;
                                        if actions.copy {
                                            let response = ui
                                                .add(
                                                    egui::Button::new(
                                                        egui::RichText::new("Copy")
                                                            .size(type_size::META)
                                                            .color(self.theme.text_secondary),
                                                    )
                                                    .frame(false),
                                                )
                                                .on_hover_text(action_accessibility_label(
                                                    "Copy response",
                                                    &turn.content,
                                                ));
                                            if response.clicked() {
                                                copy_index = Some(index);
                                            }
                                        }
                                        if actions.retry {
                                            let response = ui
                                                .add(
                                                    egui::Button::new(
                                                        egui::RichText::new("Retry")
                                                            .size(type_size::META)
                                                            .color(self.theme.text_secondary),
                                                    )
                                                    .frame(false),
                                                )
                                                .on_hover_text(action_accessibility_label(
                                                    "Retry response",
                                                    &turn.content,
                                                ));
                                            if response.clicked() {
                                                retry_index = Some(index);
                                            }
                                        }
                                        if actions.regenerate {
                                            let response = ui
                                                .add(
                                                    egui::Button::new(
                                                        egui::RichText::new("Regenerate")
                                                            .size(type_size::META)
                                                            .color(self.theme.text_secondary),
                                                    )
                                                    .frame(false),
                                                )
                                                .on_hover_text(action_accessibility_label(
                                                    "Regenerate response",
                                                    &turn.content,
                                                ));
                                            if response.clicked() {
                                                regenerate_index = Some(index);
                    }
                }
            });
                                }
                                ui.add_space(space::LG);
                            }
                            if show_typing_indicator(conversation_state, has_streaming)
                                || (self.awaiting_reply && !has_streaming)
                            {
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
        if let Some(index) = copy_index {
            self.copy_assistant_turn(ui.ctx(), index);
        }
        if retry_index.is_some() {
            self.retry_response();
        }
        if regenerate_index.is_some() {
            self.regenerate_response();
        }
        if has_streaming || self.awaiting_reply {
            ui.ctx().request_repaint();
        }
    }

    fn render_conversation_empty_state(&mut self, ui: &mut egui::Ui) {
        let available = ui.available_size();
        let mut prefill: Option<&'static str> = None;
        let mut open_project = false;
        let mut open_coding = false;
        ui.allocate_ui_with_layout(
            available,
            egui::Layout::top_down(egui::Align::Center),
            |ui| {
                // True vertical center of the conversation surface.
                let block_height = 260.0;
                let top = ((ui.available_height() - block_height) * 0.5).max(space::XL);
                ui.add_space(top);

                // Jaymi's presence is never a plain icon — the soft blob mark.
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(64.0, 64.0), egui::Sense::hover());
                ui.painter().rect_filled(
                    rect,
                    egui::CornerRadius { nw: 32, ne: 27, sw: 30, se: 35 },
                    self.theme.accent2_soft,
                );
                icons::paint(ui.painter(), Icon::Creation, rect.center(), 13.0, self.theme.accent2_deep);
                ui.add_space(space::MD + space::XS);

                ui.label(
                    egui::RichText::new("Hi, I'm Jaymi")
                        .font(crate::theme::display_font(type_size::WELCOME))
                        .color(self.theme.text_primary),
                );
                ui.add_space(space::SM + space::XS);
                ui.set_max_width((ui.available_width() * 0.7).clamp(260.0, 480.0));
                ui.label(
                    egui::RichText::new(
                        "Your whole computer, one conversation.\nAsk anything — I'm ready when you are.",
                    )
                    .size(type_size::BODY + 1.0)
                    .color(self.theme.text_secondary),
                );

                ui.add_space(space::LG);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(space::SM, space::SM);
                    ui.set_max_width((ui.available_width() * 0.7).clamp(260.0, 480.0));
                    if suggestion_chip(ui, &self.theme, "What can you help me with?").clicked() {
                        prefill = Some("What can you help me with?");
                    }
                    if suggestion_chip(ui, &self.theme, "Open a project").clicked() {
                        open_project = true;
                    }
                    if suggestion_chip(ui, &self.theme, "Show me the Coding workspace").clicked() {
                        open_coding = true;
                    }
                });
            },
        );
        if let Some(text) = prefill {
            self.prompt = text.to_string();
            self.focus_composer = true;
        }
        if open_project {
            self.open_project_folder();
        }
        if open_coding {
            self.settings_open = false;
            if self.app.active_project_id().is_some() {
                self.start_coding_project();
            } else {
                self.open_project_folder();
            }
        }
    }

    fn render_timestamp_separator(&self, ui: &mut egui::Ui, created_at: i64) {
        ui.add_space(space::MD);
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new(format_day_separator(created_at))
                    .size(type_size::META)
                    .color(self.theme.text_faint),
            );
        });
        ui.add_space(space::MD);
    }

    /// Presence, not spinners — a breathing sage dot with a live status verb.
    fn render_typing_indicator(&self, ui: &mut egui::Ui) {
        let state = self.experience.conversation_state();
        let label = if state.shows_progress_indicator() || state.is_active() {
            let text = state.status_label();
            if text.is_empty() {
                "Working…"
            } else {
                text
            }
        } else {
            "Preparing context…"
        };
        let progress = self
            .loading_started_at
            .map(|started| (started.elapsed().as_secs_f32() / 0.28).clamp(0.0, 1.0))
            .unwrap_or(1.0);
        let opacity = loading_opacity(progress);
        let pulse = pulse_alpha(ui, 3.0);
        let dot_color = self.theme.accent2.gamma_multiply(pulse);
        let secondary = self.theme.text_secondary;
        let faded = egui::Color32::from_rgba_unmultiplied(
            secondary.r(),
            secondary.g(),
            secondary.b(),
            (opacity * 255.0) as u8,
        );
        let response = ui
            .horizontal(|ui| {
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                ui.painter().circle_filled(rect.center(), 5.0, dot_color);
                ui.add_space(space::SM);
                ui.label(
                    egui::RichText::new(label)
                        .size(type_size::BODY)
                        .color(faded),
                );
            })
            .response;
        response.on_hover_text(progress_accessibility_label(state));
        ui.add_space(space::MD);
        ui.ctx().request_repaint();
    }

    fn render_chat_bubble(
        &self,
        ui: &mut egui::Ui,
        role: MessageRole,
        content: &str,
        streaming: bool,
    ) {
        // Cap only — short messages shrink-wrap; long ones wrap at this width.
        let max_bubble = (ui.available_width() * 0.78).clamp(240.0, 720.0);

        match role {
            MessageRole::User => {
                // Terracotta is the user's voice — a soft tint fill, not a
                // solid block; deep-accent text carries the contrast instead.
                let pad_x = space::MD;
                let pad_y = space::SM + space::XS;
                let max_inner = (max_bubble - pad_x * 2.0).max(48.0);
                let color = self.theme.accent_deep;
                let body_font = egui::FontId::proportional(type_size::BODY);

                // Measure intrinsic width so the bubble shrink-wraps (cap at max_inner).
                let inner_w = ui.fonts(|fonts| {
                    fonts
                        .layout(content.to_string(), body_font, color, max_inner)
                        .size()
                        .x
                        .clamp(1.0, max_inner)
                });

                // Dock to the conversation column's right edge; text right-aligned.
                // A small tail: sharp bottom-right corner, round everywhere else.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                    egui::Frame::new()
                        .corner_radius(egui::CornerRadius {
                            nw: radius::LG as u8,
                            ne: radius::LG as u8,
                            sw: radius::LG as u8,
                            se: radius::XS as u8,
                        })
                        .inner_margin(inset(pad_x, pad_y))
                        .fill(self.theme.accent_soft)
                        .show(ui, |ui| {
                            ui.set_width(inner_w);
                            ui.with_layout(egui::Layout::top_down(egui::Align::Max), |ui| {
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(content)
                                            .size(type_size::BODY)
                                            .color(color),
                                    )
                                    .wrap(),
                                );
                            });
                        });
                });
            }
            MessageRole::Assistant | MessageRole::System => {
                // Sage is Jaymi's voice — the soft blob mark stands in for a
                // label; presence pulses gently while a turn streams in.
                ui.horizontal_top(|ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(26.0, 26.0), egui::Sense::hover());
                    let icon_color = if streaming {
                        self.theme.accent2_deep.gamma_multiply(pulse_alpha(ui, 3.0))
                    } else {
                        self.theme.accent2_deep
                    };
                    ui.painter().rect_filled(
                        rect,
                        egui::CornerRadius { nw: 13, ne: 11, sw: 12, se: 14 },
                        self.theme.accent2_soft,
                    );
                    icons::paint(ui.painter(), Icon::Creation, rect.center(), 6.0, icon_color);
                    ui.add_space(space::SM);
                    ui.vertical(|ui| {
                        ui.set_max_width(max_bubble);
                        ui.label(
                            egui::RichText::new(content)
                                .size(type_size::BODY)
                                .color(self.theme.text_primary),
                        );
                    });
                });
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
        let mut stop_clicked = false;
        let generation_active = self.awaiting_reply || self.app.generation_active();

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
                        let attach = icon_pill_button(ui, &self.theme, Icon::Plus, 28.0, None, self.theme.text_secondary)
                            .on_hover_text("Attach files (soon)");
                        if attach.clicked() {
                            attach_clicked = true;
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = space::SM;
                            if generation_active {
                                let stop = paint_stop_button(ui, &self.theme)
                                    .on_hover_text("Stop generating (Esc)");
                                if stop.clicked() {
                                    stop_clicked = true;
                                }
                            } else {
                                let send = icon_pill_button(ui, &self.theme, Icon::Send, 32.0, Some(self.theme.accent), self.theme.on_accent())
                                    .on_hover_text("Send (Enter)");
                                if send.clicked() {
                                    send_clicked = true;
                                }
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

                        let attach = icon_pill_button(ui, &self.theme, Icon::Plus, 28.0, None, self.theme.text_secondary)
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
                            if generation_active {
                                let stop = paint_stop_button(ui, &self.theme)
                                    .on_hover_text("Stop generating (Esc)");
                                if stop.clicked() {
                                    stop_clicked = true;
                                }
                            } else {
                                let send = icon_pill_button(ui, &self.theme, Icon::Send, 32.0, Some(self.theme.accent), self.theme.on_accent())
                                    .on_hover_text("Send (Enter)");
                                if send.clicked() {
                                    send_clicked = true;
                                }
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
                    let enter_send = !generation_active
                        && response.has_focus()
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
        if stop_clicked {
            let _ = self.app.cancel_generation();
        }
        if send_clicked {
            self.send_prompt();
        }
    }

    /// The unified top bar: traffic-light inset, nav toggle, then (right-
    /// aligned) the appearance toggle, search/palette pill, an optional
    /// close-panel button, and the five-icon workspace switcher.
    fn render_top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_centered(|ui| {
            ui.add_space(TRAFFIC_LIGHT_INSET);
            self.render_nav_toggle(ui);

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                self.render_appearance_toggle(ui);
                ui.add_space(space::SM);
                self.render_palette_pill(ui);
                if self.experience.workspace_expanded() || self.settings_open {
                    ui.add_space(space::SM);
                    self.render_close_panel_button(ui);
                }
                ui.add_space(space::MD);
                self.render_workspace_switcher(ui);
            });
        });
    }

    /// The five-icon workspace switcher (Coding / Research / Knowledge /
    /// Creation / Settings), pill-grouped on the panel fill.
    fn render_workspace_switcher(&mut self, ui: &mut egui::Ui) {
        let active_kind = self.experience.active_workspace_kind();
        let entries: [(Icon, &str, Option<WorkspaceKind>); 5] = [
            (Icon::Coding, "Coding", Some(WorkspaceKind::Coding)),
            (Icon::Research, "Research", Some(WorkspaceKind::Research)),
            (Icon::Knowledge, "Knowledge", Some(WorkspaceKind::Knowledge)),
            (Icon::Creation, "Creation", Some(WorkspaceKind::Creation)),
            (Icon::Settings, "Settings", None),
        ];

        egui::Frame::new()
            .fill(self.theme.panel)
            .corner_radius(egui::CornerRadius::same(radius::PILL as u8))
            .inner_margin(egui::Margin::same(4))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    for (icon, label, kind) in entries {
                        let is_active = match kind {
                            Some(kind) => !self.settings_open && active_kind == Some(kind),
                            None => self.settings_open,
                        };
                        let fill = is_active.then_some(self.theme.surface);
                        let icon_color = if is_active {
                            self.theme.accent_deep
                        } else {
                            self.theme.text_secondary
                        };
                        let response = icon_pill_button(ui, &self.theme, icon, 32.0, fill, icon_color)
                            .on_hover_text(label);
                        if response.clicked() {
                            match kind {
                                Some(WorkspaceKind::Coding) => {
                                    self.settings_open = false;
                                    if self.app.active_project_id().is_some() {
                                        self.start_coding_project();
                                    } else {
                                        self.open_project_folder();
                                    }
                                }
                                Some(WorkspaceKind::Research) => self.open_capability_workspace(
                                    Capability::Search,
                                    "Opened Research from the top bar",
                                ),
                                Some(WorkspaceKind::Knowledge) => {
                                    self.open_capability_workspace(
                                        Capability::Discover,
                                        "Opened Knowledge from the top bar",
                                    );
                                    self.refresh_knowledge();
                                }
                                Some(WorkspaceKind::Creation) => self.open_capability_workspace(
                                    Capability::GenerateImages,
                                    "Opened Creation from the top bar",
                                ),
                                Some(WorkspaceKind::Conversation) => {}
                                None => self.open_settings_workspace(),
                            }
                        }
                    }
                });
            });
    }

    /// Expand a capability workspace directly from a manual UI action (not a
    /// planner/AI-driven expansion) — mirrors [`Self::start_coding_project`]
    /// for the workspaces that need no extra setup.
    fn open_capability_workspace(&mut self, capability: Capability, reason: &str) {
        self.settings_open = false;
        let Some(expansion) = workspace_expansion_for(capability, reason) else {
            return;
        };
        match self.app.expand_ui_workspace(expansion) {
            Ok(()) => {
                self.error = None;
                if let Ok(session) = self.app.experience() {
                    self.experience = session;
                }
            }
            Err(error) => self.error = Some(error.message().to_string()),
        }
    }

    /// Closes whichever right-side panel is currently open (a capability
    /// workspace or Settings) back to the plain conversation view.
    fn render_close_panel_button(&mut self, ui: &mut egui::Ui) {
        let response = icon_pill_button(ui, &self.theme, Icon::Close, 28.0, None, self.theme.text_secondary)
            .on_hover_text("Return to conversation");
        if response.clicked() {
            if self.settings_open {
                self.settings_open = false;
            } else {
                self.close_workspace();
            }
        }
    }

    /// Light/dark toggle — persists the preference; `sync_theme` picks up
    /// the change and cross-fades the shell next frame.
    fn render_appearance_toggle(&mut self, ui: &mut egui::Ui) {
        let dark = self.theme.mode.is_dark();
        let response = icon_pill_button(
            ui,
            &self.theme,
            Icon::Moon,
            32.0,
            Some(self.theme.panel),
            self.theme.text_secondary,
        )
        .on_hover_text("Toggle appearance");
        if response.clicked() {
            let next = if dark { ThemePreference::Light } else { ThemePreference::Dark };
            let _ = self.app.set_theme_preference(next);
        }
    }

    /// The `⌘P` search pill — opens the Command Palette.
    fn render_palette_pill(&mut self, ui: &mut egui::Ui) {
        let font = egui::FontId::proportional(type_size::META + 0.5);
        let label = "⌘P";
        let galley = ui.painter().layout_no_wrap(label.to_string(), font.clone(), egui::Color32::PLACEHOLDER);
        let width = 14.0 + space::SM + galley.size().x + space::MD;
        let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 32.0), egui::Sense::click());
        let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
        let hovered = response.hovered();
        ui.painter().rect_filled(rect, egui::CornerRadius::same(radius::PILL as u8), self.theme.panel);
        let icon_center = egui::pos2(rect.left() + space::MD, rect.center().y);
        icons::paint(ui.painter(), Icon::Search, icon_center, 6.5, self.theme.text_secondary);
        ui.painter().text(
            egui::pos2(icon_center.x + 14.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            font,
            if hovered { self.theme.text_primary } else { self.theme.text_secondary },
        );
        if response.clicked() {
            self.command_palette.open();
            self.refresh_command_palette();
        }
    }

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
        let active_project_id = self.app.active_project_id();
        let has_project = active_project_id.is_some();
        let coding_open = self.experience.active_workspace_kind() == Some(WorkspaceKind::Coding);
        let project_label = active_project_id.as_ref().and_then(|id| {
            recent_projects
                .iter()
                .find(|(project_id, _)| project_id == id)
                .map(|(_, label)| label.clone())
        });
        let project_meta = self
            .app
            .active_project_root_path()
            .map(|root| root.to_string_lossy().into_owned());

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
                project_label: project_label.as_deref(),
                project_meta: project_meta.as_deref(),
                conversations: &conversations,
                active_conversation_id: active_conversation_id.as_deref(),
                has_project,
                coding_open,
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
                NavRailEvent::NewConversation => self.start_new_conversation(),
                NavRailEvent::OpenCoding => {
                    self.settings_open = false;
                    if self.app.active_project_id().is_some() {
                        self.start_coding_project();
                    } else {
                        self.open_project_folder();
                    }
                }
            }
        }
    }

    /// Create a new conversation in the active project and switch to it.
    fn start_new_conversation(&mut self) {
        let project_id = self.app.active_project_id();
        let request = CreateConversationRequest {
            conversation_id: None,
            title: None,
            project_id,
        };
        match self.app.create_conversation(&request) {
            Ok(meta) => match self.app.switch_to_conversation(&meta.id.to_string()) {
                Ok(()) => {
                    self.error = None;
                    if let Ok(session) = self.app.experience() {
                        self.experience = session;
                    }
                }
                Err(error) => self.error = Some(error.message().to_string()),
            },
            Err(error) => self.error = Some(error.message().to_string()),
        }
    }

    /// Re-run the Knowledge search/vault queries and push results into the
    /// active `CapabilityState::Knowledge` (no-op when Knowledge isn't open).
    fn refresh_knowledge(&mut self) {
        let query = self.knowledge.query.clone();
        let items = if let Some(vault) = &self.knowledge.selected_vault {
            self.app
                .knowledge_items_in_collection(vault, 60)
                .unwrap_or_default()
        } else if !query.trim().is_empty() {
            self.app.search_knowledge(&query, 60).unwrap_or_default()
        } else {
            Vec::new()
        };
        let hits: Vec<jaymi_capabilities::KnowledgeHitState> = items
            .into_iter()
            .map(|item| jaymi_capabilities::KnowledgeHitState {
                id: item.path.to_string_lossy().into_owned(),
                vault_id: String::new(),
                title: item.filename,
                snippet: item
                    .parent
                    .map(|parent| parent.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            })
            .collect();
        let vaults: Vec<jaymi_capabilities::KnowledgeVaultState> = self
            .app
            .list_knowledge_collections()
            .unwrap_or_default()
            .into_iter()
            .map(|collection| jaymi_capabilities::KnowledgeVaultState {
                id: collection.id.slug().to_string(),
                name: collection.name,
                meta: format!(
                    "{} item{}",
                    collection.item_count,
                    if collection.item_count == 1 { "" } else { "s" }
                ),
            })
            .collect();
        let _ = self.app.with_knowledge_state(|state| {
            state.hits = hits;
            state.vaults = vaults;
        });
    }

    fn handle_knowledge_events(&mut self, events: Vec<KnowledgeWorkspaceEvent>) {
        let mut needs_refresh = false;
        for event in events {
            match event {
                KnowledgeWorkspaceEvent::QueryChanged(query) => {
                    self.knowledge.query = query;
                    self.knowledge.selected_vault = None;
                    needs_refresh = true;
                }
                KnowledgeWorkspaceEvent::SelectVault(vault) => {
                    self.knowledge.selected_vault = vault;
                    self.knowledge.query.clear();
                    needs_refresh = true;
                }
                KnowledgeWorkspaceEvent::SelectHit(id) => {
                    self.knowledge.selected_hit = Some(id);
                }
                KnowledgeWorkspaceEvent::RevealInFinder(path) => {
                    let _ = self.app.reveal_in_file_manager(&path);
                }
            }
        }
        if needs_refresh {
            self.refresh_knowledge();
        }
    }

    fn open_settings_workspace(&mut self) {
        if self.experience.workspace_expanded() {
            self.close_workspace();
        }
        self.settings_open = true;
        if self.settings.category != SettingsCategory::Reasoning
            && matches!(
                self.settings.category,
                SettingsCategory::General | SettingsCategory::Appearance
            )
        {
            // Keep user's last category when re-entering; default first open is Reasoning.
        }
        self.settings_busy = false;
        match self.app.reasoning_settings_snapshot() {
            Ok(snapshot) => {
                self.reasoning_settings = snapshot;
                self.error = None;
            }
            Err(error) => {
                self.error = Some(error.message().to_string());
            }
        }
    }

    fn render_settings_surface(&mut self, ui: &mut egui::Ui) {
        let mut events = Vec::new();
        {
            let ctx = SettingsWorkspaceContext {
                theme: &self.theme,
                state: &self.settings,
                reasoning: &self.reasoning_settings,
                busy: self.settings_busy,
                theme_preference: self.app.theme_preference().unwrap_or_default(),
            };
            render_settings_workspace(ui, &ctx, &mut events);
        }
        self.handle_settings_events(events);
    }

    fn handle_settings_events(&mut self, events: Vec<SettingsWorkspaceEvent>) {
        for event in events {
            match event {
                SettingsWorkspaceEvent::SelectCategory(category) => {
                    self.settings.category = category;
                }
                SettingsWorkspaceEvent::Close => {
                    self.settings_open = false;
                }
                SettingsWorkspaceEvent::SelectDefaultModel {
                    provider_id,
                    model_name,
                } => match self
                    .app
                    .set_default_reasoning_model(provider_id, model_name)
                {
                    Ok(snapshot) => {
                        self.reasoning_settings = snapshot;
                        self.status = Some("Default reasoning model updated.".into());
                        self.error = None;
                    }
                    Err(error) => self.error = Some(error.message().to_string()),
                },
                SettingsWorkspaceEvent::RefreshModels => {
                    self.settings_busy = true;
                    match self.app.refresh_reasoning_models() {
                        Ok(snapshot) => {
                            self.reasoning_settings = snapshot;
                            self.status = Some("Models refreshed.".into());
                            self.error = None;
                        }
                        Err(error) => self.error = Some(error.message().to_string()),
                    }
                    self.settings_busy = false;
                }
                SettingsWorkspaceEvent::TestConnection => {
                    self.settings_busy = true;
                    match self.app.test_reasoning_connection() {
                        Ok(snapshot) => {
                            self.reasoning_settings = snapshot;
                            self.error = None;
                        }
                        Err(error) => self.error = Some(error.message().to_string()),
                    }
                    self.settings_busy = false;
                }
                SettingsWorkspaceEvent::SetThemePreference(preference) => {
                    match self.app.set_theme_preference(preference) {
                        Ok(()) => self.error = None,
                        Err(error) => self.error = Some(error.message().to_string()),
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
                CodingShellEvent::SetSelection {
                    pane,
                    path,
                    start_line,
                    start_column,
                    end_line,
                    end_column,
                    text,
                } => self.app.set_coding_tab_selection_in_pane(
                    &pane,
                    &path,
                    EditorSelection::new(
                        start_line,
                        start_column,
                        end_line,
                        end_column,
                        text,
                    ),
                ),
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
                    self.open_settings_workspace();
                    Ok(())
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
                    let focus_terminal = tab == CodingBottomTab::Terminal;
                    let result = self.app.with_coding_state(|coding| {
                        if tab.is_page() {
                            coding.show_bottom_tab(tab);
                        } else {
                            coding.hide_bottom_dock();
                        }
                    });
                    if result.is_ok() {
                        let _ = self.app.persist_coding_editor_workspace();
                        if focus_terminal {
                            if let Ok(Some(id)) = self.app.with_coding_state(|coding| {
                                coding.active_terminal_id.clone().or_else(|| {
                                    coding.terminal_sessions.first().map(|s| s.id.clone())
                                })
                            }) {
                                self.pending_terminal_focus = Some(id);
                                self.egui_wants_keyboard = true;
                            }
                        }
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
                CodingShellEvent::TerminalWantsKeyboard => {
                    self.egui_wants_keyboard = true;
                    Ok(())
                }
                CodingShellEvent::TerminalFocusInput { session_id } => {
                    self.pending_terminal_focus = Some(session_id);
                    self.egui_wants_keyboard = true;
                    Ok(())
                }
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
                    if self.awaiting_reply || self.app.generation_active() {
                        Ok(())
                    } else {
                        self.awaiting_reply = true;
                        self.loading_started_at = Some(std::time::Instant::now());
                        let result = match dispatch_quick_action(action) {
                            QuickActionEffect::SubmitExplain => {
                                self.app.begin_explain_coding_action()
                            }
                            QuickActionEffect::SubmitCodingAction(coding_action) => {
                                self.app.begin_coding_action(coding_action)
                            }
                        };
                        match result {
                            Ok(BeginGeneration::Started) => {
                                self.error = None;
                                self.status = None;
                                if let Ok(session) = self.app.experience() {
                                    self.experience = session;
                                }
                                Ok(())
                            }
                            Ok(BeginGeneration::Completed(response)) => {
                                self.awaiting_reply = false;
                                self.loading_started_at = None;
                                self.error = None;
                                self.status = None;
                                if let Ok(session) = self.app.experience() {
                                    self.experience = session;
                                } else {
                                    self.experience
                                        .mirror_conversation_state(response.conversation_state);
                                }
                                Ok(())
                            }
                            Err(error) => {
                                self.awaiting_reply = false;
                                self.loading_started_at = None;
                                Err(error)
                            }
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
            // Apply pending terminal focus before paint so TextEdit can request_focus.
            if let Some(session_id) = self.pending_terminal_focus.take() {
                let focus_id = egui::Id::new(("terminal_request_focus", session_id));
                ui.ctx()
                    .data_mut(|data| data.insert_temp(focus_id, true));
            }
            // Reset each frame; TerminalWantsKeyboard / focus events set it again.
            self.egui_wants_keyboard = false;

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

        if workspace.kind == WorkspaceKind::Knowledge {
            let knowledge = self
                .experience
                .capability_state()
                .and_then(|state| state.knowledge())
                .cloned();
            let mut events = Vec::new();
            egui::ScrollArea::vertical()
                .id_salt("knowledge_workspace_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let ctx = KnowledgeWorkspaceContext {
                        theme: &self.theme,
                        state: &self.knowledge,
                        knowledge: knowledge.as_ref(),
                    };
                    render_knowledge_workspace(ui, &ctx, &mut events);
                });
            self.handle_knowledge_events(events);
            return None;
        }

        if workspace.kind == WorkspaceKind::Research {
            let research = self
                .experience
                .capability_state()
                .and_then(|state| state.research())
                .cloned();
            egui::ScrollArea::vertical()
                .id_salt("research_workspace_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    render_research_workspace(ui, &self.theme, research.as_ref());
                });
            return None;
        }

        if workspace.kind == WorkspaceKind::Creation {
            let creation = self
                .experience
                .capability_state()
                .and_then(|state| state.creation())
                .cloned();
            egui::ScrollArea::vertical()
                .id_salt("creation_workspace_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    render_creation_workspace(ui, &self.theme, creation.as_ref());
                });
            return None;
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
                MonacoIpcMessage::Selection {
                    path,
                    start_line,
                    start_column,
                    end_line,
                    end_column,
                    text,
                } => {
                    events.push(CodingShellEvent::SetSelection {
                        pane: focused_pane.clone(),
                        path,
                        start_line,
                        start_column,
                        end_line,
                        end_column,
                        text,
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
        // Also hide while the terminal command field needs keys (WKWebView would
        // otherwise keep first-responder and swallow typing).
        let block_for_keyboard =
            self.command_palette.is_open() || self.egui_wants_keyboard;
        let document = surface
            .filter(|_| !block_for_keyboard)
            .map(|surface| self.monaco_document_from_state(surface));
        let surface = surface.filter(|_| !block_for_keyboard);
        let theme_id = self.theme.monaco_theme_id().to_string();
        let definition = self.theme.monaco_definition_json();
        let Some(host) = self.monaco.as_mut() else {
            return;
        };
        if block_for_keyboard {
            let _ = host.release_keyboard();
        } else {
            host.clear_keyboard_release();
        }
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

    fn pump_active_generation(&mut self, ctx: &egui::Context) {
        if !self.app.generation_active() {
            return;
        }
        match self.app.pump_generation(24) {
            Ok(PumpGeneration::Active { .. }) => {
                self.awaiting_reply = true;
                if let Ok(session) = self.app.experience() {
                    self.experience = session;
                }
                // Keep polling — Starting + Pending frames must wake for first token.
                ctx.request_repaint();
            }
            Ok(PumpGeneration::Finished(_response)) => {
                self.awaiting_reply = false;
                self.loading_started_at = None;
                if let Ok(session) = self.app.experience() {
                    self.experience = session;
                }
                self.error = None;
            }
            Ok(PumpGeneration::Idle) => {
                self.awaiting_reply = false;
                self.loading_started_at = None;
            }
            Err(error) => {
                self.awaiting_reply = false;
                self.loading_started_at = None;
                self.error = Some(error.message().to_string());
                let _ = self.app.cancel_generation();
            }
        }
    }

    fn send_prompt(&mut self) {
        let prompt = self.prompt.trim().to_string();
        if prompt.is_empty() || self.awaiting_reply || self.app.generation_active() {
            return;
        }
        self.prompt.clear();
        self.awaiting_reply = true;
        self.loading_started_at = Some(std::time::Instant::now());
        // Do not invent ConversationState — sync from Application after Planner ack.
        match self.app.begin_generation(prompt) {
            Ok(BeginGeneration::Started) => {
                self.error = None;
                self.status = None;
                if let Ok(session) = self.app.experience() {
                    self.experience = session;
                }
            }
            Ok(BeginGeneration::Completed(response)) => {
                // Legacy sync completion (should not occur on the interactive path).
                self.awaiting_reply = false;
                self.loading_started_at = None;
                self.error = None;
                self.status = None;
                if let Ok(session) = self.app.experience() {
                    self.experience = session;
                } else {
                    self.experience
                        .mirror_conversation_state(response.conversation_state);
                }
            }
            Err(error) => {
                self.awaiting_reply = false;
                self.loading_started_at = None;
                self.status = None;
                self.error = Some(error.message().to_string());
                if let Ok(session) = self.app.experience() {
                    self.experience = session;
                }
            }
        }
    }

    fn copy_assistant_turn(&mut self, ctx: &egui::Context, turn_index: usize) {
        match self.app.assistant_turn_text(turn_index) {
            Ok(text) => {
                ctx.copy_text(text.clone());
                self.last_clipboard = Some(text);
                self.status = Some("Copied response.".into());
                self.error = None;
            }
            Err(error) => {
                self.error = Some(error.message().to_string());
            }
        }
    }

    fn retry_response(&mut self) {
        self.awaiting_reply = true;
        self.loading_started_at = Some(std::time::Instant::now());
        match self.app.retry_generation(false) {
            Ok(BeginGeneration::Started) => {
                self.error = None;
                if let Ok(session) = self.app.experience() {
                    self.experience = session;
                }
            }
            Ok(BeginGeneration::Completed(response)) => {
                self.awaiting_reply = false;
                self.loading_started_at = None;
                if let Ok(session) = self.app.experience() {
                    self.experience = session;
                } else {
                    self.experience
                        .mirror_conversation_state(response.conversation_state);
                }
            }
            Err(error) => {
                self.awaiting_reply = false;
                self.loading_started_at = None;
                self.error = Some(error.message().to_string());
            }
        }
    }

    fn regenerate_response(&mut self) {
        self.awaiting_reply = true;
        self.loading_started_at = Some(std::time::Instant::now());
        match self.app.regenerate_response() {
            Ok(BeginGeneration::Started) => {
                self.error = None;
                if let Ok(session) = self.app.experience() {
                    self.experience = session;
                }
            }
            Ok(BeginGeneration::Completed(response)) => {
                self.awaiting_reply = false;
                self.loading_started_at = None;
                if let Ok(session) = self.app.experience() {
                    self.experience = session;
                } else {
                    self.experience
                        .mirror_conversation_state(response.conversation_state);
                }
            }
            Err(error) => {
                self.awaiting_reply = false;
                self.loading_started_at = None;
                self.error = Some(error.message().to_string());
            }
        }
    }

    /// Apply a Review Card button: record intent, then Planner pause/resume.
    ///
    /// Approve resumes the paused plan without replanning. Modify regenerates
    /// affected steps into a child plan (re-paused for approval). Cancel drops
    /// the pause. The card itself never executes tools — the Planner does.
    fn handle_review_intent(&mut self, intent: ReviewIntent) {
        if let ReviewIntent::Modify { plan_id, .. } = &intent {
            self.review_modify_notes.remove(plan_id.as_str());
            self.review_preview_expanded.remove(plan_id.as_str());
        }
        if let ReviewIntent::Approve { plan_id } | ReviewIntent::Cancel { plan_id } = &intent {
            self.review_preview_expanded.remove(plan_id.as_str());
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

        // Performance dashboard — observational timings (Developer Diagnostics only).
        {
            let performance = self.snapshot.performance_dashboard();
            if performance.has_content() {
                ui.add_space(space::MD);
                ui.label(
                    egui::RichText::new("Performance")
                        .strong()
                        .size(type_size::UI)
                        .color(self.theme.text_primary),
                );
                ui.add_space(space::XS);
                ui.label(
                    egui::RichText::new(
                        "Last-turn pipeline timings · TTFT · provider / context timings · cache · prompt sizes. Observational only — not shown in conversation.",
                    )
                    .size(type_size::META)
                    .color(self.theme.text_secondary),
                );
                ui.add_space(space::SM);
                egui::Grid::new("performance_metrics")
                    .striped(true)
                    .num_columns(2)
                    .spacing([space::MD, space::XS])
                    .min_col_width(140.0)
                    .show(ui, |ui| {
                        ui.strong("Metric");
                        ui.strong("Value");
                        ui.end_row();
                        for (label, value) in performance.metric_rows() {
                            ui.label(label);
                            ui.label(value);
                            ui.end_row();
                        }
                    });
                if !performance.provider_timings.is_empty() {
                    ui.add_space(space::SM);
                    ui.label(
                        egui::RichText::new("Provider timings")
                            .strong()
                            .size(type_size::META)
                            .color(self.theme.text_primary),
                    );
                    egui::Grid::new("performance_provider_timings")
                        .striped(true)
                        .num_columns(2)
                        .spacing([space::MD, space::XS])
                        .min_col_width(120.0)
                        .show(ui, |ui| {
                            for (label, value) in &performance.provider_timings {
                                ui.label(label);
                                ui.label(value);
                                ui.end_row();
                            }
                        });
                }
                if !performance.context_provider_timings.is_empty() {
                    ui.add_space(space::SM);
                    ui.label(
                        egui::RichText::new("Context provider timings")
                            .strong()
                            .size(type_size::META)
                            .color(self.theme.text_primary),
                    );
                    egui::Grid::new("performance_context_provider_timings")
                        .striped(true)
                        .num_columns(2)
                        .spacing([space::MD, space::XS])
                        .min_col_width(120.0)
                        .show(ui, |ui| {
                            for (label, value) in &performance.context_provider_timings {
                                ui.label(label);
                                ui.label(value);
                                ui.end_row();
                            }
                        });
                }
                if !performance.timeline.is_empty() {
                    ui.add_space(space::SM);
                    ui.label(
                        egui::RichText::new("Pipeline timeline")
                            .strong()
                            .size(type_size::META)
                            .color(self.theme.text_primary),
                    );
                    ui.add_space(space::XS);
                    let max_ms = performance
                        .timeline
                        .iter()
                        .map(|row| row.duration_ms)
                        .max()
                        .unwrap_or(1)
                        .max(1);
                    egui::Grid::new("performance_pipeline_timeline")
                        .striped(true)
                        .num_columns(3)
                        .spacing([space::MD, space::XS])
                        .min_col_width(80.0)
                        .show(ui, |ui| {
                            ui.strong("Stage");
                            ui.strong("Duration");
                            ui.strong("");
                            ui.end_row();
                            for row in &performance.timeline {
                                ui.label(&row.label);
                                ui.label(format!("{} ms", row.duration_ms));
                                let fraction =
                                    (row.duration_ms as f32 / max_ms as f32).clamp(0.0, 1.0);
                                ui.add(
                                    egui::ProgressBar::new(fraction)
                                        .desired_width(140.0),
                                );
                                ui.end_row();
                            }
                        });
                }
            }
        }

        // Workspace Intelligence — freshness, maintenance, candidates, policy, budget (B2.11).
        if let Some(workspace) = &self.snapshot.workspace_inspector {
            if workspace.has_content() {
                ui.add_space(space::MD);
                ui.label(
                    egui::RichText::new("Workspace Intelligence")
                        .strong()
                        .size(type_size::UI)
                        .color(self.theme.text_primary),
                );
                ui.add_space(space::XS);
                ui.label(
                    egui::RichText::new(
                        "Snapshot freshness · provider timings · maintenance · candidates · policy · context budget. Developer-only — never written to conversation.",
                    )
                    .size(type_size::META)
                    .color(self.theme.text_secondary),
                );
                ui.add_space(space::SM);
                egui::Grid::new("workspace_intelligence_metrics")
                    .striped(true)
                    .num_columns(2)
                    .spacing([space::MD, space::XS])
                    .min_col_width(140.0)
                    .show(ui, |ui| {
                        ui.strong("Metric");
                        ui.strong("Value");
                        ui.end_row();
                        for (label, value) in workspace.labeled_values() {
                            ui.label(label);
                            ui.label(value);
                            ui.end_row();
                        }
                    });
                if !workspace.snapshot_freshness.is_empty() {
                    ui.add_space(space::SM);
                    ui.label(
                        egui::RichText::new("Snapshot freshness")
                            .strong()
                            .size(type_size::META)
                            .color(self.theme.text_primary),
                    );
                    egui::Grid::new("workspace_snapshot_freshness")
                        .striped(true)
                        .num_columns(4)
                        .spacing([space::MD, space::XS])
                        .min_col_width(64.0)
                        .show(ui, |ui| {
                            ui.strong("Kind");
                            ui.strong("Present");
                            ui.strong("Freshness");
                            ui.strong("Age");
                            ui.end_row();
                            for row in &workspace.snapshot_freshness {
                                ui.label(&row.kind);
                                ui.label(if row.present { "yes" } else { "no" });
                                ui.label(&row.freshness);
                                ui.label(
                                    row.age_seconds
                                        .map(|s| format!("{s}s"))
                                        .unwrap_or_else(|| "-".into()),
                                );
                                ui.end_row();
                            }
                        });
                }
                if !workspace.maintenance_status.is_empty() {
                    ui.add_space(space::SM);
                    ui.label(
                        egui::RichText::new("Maintenance status")
                            .strong()
                            .size(type_size::META)
                            .color(self.theme.text_primary),
                    );
                    egui::Grid::new("workspace_maintenance_status")
                        .striped(true)
                        .num_columns(3)
                        .spacing([space::MD, space::XS])
                        .min_col_width(80.0)
                        .show(ui, |ui| {
                            ui.strong("Kind");
                            ui.strong("Inflight");
                            ui.strong("Completed");
                            ui.end_row();
                            for row in &workspace.maintenance_status {
                                ui.label(&row.kind);
                                ui.label(if row.inflight { "yes" } else { "no" });
                                ui.label(if row.has_completed { "yes" } else { "no" });
                                ui.end_row();
                            }
                        });
                }
                if !workspace.provider_timings.is_empty() {
                    ui.add_space(space::SM);
                    ui.label(
                        egui::RichText::new("Provider timings")
                            .strong()
                            .size(type_size::META)
                            .color(self.theme.text_primary),
                    );
                    egui::Grid::new("workspace_provider_timings")
                        .striped(true)
                        .num_columns(2)
                        .spacing([space::MD, space::XS])
                        .min_col_width(120.0)
                        .show(ui, |ui| {
                            for (id, detail) in &workspace.provider_timings {
                                ui.label(id);
                                ui.label(detail);
                                ui.end_row();
                            }
                        });
                }
                if !workspace.candidate_rows().is_empty() {
                    ui.add_space(space::SM);
                    ui.label(
                        egui::RichText::new("Candidate selection")
                            .strong()
                            .size(type_size::META)
                            .color(self.theme.text_primary),
                    );
                    egui::Grid::new("workspace_candidate_selection")
                        .striped(true)
                        .num_columns(6)
                        .spacing([space::MD, space::XS])
                        .min_col_width(48.0)
                        .show(ui, |ui| {
                            ui.strong("Provider");
                            ui.strong("Candidate");
                            ui.strong("Selected");
                            ui.strong("Rel");
                            ui.strong("Reason");
                            ui.strong("Chars");
                            ui.end_row();
                            for decision in workspace.candidate_rows().iter().take(48) {
                                ui.label(&decision.provider_id);
                                ui.label(&decision.candidate_id);
                                ui.label(if decision.selected { "yes" } else { "no" });
                                ui.label(decision.relevance.to_string());
                                ui.label(&decision.reason);
                                ui.label(decision.estimated_chars.to_string());
                                ui.end_row();
                            }
                        });
                }
                if !workspace.policy_decisions.is_empty() {
                    ui.add_space(space::SM);
                    ui.label(
                        egui::RichText::new("Policy decisions")
                            .strong()
                            .size(type_size::META)
                            .color(self.theme.text_primary),
                    );
                    egui::Grid::new("workspace_policy_decisions")
                        .striped(true)
                        .num_columns(3)
                        .spacing([space::MD, space::XS])
                        .min_col_width(80.0)
                        .show(ui, |ui| {
                            ui.strong("Provider");
                            ui.strong("Included");
                            ui.strong("Reason");
                            ui.end_row();
                            for decision in workspace.policy_decisions.iter().take(48) {
                                ui.label(&decision.provider_id);
                                ui.label(if decision.included { "yes" } else { "no" });
                                ui.label(&decision.reason);
                                ui.end_row();
                            }
                        });
                }
            }
        }

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

        if let Some(inspector) = &self.snapshot.reasoning_inspector {
            ui.add_space(space::MD);
            ui.label(
                egui::RichText::new("Conversational Reasoning")
                    .strong()
                    .size(type_size::UI)
                    .color(self.theme.text_primary),
            );
            ui.add_space(space::XS);
            ui.label(
                egui::RichText::new(
                    "Lifecycle: Idle → Preparing Context → Reasoning / Streaming → Completed | Cancelled | Failed",
                )
                .size(type_size::META)
                .color(self.theme.text_secondary),
            );
            ui.add_space(space::XS);
            ui.label(
                egui::RichText::new(inspector.summary_line())
                    .size(type_size::META)
                    .color(self.theme.text_secondary),
            );
            ui.add_space(space::SM);
            egui::Grid::new("reasoning_inspector_fields")
                .striped(true)
                .num_columns(2)
                .spacing([space::MD, space::XS])
                .min_col_width(120.0)
                .show(ui, |ui| {
                    ui.strong("Field");
                    ui.strong("Value");
                    ui.end_row();
                    for (label, value) in inspector.labeled_values() {
                        ui.label(label);
                        ui.label(value);
                        ui.end_row();
                    }
                });
            if !inspector.prompt_sections.is_empty() {
                ui.add_space(space::SM);
                egui::Grid::new("reasoning_inspector_prompt_sections")
                    .striped(true)
                    .num_columns(5)
                    .spacing([space::MD, space::XS])
                    .min_col_width(64.0)
                    .show(ui, |ui| {
                        ui.strong("Section");
                        ui.strong("Chars");
                        ui.strong("Tokens");
                        ui.strong("State");
                        ui.strong("Note");
                        ui.end_row();
                        for section in &inspector.prompt_sections {
                            let state = if section.truncated {
                                "trunc"
                            } else if section.included {
                                "included"
                            } else {
                                "omitted"
                            };
                            ui.label(section.id.as_str());
                            ui.label(section.characters.to_string());
                            ui.label(section.estimated_tokens.to_string());
                            ui.label(state);
                            ui.label(section.note.as_deref().unwrap_or("-"));
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
