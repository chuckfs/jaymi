//! Explorer icons — painted shapes (no Unicode tofu / □ placeholders).
//!
//! One coherent system: chevrons, folder tiles, and file marks drawn with the
//! egui painter so Light and Dark themes stay consistent without icon fonts.

use eframe::egui;

use crate::theme::Theme;
use jaymi_capabilities::ExplorerNode;

/// Paint a disclosure chevron for directories.
pub fn paint_disclosure(
    painter: &egui::Painter,
    center: egui::Pos2,
    expanded: bool,
    color: egui::Color32,
) {
    let size = 4.5;
    let points = if expanded {
        [
            center + egui::vec2(-size, -size * 0.4),
            center + egui::vec2(size, -size * 0.4),
            center + egui::vec2(0.0, size * 0.7),
        ]
    } else {
        [
            center + egui::vec2(-size * 0.4, -size),
            center + egui::vec2(size * 0.7, 0.0),
            center + egui::vec2(-size * 0.4, size),
        ]
    };
    painter.add(egui::Shape::convex_polygon(
        points.to_vec(),
        color,
        egui::Stroke::NONE,
    ));
}

/// Paint a folder tile (filled when expanded).
pub fn paint_folder(painter: &egui::Painter, center: egui::Pos2, expanded: bool, theme: &Theme) {
    let w = 10.0;
    let h = 8.0;
    let rect = egui::Rect::from_center_size(center + egui::vec2(0.0, 0.5), egui::vec2(w, h));
    let tab = egui::Rect::from_min_size(
        rect.left_top() + egui::vec2(0.0, -2.0),
        egui::vec2(4.0, 2.5),
    );
    if expanded {
        painter.rect_filled(rect, egui::CornerRadius::same(1), theme.text_secondary);
        painter.rect_filled(tab, egui::CornerRadius::same(1), theme.text_secondary);
    } else {
        painter.rect_stroke(
            rect,
            egui::CornerRadius::same(1),
            egui::Stroke::new(1.0, theme.text_secondary),
            egui::StrokeKind::Outside,
        );
        painter.rect_filled(tab, egui::CornerRadius::same(1), theme.text_secondary);
    }
}

/// Paint a simple file mark (small document).
pub fn paint_file(painter: &egui::Painter, center: egui::Pos2, theme: &Theme, node: &ExplorerNode) {
    let w = 8.0;
    let h = 10.0;
    let rect = egui::Rect::from_center_size(center, egui::vec2(w, h));
    let color = file_accent(theme, node);
    painter.rect_stroke(
        rect,
        egui::CornerRadius::same(1),
        egui::Stroke::new(1.0, color),
        egui::StrokeKind::Outside,
    );
    // Folded corner cue.
    let fold = egui::pos2(rect.right() - 3.0, rect.top());
    painter.line_segment(
        [fold, egui::pos2(rect.right(), rect.top() + 3.0)],
        egui::Stroke::new(1.0, color),
    );
}

fn file_accent(theme: &Theme, node: &ExplorerNode) -> egui::Color32 {
    let lower = node.name.to_ascii_lowercase();
    let ext = std::path::Path::new(&lower)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    match ext {
        "rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "go" => theme.accent,
        "md" | "txt" | "rst" => theme.text_secondary,
        "json" | "toml" | "yaml" | "yml" => theme.warning,
        _ => theme.text_secondary,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jaymi_capabilities::ExplorerNode;

    #[test]
    fn file_accent_prefers_code_extensions() {
        let theme = Theme::light();
        let node = ExplorerNode {
            name: "lib.rs".into(),
            path: "/tmp/lib.rs".into(),
            is_dir: false,
            children: Vec::new(),
        };
        assert_eq!(file_accent(&theme, &node), theme.accent);
    }
}
