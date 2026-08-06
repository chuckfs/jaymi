//! Settings Workspace — preferences UI only.
//!
//! Settings exposes and persists user preferences. It never owns reasoning,
//! providers, discovery, or model state. The UI paints immutable Application
//! snapshots and emits user intents; Application coordinates Config, Planner,
//! and ModelRegistry.

use eframe::egui::{self, Align, Layout, RichText};

use crate::theme::{inset, radius, space, stroke, type_size, Theme};

/// Categories in the Settings left rail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SettingsCategory {
    /// App-wide defaults.
    #[default]
    General,
    /// Theme and chrome.
    Appearance,
    /// Reasoning providers and default model.
    Reasoning,
    /// Privacy and offline preferences.
    Privacy,
    /// Project defaults.
    Projects,
    /// Coding workspace defaults.
    Coding,
    /// Provider catalog (future multi-backend).
    Providers,
    /// User-facing diagnostics preferences.
    Diagnostics,
    /// About Jaymi.
    About,
}

impl SettingsCategory {
    /// All categories in display order.
    pub fn all() -> &'static [SettingsCategory] {
        &[
            Self::General,
            Self::Appearance,
            Self::Reasoning,
            Self::Privacy,
            Self::Projects,
            Self::Coding,
            Self::Providers,
            Self::Diagnostics,
            Self::About,
        ]
    }

    /// Short label for the rail.
    pub fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Appearance => "Appearance",
            Self::Reasoning => "Reasoning",
            Self::Privacy => "Privacy",
            Self::Projects => "Projects",
            Self::Coding => "Coding",
            Self::Providers => "Providers",
            Self::Diagnostics => "Diagnostics",
            Self::About => "About",
        }
    }
}

/// UI selection state for the Settings Workspace (not system state).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SettingsWorkspaceState {
    /// Active category.
    pub category: SettingsCategory,
}

/// Connection status for the Reasoning settings page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReasoningConnectionStatus {
    /// Provider reachable and usable.
    Connected,
    /// Refresh / probe in flight.
    Connecting,
    /// Provider unreachable (e.g. daemon not running).
    Offline,
    /// Reachable path failed for another reason.
    Error,
}

impl ReasoningConnectionStatus {
    /// Short label for the status pill.
    pub fn label(self) -> &'static str {
        match self {
            Self::Connected => "Connected",
            Self::Connecting => "Connecting",
            Self::Offline => "Offline",
            Self::Error => "Error",
        }
    }
}

/// One installed / discovered model for Settings (immutable snapshot).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReasoningSettingsModel {
    /// Provider registration id.
    pub provider_id: String,
    /// Model name within the provider.
    pub model_name: String,
    /// Display name.
    pub display_name: String,
    /// Parameter size label when known.
    pub parameter_size: Option<String>,
    /// Context length when known.
    pub context_length: Option<u64>,
    /// Quantization label when known.
    pub quantization: Option<String>,
    /// Local vs cloud.
    pub local: bool,
    /// Capability chip labels.
    pub capability_labels: Vec<String>,
    /// True when this is the current default.
    pub is_default: bool,
    /// True when the provider is currently usable.
    pub available: bool,
}

impl ReasoningSettingsModel {
    /// Stable selection key.
    pub fn selection_key(&self) -> String {
        format!("{}/{}", self.provider_id, self.model_name)
    }
}

/// One registered reasoning provider in the snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReasoningSettingsProvider {
    /// Registration id.
    pub id: String,
    /// Human display name.
    pub display_name: String,
    /// Mapped connection status for this provider.
    pub status: ReasoningConnectionStatus,
    /// Plain-language status detail.
    pub detail: String,
    /// Models discovered for this provider.
    pub model_count: usize,
}

/// Immutable Reasoning settings snapshot from Application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReasoningSettingsSnapshot {
    /// Overall status (primary / active provider aggregate).
    pub status: ReasoningConnectionStatus,
    /// Plain-language diagnostic for the status row.
    pub message: String,
    /// Active / preferred provider id when known.
    pub active_provider_id: Option<String>,
    /// Active / preferred provider display name.
    pub active_provider_name: Option<String>,
    /// Default model key `provider/name` when set.
    pub default_model_key: Option<String>,
    /// Providers (extensible — not Ollama-hardcoded).
    pub providers: Vec<ReasoningSettingsProvider>,
    /// Models across all providers.
    pub models: Vec<ReasoningSettingsModel>,
}

impl Default for ReasoningSettingsSnapshot {
    fn default() -> Self {
        Self {
            status: ReasoningConnectionStatus::Offline,
            message: "No reasoning providers are registered.".into(),
            active_provider_id: None,
            active_provider_name: None,
            default_model_key: None,
            providers: Vec::new(),
            models: Vec::new(),
        }
    }
}

/// User intents from the Settings Workspace (Application handles them).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsWorkspaceEvent {
    /// Switch category in the left rail.
    SelectCategory(SettingsCategory),
    /// Close Settings and return to Conversations.
    Close,
    /// User selected a default reasoning model.
    SelectDefaultModel {
        /// Provider id.
        provider_id: String,
        /// Model name within the provider.
        model_name: String,
    },
    /// Re-query providers / rebuild registry via Application.
    RefreshModels,
    /// Probe connection via Application.
    TestConnection,
}

/// Inputs required to paint Settings.
pub struct SettingsWorkspaceContext<'a> {
    /// Theme tokens.
    pub theme: &'a Theme,
    /// Local UI selection state.
    pub state: &'a SettingsWorkspaceState,
    /// Latest Reasoning snapshot from Application.
    pub reasoning: &'a ReasoningSettingsSnapshot,
    /// True while Application is refreshing / testing.
    pub busy: bool,
}

/// Render the Settings Workspace surface.
pub fn render_settings_workspace(
    ui: &mut egui::Ui,
    ctx: &SettingsWorkspaceContext<'_>,
    events: &mut Vec<SettingsWorkspaceEvent>,
) {
    let theme = ctx.theme;
    ui.horizontal(|ui| {
        ui.heading(
            RichText::new("Settings")
                .size(type_size::DISPLAY)
                .color(theme.text_primary),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui
                .add(
                    egui::Button::new(RichText::new("Done").size(type_size::UI))
                        .fill(theme.surface_alt)
                        .corner_radius(radius::MD),
                )
                .clicked()
            {
                events.push(SettingsWorkspaceEvent::Close);
            }
        });
    });
    ui.add_space(space::MD);
    ui.separator();

    egui::SidePanel::left("jaymi_settings_categories")
        .exact_width(200.0)
        .resizable(false)
        .frame(
            egui::Frame::new()
                .fill(theme.surface)
                .inner_margin(inset(space::SM, space::MD)),
        )
        .show_inside(ui, |ui| {
            ui.label(
                RichText::new("Categories")
                    .size(type_size::META)
                    .color(theme.text_secondary),
            );
            ui.add_space(space::SM);
            for category in SettingsCategory::all() {
                let selected = ctx.state.category == *category;
                let label = RichText::new(category.label()).size(type_size::UI).color(
                    if selected {
                        theme.accent
                    } else {
                        theme.text_primary
                    },
                );
                let response = ui.add_sized(
                    [ui.available_width(), 32.0],
                    egui::SelectableLabel::new(selected, label),
                );
                if response.clicked() {
                    events.push(SettingsWorkspaceEvent::SelectCategory(*category));
                }
                ui.add_space(space::XS);
            }
        });

    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(theme.background)
                .inner_margin(inset(space::LG, space::MD)),
        )
        .show_inside(ui, |ui| {
            match ctx.state.category {
                SettingsCategory::Reasoning => render_reasoning_page(ui, theme, ctx, events),
                other => render_coming_soon(ui, theme, other),
            }
        });
}

fn render_coming_soon(ui: &mut egui::Ui, theme: &Theme, category: SettingsCategory) {
    ui.heading(
        RichText::new(category.label())
            .size(type_size::TITLE)
            .color(theme.text_primary),
    );
    ui.add_space(space::SM);
    ui.label(
        RichText::new("Coming soon")
            .size(type_size::BODY)
            .color(theme.text_secondary),
    );
    ui.add_space(space::XS);
    ui.label(
        RichText::new("This settings category will be available in a later release.")
            .size(type_size::META)
            .color(theme.text_secondary),
    );
}

fn render_reasoning_page(
    ui: &mut egui::Ui,
    theme: &Theme,
    ctx: &SettingsWorkspaceContext<'_>,
    events: &mut Vec<SettingsWorkspaceEvent>,
) {
    egui::ScrollArea::vertical()
        .id_salt("jaymi_settings_reasoning_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            render_reasoning_page_body(ui, theme, ctx, events);
        });
}

fn render_reasoning_page_body(
    ui: &mut egui::Ui,
    theme: &Theme,
    ctx: &SettingsWorkspaceContext<'_>,
    events: &mut Vec<SettingsWorkspaceEvent>,
) {
    let snap = ctx.reasoning;
    ui.heading(
        RichText::new("Reasoning")
            .size(type_size::TITLE)
            .color(theme.text_primary),
    );
    ui.add_space(space::XS);
    ui.label(
        RichText::new("Choose how Jaymi talks to local and future reasoning providers.")
            .size(type_size::META)
            .color(theme.text_secondary),
    );
    ui.add_space(space::LG);

    // Status
    section_label(ui, theme, "Status");
    ui.add_space(space::SM);
    egui::Frame::new()
        .fill(theme.surface)
        .corner_radius(radius::LG)
        .inner_margin(inset(space::MD, space::MD))
        .stroke(egui::Stroke::new(stroke::HAIRLINE, theme.border))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                status_pill(ui, theme, if ctx.busy {
                    ReasoningConnectionStatus::Connecting
                } else {
                    snap.status
                });
                ui.add_space(space::SM);
                ui.label(
                    RichText::new(if ctx.busy {
                        "Updating…"
                    } else {
                        snap.message.as_str()
                    })
                    .size(type_size::BODY)
                    .color(theme.text_primary),
                );
            });
            ui.add_space(space::SM);
            if let Some(name) = &snap.active_provider_name {
                ui.label(
                    RichText::new(format!("Active provider · {name}"))
                        .size(type_size::META)
                        .color(theme.text_secondary),
                );
            }
            ui.add_space(space::MD);
            ui.horizontal(|ui| {
                let refresh = ui.add_enabled(
                    !ctx.busy,
                    egui::Button::new(RichText::new("Refresh Models").size(type_size::UI))
                        .corner_radius(radius::MD),
                );
                if refresh.clicked() {
                    events.push(SettingsWorkspaceEvent::RefreshModels);
                }
                let test = ui.add_enabled(
                    !ctx.busy,
                    egui::Button::new(RichText::new("Test Connection").size(type_size::UI))
                        .corner_radius(radius::MD),
                );
                if test.clicked() {
                    events.push(SettingsWorkspaceEvent::TestConnection);
                }
            });
        });

    ui.add_space(space::LG);
    section_label(ui, theme, "Default model");
    ui.add_space(space::SM);
    if snap.models.is_empty() {
        egui::Frame::new()
            .fill(theme.surface)
            .corner_radius(radius::LG)
            .inner_margin(inset(space::MD, space::MD))
            .show(ui, |ui| {
                ui.label(
                    RichText::new("No models discovered yet. Refresh after installing a model.")
                        .size(type_size::BODY)
                        .color(theme.text_secondary),
                );
            });
    } else {
        egui::Frame::new()
            .fill(theme.surface)
            .corner_radius(radius::LG)
            .inner_margin(inset(space::MD, space::SM))
            .stroke(egui::Stroke::new(stroke::HAIRLINE, theme.border))
            .show(ui, |ui| {
                let mut last_provider = String::new();
                for model in &snap.models {
                    if model.provider_id != last_provider {
                        if !last_provider.is_empty() {
                            ui.add_space(space::SM);
                        }
                        ui.label(
                            RichText::new(model.provider_id.to_uppercase())
                                .size(type_size::META)
                                .color(theme.text_secondary),
                        );
                        ui.add_space(space::XS);
                        last_provider = model.provider_id.clone();
                    }
                    let selected = snap
                        .default_model_key
                        .as_deref()
                        .is_some_and(|key| key == model.selection_key());
                    let response = ui.add_enabled(
                        model.available || selected,
                        egui::SelectableLabel::new(
                            selected,
                            RichText::new(format!(
                                "{}{}",
                                model.display_name,
                                if model.available { "" } else { " (unavailable)" }
                            ))
                            .size(type_size::BODY),
                        ),
                    );
                    if response.clicked() {
                        events.push(SettingsWorkspaceEvent::SelectDefaultModel {
                            provider_id: model.provider_id.clone(),
                            model_name: model.model_name.clone(),
                        });
                    }
                }
            });
    }

    ui.add_space(space::LG);
    section_label(ui, theme, "Installed models");
    ui.add_space(space::SM);
    for model in &snap.models {
        egui::Frame::new()
            .fill(theme.surface)
            .corner_radius(radius::LG)
            .inner_margin(inset(space::MD, space::MD))
            .stroke(egui::Stroke::new(stroke::HAIRLINE, theme.border))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(&model.display_name)
                            .size(type_size::BODY)
                            .strong()
                            .color(theme.text_primary),
                    );
                    if model.is_default {
                        ui.label(
                            RichText::new("Default")
                                .size(type_size::META)
                                .color(theme.accent),
                        );
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(if model.local { "Local" } else { "Cloud" })
                                .size(type_size::META)
                                .color(theme.text_secondary),
                        );
                    });
                });
                ui.add_space(space::XS);
                let mut meta = Vec::new();
                if let Some(params) = &model.parameter_size {
                    meta.push(params.clone());
                }
                if let Some(ctx_len) = model.context_length {
                    meta.push(format!("{ctx_len} context"));
                }
                if let Some(quant) = &model.quantization {
                    meta.push(quant.clone());
                }
                meta.push(model.provider_id.clone());
                ui.label(
                    RichText::new(meta.join(" · "))
                        .size(type_size::META)
                        .color(theme.text_secondary),
                );
                if !model.capability_labels.is_empty() {
                    ui.add_space(space::XS);
                    ui.horizontal_wrapped(|ui| {
                        for label in &model.capability_labels {
                            egui::Frame::new()
                                .fill(theme.surface_alt)
                                .corner_radius(radius::SM)
                                .inner_margin(inset(space::SM, 2.0))
                                .show(ui, |ui| {
                                    ui.label(
                                        RichText::new(label)
                                            .size(type_size::META)
                                            .color(theme.text_secondary),
                                    );
                                });
                        }
                    });
                }
            });
        ui.add_space(space::SM);
    }

    ui.add_space(space::LG);
    section_label(ui, theme, "Providers");
    ui.add_space(space::SM);
    for provider in &snap.providers {
        egui::Frame::new()
            .fill(theme.surface)
            .corner_radius(radius::LG)
            .inner_margin(inset(space::MD, space::MD))
            .stroke(egui::Stroke::new(stroke::HAIRLINE, theme.border))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(&provider.display_name)
                            .size(type_size::BODY)
                            .strong()
                            .color(theme.text_primary),
                    );
                    status_pill(ui, theme, provider.status);
                });
                ui.add_space(space::XS);
                ui.label(
                    RichText::new(format!(
                        "{} · {} model{}",
                        provider.detail,
                        provider.model_count,
                        if provider.model_count == 1 { "" } else { "s" }
                    ))
                    .size(type_size::META)
                    .color(theme.text_secondary),
                );
            });
        ui.add_space(space::SM);
    }
}

fn section_label(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.label(
        RichText::new(text.to_ascii_uppercase())
            .size(type_size::META)
            .color(theme.text_secondary),
    );
}

fn status_pill(ui: &mut egui::Ui, theme: &Theme, status: ReasoningConnectionStatus) {
    let (fill, text) = match status {
        ReasoningConnectionStatus::Connected => (theme.success.gamma_multiply(0.25), theme.success),
        ReasoningConnectionStatus::Connecting => {
            (theme.warning.gamma_multiply(0.25), theme.warning)
        }
        ReasoningConnectionStatus::Offline => {
            (theme.text_secondary.gamma_multiply(0.2), theme.text_secondary)
        }
        ReasoningConnectionStatus::Error => (theme.error.gamma_multiply(0.25), theme.error),
    };
    egui::Frame::new()
        .fill(fill)
        .corner_radius(radius::SM)
        .inner_margin(inset(space::SM, 2.0))
        .show(ui, |ui| {
            ui.label(
                RichText::new(status.label())
                    .size(type_size::META)
                    .color(text),
            );
        });
}
