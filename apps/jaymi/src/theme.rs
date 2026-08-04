//! Central application theme for Jaymi's desktop UI.
//!
//! Every surface (conversation, Coding chrome, Explorer, Monaco, overlays)
//! derives colors from [`Theme`] instead of scattering RGB constants.
//! Monaco themes (`jaymi-light` / `jaymi-dark`) are generated from the same
//! palette so the editor blends into the workspace.

use eframe::egui::{self, Color32, CornerRadius, Stroke, Visuals};

use jaymi_config::Theme as ThemePreference;

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

/// Application-wide color tokens.
///
/// Build with [`Theme::light`], [`Theme::dark`], or [`Theme::resolve`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    /// Light or dark variant.
    pub mode: ThemeMode,
    /// Window / panel base fill.
    pub background: Color32,
    /// Elevated surfaces (composer, cards that must exist, overlays).
    pub surface: Color32,
    /// Primary text.
    pub foreground: Color32,
    /// Muted / secondary text.
    pub secondary_foreground: Color32,
    /// Hairline separators (prefer over heavy borders).
    pub border: Color32,
    /// Interactive accent (links, primary buttons, focus).
    pub accent: Color32,
    /// Selection / highlight fill.
    pub selection: Color32,
    /// Error / destructive status.
    pub error: Color32,
    /// Warning status.
    pub warning: Color32,
    /// Success / healthy status.
    pub success: Color32,
    /// Text on accent fills (e.g. primary button label).
    pub accent_foreground: Color32,
    /// Modal backdrop scrim.
    pub overlay_scrim: Color32,
    /// Current-line highlight in Monaco (soft).
    pub line_highlight: Color32,
    /// Editor cursor.
    pub cursor: Color32,
    /// Subtle gutter / line-number color.
    pub gutter_foreground: Color32,
}

impl Theme {
    /// Modern native light palette (bright, high contrast, whitespace-driven).
    pub fn light() -> Self {
        Self {
            mode: ThemeMode::Light,
            background: Color32::from_rgb(252, 252, 253),
            surface: Color32::from_rgb(245, 245, 247),
            foreground: Color32::from_rgb(29, 29, 31),
            secondary_foreground: Color32::from_rgb(110, 110, 115),
            border: Color32::from_rgb(220, 220, 224),
            accent: Color32::from_rgb(36, 99, 235),
            selection: Color32::from_rgba_unmultiplied(36, 99, 235, 48),
            error: Color32::from_rgb(196, 52, 52),
            warning: Color32::from_rgb(176, 120, 16),
            success: Color32::from_rgb(46, 140, 70),
            accent_foreground: Color32::WHITE,
            overlay_scrim: Color32::from_black_alpha(120),
            line_highlight: Color32::from_rgb(240, 242, 246),
            cursor: Color32::from_rgb(29, 29, 31),
            gutter_foreground: Color32::from_rgb(160, 160, 168),
        }
    }

    /// Continuous dark surface (avoids pure black and isolated panels).
    pub fn dark() -> Self {
        Self {
            mode: ThemeMode::Dark,
            background: Color32::from_rgb(28, 28, 30),
            surface: Color32::from_rgb(38, 38, 41),
            foreground: Color32::from_rgb(245, 245, 247),
            secondary_foreground: Color32::from_rgb(160, 160, 168),
            border: Color32::from_rgb(58, 58, 62),
            accent: Color32::from_rgb(90, 148, 255),
            selection: Color32::from_rgba_unmultiplied(90, 148, 255, 64),
            error: Color32::from_rgb(255, 105, 97),
            warning: Color32::from_rgb(240, 180, 72),
            success: Color32::from_rgb(90, 200, 120),
            accent_foreground: Color32::from_rgb(20, 24, 32),
            overlay_scrim: Color32::from_black_alpha(160),
            line_highlight: Color32::from_rgb(36, 36, 40),
            cursor: Color32::from_rgb(245, 245, 247),
            gutter_foreground: Color32::from_rgb(110, 110, 118),
        }
    }

    /// Resolve a persisted preference against the OS appearance.
    pub fn resolve(preference: ThemePreference, system_dark: bool) -> Self {
        match preference {
            ThemePreference::Light => Self::light(),
            ThemePreference::Dark => Self::dark(),
            ThemePreference::System => {
                if system_dark {
                    Self::dark()
                } else {
                    Self::light()
                }
            }
        }
    }

    /// Monaco theme id registered in the WebView (`jaymi-light` / `jaymi-dark`).
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

    /// JSON object passed to `monaco.editor.defineTheme` (colors + token rules).
    pub fn monaco_definition_json(&self) -> String {
        let bg = hex(self.background);
        let fg = hex(self.foreground);
        let muted = hex(self.secondary_foreground);
        let gutter = hex(self.gutter_foreground);
        let line = hex(self.line_highlight);
        let selection = hex_opaque(self.selection, self.background);
        let cursor = hex(self.cursor);
        let border = hex(self.border);
        let accent = hex(self.accent);
        let surface = hex(self.surface);
        let error = hex(self.error);
        let warning = hex(self.warning);

        // Token foregrounds are hex without '#'. Prefer readability over decoration.
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

    /// Build egui [`Visuals`] from theme tokens.
    pub fn to_egui_visuals(&self) -> Visuals {
        let mut visuals = if self.mode.is_dark() {
            Visuals::dark()
        } else {
            Visuals::light()
        };

        visuals.dark_mode = self.mode.is_dark();
        visuals.panel_fill = self.background;
        visuals.window_fill = self.surface;
        visuals.extreme_bg_color = self.surface;
        visuals.faint_bg_color = self.surface;
        visuals.code_bg_color = self.background;
        visuals.override_text_color = Some(self.foreground);
        visuals.hyperlink_color = self.accent;
        visuals.warn_fg_color = self.warning;
        visuals.error_fg_color = self.error;

        visuals.selection.bg_fill = self.selection;
        visuals.selection.stroke = Stroke::new(1.0, self.accent);

        let weak = Stroke::new(1.0, self.border);
        visuals.widgets.noninteractive.bg_fill = self.background;
        visuals.widgets.noninteractive.weak_bg_fill = self.surface;
        visuals.widgets.noninteractive.bg_stroke = weak;
        visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, self.foreground);

        visuals.widgets.inactive.bg_fill = self.surface;
        visuals.widgets.inactive.weak_bg_fill = self.surface;
        visuals.widgets.inactive.bg_stroke = weak;
        visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, self.secondary_foreground);

        visuals.widgets.hovered.bg_fill = self.selection;
        visuals.widgets.hovered.weak_bg_fill = self.selection;
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, self.accent);
        visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, self.foreground);

        visuals.widgets.active.bg_fill = self.accent;
        visuals.widgets.active.weak_bg_fill = self.accent;
        visuals.widgets.active.bg_stroke = Stroke::new(1.0, self.accent);
        visuals.widgets.active.fg_stroke = Stroke::new(1.0, self.accent_foreground);

        visuals.widgets.open.bg_fill = self.surface;
        visuals.widgets.open.weak_bg_fill = self.surface;
        visuals.widgets.open.bg_stroke = Stroke::new(1.0, self.accent);
        visuals.widgets.open.fg_stroke = Stroke::new(1.0, self.foreground);

        // Prefer separators over heavy boxed chrome.
        visuals.window_stroke = Stroke::new(1.0, self.border);
        visuals.window_corner_radius = CornerRadius::same(6);
        visuals.menu_corner_radius = CornerRadius::same(6);
        visuals.popup_shadow.color =
            Color32::from_black_alpha(if self.mode.is_dark() { 80 } else { 40 });

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
            assert_ne!(theme.background, theme.foreground);
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
}
