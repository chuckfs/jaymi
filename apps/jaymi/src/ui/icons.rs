//! Hand-painted Lucide-style icon marks for the Organic shell.
//!
//! Each glyph is redrawn from Lucide's (ISC-licensed) path geometry as plain
//! `egui::Painter` primitives — no SVG-rendering dependency, matching the
//! codebase's existing hand-painted-icon convention (see `ui::nav_rail` and
//! `ui::explorer::icons`, which predate this module and stay as-is until
//! their call sites are redesigned). Every icon is defined in Lucide's
//! 24x24 viewBox and scaled to whatever radius the caller asks for, at
//! [`crate::theme::stroke::ICON`] (2.75 in 24-space) stroke weight.
//!
//! Line joins are drawn with egui's default (miter) join rather than a true
//! SVG `stroke-linecap: round` — close enough at the 14-20px sizes these
//! render at in practice; revisit with a real SVG rasterizer only if that
//! stops being true.

use eframe::egui::{self, Color32, Pos2, Shape, Stroke};

/// Which glyph to paint. Names track the concept, not the exact Lucide icon
/// id, since several of these (research, knowledge, creation) are the
/// design system's own custom marks rather than stock Lucide icons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Coding,
    Research,
    Knowledge,
    Creation,
    Settings,
    Search,
    Send,
    Plus,
    Close,
    Check,
    ChevronDown,
    ChevronRight,
    Moon,
    Folder,
    File,
}

/// Paint `icon` centered at `center`, sized so its 24x24 source viewBox maps
/// onto a `2r x 2r` box, in `color`.
pub fn paint(painter: &egui::Painter, icon: Icon, center: Pos2, r: f32, color: Color32) {
    match icon {
        Icon::Coding => coding(painter, center, r, color),
        Icon::Research => research(painter, center, r, color),
        Icon::Knowledge => knowledge(painter, center, r, color),
        Icon::Creation => creation(painter, center, r, color),
        Icon::Settings => settings(painter, center, r, color),
        Icon::Search => search(painter, center, r, color),
        Icon::Send => send(painter, center, r, color),
        Icon::Plus => plus(painter, center, r, color),
        Icon::Close => close(painter, center, r, color),
        Icon::Check => check(painter, center, r, color),
        Icon::ChevronDown => chevron_down(painter, center, r, color),
        Icon::ChevronRight => chevron_right(painter, center, r, color),
        Icon::Moon => moon(painter, center, r, color, color),
        Icon::Folder => folder(painter, center, r, color),
        Icon::File => file(painter, center, r, color),
    }
}

/// Map a point in Lucide's 0..24 viewBox space onto painter space.
fn pt(center: Pos2, scale: f32, x: f32, y: f32) -> Pos2 {
    center + egui::vec2(x - 12.0, y - 12.0) * scale
}

fn icon_stroke(r: f32, color: Color32) -> Stroke {
    // stroke::ICON is defined in the same 24-unit space as the path data.
    Stroke::new(crate::theme::stroke::ICON * (r / 12.0), color)
}

fn line(painter: &egui::Painter, stroke: Stroke, points: Vec<Pos2>) {
    painter.add(Shape::line(points, stroke));
}

fn coding(painter: &egui::Painter, center: Pos2, r: f32, color: Color32) {
    let s = r / 12.0;
    let stroke = icon_stroke(r, color);
    line(
        painter,
        stroke,
        vec![pt(center, s, 16.0, 18.0), pt(center, s, 22.0, 12.0), pt(center, s, 16.0, 6.0)],
    );
    line(
        painter,
        stroke,
        vec![pt(center, s, 8.0, 6.0), pt(center, s, 2.0, 12.0), pt(center, s, 8.0, 18.0)],
    );
}

fn research(painter: &egui::Painter, center: Pos2, r: f32, color: Color32) {
    let s = r / 12.0;
    let stroke = icon_stroke(r, color);
    painter.circle_stroke(center, 10.0 * s, stroke);
    line(painter, stroke, vec![pt(center, s, 2.0, 12.0), pt(center, s, 22.0, 12.0)]);
    // The lens: two symmetric arcs from (12,2) to (12,22), bulging to x=2 / x=22.
    line(
        painter,
        stroke,
        quad_bezier(pt(center, s, 12.0, 2.0), pt(center, s, 2.0, 12.0), pt(center, s, 12.0, 22.0), 14),
    );
    line(
        painter,
        stroke,
        quad_bezier(pt(center, s, 12.0, 2.0), pt(center, s, 22.0, 12.0), pt(center, s, 12.0, 22.0), 14),
    );
}

fn knowledge(painter: &egui::Painter, center: Pos2, r: f32, color: Color32) {
    let s = r / 12.0;
    let stroke = icon_stroke(r, color);
    let rect = egui::Rect::from_min_max(pt(center, s, 6.5, 2.0), pt(center, s, 20.0, 22.0));
    let corner = egui::CornerRadius {
        nw: (2.5 * s) as u8,
        sw: (2.5 * s) as u8,
        ne: 0,
        se: 0,
    };
    painter.rect_stroke(rect, corner, stroke, egui::StrokeKind::Outside);
    // The lower spine tab, echoing the source path's inward step at y=19.5.
    line(
        painter,
        stroke,
        vec![pt(center, s, 6.5, 17.0), pt(center, s, 20.0, 17.0)],
    );
    line(
        painter,
        stroke,
        vec![pt(center, s, 4.0, 19.5), pt(center, s, 4.0, 4.5)],
    );
}

fn creation(painter: &egui::Painter, center: Pos2, r: f32, color: Color32) {
    sparkle(painter, center + egui::vec2(-1.5, -1.5) * (r / 12.0), r * 0.85, color);
    sparkle(painter, center + egui::vec2(6.0, 6.5) * (r / 12.0), r * 0.34, color);
}

/// A four-point sparkle/star, filled, fanned from its own center (the shape
/// is star-shaped about its centroid so a per-triangle fan tessellates
/// cleanly even though the outline is concave).
fn sparkle(painter: &egui::Painter, center: Pos2, r: f32, color: Color32) {
    let outer = r;
    let inner = r * 0.34;
    let mut outline = Vec::with_capacity(8);
    for i in 0..8 {
        let angle = (i as f32) * std::f32::consts::TAU / 8.0 - std::f32::consts::FRAC_PI_2;
        let radius = if i % 2 == 0 { outer } else { inner };
        outline.push(center + egui::vec2(angle.cos(), angle.sin()) * radius);
    }
    for i in 0..outline.len() {
        let next = (i + 1) % outline.len();
        painter.add(Shape::convex_polygon(
            vec![center, outline[i], outline[next]],
            color,
            Stroke::NONE,
        ));
    }
}

fn settings(painter: &egui::Painter, center: Pos2, r: f32, color: Color32) {
    let s = r / 12.0;
    let stroke = icon_stroke(r, color);
    let rows: [(f32, f32, f32, f32); 3] = [(21.0, 14.0, 10.0, 3.0), (21.0, 12.0, 8.0, 3.0), (21.0, 16.0, 12.0, 3.0)];
    let ys = [6.0, 12.0, 18.0];
    for (row_idx, (x1, gap_start, gap_end, x2)) in rows.into_iter().enumerate() {
        let y = ys[row_idx];
        line(painter, stroke, vec![pt(center, s, x1, y), pt(center, s, gap_start, y)]);
        line(painter, stroke, vec![pt(center, s, gap_end, y), pt(center, s, x2, y)]);
    }
    let knobs = [(12.0, 6.0), (10.0, 12.0), (14.0, 18.0)];
    for (x, y) in knobs {
        painter.circle_stroke(pt(center, s, x, y), 2.0 * s, stroke);
    }
}

fn search(painter: &egui::Painter, center: Pos2, r: f32, color: Color32) {
    let s = r / 12.0;
    let stroke = icon_stroke(r, color);
    painter.circle_stroke(pt(center, s, 11.0, 11.0), 7.0 * s, stroke);
    line(painter, stroke, vec![pt(center, s, 21.0, 21.0), pt(center, s, 16.7, 16.7)]);
}

fn send(painter: &egui::Painter, center: Pos2, r: f32, color: Color32) {
    let s = r / 12.0;
    let stroke = icon_stroke(r, color);
    line(painter, stroke, vec![pt(center, s, 12.0, 19.0), pt(center, s, 12.0, 5.0)]);
    line(
        painter,
        stroke,
        vec![pt(center, s, 5.0, 12.0), pt(center, s, 12.0, 5.0), pt(center, s, 19.0, 12.0)],
    );
}

fn plus(painter: &egui::Painter, center: Pos2, r: f32, color: Color32) {
    let s = r / 12.0;
    let stroke = icon_stroke(r, color);
    line(painter, stroke, vec![pt(center, s, 12.0, 5.0), pt(center, s, 12.0, 19.0)]);
    line(painter, stroke, vec![pt(center, s, 5.0, 12.0), pt(center, s, 19.0, 12.0)]);
}

fn close(painter: &egui::Painter, center: Pos2, r: f32, color: Color32) {
    let s = r / 12.0;
    let stroke = icon_stroke(r, color);
    line(painter, stroke, vec![pt(center, s, 18.0, 6.0), pt(center, s, 6.0, 18.0)]);
    line(painter, stroke, vec![pt(center, s, 6.0, 6.0), pt(center, s, 18.0, 18.0)]);
}

fn check(painter: &egui::Painter, center: Pos2, r: f32, color: Color32) {
    let s = r / 12.0;
    let stroke = icon_stroke(r, color);
    line(
        painter,
        stroke,
        vec![pt(center, s, 20.0, 6.0), pt(center, s, 9.0, 17.0), pt(center, s, 4.0, 12.0)],
    );
}

fn chevron_down(painter: &egui::Painter, center: Pos2, r: f32, color: Color32) {
    let s = r / 12.0;
    let stroke = icon_stroke(r, color);
    line(
        painter,
        stroke,
        vec![pt(center, s, 6.0, 9.0), pt(center, s, 12.0, 15.0), pt(center, s, 18.0, 9.0)],
    );
}

fn chevron_right(painter: &egui::Painter, center: Pos2, r: f32, color: Color32) {
    let s = r / 12.0;
    let stroke = icon_stroke(r, color);
    line(
        painter,
        stroke,
        vec![pt(center, s, 9.0, 6.0), pt(center, s, 15.0, 12.0), pt(center, s, 9.0, 18.0)],
    );
}

/// A crescent moon: a filled disc with a smaller disc, in `punch_color`,
/// offset over it to cut the crescent — the standard trick for faking a
/// boolean subtraction without a general path-clip.
fn moon(painter: &egui::Painter, center: Pos2, r: f32, color: Color32, _unused: Color32) {
    painter.circle_filled(center, r * 0.82, color);
}

/// Punch a crescent out of an already-painted moon disc using the surface
/// color behind it. Call after [`paint`]`(Icon::Moon, ...)` when the
/// backing fill is known (button chrome, not transparent).
pub fn punch_moon_crescent(painter: &egui::Painter, center: Pos2, r: f32, backing: Color32) {
    painter.circle_filled(center + egui::vec2(r * 0.32, -r * 0.28), r * 0.62, backing);
}

fn folder(painter: &egui::Painter, center: Pos2, r: f32, color: Color32) {
    let s = r / 12.0;
    let stroke = icon_stroke(r, color);
    let body = egui::Rect::from_min_max(pt(center, s, 2.0, 6.0), pt(center, s, 22.0, 20.0));
    painter.rect_stroke(body, egui::CornerRadius::same((2.0 * s) as u8), stroke, egui::StrokeKind::Outside);
    line(
        painter,
        stroke,
        vec![
            pt(center, s, 2.0, 6.0),
            pt(center, s, 2.0, 5.0),
            pt(center, s, 9.5, 5.0),
            pt(center, s, 11.0, 6.5),
        ],
    );
}

fn file(painter: &egui::Painter, center: Pos2, r: f32, color: Color32) {
    let s = r / 12.0;
    let stroke = icon_stroke(r, color);
    line(
        painter,
        stroke,
        vec![
            pt(center, s, 14.0, 2.0),
            pt(center, s, 6.0, 2.0),
            pt(center, s, 6.0, 22.0),
            pt(center, s, 18.0, 22.0),
            pt(center, s, 18.0, 7.0),
            pt(center, s, 14.0, 2.0),
        ],
    );
    line(
        painter,
        stroke,
        vec![pt(center, s, 14.0, 2.0), pt(center, s, 14.0, 7.0), pt(center, s, 18.0, 7.0)],
    );
}

/// Sample a quadratic Bezier curve (De Casteljau) into `segments + 1` points.
fn quad_bezier(p0: Pos2, p1: Pos2, p2: Pos2, segments: usize) -> Vec<Pos2> {
    (0..=segments)
        .map(|i| {
            let t = i as f32 / segments as f32;
            let a = p0.lerp(p1, t);
            let b = p1.lerp(p2, t);
            a.lerp(b, t)
        })
        .collect()
}

trait Lerp {
    fn lerp(self, other: Self, t: f32) -> Self;
}

impl Lerp for Pos2 {
    fn lerp(self, other: Self, t: f32) -> Self {
        self + (other - self) * t
    }
}
