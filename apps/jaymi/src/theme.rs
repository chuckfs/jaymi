//! Central application theme for Jaymi's desktop UI.
//!
//! Every visible surface in the surrounding shell (conversation, Coding chrome,
//! Explorer, overlays, status) derives colors from [`Theme`]. Monaco keeps its
//! own editor themes (`jaymi-light` / `jaymi-dark`) and is not painted with
//! egui `Color32` tokens — it receives a separate Monaco theme definition when
//! light/dark mode changes.

use eframe::egui::{self, Color32, CornerRadius, Stroke, Visuals};

use jaymi_config::Theme as ThemePreference;

/// 8px spacing system for shell layout (prefer whitespace over borders).
pub mod space {
    /// Extra-tight: icon padding, meta gaps.
    pub const XS: f32 = 4.0;
    /// Default small gap between related controls.
    pub const SM: f32 = 8.0;
    /// Section padding / standard inset.
    pub const MD: f32 = 16.0;
    /// Breathing room between major blocks.
    pub const LG: f32 = 24.0;
    /// Large empty-state / hero spacing.
    pub const XL: f32 = 32.0;
}

/// Corner radii — keep few steps so chrome feels cohesive.
pub mod radius {
    /// Rows, chips, small buttons.
    pub const XS: f32 = 4.0;
    /// Compact controls / icon tiles.
    pub const SM: f32 = 6.0;
    /// Inputs, status chips, dock chrome.
    pub const MD: f32 = 8.0;
    /// Conversation bubbles, large panels.
    pub const LG: f32 = 12.0;
    /// Floating composer / elevated chat chrome.
    pub const XL: f32 = 16.0;
}

/// Typography scale (points) for shell chrome — Monaco keeps its own size.
pub mod type_size {
    /// Day separators, paths, meta labels, hints.
    pub const META: f32 = 12.0;
    /// Toolbar buttons, panel headers, chrome labels.
    pub const UI: f32 = 13.0;
    /// Body copy in chat and dense panels.
    pub const BODY: f32 = 14.0;
    /// Section headlines.
    pub const TITLE: f32 = 16.0;
    /// Application brand / empty-state section title.
    pub const DISPLAY: f32 = 22.0;
    /// Centered welcome hero ("Hi, I'm Jaymi").
    pub const WELCOME: f32 = 42.0;
}

/// Stroke widths — hairlines only; prefer fill hierarchy over thick borders.
pub mod stroke {
    /// Separators, focus cues, soft outlines.
    pub const HAIRLINE: f32 = 1.0;
}

/// Horizontal + vertical inset using the 8px grid.
pub fn inset(horizontal: f32, vertical: f32) -> egui::Margin {
    egui::Margin::symmetric(horizontal as i8, vertical as i8)
}

/// Resolved light or dark appearance (after System preference is expanded).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
    /// Bright, macOS-like productivity look.
    Light,
    /// Continuous dark surface (not pure black).
    Dark,
}

impl ThemeMode {
    /// Whether this mode is dark.
    pub fn is_dark(self) -> bool {
        matches!(self, Self::Dark)
    }
}

/// Application-wide color tokens for the Jaymi shell UI.
///
/// Build with [`Theme::light`], [`Theme::dark`], or [`Theme::resolve`].
/// Labels, headings, buttons, borders, separators, and icons should read from
/// these fields (or from egui `Visuals` produced by [`Theme::apply_egui`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    /// Light or dark variant.
    pub mode: ThemeMode,
    /// Window / panel base fill.
    pub background: Color32,
    /// Elevated surfaces (composer, cards that must exist, overlays).
    pub surface: Color32,
    /// Alternate elevated fill (status bars, secondary strips, zebra rows).
    pub surface_alt: Color32,
    /// Hairline separators and widget outlines.
    pub border: Color32,
    /// Primary text and icons.
    pub text_primary: Color32,
    /// Muted / secondary text and icons.
    pub text_secondary: Color32,
    /// Interactive accent (links, primary buttons, focus).
    pub accent: Color32,
    /// Success / healthy status.
    pub success: Color32,
    /// Warning status.
    pub warning: Color32,
    /// Error / destructive status.
    pub error: Color32,
}

impl Theme {
    /// Modern native light palette (bright, high contrast, whitespace-driven).
    pub fn light() -> Self {
        Self {
            mode: ThemeMode::Light,
            background: Color32::from_rgb(252, 252, 253),
            surface: Color32::from_rgb(245, 245, 247),
            surface_alt: Color32::from_rgb(238, 238, 241),
            border: Color32::from_rgb(220, 220, 224),
            text_primary: Color32::from_rgb(29, 29, 31),
            text_secondary: Color32::from_rgb(110, 110, 115),
            accent: Color32::from_rgb(36, 99, 235),
            success: Color32::from_rgb(46, 140, 70),
            warning: Color32::from_rgb(176, 120, 16),
            error: Color32::from_rgb(196, 52, 52),
        }
    }

    /// Continuous dark surface (avoids pure black and isolated panels).
    pub fn dark() -> Self {
        Self {
            mode: ThemeMode::Dark,
            background: Color32::from_rgb(28, 28, 30),
            surface: Color32::from_rgb(38, 38, 41),
            surface_alt: Color32::from_rgb(48, 48, 52),
            border: Color32::from_rgb(58, 58, 62),
            text_primary: Color32::from_rgb(245, 245, 247),
            text_secondary: Color32::from_rgb(160, 160, 168),
            accent: Color32::from_rgb(90, 148, 255),
            success: Color32::from_rgb(90, 200, 120),
            warning: Color32::from_rgb(240, 180, 72),
            error: Color32::from_rgb(255, 105, 97),
        }
    }

    /// Resolve a persisted preference against the OS appearance and accent.
    pub fn resolve(preference: ThemePreference, system_dark: bool) -> Self {
        Self::resolve_with_accent(
            preference,
            system_dark,
            crate::system_accent::system_accent_color(),
        )
    }

    /// Resolve theme colors, optionally overriding the interactive accent.
    pub fn resolve_with_accent(
        preference: ThemePreference,
        system_dark: bool,
        system_accent: Option<Color32>,
    ) -> Self {
        let mut theme = match preference {
            ThemePreference::Light => Self::light(),
            ThemePreference::Dark => Self::dark(),
            ThemePreference::System => {
                if system_dark {
                    Self::dark()
                } else {
                    Self::light()
                }
            }
        };
        if let Some(accent) = system_accent {
            theme.accent = crate::system_accent::accent_for_mode(accent, theme.mode.is_dark());
        }
        theme
    }

    /// Soft selection / hover fill derived from [`Self::accent`].
    pub fn selection(&self) -> Color32 {
        let alpha = if self.mode.is_dark() { 64 } else { 48 };
        Color32::from_rgba_unmultiplied(self.accent.r(), self.accent.g(), self.accent.b(), alpha)
    }

    /// Text / icon color drawn on top of an accent fill (primary buttons).
    pub fn on_accent(&self) -> Color32 {
        crate::system_accent::contrasting_on_accent(self.accent)
    }

    /// Modal backdrop scrim for command palette / quick open.
    pub fn overlay_scrim(&self) -> Color32 {
        let alpha = if self.mode.is_dark() { 160 } else { 120 };
        Color32::from_rgba_unmultiplied(0, 0, 0, alpha)
    }

    /// Soft drop shadow for floating chrome (composer).
    pub fn elevation_shadow(&self) -> egui::Shadow {
        let alpha = if self.mode.is_dark() { 90 } else { 36 };
        egui::Shadow {
            offset: [0, 6],
            blur: 18,
            spread: 0,
            color: Color32::from_rgba_unmultiplied(0, 0, 0, alpha),
        }
    }

    /// Monaco theme id registered in the WebView (`jaymi-light` / `jaymi-dark`).
    ///
    /// Monaco continues to use its own editor theme system; surrounding UI uses
    /// the Jaymi [`Theme`] tokens via egui.
    pub fn monaco_theme_id(&self) -> &'static str {
        match self.mode {
            ThemeMode::Light => "jaymi-light",
            ThemeMode::Dark => "jaymi-dark",
        }
    }

    /// Monaco `base` theme for `defineTheme` (`vs` / `vs-dark`).
    pub fn monaco_base(&self) -> &'static str {
        match self.mode {
            ThemeMode::Light => "vs",
            ThemeMode::Dark => "vs-dark",
        }
    }

    /// JSON object passed to `monaco.editor.defineTheme` (editor-only colors).
    ///
    /// Syntax token colors live in the Monaco theme; chrome colors follow the
    /// current light/dark shell so the editor sits flush with the workspace.
    pub fn monaco_definition_json(&self) -> String {
        let bg = hex(self.background);
        let fg = hex(self.text_primary);
        let muted = hex(self.text_secondary);
        let gutter = hex(self.text_secondary);
        let line = hex(self.surface_alt);
        let selection = hex_opaque(self.selection(), self.background);
        let cursor = hex(self.text_primary);
        let border = hex(self.border);
        let accent = hex(self.accent);
        let surface = hex(self.surface);
        let error = hex(self.error);
        let warning = hex(self.warning);

        // Monaco-owned token foregrounds (hex without '#').
        let (comment, string, keyword, number, typ, constant) = match self.mode {
            ThemeMode::Light => ("6B7280", "0F7B6C", "1D4ED8", "B45309", "7C3AED", "BE185D"),
            ThemeMode::Dark => ("9CA3AF", "5EEAD4", "93C5FD", "FBBF24", "C4B5FD", "F9A8D4"),
        };
        let fg_token = fg.trim_start_matches('#').to_string();
        let muted_token = muted.trim_start_matches('#').to_string();

        let definition = serde_json::json!({
            "base": self.monaco_base(),
            "inherit": true,
            "rules": [
                { "token": "comment", "foreground": comment, "fontStyle": "italic" },
                { "token": "string", "foreground": string },
                { "token": "keyword", "foreground": keyword },
                { "token": "number", "foreground": number },
                { "token": "type", "foreground": typ },
                { "token": "class", "foreground": typ },
                { "token": "interface", "foreground": typ },
                { "token": "struct", "foreground": typ },
                { "token": "enum", "foreground": typ },
                { "token": "constant", "foreground": constant },
                { "token": "variable", "foreground": fg_token },
                { "token": "delimiter", "foreground": muted_token },
                { "token": "operator", "foreground": muted_token }
            ],
            "colors": {
                "editor.background": bg,
                "editor.foreground": fg,
                "editorLineNumber.foreground": gutter,
                "editorLineNumber.activeForeground": muted,
                "editorCursor.foreground": cursor,
                "editor.selectionBackground": selection,
                "editor.inactiveSelectionBackground": selection,
                "editor.lineHighlightBackground": line,
                "editor.lineHighlightBorder": "#00000000",
                "editorWhitespace.foreground": border,
                "editorIndentGuide.background": border,
                "editorIndentGuide.activeBackground": muted,
                "editorWidget.background": surface,
                "editorWidget.border": border,
                "editorSuggestWidget.background": surface,
                "editorSuggestWidget.border": border,
                "editorSuggestWidget.selectedBackground": selection,
                "editorHoverWidget.background": surface,
                "editorHoverWidget.border": border,
                "editorGutter.background": bg,
                "editorOverviewRuler.border": "#00000000",
                "scrollbar.shadow": "#00000000",
                "scrollbarSlider.background": format!("{border}99"),
                "scrollbarSlider.hoverBackground": format!("{muted}99"),
                "scrollbarSlider.activeBackground": format!("{accent}99"),
                "minimap.background": bg,
                "focusBorder": accent,
                "editorError.foreground": error,
                "editorWarning.foreground": warning,
                "editorBracketMatch.background": selection,
                "editorBracketMatch.border": accent
            }
        });
        definition.to_string()
    }

    /// Apply this theme to the egui context (panels, widgets, selection).
    pub fn apply_egui(&self, ctx: &egui::Context) {
        ctx.set_visuals(self.to_egui_visuals());
    }

    /// Build egui [`Visuals`] from theme tokens so widgets inherit Theme colors.
    pub fn to_egui_visuals(&self) -> Visuals {
        let mut visuals = if self.mode.is_dark() {
            Visuals::dark()
        } else {
            Visuals::light()
        };

        let selection = self.selection();
        let on_accent = self.on_accent();

        visuals.dark_mode = self.mode.is_dark();
        visuals.panel_fill = self.background;
        visuals.window_fill = self.surface;
        visuals.extreme_bg_color = self.surface_alt;
        visuals.faint_bg_color = self.surface;
        visuals.code_bg_color = self.background;
        visuals.override_text_color = Some(self.text_primary);
        visuals.hyperlink_color = self.accent;
        visuals.warn_fg_color = self.warning;
        visuals.error_fg_color = self.error;

        visuals.selection.bg_fill = selection;
        // Soft accent outline for keyboard selection / focus (fill-first elsewhere).
        visuals.selection.stroke = Stroke::new(stroke::HAIRLINE, self.accent);

        // Prefer fill hierarchy over outlined chrome — inactive controls are
        // borderless; hover uses a soft selection wash without an accent ring.
        let hairline = Stroke::new(stroke::HAIRLINE, self.border);
        visuals.widgets.noninteractive.bg_fill = self.background;
        visuals.widgets.noninteractive.weak_bg_fill = self.surface;
        visuals.widgets.noninteractive.bg_stroke = Stroke::NONE;
        visuals.widgets.noninteractive.fg_stroke = Stroke::new(stroke::HAIRLINE, self.text_primary);

        visuals.widgets.inactive.bg_fill = self.surface;
        visuals.widgets.inactive.weak_bg_fill = self.surface;
        visuals.widgets.inactive.bg_stroke = Stroke::NONE;
        visuals.widgets.inactive.fg_stroke = Stroke::new(stroke::HAIRLINE, self.text_secondary);

        visuals.widgets.hovered.bg_fill = selection;
        visuals.widgets.hovered.weak_bg_fill = selection;
        visuals.widgets.hovered.bg_stroke = Stroke::NONE;
        visuals.widgets.hovered.fg_stroke = Stroke::new(stroke::HAIRLINE, self.text_primary);

        visuals.widgets.active.bg_fill = self.accent;
        visuals.widgets.active.weak_bg_fill = self.accent;
        visuals.widgets.active.bg_stroke = Stroke::NONE;
        visuals.widgets.active.fg_stroke = Stroke::new(stroke::HAIRLINE, on_accent);

        visuals.widgets.open.bg_fill = self.surface;
        visuals.widgets.open.weak_bg_fill = self.surface;
        visuals.widgets.open.bg_stroke = hairline;
        visuals.widgets.open.fg_stroke = Stroke::new(stroke::HAIRLINE, self.text_primary);

        visuals.window_stroke = hairline;
        visuals.window_corner_radius = CornerRadius::same(radius::MD as u8);
        visuals.menu_corner_radius = CornerRadius::same(radius::MD as u8);
        let shadow_alpha = if self.mode.is_dark() { 72 } else { 36 };
        visuals.popup_shadow.color = Color32::from_rgba_unmultiplied(0, 0, 0, shadow_alpha);

        visuals
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::light()
    }
}

fn hex(color: Color32) -> String {
    format!("#{:02X}{:02X}{:02X}", color.r(), color.g(), color.b())
}

/// Blend a (possibly translucent) selection color onto `base` for Monaco hex colors.
fn hex_opaque(tint: Color32, base: Color32) -> String {
    let alpha = f32::from(tint.a()) / 255.0;
    let blend = |c: u8, b: u8| -> u8 {
        ((f32::from(c) * alpha) + (f32::from(b) * (1.0 - alpha))).round() as u8
    };
    format!(
        "#{:02X}{:02X}{:02X}",
        blend(tint.r(), base.r()),
        blend(tint.g(), base.g()),
        blend(tint.b(), base.b())
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_and_dark_expose_required_tokens() {
        for theme in [Theme::light(), Theme::dark()] {
            assert_ne!(theme.background, theme.text_primary);
            assert_ne!(theme.surface, theme.surface_alt);
            assert!(!theme.monaco_theme_id().is_empty());
            let json = theme.monaco_definition_json();
            assert!(json.contains("editor.background"));
            assert!(json.contains(theme.monaco_base()));
        }
    }

    #[test]
    fn resolve_honors_preference_and_system() {
        assert_eq!(
            Theme::resolve(ThemePreference::Light, true).mode,
            ThemeMode::Light
        );
        assert_eq!(
            Theme::resolve(ThemePreference::Dark, false).mode,
            ThemeMode::Dark
        );
        assert_eq!(
            Theme::resolve(ThemePreference::System, true).mode,
            ThemeMode::Dark
        );
        assert_eq!(
            Theme::resolve(ThemePreference::System, false).mode,
            ThemeMode::Light
        );
    }

    #[test]
    fn egui_visuals_follow_mode() {
        let light = Theme::light().to_egui_visuals();
        assert!(!light.dark_mode);
        assert_eq!(light.panel_fill, Theme::light().background);

        let dark = Theme::dark().to_egui_visuals();
        assert!(dark.dark_mode);
        assert_eq!(dark.panel_fill, Theme::dark().background);
    }

    #[test]
    fn resolve_applies_system_accent_when_provided() {
        let orange = Color32::from_rgb(255, 149, 0);
        let theme = Theme::resolve_with_accent(ThemePreference::Light, false, Some(orange));
        assert_eq!(theme.accent, orange);
    }

    #[test]
    fn on_accent_contrasts_with_bright_accent() {
        let theme = Theme::resolve_with_accent(
            ThemePreference::Light,
            false,
            Some(Color32::from_rgb(255, 149, 0)),
        );
        // Orange is mid-luminance; on_accent should remain readable (not equal to accent).
        assert_ne!(theme.on_accent(), theme.accent);
    }

    #[test]
    fn theme_avoids_black_and_white_constants() {
        // Palette values are explicit RGB; UI code must not use Color32::BLACK / WHITE.
        for theme in [Theme::light(), Theme::dark()] {
            assert_ne!(theme.background, Color32::from_rgb(0, 0, 0));
            assert_ne!(theme.text_primary, Color32::from_rgb(0, 0, 0));
        }
    }
}
