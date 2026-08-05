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

**Current:** default experience — durable conversation surface always visible beside Coding. App top bar (hamburger toggles a left nav rail). Left rail stacks vertical rows: **Projects**, **Knowledge**, **Media**, with **Conversations** pinned at the bottom. **Projects** and **Conversations** expand inline inside the rail (ChatGPT-style collapse — Open Project + recent projects under Projects, above Knowledge; recent chats under Conversations). No floating menus. Knowledge / Media are stubs; Projects opens Coding from the right. Coding toolbar is quiet (Coding · project/path · Search · ⌘P); palette/shortcuts cover Close, ⌘⇧P, Panel, Save. No conversation title or nested chat frame — history sits on the open window background. Empty state centers **Hi, I'm Jaymi** / **Ask anything, or open a Coding Workspace.** with an **Open Project** CTA; after the first message it becomes a normal scrolling chat. Floating bottom composer (rounded, elevated shadow, inset from edges). Opening Coding expands a workspace on the right and never replaces or clears chat history. Conversation min width ~380px; default ~32% beside Coding. Colors and spacing come from the shared application `Theme` (8px grid). Shortcuts: ⌘P Quick Open, ⌘⇧P Command Palette, ⌘⇧F Find in Files, ⌘S Save, Enter to send.

### Theme

**Current:** `apps/jaymi/src/theme.rs` owns a single `Theme` with color tokens for `background`, `surface`, `surface_alt`, `border`, `text_primary`, `text_secondary`, `accent`, `success`, `warning`, and `error`, plus shared layout tokens (`space` 4/8/16/24/32, `radius`, `type_size`, `stroke`). Light / Dark / System resolve automatically (System follows OS appearance). Interactive `accent` follows the OS accent color when available (macOS `NSColor.controlAccentColor`, Windows DWM colorization) so buttons, selection, and focus match the desktop; otherwise it falls back to Jaymi blue. Every conversation, Coding chrome, Explorer, overlay, status, and icon color derives from these tokens via `Theme::apply_egui` and direct field use. Monaco keeps its own editor themes (`jaymi-light` / `jaymi-dark`) — syntax highlighting and editor chrome stay in Monaco while the surrounding shell uses Jaymi `Theme`.

### Coding Workspace

**Current:** chat-forward side expansion beside conversation with a docked IDE shell bound to temporary `CodingState` (Conversation | Editor | Explorer | Bottom Panel):

- **Editor** hierarchy: Tabs (full-width under toolbar) → Monaco (full-bleed centerpiece) → Status Bar · Explorer beside Monaco only · bottom dock full-width (Monaco via wry WebView overlay on the focused pane only; egui buffer fallback for every other pane; Save via ⌘S)
  - Workspace **title toolbar** (~40pt): left — Coding icon, title, muted project/path; right — Search (⌘⇧F), Quick Open (⌘P). Close Workspace, Command Palette (⌘⇧P), Panel, Save, and the rest live in the Command Palette / shortcuts.
  - Monaco uses its own **Jaymi Light** / **Jaymi Dark** editor themes (`jaymi-light` / `jaymi-dark`) synced to the shell’s light/dark mode — surrounding UI (toolbar, tabs, status, panels) uses Jaymi [`Theme`](../apps/jaymi/src/theme.rs) tokens, not Monaco colors
  - VS Code-style tabs: unlimited open sessions, dirty `*` indicator, close `x`, middle-click close
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
- **IDE region layout**: Coding Workspace uses egui panels inside the workspace column — `TopBottomPanel` (toolbar) · `TopBottomPanel` (editor tabs) · `TopBottomPanel` (bottom dock) · `SidePanel` right (Explorer beside Monaco) · `CentralPanel` (Monaco centerpiece). Regions resize via egui’s built-in panel handles (sizes remembered in `.jaymi/workspace.json`). Avoids nested Groups/Frames — separator lines define regions. The conversation↔workspace divider defaults to ~32% conversation / 68% workspace, never shrinks conversation below 380px or above 48% of the window, and animates smoothly open rather than snapping.
- **Visual polish**: shared 8px spacing (`space`), radius, and type-size tokens in `theme.rs`; hairline dividers; borderless inactive widgets; hover/selection via soft accent wash with a soft selection outline for keyboard focus. Conversation, Coding chrome, Explorer, and dock share the same scale so the shell reads as one app. Explorer uses painted chevron/folder/file icons (no Unicode tofu). Developer Diagnostics live under the left nav rail footer (separate from the Coding dock Diagnostics page). Status messages use muted text; errors stay red. Research/Creation menu entries are disabled as “(soon)” until those workspaces ship.
- **Command Palette** (⌘⇧P): searches all commands registered in `CommandRegistry` (never hardcoded). Built-ins include Open File/Folder, Search Files, Quick Open, Find in Files, panel toggles (Terminal / Problems / Search / Git / Diagnostics / Output / Panel), Create File/Folder, Save, Close Editor/Workspace, open Coding/Research/Creative workspaces, Index Project, Search Knowledge/Memory, Run Planner. Plugins register additional commands on the same registry.
- **Quick Open** (⌘P): a lighter, filename-only modal (same overlay shell as the Command Palette). Types a query, fuzzy-matches project filenames through `Application::project_search` (`SearchRequest::filename`), and opens the selected file in Monaco. Every keystroke re-runs the same Planner-mediated search used everywhere else — Quick Open never touches its own index.
- **Explorer** interactive tree on the **right** of the Coding workspace (Planner → `list_project_tree`; reusable `ui::explorer` component)
  - IDE-style rows: painted disclosure chevron · folder/file icon (by type) · truncated name · painted dirty dot for unsaved open buffers; depth indentation; soft hover / selection wash with leading accent bar; animated scroll
  - Toggle via Command Palette (`Toggle Explorer`); width is user-resizable and persisted in `.jaymi/workspace.json`
  - Single-click selects + preview-opens editable files; double-click opens permanent
  - Context menu: New File / New Folder / Rename / Delete / Reveal in Finder
  - File/folder icons by type; create/rename drafts inline; current file highlighted
- **Find in Files** (bottom "Search" tab, or Command Palette → `Find in Files` / `Search Files`): project-wide search + replace panel bound to `CodingState::search` (`SearchPanelState`)
  - Query + Replace boxes, toggles for Regex / Case Sensitive / Whole Word / Files only
  - "Search" runs `Application::run_coding_search_from_panel`, which always goes Planner → `search_knowledge` → Search Engine (no dedicated index — Quick Open, Search Files, and Find in Files all resolve through the same `SearchEngine`)
  - Results list one row per located match (`path:line:column` + one-line preview); clicking a row opens the file in Monaco and positions the cursor at the match via `set_coding_tab_cursor`
  - "Replace All" (disabled for Files only) rewrites every located match through `Application::replace_in_search_results` — open editor buffers are edited in place (left dirty for review), files with no open buffer are read/written straight through the Planner; it reuses the exact match semantics (`jaymi_search::locate_matches` / `replace_matches`) that produced the results, so Replace All never disagrees with what Find in Files reported
- **Bottom dock** (VS Code-style): pages Terminal / Problems / Search / Git / Diagnostics / Output — only one page visible at a time. Active tab is highlighted; height is user-resizable and persisted with the last selected page in `.jaymi/workspace.json` (`bottom_tab`, `last_bottom_tab`, `bottom_panel_height`). Closing the dock (**Hide**, active-tab click, toolbar **Hide Panel**, or `Toggle Panel`) collapses it completely (no tab-strip chrome); **Panel** / `Toggle Panel` reopens the last page. Panel data lives in `CodingState` so switching tabs preserves Terminal sessions, Search results, Git status, and Problems without remounting providers. Default expanded content height is 180px.
- Language Server (Rust Analyzer via Planner → `language_server`)
- **Terminal** (PTY via Planner → `terminal` tool → `TerminalProvider` / `TerminalManager`): a tab strip of independent `TerminalSession`s, each its own persistent PTY shell rooted at the current project root
  - "+ New" spawns another session (`Application::create_coding_terminal`, cwd always the project root) and makes it active
  - Click a tab to switch (`select_coding_terminal`); the "Rename" button opens an inline rename (`rename_coding_terminal`, persists on `TerminalSessionState.title`); "Close" kills the session (`kill_coding_terminal`) and a neighboring tab becomes active
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
- **Output panel** (bottom "Output" tab): placeholder page for future build / tool output channels — architecture is ready for providers without changing the dock shell.

**Activation (UI):** hamburger → left nav **Projects** (or **Open Project** / recent rows) opens Coding beside the conversation. That reuses the existing Coding shell and `CodingState` without creating a second conversation. Closing the workspace returns to the same chat. The Project Explorer empty state also offers **Open Project**.

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
