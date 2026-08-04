//! Command Palette host — queries [`CommandRegistry`], never hardcodes the list.
//!
//! Shortcut: ⌘⇧P opens the palette (plain ⌘P is Quick Open). Commands that need
//! an argument enter a second prompt step before dispatch.

use eframe::egui;

use jaymi_commands::{CommandDescriptor, CommandRegistry};

/// Palette UI mode.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum CommandPaletteMode {
    /// Closed.
    #[default]
    Closed,
    /// Filtering / picking a command.
    Commands {
        /// Filter text.
        query: String,
        /// Selected row index into the filtered list.
        selected: usize,
    },
    /// Collecting a free-text argument for a command.
    Argument {
        /// Command awaiting an argument.
        command: CommandDescriptor,
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
}

impl CommandPaletteState {
    /// Whether the palette overlay should capture input.
    pub fn is_open(&self) -> bool {
        !matches!(self.mode, CommandPaletteMode::Closed)
    }

    /// Open the command list (⌘⇧P).
    pub fn open(&mut self) {
        self.mode = CommandPaletteMode::Commands {
            query: String::new(),
            selected: 0,
        };
    }

    /// Close the palette.
    pub fn close(&mut self) {
        self.mode = CommandPaletteMode::Closed;
    }

    /// Enter argument collection for `command`.
    pub fn prompt_argument(&mut self, command: CommandDescriptor) {
        let prompt = command
            .argument_prompt
            .clone()
            .unwrap_or_else(|| command.title.clone());
        self.mode = CommandPaletteMode::Argument {
            command,
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
    /// Execute a command with an optional argument.
    Run {
        /// Command id from the registry.
        id: String,
        /// Optional free-text argument.
        argument: Option<String>,
    },
}

/// Render the palette overlay and return any execute request.
pub fn render_command_palette(
    ctx: &egui::Context,
    state: &mut CommandPaletteState,
    registry: &CommandRegistry,
    overlay_scrim: egui::Color32,
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
            ui.painter().rect_filled(screen, 0.0, overlay_scrim);
            if response.clicked() {
                dismiss = true;
            }
        });

    let panel_width = (screen.width() * 0.55).clamp(420.0, 640.0);
    let panel_height = match &state.mode {
        CommandPaletteMode::Argument { .. } => 120.0,
        _ => 360.0,
    };

    egui::Window::new("Command Palette")
        .id(egui::Id::new("jaymi_command_palette"))
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 72.0))
        .fixed_size(egui::vec2(panel_width, panel_height))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                dismiss = true;
            }

            match &mut state.mode {
                CommandPaletteMode::Closed => {}
                CommandPaletteMode::Commands { query, selected } => {
                    ui.horizontal(|ui| {
                        ui.weak(">");
                        let response = ui.add(
                            egui::TextEdit::singleline(query)
                                .hint_text("Type a command…")
                                .desired_width(panel_width - 48.0),
                        );
                        response.request_focus();
                        if response.changed() {
                            *selected = 0;
                        }
                    });
                    ui.add_space(6.0);
                    ui.separator();

                    let commands = registry.search(query).unwrap_or_default();
                    if commands.is_empty() {
                        ui.weak("No matching commands");
                    } else {
                        *selected = (*selected).min(commands.len().saturating_sub(1));
                        egui::ScrollArea::vertical()
                            .max_height(panel_height - 64.0)
                            .show(ui, |ui| {
                                for (index, command) in commands.iter().enumerate() {
                                    let selected_row = index == *selected;
                                    let label =
                                        format!("{} · {}", command.category.label(), command.title);
                                    let mut text = egui::RichText::new(label);
                                    if selected_row {
                                        text = text.strong();
                                    }
                                    let response = ui.selectable_label(selected_row, text);
                                    if let Some(hint) = &command.keybinding {
                                        response.clone().on_hover_text(hint);
                                    }
                                    if response.clicked() {
                                        outcome = CommandPaletteOutcome::Run {
                                            id: command.id.clone(),
                                            argument: None,
                                        };
                                    }
                                }
                            });

                        let enter = ui.input(|input| input.key_pressed(egui::Key::Enter));
                        let up = ui.input(|input| input.key_pressed(egui::Key::ArrowUp));
                        let down = ui.input(|input| input.key_pressed(egui::Key::ArrowDown));
                        if up {
                            *selected = selected.saturating_sub(1);
                        }
                        if down {
                            *selected = (*selected + 1).min(commands.len().saturating_sub(1));
                        }
                        if enter {
                            if let Some(command) = commands.get(*selected) {
                                outcome = CommandPaletteOutcome::Run {
                                    id: command.id.clone(),
                                    argument: None,
                                };
                            }
                        }
                    }
                }
                CommandPaletteMode::Argument {
                    command,
                    prompt,
                    value,
                } => {
                    ui.label(egui::RichText::new(command.title.clone()).strong());
                    ui.weak(prompt.as_str());
                    ui.add_space(4.0);
                    let response = ui.add(
                        egui::TextEdit::singleline(value)
                            .hint_text(prompt.as_str())
                            .desired_width(panel_width - 24.0),
                    );
                    response.request_focus();
                    if ui.input(|input| input.key_pressed(egui::Key::Enter))
                        && !value.trim().is_empty()
                    {
                        outcome = CommandPaletteOutcome::Run {
                            id: command.id.clone(),
                            argument: Some(value.trim().to_string()),
                        };
                    }
                }
            }
        });

    match outcome {
        CommandPaletteOutcome::Run { id, argument: None } => {
            if let Ok(Some(command)) = registry.get(&id) {
                if command.argument_prompt.is_some() {
                    state.prompt_argument(command);
                    return CommandPaletteOutcome::None;
                }
            }
            state.close();
            CommandPaletteOutcome::Run { id, argument: None }
        }
        CommandPaletteOutcome::Run {
            id,
            argument: Some(argument),
        } => {
            state.close();
            CommandPaletteOutcome::Run {
                id,
                argument: Some(argument),
            }
        }
        CommandPaletteOutcome::None => {
            if dismiss {
                state.close();
            }
            CommandPaletteOutcome::None
        }
    }
}
