//! Global Command Palette — floating multi-source launcher (⌘P).
//!
//! Independent of the Coding Workspace. Ranking uses fuzzy scoring (shared with
//! the Command Registry); file/knowledge hits come from the Search Engine /
//! Planner. Selection returns [`PaletteAction`] for the host to dispatch.

use eframe::egui;

use jaymi_commands::score_text;

use crate::theme::{radius, space, stroke, type_size, Theme};

/// Which catalog a palette row came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PaletteSource {
    /// Registered projects.
    Project,
    /// Project files (Search Engine).
    File,
    /// Command Registry entries.
    Command,
    /// Capability Engine catalog.
    Capability,
    /// Recent / project conversations.
    Conversation,
    /// Project knowledge hits.
    Knowledge,
}

impl PaletteSource {
    /// Section label shown beside the row.
    pub fn label(self) -> &'static str {
        match self {
            Self::Project => "Project",
            Self::File => "File",
            Self::Command => "Command",
            Self::Capability => "Capability",
            Self::Conversation => "Conversation",
            Self::Knowledge => "Knowledge",
        }
    }
}

/// Host-side intent produced when the user executes a palette row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaletteAction {
    /// Run a Command Registry id (Planner / Application dispatch).
    RunCommand {
        /// Stable command id.
        id: String,
        /// Optional free-text argument.
        argument: Option<String>,
    },
    /// Open / switch to a project by id (Planner → open_project).
    OpenProject {
        /// Project id.
        project_id: String,
    },
    /// Open a file path in Coding (Planner-mediated).
    OpenFile {
        /// Absolute path.
        path: String,
    },
    /// Continue / switch to a conversation.
    OpenConversation {
        /// Conversation id.
        conversation_id: String,
    },
    /// Focus the conversation composer with an optional seed prompt.
    ContinueConversation {
        /// Optional prompt to insert.
        prompt: Option<String>,
    },
    /// Describe / surface a capability (status + Planner describe path).
    OpenCapability {
        /// Capability id string.
        capability_id: String,
    },
    /// Jump to knowledge search for a query / open a knowledge path when present.
    OpenKnowledge {
        /// Knowledge hit title.
        title: String,
        /// Optional filesystem path.
        path: Option<String>,
        /// Query that produced the hit.
        query: String,
    },
}

/// One ranked row in the palette.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteItem {
    /// Stable row id (unique within a results set).
    pub id: String,
    /// Primary label.
    pub title: String,
    /// Secondary detail (path, category, …).
    pub subtitle: Option<String>,
    /// Source catalog.
    pub source: PaletteSource,
    /// Extra fuzzy tokens.
    pub keywords: Vec<String>,
    /// Action on Enter / click.
    pub action: PaletteAction,
    /// Precomputed rank boost from the Search Engine (files / knowledge).
    pub engine_score: u32,
}

/// Fuzzy-rank a palette item against `query` (empty query → baseline score).
pub fn palette_item_score(item: &PaletteItem, query: &str) -> u32 {
    let query = query.trim();
    if query.is_empty() {
        return 1 + item.engine_score.min(50);
    }
    let needle = query.to_ascii_lowercase();
    let mut best = 0_u32;
    best = best.max(score_text(&item.title, &needle));
    if let Some(subtitle) = &item.subtitle {
        best = best.max(score_text(subtitle, &needle) / 2);
    }
    best = best.max(score_text(item.source.label(), &needle) / 3);
    for keyword in &item.keywords {
        best = best.max(score_text(keyword, &needle));
    }
    if best == 0 {
        return 0;
    }
    best.saturating_add(item.engine_score.min(200))
}

/// Inputs for building a palette catalog (host gathers; palette ranks).
#[derive(Debug, Clone)]
pub struct PaletteGatherInput {
    /// Command Registry descriptors.
    pub commands: Vec<PaletteCommandRef>,
    /// Projects `(id, name)`.
    pub projects: Vec<(String, String)>,
    /// Conversations `(id, title)`.
    pub conversations: Vec<(String, String)>,
    /// Capabilities `(id, title)`.
    pub capabilities: Vec<(String, String)>,
    /// Files from Search Engine `(path, title, score)`.
    pub files: Vec<(String, String, u32)>,
    /// Knowledge hits `(title, detail, path, score)`.
    pub knowledge: Vec<(String, String, Option<String>, u32)>,
    /// Current filter query.
    pub query: String,
}

/// Lightweight command metadata for palette rows (avoids pulling full descriptors).
#[derive(Debug, Clone)]
pub struct PaletteCommandRef {
    /// Command id.
    pub id: String,
    /// Display title.
    pub title: String,
    /// Category label.
    pub category: String,
    /// Keywords.
    pub keywords: Vec<String>,
    /// Optional keybinding label.
    pub keybinding: Option<String>,
    /// Optional argument prompt (marks that Enter should collect an argument).
    pub argument_prompt: Option<String>,
}

/// Build + fuzzy-rank the global palette catalog from host-provided sources.
pub fn gather_palette_items(input: &PaletteGatherInput) -> Vec<PaletteItem> {
    let query = input.query.trim();
    let mut items = Vec::new();

    for command in &input.commands {
        // Prefer human titles for the examples in the product brief.
        let mut keywords = command.keywords.clone();
        keywords.push(command.category.clone());
        if let Some(binding) = &command.keybinding {
            keywords.push(binding.clone());
        }
        items.push(PaletteItem {
            id: format!("command:{}", command.id),
            title: command.title.clone(),
            subtitle: Some(match &command.keybinding {
                Some(binding) if !binding.is_empty() => {
                    format!("{} · {binding}", command.category)
                }
                _ => command.category.clone(),
            }),
            source: PaletteSource::Command,
            keywords,
            action: PaletteAction::RunCommand {
                id: command.id.clone(),
                argument: None,
            },
            engine_score: 0,
        });
    }

    for (project_id, name) in &input.projects {
        items.push(PaletteItem {
            id: format!("project:{project_id}"),
            title: name.clone(),
            subtitle: Some(project_id.clone()),
            source: PaletteSource::Project,
            keywords: vec!["project".into(), "open".into(), "switch".into()],
            action: PaletteAction::OpenProject {
                project_id: project_id.clone(),
            },
            engine_score: 0,
        });
    }

    for (conversation_id, title) in &input.conversations {
        items.push(PaletteItem {
            id: format!("conversation:{conversation_id}"),
            title: title.clone(),
            subtitle: Some("Recent conversation".into()),
            source: PaletteSource::Conversation,
            keywords: vec!["chat".into(), "continue".into()],
            action: PaletteAction::OpenConversation {
                conversation_id: conversation_id.clone(),
            },
            engine_score: 0,
        });
    }

    for (capability_id, title) in &input.capabilities {
        items.push(PaletteItem {
            id: format!("capability:{capability_id}"),
            title: title.clone(),
            subtitle: Some("Capability".into()),
            source: PaletteSource::Capability,
            keywords: vec!["capability".into(), capability_id.clone()],
            action: PaletteAction::OpenCapability {
                capability_id: capability_id.clone(),
            },
            engine_score: 0,
        });
    }

    for (path, title, score) in &input.files {
        items.push(PaletteItem {
            id: format!("file:{path}"),
            title: title.clone(),
            subtitle: Some(path.clone()),
            source: PaletteSource::File,
            keywords: vec!["file".into()],
            action: PaletteAction::OpenFile { path: path.clone() },
            engine_score: *score,
        });
    }

    for (title, detail, path, score) in &input.knowledge {
        items.push(PaletteItem {
            id: format!("knowledge:{title}:{detail}"),
            title: title.clone(),
            subtitle: Some(detail.clone()),
            source: PaletteSource::Knowledge,
            keywords: vec!["knowledge".into()],
            action: PaletteAction::OpenKnowledge {
                title: title.clone(),
                path: path.clone(),
                query: query.to_string(),
            },
            engine_score: *score,
        });
    }

    let mut ranked = filter_palette_items(&items, query);
    if query.is_empty() {
        // Empty query: keep a curated head — commands first, then projects / chats.
        ranked.truncate(40);
    } else {
        ranked.truncate(60);
    }
    ranked
}

/// Filter and rank items for `query`. Empty query keeps input order (caller-curated).
pub fn filter_palette_items(items: &[PaletteItem], query: &str) -> Vec<PaletteItem> {
    let query = query.trim();
    if query.is_empty() {
        return items.to_vec();
    }
    let mut scored: Vec<_> = items
        .iter()
        .filter_map(|item| {
            let score = palette_item_score(item, query);
            (score > 0).then_some((score, item.clone()))
        })
        .collect();
    scored.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then(a.1.source.label().cmp(b.1.source.label()))
            .then(a.1.title.cmp(&b.1.title))
    });
    scored.into_iter().map(|(_, item)| item).collect()
}

/// Move the selection with arrow-key delta; clamps to `[0, len)`.
pub fn move_palette_selection(selected: usize, len: usize, delta: i32) -> usize {
    if len == 0 {
        return 0;
    }
    let max = len - 1;
    if delta < 0 {
        selected.saturating_sub((-delta) as usize)
    } else {
        (selected + delta as usize).min(max)
    }
}

/// Resolve which action Enter should run for the current selection.
pub fn palette_dispatch(
    items: &[PaletteItem],
    selected: usize,
) -> Option<PaletteAction> {
    items.get(selected).map(|item| item.action.clone())
}

/// Palette UI mode.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum CommandPaletteMode {
    /// Closed.
    #[default]
    Closed,
    /// Filtering / picking from the global catalog.
    Search {
        /// Filter text.
        query: String,
        /// Selected row index into the filtered list.
        selected: usize,
    },
    /// Collecting a free-text argument for a command.
    Argument {
        /// Command id awaiting an argument.
        command_id: String,
        /// Display title.
        title: String,
        /// Prompt label.
        prompt: String,
        /// User input.
        value: String,
    },
}

/// Mutable Command Palette state owned by the desktop UI.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CommandPaletteState {
    /// Current mode.
    pub mode: CommandPaletteMode,
    /// Latest ranked results (host refreshes on query change).
    pub results: Vec<PaletteItem>,
}

impl CommandPaletteState {
    /// Whether the palette overlay should capture input.
    pub fn is_open(&self) -> bool {
        !matches!(self.mode, CommandPaletteMode::Closed)
    }

    /// Open the global palette (⌘P).
    pub fn open(&mut self) {
        self.mode = CommandPaletteMode::Search {
            query: String::new(),
            selected: 0,
        };
        self.results.clear();
    }

    /// Close the palette.
    pub fn close(&mut self) {
        self.mode = CommandPaletteMode::Closed;
        self.results.clear();
    }

    /// Current filter query, when searching.
    pub fn query(&self) -> &str {
        match &self.mode {
            CommandPaletteMode::Search { query, .. } => query.as_str(),
            _ => "",
        }
    }

    /// Replace ranked results and clamp selection.
    pub fn set_results(&mut self, results: Vec<PaletteItem>) {
        self.results = results;
        if let CommandPaletteMode::Search { selected, .. } = &mut self.mode {
            *selected = (*selected).min(self.results.len().saturating_sub(1));
        }
    }

    /// Enter argument collection for a command that needs one.
    pub fn prompt_argument(&mut self, command_id: String, title: String, prompt: String) {
        self.mode = CommandPaletteMode::Argument {
            command_id,
            title,
            prompt,
            value: String::new(),
        };
    }
}

/// Result of interacting with the palette for one frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandPaletteOutcome {
    /// No action.
    None,
    /// Query text changed — host should re-gather / re-rank.
    QueryChanged(String),
    /// Execute a palette action.
    Execute(PaletteAction),
}

/// Render the floating centered palette. No Planner / Search calls inside paint.
pub fn render_command_palette(
    ctx: &egui::Context,
    state: &mut CommandPaletteState,
    theme: &Theme,
) -> CommandPaletteOutcome {
    if !state.is_open() {
        return CommandPaletteOutcome::None;
    }

    let mut outcome = CommandPaletteOutcome::None;
    let mut dismiss = false;

    let screen = ctx.screen_rect();
    egui::Area::new(egui::Id::new("jaymi_command_palette_backdrop"))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            let response = ui.allocate_response(screen.size(), egui::Sense::click());
            ui.painter()
                .rect_filled(screen, 0.0, theme.overlay_scrim());
            if response.clicked() {
                dismiss = true;
            }
        });

    let panel_width = (screen.width() * 0.55).clamp(440.0, 680.0);
    let panel_height = match &state.mode {
        CommandPaletteMode::Argument { .. } => 128.0,
        _ => 420.0,
    };

    egui::Window::new("Command Palette")
        .id(egui::Id::new("jaymi_command_palette"))
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, -40.0))
        .fixed_size(egui::vec2(panel_width, panel_height))
        .frame(
            egui::Frame::new()
                .fill(theme.surface)
                .stroke(egui::Stroke::new(stroke::HAIRLINE, theme.border))
                .corner_radius(egui::CornerRadius::same(radius::LG as u8))
                .inner_margin(egui::Margin::same(space::MD as i8))
                .shadow(theme.elevation_shadow()),
        )
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                dismiss = true;
            }

            match &mut state.mode {
                CommandPaletteMode::Closed => {}
                CommandPaletteMode::Search { query, selected } => {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("⌘P")
                                .size(type_size::META)
                                .color(theme.text_secondary),
                        );
                        let response = ui.add(
                            egui::TextEdit::singleline(query)
                                .hint_text(
                                    egui::RichText::new(
                                        "Search projects, files, commands, knowledge…",
                                    )
                                    .color(theme.text_secondary),
                                )
                                .text_color(theme.text_primary)
                                .frame(false)
                                .desired_width(panel_width - 72.0),
                        );
                        response.request_focus();
                        if response.changed() {
                            *selected = 0;
                            outcome = CommandPaletteOutcome::QueryChanged(query.clone());
                        }
                    });
                    ui.add_space(space::SM);
                    ui.painter().hline(
                        ui.max_rect().x_range(),
                        ui.cursor().top(),
                        egui::Stroke::new(stroke::HAIRLINE, theme.border),
                    );
                    ui.add_space(space::SM);

                    let results = state.results.clone();
                    if results.is_empty() {
                        ui.label(
                            egui::RichText::new(if query.trim().is_empty() {
                                "Type to search Jaymi…"
                            } else {
                                "No matching results"
                            })
                            .size(type_size::UI)
                            .color(theme.text_secondary),
                        );
                    } else {
                        *selected = (*selected).min(results.len().saturating_sub(1));
                        egui::ScrollArea::vertical()
                            .max_height(panel_height - 88.0)
                            .show(ui, |ui| {
                                for (index, item) in results.iter().enumerate() {
                                    let selected_row = index == *selected;
                                    let row = render_palette_row(ui, theme, item, selected_row);
                                    if row.clicked() {
                                        outcome =
                                            CommandPaletteOutcome::Execute(item.action.clone());
                                    }
                                }
                            });

                        let enter = ui.input(|input| input.key_pressed(egui::Key::Enter));
                        let up = ui.input(|input| input.key_pressed(egui::Key::ArrowUp));
                        let down = ui.input(|input| input.key_pressed(egui::Key::ArrowDown));
                        if up {
                            *selected = move_palette_selection(*selected, results.len(), -1);
                        }
                        if down {
                            *selected = move_palette_selection(*selected, results.len(), 1);
                        }
                        if enter {
                            if let Some(action) = palette_dispatch(&results, *selected) {
                                outcome = CommandPaletteOutcome::Execute(action);
                            }
                        }
                    }
                }
                CommandPaletteMode::Argument {
                    command_id,
                    title,
                    prompt,
                    value,
                } => {
                    ui.label(
                        egui::RichText::new(title.clone())
                            .strong()
                            .size(type_size::BODY)
                            .color(theme.text_primary),
                    );
                    ui.label(
                        egui::RichText::new(prompt.as_str())
                            .size(type_size::UI)
                            .color(theme.text_secondary),
                    );
                    ui.add_space(space::XS);
                    let response = ui.add(
                        egui::TextEdit::singleline(value)
                            .hint_text(
                                egui::RichText::new(prompt.as_str())
                                    .color(theme.text_secondary),
                            )
                            .text_color(theme.text_primary)
                            .desired_width(panel_width - 32.0),
                    );
                    response.request_focus();
                    if ui.input(|input| input.key_pressed(egui::Key::Enter))
                        && !value.trim().is_empty()
                    {
                        outcome = CommandPaletteOutcome::Execute(PaletteAction::RunCommand {
                            id: command_id.clone(),
                            argument: Some(value.trim().to_string()),
                        });
                    }
                }
            }
        });

    match outcome {
        CommandPaletteOutcome::Execute(action) => CommandPaletteOutcome::Execute(action),
        CommandPaletteOutcome::QueryChanged(query) => CommandPaletteOutcome::QueryChanged(query),
        CommandPaletteOutcome::None => {
            if dismiss {
                state.close();
            }
            CommandPaletteOutcome::None
        }
    }
}

fn render_palette_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    item: &PaletteItem,
    selected: bool,
) -> egui::Response {
    let fill = if selected {
        theme.accent.linear_multiply(0.18)
    } else {
        egui::Color32::TRANSPARENT
    };
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 36.0),
        egui::Sense::click(),
    );
    if fill.a() > 0 {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(radius::SM as u8), fill);
    }
    let text_color = if selected {
        theme.text_primary
    } else {
        theme.text_primary
    };
    let source_color = theme.text_secondary;
    ui.painter().text(
        egui::pos2(rect.left() + space::SM, rect.center().y - 7.0),
        egui::Align2::LEFT_CENTER,
        &item.title,
        egui::FontId::proportional(type_size::UI),
        text_color,
    );
    let detail = match &item.subtitle {
        Some(subtitle) if !subtitle.is_empty() => {
            format!("{} · {subtitle}", item.source.label())
        }
        _ => item.source.label().to_string(),
    };
    ui.painter().text(
        egui::pos2(rect.left() + space::SM, rect.center().y + 8.0),
        egui::Align2::LEFT_CENTER,
        detail,
        egui::FontId::proportional(type_size::META),
        source_color,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(
        id: &str,
        title: &str,
        source: PaletteSource,
        action: PaletteAction,
        keywords: &[&str],
    ) -> PaletteItem {
        PaletteItem {
            id: id.into(),
            title: title.into(),
            subtitle: None,
            source,
            keywords: keywords.iter().map(|s| (*s).to_string()).collect(),
            action,
            engine_score: 0,
        }
    }

    #[test]
    fn command_palette_search() {
        let catalog = vec![
            item(
                "cmd:open-project",
                "Open Project",
                PaletteSource::Command,
                PaletteAction::RunCommand {
                    id: "jaymi.workbench.openFolder".into(),
                    argument: None,
                },
                &["folder", "project"],
            ),
            item(
                "cmd:open-terminal",
                "Open Terminal",
                PaletteSource::Command,
                PaletteAction::RunCommand {
                    id: "jaymi.workbench.openTerminal".into(),
                    argument: None,
                },
                &["shell"],
            ),
            item(
                "proj:jaymi",
                "Jaymi",
                PaletteSource::Project,
                PaletteAction::OpenProject {
                    project_id: "project:jaymi".into(),
                },
                &["code"],
            ),
            item(
                "file:lib",
                "lib.rs",
                PaletteSource::File,
                PaletteAction::OpenFile {
                    path: "/tmp/lib.rs".into(),
                },
                &[],
            ),
            item(
                "cap:code",
                "Code",
                PaletteSource::Capability,
                PaletteAction::OpenCapability {
                    capability_id: "code".into(),
                },
                &["coding"],
            ),
            item(
                "conv:1",
                "Continue Conversation",
                PaletteSource::Conversation,
                PaletteAction::ContinueConversation { prompt: None },
                &["chat"],
            ),
            item(
                "know:1",
                "Search Knowledge",
                PaletteSource::Knowledge,
                PaletteAction::OpenKnowledge {
                    title: "Search Knowledge".into(),
                    path: None,
                    query: "search".into(),
                },
                &["knowledge"],
            ),
        ];

        let terminal = filter_palette_items(&catalog, "term");
        assert_eq!(terminal[0].title, "Open Terminal");

        let project = filter_palette_items(&catalog, "jaymi");
        assert!(project.iter().any(|item| item.source == PaletteSource::Project));

        let empty = filter_palette_items(&catalog, "");
        assert_eq!(empty.len(), catalog.len());

        let none = filter_palette_items(&catalog, "zzz-no-match-zzz");
        assert!(none.is_empty());
    }

    #[test]
    fn command_palette_navigation() {
        assert_eq!(move_palette_selection(0, 0, 1), 0);
        assert_eq!(move_palette_selection(2, 5, -1), 1);
        assert_eq!(move_palette_selection(0, 5, -1), 0);
        assert_eq!(move_palette_selection(4, 5, 1), 4);
        assert_eq!(move_palette_selection(1, 5, 2), 3);
    }

    #[test]
    fn command_palette_dispatch() {
        let items = vec![
            item(
                "cmd:git",
                "Open Git",
                PaletteSource::Command,
                PaletteAction::RunCommand {
                    id: "jaymi.workbench.openGit".into(),
                    argument: None,
                },
                &[],
            ),
            item(
                "file:main",
                "main.rs",
                PaletteSource::File,
                PaletteAction::OpenFile {
                    path: "/proj/main.rs".into(),
                },
                &[],
            ),
        ];
        assert_eq!(
            palette_dispatch(&items, 0),
            Some(PaletteAction::RunCommand {
                id: "jaymi.workbench.openGit".into(),
                argument: None,
            })
        );
        assert_eq!(
            palette_dispatch(&items, 1),
            Some(PaletteAction::OpenFile {
                path: "/proj/main.rs".into(),
            })
        );
        assert_eq!(palette_dispatch(&items, 9), None);
        assert_eq!(palette_dispatch(&[], 0), None);
    }
}
