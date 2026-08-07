//! Central application theme for Jaymi's desktop UI — the "Organic" system.
//!
//! Every visible surface in the surrounding shell (conversation, Coding chrome,
//! Explorer, overlays, status) derives colors from [`Theme`]. Monaco keeps its
//! own editor themes (`jaymi-light` / `jaymi-dark`) and is not painted with
//! egui `Color32` tokens — it receives a separate Monaco theme definition when
//! light/dark mode changes.
//!
//! Palette: a cream ground with a terracotta accent (the user's voice — their
//! words, actions, approvals) and a sage second accent (Jaymi's voice —
//! intelligence, context, anything the AI knows or proposes). Dark mode is
//! warm ink, never grey or pure black. See the design language reference for
//! the full rationale.

use eframe::egui::{self, Color32, CornerRadius, Stroke, Visuals};

use jaymi_config::Theme as ThemePreference;

/// Spacing system for shell layout — density 1.10x over an 8px grid (prefer
/// whitespace over borders; panels float on the ground rather than being
/// fenced in by dividers).
pub mod space {
    /// Extra-tight: icon padding, meta gaps.
    pub const XS: f32 = 4.4;
    /// Default small gap between related controls.
    pub const SM: f32 = 8.8;
    /// Section padding / standard inset.
    pub const MD: f32 = 17.6;
    /// Breathing room between major blocks.
    pub const LG: f32 = 26.4;
    /// Large empty-state / hero spacing.
    pub const XL: f32 = 35.2;
}

/// Corner radii — over-rounded containers, pill controls. Keep few steps so
/// chrome feels cohesive; lean round rather than sharp everywhere.
pub mod radius {
    /// Rare minimal rounding (small inline marks).
    pub const XS: f32 = 8.0;
    /// Compact controls / icon tiles.
    pub const SM: f32 = 12.0;
    /// Inputs, status chips, dock chrome — the Organic base radius.
    pub const MD: f32 = 16.0;
    /// Conversation bubbles, mid-size panels.
    pub const LG: f32 = 24.0;
    /// Floating composer / elevated cards / the workspace panel shell.
    pub const XL: f32 = 28.0;
    /// Anything tappable: buttons, tags, inputs, segmented controls.
    pub const PILL: f32 = 999.0;
}

/// Typography scale (points) for shell chrome — Monaco keeps its own size.
///
/// Caprasimo (see [`font::DISPLAY`]) speaks only at brand moments — the
/// welcome hero, workspace titles, plan titles. Figtree, the default egui
/// proportional family after `configure_fonts` runs, does everything else.
/// Never set Caprasimo below 18px.
pub mod type_size {
    /// Day separators, paths, meta labels, hints, kickers.
    pub const META: f32 = 11.5;
    /// Toolbar buttons, panel headers, chrome labels.
    pub const UI: f32 = 13.0;
    /// Body copy in chat and dense panels.
    pub const BODY: f32 = 14.5;
    /// Section headlines, workspace panel titles (Caprasimo).
    pub const TITLE: f32 = 19.0;
    /// Application brand / empty-state section title (Caprasimo).
    pub const DISPLAY: f32 = 22.0;
    /// Centered welcome hero ("Hi, I'm Jaymi") (Caprasimo).
    pub const WELCOME: f32 = 40.0;
}

/// Stroke widths — hairlines only; prefer fill hierarchy over thick borders.
pub mod stroke {
    /// Separators, focus cues, soft outlines.
    pub const HAIRLINE: f32 = 1.0;
    /// Icon strokes drawn with [`crate::ui::icons`] — rounder, heavier than a
    /// hairline, matching the pill geometry throughout the shell.
    pub const ICON: f32 = 2.75;
}

/// Font family names registered by `ui::configure_fonts`.
pub mod font {
    /// The Caprasimo display family — headings only, never below 18px.
    pub const DISPLAY: &str = "caprasimo";
}

/// Horizontal + vertical inset using the spacing grid.
pub fn inset(horizontal: f32, vertical: f32) -> egui::Margin {
    egui::Margin::symmetric(horizontal as i8, vertical as i8)
}

/// A Caprasimo [`egui::FontId`] at the given size. Panics-free even if the
/// family failed to load (egui falls back to its default family silently).
pub fn display_font(size: f32) -> egui::FontId {
    egui::FontId::new(size, egui::FontFamily::Name(font::DISPLAY.into()))
}

/// Resolved light or dark appearance (after System preference is expanded).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
    /// Cream ground, warm ink text.
    Light,
    /// Continuous warm-ink surface (never grey, never pure black).
    Dark,
}

impl ThemeMode {
    /// Whether this mode is dark.
    pub fn is_dark(self) -> bool {
        matches!(self, Self::Dark)
    }
}

/// Application-wide color tokens for the Jaymi shell UI ("Organic" system).
///
/// Build with [`Theme::light`], [`Theme::dark`], or [`Theme::resolve`].
/// Labels, headings, buttons, borders, separators, and icons should read from
/// these fields (or from egui `Visuals` produced by [`Theme::apply_egui`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    /// Light or dark variant.
    pub mode: ThemeMode,
    /// Window / panel base fill (the "ground").
    pub background: Color32,
    /// The workspace panel shell fill — sits between ground and card.
    pub panel: Color32,
    /// Elevated surfaces (composer, cards that must exist, overlays).
    pub surface: Color32,
    /// Alternate elevated fill (status bars, secondary strips, zebra rows,
    /// hover fill for card-level rows).
    pub surface_alt: Color32,
    /// Hairline separators and widget outlines — a soft tint, not a hard
    /// line; prefer whitespace or a shadow step over reaching for this.
    pub border: Color32,
    /// Primary text and icons.
    pub text_primary: Color32,
    /// Muted / secondary text and icons.
    pub text_secondary: Color32,
    /// Faintest text — timestamps, placeholder-weight labels.
    pub text_faint: Color32,
    /// Interactive accent (terracotta) — the user's voice: actions, approvals.
    pub accent: Color32,
    /// Deep accent step — text/icons on a tinted accent fill, pressed state.
    pub accent_deep: Color32,
    /// Soft accent step — user chat bubble fill.
    pub accent_soft: Color32,
    /// Tint accent step — subtle chip/badge fill.
    pub accent_tint: Color32,
    /// Text / icon color drawn on a solid accent fill (primary buttons).
    pub on_accent: Color32,
    /// Second accent (sage) — Jaymi's voice: intelligence, context, proposals.
    pub accent2: Color32,
    /// Deep second-accent step.
    pub accent2_deep: Color32,
    /// Soft second-accent step.
    pub accent2_soft: Color32,
    /// Tint second-accent step — "Review before action" / "Connected" chips.
    pub accent2_tint: Color32,
    /// Warm ink — terminal / code-block backgrounds, deepest surface.
    pub ink: Color32,
    /// Second ink step.
    pub ink2: Color32,
    /// Success / healthy status (kept in-palette: sage-leaning).
    pub success: Color32,
    /// Warning status (kept in-palette: warm amber).
    pub warning: Color32,
    /// Error / destructive status (kept in-palette: warm brick, distinct
    /// from the terracotta accent).
    pub error: Color32,
}

impl Theme {
    /// Organic light palette — cream ground, warm ink text.
    pub fn light() -> Self {
        Self {
            mode: ThemeMode::Light,
            background: Color32::from_rgb(0xf5, 0xea, 0xd8),
            panel: Color32::from_rgb(0xef, 0xe3, 0xcd),
            surface: Color32::from_rgb(0xf9, 0xf4, 0xed),
            surface_alt: Color32::from_rgb(0xee, 0xe7, 0xdb),
            border: Color32::from_rgba_unmultiplied(0x20, 0x1e, 0x1d, 31),
            text_primary: Color32::from_rgb(0x20, 0x1e, 0x1d),
            text_secondary: Color32::from_rgb(0x82, 0x79, 0x6a),
            text_faint: Color32::from_rgb(0xa1, 0x97, 0x86),
            accent: Color32::from_rgb(0xc6, 0x71, 0x39),
            accent_deep: Color32::from_rgb(0x8c, 0x49, 0x1a),
            accent_soft: Color32::from_rgb(0xff, 0xe1, 0xd0),
            accent_tint: Color32::from_rgb(0xff, 0xf2, 0xeb),
            on_accent: Color32::from_rgb(0xff, 0xf8, 0xf0),
            accent2: Color32::from_rgb(0x7a, 0x8a, 0x5e),
            accent2_deep: Color32::from_rgb(0x56, 0x63, 0x3f),
            accent2_soft: Color32::from_rgb(0xe1, 0xee, 0xcc),
            accent2_tint: Color32::from_rgb(0xf0, 0xfa, 0xe1),
            ink: Color32::from_rgb(0x2e, 0x2b, 0x25),
            ink2: Color32::from_rgb(0x47, 0x42, 0x38),
            success: Color32::from_rgb(0x56, 0x63, 0x3f),
            warning: Color32::from_rgb(0xa9, 0x63, 0x1f),
            error: Color32::from_rgb(0xb2, 0x3b, 0x2e),
        }
    }

    /// Organic dark palette — warm ink ground, never grey, never pure black.
    pub fn dark() -> Self {
        Self {
            mode: ThemeMode::Dark,
            background: Color32::from_rgb(0x20, 0x1e, 0x1d),
            panel: Color32::from_rgb(0x28, 0x25, 0x21),
            surface: Color32::from_rgb(0x2e, 0x2b, 0x25),
            surface_alt: Color32::from_rgb(0x3a, 0x36, 0x2e),
            border: Color32::from_rgba_unmultiplied(0xf5, 0xea, 0xd8, 36),
            text_primary: Color32::from_rgb(0xf2, 0xeb, 0xdd),
            text_secondary: Color32::from_rgb(0xc0, 0xb6, 0xa5),
            text_faint: Color32::from_rgb(0x82, 0x79, 0x6a),
            accent: Color32::from_rgb(0xf6, 0xa0, 0x6b),
            accent_deep: Color32::from_rgb(0xff, 0xc6, 0xa5),
            accent_soft: Color32::from_rgb(0x64, 0x33, 0x12),
            accent_tint: Color32::from_rgb(0x40, 0x23, 0x10),
            on_accent: Color32::from_rgb(0x40, 0x23, 0x10),
            accent2: Color32::from_rgb(0xae, 0xbf, 0x92),
            accent2_deep: Color32::from_rgb(0xcc, 0xdb, 0xb2),
            accent2_soft: Color32::from_rgb(0x3d, 0x47, 0x2b),
            accent2_tint: Color32::from_rgb(0x27, 0x2e, 0x1b),
            ink: Color32::from_rgb(0x19, 0x17, 0x15),
            ink2: Color32::from_rgb(0x2e, 0x2b, 0x25),
            success: Color32::from_rgb(0xcc, 0xdb, 0xb2),
            warning: Color32::from_rgb(0xe0, 0xa0, 0x5c),
            error: Color32::from_rgb(0xff, 0x95, 0x85),
        }
    }

    /// Resolve a persisted preference against the OS appearance.
    ///
    /// The Organic palette carries a fixed terracotta accent rather than the
    /// OS system accent color — pass `None` through to
    /// [`Self::resolve_with_accent`] so the per-mode default in [`Self::light`]
    /// / [`Self::dark`] wins. `crate::system_accent` stays available (and its
    /// `accent_for_mode`/`contrasting_on_accent` helpers stay reusable) for
    /// anyone who wants to opt back into system-accent theming later.
    pub fn resolve(preference: ThemePreference, system_dark: bool) -> Self {
        Self::resolve_with_accent(preference, system_dark, None)
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
        self.on_accent
    }

    /// Modal backdrop scrim for command palette / quick open.
    pub fn overlay_scrim(&self) -> Color32 {
        Color32::from_rgba_unmultiplied(self.ink.r(), self.ink.g(), self.ink.b(), 102)
    }

    /// Small elevation step — hairline-adjacent chrome (rows, small chips).
    pub fn shadow_sm(&self) -> egui::Shadow {
        let alpha = if self.mode.is_dark() { 102 } else { 36 };
        egui::Shadow {
            offset: [0, 1],
            blur: 4,
            spread: 0,
            color: Color32::from_rgba_unmultiplied(0, 0, 0, alpha),
        }
    }

    /// Mid elevation step — resting cards, the composer.
    pub fn shadow_md(&self) -> egui::Shadow {
        let alpha = if self.mode.is_dark() { 115 } else { 41 };
        egui::Shadow {
            offset: [0, 3],
            blur: 10,
            spread: 0,
            color: Color32::from_rgba_unmultiplied(0, 0, 0, alpha),
        }
    }

    /// Large elevation step — the command palette, floating overlays.
    pub fn shadow_lg(&self) -> egui::Shadow {
        let alpha = if self.mode.is_dark() { 140 } else { 56 };
        egui::Shadow {
            offset: [0, 12],
            blur: 32,
            spread: 0,
            color: Color32::from_rgba_unmultiplied(0, 0, 0, alpha),
        }
    }

    /// Soft drop shadow for floating chrome (composer). Alias of
    /// [`Self::shadow_md`] kept for existing call sites.
    pub fn elevation_shadow(&self) -> egui::Shadow {
        self.shadow_md()
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
        let border = hex_opaque(self.border, self.background);
        let accent = hex(self.accent);
        let surface = hex(self.surface);
        let error = hex(self.error);
        let warning = hex(self.warning);

        // Monaco-owned token foregrounds (hex without '#').
        let (comment, string, keyword, number, typ, constant) = match self.mode {
            ThemeMode::Light => ("8C7C68", "56633F", "8C491A", "B2622D", "728157", "8C491A"),
            ThemeMode::Dark => ("A9998A", "CCDBB2", "FFC6A5", "F6A06B", "AEBF92", "FFC6A5"),
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
    fn resolve_defaults_to_fixed_terracotta_accent() {
        // Organic keeps a fixed accent rather than the OS system accent.
        assert_eq!(Theme::resolve(ThemePreference::Light, false).accent, Theme::light().accent);
        assert_eq!(Theme::resolve(ThemePreference::Dark, true).accent, Theme::dark().accent);
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
