//! Explorer icons — painted shapes (no Unicode tofu / □ placeholders).
//!
//! Folders get a disclosure arrowhead (right when closed, down when open);
//! files carry no marker at all.

use eframe::egui;

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

