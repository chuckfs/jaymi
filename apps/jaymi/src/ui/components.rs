//! Reusable Organic-system building blocks — pill buttons, cards, tags, and
//! the segmented control — mirroring the design system's `.btn`/`.card`/
//! `.tag`/`.seg` CSS classes so screens compose from one place instead of
//! duplicating `Frame`/`Stroke` boilerplate per call site.

use eframe::egui::{self, Color32, Response, Sense, Ui};

use crate::theme::{radius, space, stroke, type_size, Theme};
use crate::ui::icons::{self, Icon};

/// Visual weight for [`pill_button`] — mirrors `.btn-primary` / `.btn-secondary`
/// / `.btn-ghost`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonStyle {
    /// Solid accent fill — the one primary action per surface.
    Primary,
    /// Hairline outline, transparent fill.
    Secondary,
    /// No outline, no fill — lowest-emphasis action.
    Ghost,
}

/// A pill-shaped button (`border-radius: 999px`), hand-painted to match the
/// Organic button spec: solid/hairline/ghost, with a hover wash and a
/// pressed step one ramp deeper than the base fill.
pub fn pill_button(ui: &mut Ui, theme: &Theme, label: &str, style: ButtonStyle) -> Response {
    let font = egui::FontId::new(type_size::UI, egui::FontFamily::Name("figtree-semibold".into()));
    let galley = ui.painter().layout_no_wrap(label.to_string(), font.clone(), Color32::PLACEHOLDER);
    let pad_x = space::MD;
    let height = 34.0;
    let size = egui::vec2(galley.size().x + pad_x * 2.0, height);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);

    if ui.is_rect_visible(rect) {
        let (fill, text_color, outline) = match style {
            ButtonStyle::Primary => {
                let fill = if response.is_pointer_button_down_on() {
                    theme.accent_deep
                } else if response.hovered() {
                    mix(theme.accent, theme.accent_deep, 0.5)
                } else {
                    theme.accent
                };
                (Some(fill), theme.on_accent(), None)
            }
            ButtonStyle::Secondary => {
                let fill = if response.hovered() {
                    Some(theme.surface_alt)
                } else {
                    None
                };
                (fill, theme.text_primary, Some(theme.border))
            }
            ButtonStyle::Ghost => {
                let fill = if response.hovered() {
                    Some(theme.accent_tint)
                } else {
                    None
                };
                (fill, theme.accent_deep, None)
            }
        };

        let corner = egui::CornerRadius::same(radius::PILL as u8);
        if let Some(fill) = fill {
            ui.painter().rect_filled(rect, corner, fill);
        }
        if let Some(outline) = outline {
            ui.painter().rect_stroke(
                rect,
                corner,
                egui::Stroke::new(stroke::HAIRLINE * 1.5, outline),
                egui::StrokeKind::Inside,
            );
        }
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            label,
            font,
            text_color,
        );
    }

    response
}

/// Shared workspace header — icon blob + Caprasimo title + optional muted
/// subtitle. Every capability workspace (Coding, Research, Knowledge,
/// Creation) opens with this same row; closing stays with the app's single
/// top-bar close button (see `render_close_panel_button`), so this never
/// paints its own close control.
pub fn render_workspace_header(
    ui: &mut Ui,
    theme: &Theme,
    icon: Icon,
    icon_bg: Color32,
    icon_color: Color32,
    title: &str,
    subtitle: Option<&str>,
) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(32.0, 32.0), Sense::hover());
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius { nw: 16, ne: 13, sw: 15, se: 17 },
            icon_bg,
        );
        icons::paint(ui.painter(), icon, rect.center(), 7.5, icon_color);
        ui.add_space(space::SM);
        ui.label(
            egui::RichText::new(title)
                .font(crate::theme::display_font(type_size::TITLE))
                .color(theme.text_primary),
        );
        if let Some(subtitle) = subtitle {
            if !subtitle.is_empty() {
                ui.add_space(space::XS);
                ui.label(egui::RichText::new(subtitle).size(type_size::META).color(theme.text_secondary));
            }
        }
    });
}

/// A compact pill button for dense chrome (dock/panel toolbars) where
/// [`pill_button`]'s 34px height doesn't fit — same solid/hairline/ghost
/// language, sized down (height 24, tighter padding, `type_size::META`).
pub fn mini_pill_button(
    ui: &mut Ui,
    theme: &Theme,
    label: &str,
    style: ButtonStyle,
    enabled: bool,
) -> Response {
    let font = egui::FontId::proportional(type_size::META);
    let galley = ui.painter().layout_no_wrap(label.to_string(), font.clone(), Color32::PLACEHOLDER);
    let pad_x = space::SM;
    let height = 24.0;
    let size = egui::vec2(galley.size().x + pad_x * 2.0, height);
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    let response = if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    };

    if ui.is_rect_visible(rect) {
        let (fill, text_color, outline) = match style {
            ButtonStyle::Primary => {
                let base = if enabled { theme.accent } else { theme.border };
                let fill = if enabled && response.is_pointer_button_down_on() {
                    theme.accent_deep
                } else if enabled && response.hovered() {
                    mix(theme.accent, theme.accent_deep, 0.5)
                } else {
                    base
                };
                let text = if enabled { theme.on_accent() } else { theme.text_faint };
                (Some(fill), text, None)
            }
            ButtonStyle::Secondary => {
                let fill = if enabled && response.hovered() {
                    Some(theme.surface_alt)
                } else {
                    None
                };
                let text = if enabled { theme.text_primary } else { theme.text_faint };
                (fill, text, Some(theme.border))
            }
            ButtonStyle::Ghost => {
                let fill = if enabled && response.hovered() {
                    Some(theme.accent_tint)
                } else {
                    None
                };
                let text = if enabled { theme.accent_deep } else { theme.text_faint };
                (fill, text, None)
            }
        };

        let corner = egui::CornerRadius::same(radius::PILL as u8);
        if let Some(fill) = fill {
            ui.painter().rect_filled(rect, corner, fill);
        }
        if let Some(outline) = outline {
            ui.painter().rect_stroke(
                rect,
                corner,
                egui::Stroke::new(stroke::HAIRLINE, outline),
                egui::StrokeKind::Inside,
            );
        }
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            label,
            font,
            text_color,
        );
    }

    response
}

/// A circular pill icon button (send, close, attach, workspace switch).
pub fn icon_pill_button(
    ui: &mut Ui,
    theme: &Theme,
    icon: Icon,
    diameter: f32,
    fill: Option<Color32>,
    icon_color: Color32,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(diameter, diameter), Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);

    if ui.is_rect_visible(rect) {
        let hover_fill = if response.hovered() {
            Some(fill.map(|c| mix(c, theme.text_primary, 0.08)).unwrap_or(theme.selection()))
        } else {
            fill
        };
        if let Some(fill) = hover_fill {
            ui.painter().circle_filled(rect.center(), diameter * 0.5, fill);
        }
        icons::paint(ui.painter(), icon, rect.center(), diameter * 0.34, icon_color);
    }

    response
}

/// Fill/kicker color pairing for [`tag`] — mirrors `.tag-accent` /
/// `.tag-accent-2` / `.tag-neutral` / `.tag-outline`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagStyle {
    /// Terracotta tint — the user's decisions/labels.
    Accent,
    /// Sage tint — Jaymi's proposals/context ("Review before action").
    Accent2,
    /// Neutral card2 fill — file paths, plain metadata chips.
    Neutral,
    /// Hairline outline only.
    Outline,
}

/// A small pill label, non-interactive (chips, kickers, status badges).
pub fn tag(ui: &mut Ui, theme: &Theme, label: &str, style: TagStyle) -> Response {
    let font = egui::FontId::new(type_size::META, egui::FontFamily::Proportional);
    let galley = ui.painter().layout_no_wrap(label.to_string(), font.clone(), Color32::PLACEHOLDER);
    let size = egui::vec2(galley.size().x + space::SM * 2.0, 22.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());

    if ui.is_rect_visible(rect) {
        let (fill, text_color, outline) = match style {
            TagStyle::Accent => (Some(theme.accent_tint), theme.accent_deep, None),
            TagStyle::Accent2 => (Some(theme.accent2_tint), theme.accent2_deep, None),
            TagStyle::Neutral => (Some(theme.surface_alt), theme.text_secondary, None),
            TagStyle::Outline => (None, theme.accent, Some(theme.accent)),
        };
        let corner = egui::CornerRadius::same(radius::PILL as u8);
        if let Some(fill) = fill {
            ui.painter().rect_filled(rect, corner, fill);
        }
        if let Some(outline) = outline {
            ui.painter().rect_stroke(
                rect,
                corner,
                egui::Stroke::new(stroke::HAIRLINE, outline),
                egui::StrokeKind::Inside,
            );
        }
        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, label, font, text_color);
    }

    response
}

/// An `egui::Frame` pre-configured for the Organic card surface: rounded
/// XL, card fill, soft elevation, no border (shadow carries the edge).
pub fn card_frame(theme: &Theme) -> egui::Frame {
    egui::Frame::new()
        .fill(theme.surface)
        .corner_radius(egui::CornerRadius::same(radius::XL as u8))
        .shadow(theme.shadow_sm())
        .inner_margin(egui::Margin::same(space::MD as i8))
}

/// An `egui::Frame` for the workspace panel shell (`--panel`, larger radius,
/// no shadow — it sits flush against the ground).
pub fn panel_frame(theme: &Theme) -> egui::Frame {
    egui::Frame::new()
        .fill(theme.panel)
        .corner_radius(egui::CornerRadius::same(radius::XL as u8))
}

/// A segmented control (e.g. Light / Dark). Returns `Some(index)` of the
/// option that was clicked this frame.
pub fn segmented(ui: &mut Ui, theme: &Theme, options: &[&str], selected: usize) -> Option<usize> {
    let mut clicked = None;
    egui::Frame::new()
        .fill(theme.surface_alt)
        .corner_radius(egui::CornerRadius::same(radius::PILL as u8))
        .inner_margin(egui::Margin::same(4))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                for (index, option) in options.iter().enumerate() {
                    let is_selected = index == selected;
                    let font = egui::FontId::new(
                        type_size::UI,
                        egui::FontFamily::Name("figtree-semibold".into()),
                    );
                    let galley =
                        ui.painter().layout_no_wrap((*option).to_string(), font.clone(), Color32::PLACEHOLDER);
                    let size = egui::vec2(galley.size().x + space::MD * 1.2, 28.0);
                    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
                    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
                    if response.clicked() && !is_selected {
                        clicked = Some(index);
                    }
                    if ui.is_rect_visible(rect) {
                        let corner = egui::CornerRadius::same(radius::PILL as u8);
                        if is_selected {
                            ui.painter().rect_filled(rect, corner, theme.surface);
                        } else if response.hovered() {
                            ui.painter().rect_filled(rect, corner, theme.selection());
                        }
                        let text_color = if is_selected { theme.text_primary } else { theme.text_secondary };
                        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, *option, font, text_color);
                    }
                }
            });
        });
    clicked
}

/// A rounded, elevated suggestion pill (conversation empty-state prompts).
pub fn suggestion_chip(ui: &mut Ui, theme: &Theme, label: &str) -> Response {
    egui::Frame::new()
        .fill(theme.surface)
        .corner_radius(egui::CornerRadius::same(radius::PILL as u8))
        .shadow(theme.shadow_sm())
        .inner_margin(egui::Margin::symmetric(space::MD as i8, (space::SM + 2.0) as i8))
        .show(ui, |ui| {
            // `horizontal_wrapped` sizes children against the row's remaining
            // width; a wrapping label measures against that shrunken width
            // instead of its own natural size, so a chip landing near the
            // row's edge can fold into a one-character-per-line column
            // before the layout ever moves it to the next row. Force a
            // single line so the chip always measures its true width.
            ui.add(egui::Label::new(
                egui::RichText::new(label).size(type_size::UI).color(theme.text_primary),
            ).wrap_mode(egui::TextWrapMode::Extend));
        })
        .response
        .interact(Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Oscillating alpha in `[0.35, 1.0]` on a `period_secs` sine cycle — the
/// "breathing" sage-dot presence indicator, never a spinner.
pub fn pulse_alpha(ui: &egui::Ui, period_secs: f32) -> f32 {
    let t = ui.input(|i| i.time) as f32;
    let phase = (t / period_secs) * std::f32::consts::TAU;
    0.35 + 0.65 * (0.5 + 0.5 * phase.sin())
}

/// Blend two colors by `t` (0 = `a`, 1 = `b`) in sRGB space — good enough for
/// button hover/press steps at this UI scale.
fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let lerp = |x: u8, y: u8| -> u8 { (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8 };
    Color32::from_rgb(lerp(a.r(), b.r()), lerp(a.g(), b.g()), lerp(a.b(), b.b()))
}
