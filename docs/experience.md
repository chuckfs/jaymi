# Experience

**Status: Current Implementation** (conversation shell + Coding Workspace shell + expansion model) · **Target:** full IDE / canvas / research surfaces

## Conversation First

Jaymi is conversation-first.

The conversation is the primary interface.

Users should never feel like they are switching applications.

Instead, the conversation expands into specialized workspaces as tasks become more complex.

---

## Workspaces

Jaymi defines primary workspace kinds. **Current:** conversation shell plus expansion chrome and capability/workspace state. **Target:** rich panels described below.

### Conversation

**Current:** default experience — general conversation, planning, memory. Empty state centers **Hi, I'm Jaymi** / **Ask anything, or open a Coding Workspace.**; after the first message it becomes a normal scrolling chat. Default width beside Coding is ~30% of the window (min 340px, max 45%). Colors (conversation, composer, accents, status) come from the shared application `Theme` (light / dark / system via `config.settings.theme`).

### Theme

**Current:** `apps/jaymi/src/theme.rs` owns a single `Theme` with tokens for background, surface, foreground, secondary_foreground, border, accent, selection, error, warning, and success. egui `Visuals` and Monaco (`jaymi-light` / `jaymi-dark`) are derived from the same tokens so Conversation, Explorer, Editor, Terminal, Git, Problems, Search, Diagnostics, toolbar, and status bar stay visually continuous.

### Coding Workspace

**Current:** chat-forward side expansion beside conversation with a docked IDE shell bound to temporary `CodingState` (Conversation | Editor | Explorer | Bottom Panel):

- **Editor** hierarchy: Toolbar → Tabs → Monaco (full-bleed, no card) → Status Bar (Monaco via wry WebView overlay on the focused pane only; egui buffer fallback for every other pane; Save + ⌘S)
  - Monaco uses **Jaymi Light** / **Jaymi Dark** themes generated from the central application [`Theme`](../apps/jaymi/src/theme.rs) — editor background, text, selections, and syntax colors match the surrounding shell (no stock VS Code `vs-dark` rectangle)
  - VS Code-style tabs: unlimited open sessions, dirty `*` indicator, close ✕, middle-click close
  - Preview tabs (italic): single-click a file in Explorer opens preview; double-click opens permanent
  - Scroll + cursor + folded regions restore when reactivating a session; recently-opened MRU per pane in `OpenEditors`
  - **Split editors** (VS Code-style): the editor area is a recursive `EditorLayoutNode` tree of `Split { direction, sizes, children }` / `Leaf { pane }` nodes over a set of panes that share one buffer pool
    - "Split Right" / "Split Down" clone the focused pane's active tab into a new sibling pane; "Close Split" removes a pane and flattens the tree
    - Drag a tab onto another pane's tab strip to move it there (`MoveTab`)
    - Drag the divider between split children to resize them (`ResizeSplit`, relative sizes normalized to 1.0)
    - Each pane keeps independent cursor/scroll/folds/active-tab; buffers are shared
    - Only the **focused** pane's active tab gets the Monaco overlay; other panes use egui `TextEdit`
  - Editor chrome (minimap, word wrap, font size) owned by workspace state
  - Persisted per project in `.jaymi/workspace.json`, including the full pane/layout tree and per-pane view state (paths + view/settings only — never buffer contents); restored automatically on Coding reopen / project switch
  - Pinning: TODO
- **Resizable shell chrome**: docked IDE panels — Editor, Explorer (right of the editor), and the bottom panel (Terminal / Problems / Search / Git / Diagnostics). Drag the Explorer↔Editor divider and the Editor↔Bottom divider to resize; sizes clamp to min/max and are remembered per project in `.jaymi/workspace.json` (including the active bottom tab). The conversation↔workspace divider defaults to ~30% conversation / 70% workspace, never shrinks conversation below 340px or above 45% of the window, and animates smoothly open rather than snapping.
- **Command Palette** (⌘⇧P): searches all commands registered in `CommandRegistry` (never hardcoded). Built-ins include Open File/Folder, Search Files, Quick Open, Find in Files, panel toggles, Create File/Folder, Save, Close Editor/Workspace, open Coding/Research/Creative workspaces, Index Project, Search Knowledge/Memory, Run Planner. Plugins register additional commands on the same registry.
- **Quick Open** (⌘P): a lighter, filename-only modal (same overlay shell as the Command Palette). Types a query, fuzzy-matches project filenames through `Application::project_search` (`SearchRequest::filename`), and opens the selected file in Monaco. Every keystroke re-runs the same Planner-mediated search used everywhere else — Quick Open never touches its own index.
- **Explorer** interactive tree on the **right** of the Coding workspace (Planner → `list_project_tree`; reusable `ui::explorer` component)
  - Toggle via Command Palette (`Toggle Explorer`)
  - Single-click selects + preview-opens editable files; double-click opens permanent
  - Context menu: New File / New Folder / Rename / Delete / Reveal in Finder
  - File/folder icons by type; create/rename drafts inline; current file highlighted
- **Find in Files** (bottom "Search" tab, or Command Palette → `Find in Files` / `Search Files`): project-wide search + replace panel bound to `CodingState::search` (`SearchPanelState`)
  - Query + Replace boxes, toggles for Regex / Case Sensitive / Whole Word / Files only
  - "Search" runs `Application::run_coding_search_from_panel`, which always goes Planner → `search_knowledge` → Search Engine (no dedicated index — Quick Open, Search Files, and Find in Files all resolve through the same `SearchEngine`)
  - Results list one row per located match (`path:line:column` + one-line preview); clicking a row opens the file in Monaco and positions the cursor at the match via `set_coding_tab_cursor`
  - "Replace All" (disabled for Files only) rewrites every located match through `Application::replace_in_search_results` — open editor buffers are edited in place (left dirty for review), files with no open buffer are read/written straight through the Planner; it reuses the exact match semantics (`jaymi_search::locate_matches` / `replace_matches`) that produced the results, so Replace All never disagrees with what Find in Files reported
- **Bottom tabs** toggle Terminal / Problems / Search / Git / Diagnostics without leaving the editor (also via Command Palette for most panels). Collapsed chrome is a 32px tab strip; expanded default height is 180px and resizable.
- Language Server (Rust Analyzer via Planner → `language_server`)
- **Terminal** (PTY via Planner → `terminal` tool → `TerminalProvider` / `TerminalManager`): a tab strip of independent `TerminalSession`s, each its own persistent PTY shell rooted at the current project root
  - "+ New" spawns another session (`Application::create_coding_terminal`, cwd always the project root) and makes it active
  - Click a tab to switch (`select_coding_terminal`); the "✎" button opens an inline rename (`rename_coding_terminal`, persists on `TerminalSessionState.title`); "×" kills the session (`kill_coding_terminal`) and a neighboring tab becomes active
  - Only the active session's pane is rendered; other sessions keep running (scrollback, history, and PTY process) in the background
  - Also reachable via Command Palette: `New Terminal`, `Kill Terminal`, `Rename Terminal`
  - Run anything a real shell supports — `cargo test`, `git status`, `npm test`, `python …` — through the same persistent PTY session (history + scrollback preserved across commands)
  - Conversation stays visible beside the terminal at all times; UI renders `CodingState.terminal_sessions` / `active_terminal_id` only — it never owns PTY or process logic
- **Git panel** (Planner → `git` tool → **Git Provider**): repository detection, current branch, Staged / Modified / Added / Deleted / Untracked lists, commit message + Commit, Refresh, Discard with confirmation. Coding Workspace consumes `CodingState.git` only — never shells out. Merge / rebase / cherry-pick are not implemented yet.
- **Problems panel** (bottom "Problems" tab, bound to `CodingState.problems`): a single aggregated, clickable issue list built by `ProblemsRegistry::collect_all` from every registered `ProblemsProvider` — the panel never talks to individual sources directly
  - Built-in sources: `lsp` (rust-analyzer diagnostics), `planner` (blocked Planner turns), `workspace` (Explorer load errors, Git panel errors), `permissions` (denied permission/policy decisions), `search` (unhealthy Search Engine, disabled/errored indexing, content-understanding failures), `memory` (unhealthy Memory subsystem) — plugins register additional providers on the same `ProblemsRegistry`
  - Each row shows Severity (error/warning/info/hint) · Source label (e.g. `rust-analyzer`, `Planner`, `Workspace`) · `file:line` (when known) · Message
  - Clicking a row with a path jumps to the file in Monaco and positions the cursor (`OpenProblem`, same jump path as Find in Files)
  - Header shows the live count (`N problem(s)`) plus a manual "Refresh" button (`ProblemsRefresh` → `Application::refresh_coding_problems`); refreshed automatically after LSP diagnostics apply and whenever the Coding Workspace (re)opens
  - Empty state: "No problems detected."
  - Monaco markers for the active file prefer `CodingState.problems` (filtered by path), falling back to the raw LSP working set (`CodingState.diagnostics`) when Problems hasn't been populated yet
- **Diagnostics panel** (bottom "Diagnostics" tab): read-only workspace operational sections from `CodingDiagnosticsView` / `Application::coding_diagnostics_view` (Active project, Planner activity, Tool execution, Provider status, Indexing, Memory, Permissions, Timing). Empty state: "Workspace information."

**Activation (UI):** conversation header **⋯** menu → **Open Project…** (folder picker; creates or reuses a project for that root, then opens Coding) or **Recent Projects**, or **Start Coding Project** (opens the Coding shell for the already-active project). That reuses the existing Coding shell and `CodingState` without creating a second conversation. Closing the workspace returns to the same chat. The Project Explorer empty state also offers **Open Project…**.

The conversation remains visible and persistent. Monaco and the Language Server (Rust Analyzer) are embedded in Coding Workspace (buffers and diagnostics survive UI remounts via `CodingState`). Broader LSP tooling remains **Target** for Layer 7 polish. Multi-tab Terminal PTY sessions and the Git panel are available in Coding Workspace.

The conversation becomes project-aware but never resets.

### Creation Workspace

**Target:** conversation stays; canvas / image / asset tools appear.

### Research Workspace

**Target:** conversation stays; sources and notes appear.

---

## Closing a Workspace

**Current:** closing an expanded workspace keeps the conversation and session state consistent with capability/workspace rules.

The conversation is permanent.

Workspaces expand and collapse around it.

---

## Relationship to Capabilities

**Current:** Capability Engine plans and inspects capabilities with **availability** (Ready / Experimental / Planned / Unavailable). Workspace kinds map to capability expansions; Inspector shows availability and active workspace. See [capabilities.md](capabilities.md).

**Target:** selecting Coding / Creation / Research fully materializes the specialized surfaces described above.
