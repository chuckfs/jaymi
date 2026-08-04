//! Persist Coding editor UI state under `.jaymi/workspace.json`.
//!
//! Snapshots store open tabs, active tab, view state, and editor settings.
//! Buffer contents are never serialized — files are re-read on restore.

use std::fs;
use std::path::Path;

use jaymi_capabilities::EditorWorkspaceSnapshot;
use jaymi_core::{JaymiError, JaymiResult};
use jaymi_projects::structure::JaymiProjectLayout;

/// Load a workspace editor snapshot from `project_root/.jaymi/workspace.json`.
pub fn load_editor_workspace(project_root: &Path) -> JaymiResult<Option<EditorWorkspaceSnapshot>> {
    let path = JaymiProjectLayout::for_root(project_root).workspace_json;
    if !path.is_file() {
        return Ok(None);
    }
    let body = fs::read_to_string(&path)
        .map_err(|error| JaymiError::new(format!("read {}: {error}", path.display())))?;
    let snapshot: EditorWorkspaceSnapshot = serde_json::from_str(&body)
        .map_err(|error| JaymiError::new(format!("parse {}: {error}", path.display())))?;
    Ok(Some(snapshot))
}

/// Write a workspace editor snapshot (creates `.jaymi/` as needed).
pub fn save_editor_workspace(
    project_root: &Path,
    snapshot: &EditorWorkspaceSnapshot,
) -> JaymiResult<()> {
    let layout = JaymiProjectLayout::for_root(project_root);
    fs::create_dir_all(&layout.jaymi_dir).map_err(|error| {
        JaymiError::new(format!("create {}: {error}", layout.jaymi_dir.display()))
    })?;
    let body = serde_json::to_string_pretty(snapshot)
        .map_err(|error| JaymiError::new(format!("serialize workspace editor state: {error}")))?;
    let tmp = layout.workspace_json.with_extension("json.tmp");
    fs::write(&tmp, &body)
        .map_err(|error| JaymiError::new(format!("write {}: {error}", tmp.display())))?;
    fs::rename(&tmp, &layout.workspace_json).map_err(|error| {
        JaymiError::new(format!(
            "rename {} → {}: {error}",
            tmp.display(),
            layout.workspace_json.display()
        ))
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_capabilities::{
        EditorCursor, EditorSettings, EditorViewState, FoldedRegion, PersistedEditorTab,
        EDITOR_WORKSPACE_SNAPSHOT_VERSION,
    };
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn roundtrip_omits_need_for_contents() {
        let root = std::env::temp_dir().join(format!(
            "jaymi-ws-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let snapshot = EditorWorkspaceSnapshot {
            version: EDITOR_WORKSPACE_SNAPSHOT_VERSION,
            tabs: vec![PersistedEditorTab {
                path: root.join("main.rs").to_string_lossy().into_owned(),
                preview: false,
                view: EditorViewState {
                    scroll_top: 12.0,
                    cursor: EditorCursor { line: 2, column: 5 },
                    folded_regions: vec![FoldedRegion {
                        start_line: 0,
                        end_line: 3,
                    }],
                },
            }],
            active_path: Some(root.join("main.rs").to_string_lossy().into_owned()),
            recently_opened: vec![root.join("main.rs").to_string_lossy().into_owned()],
            settings: EditorSettings {
                minimap: false,
                word_wrap: true,
                font_size: 15,
            },
            panes: Vec::new(),
            layout: None,
            focused_pane: None,
            explorer_width: Some(230.0),
            bottom_panel_height: Some(220.0),
            workspace_panel_width: Some(700.0),
            bottom_tab: Some("terminal".into()),
        };
        save_editor_workspace(&root, &snapshot).expect("save");
        let loaded = load_editor_workspace(&root)
            .expect("load")
            .expect("present");
        assert_eq!(loaded.settings.font_size, 15);
        assert!(loaded.settings.word_wrap);
        assert!(!loaded.settings.minimap);
        assert_eq!(loaded.tabs[0].view.scroll_top, 12.0);
        assert_eq!(loaded.tabs[0].view.folded_regions.len(), 1);
        assert_eq!(loaded.explorer_width, Some(230.0));
        assert_eq!(loaded.bottom_panel_height, Some(220.0));
        assert_eq!(loaded.workspace_panel_width, Some(700.0));
        assert_eq!(loaded.bottom_tab.as_deref(), Some("terminal"));
        let body = fs::read_to_string(JaymiProjectLayout::for_root(&root).workspace_json).unwrap();
        assert!(!body.contains("fn main"));
        let _ = fs::remove_dir_all(&root);
    }
}
