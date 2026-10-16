pub mod export;
pub mod screen;

use egui::{Color32, Pos2, Rect};
use tiny_skia::{LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke, Transform};

use crate::doc::{Fill, Style};

/// Opacity of highlighter strokes, on screen and in exports.
pub const HIGHLIGHT_ALPHA: f32 = 0.4;

pub fn fill_color(style: &Style) -> Option<Color32> {
    match style.fill {
        Fill::None => None,
        Fill::Solid => Some(style.color),
        Fill::Translucent => Some(style.color.gamma_multiply(0.25)),
    }
}

pub fn skia_color(c: Color32) -> tiny_skia::Color {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    tiny_skia::Color::from_rgba8(r, g, b, a)
}

pub fn paint(c: Color32) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(skia_color(c));
    p.anti_alias = true;
    p
}

/// tiny-skia path through `points`, mapped by `map`. A single point becomes a zero-length
/// segment so round and square caps still draw a dot.
pub fn polyline(points: &[Pos2], map: impl Fn(Pos2) -> Pos2) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();
    let first = map(*points.first()?);
    pb.move_to(first.x, first.y);
    if points.len() == 1 {
        pb.line_to(first.x + 0.01, first.y);
    }
    for p in &points[1..] {
        let p = map(*p);
        pb.line_to(p.x, p.y);
    }
    pb.finish()
}

pub fn highlighter_stroke(width: f32, points: usize) -> Stroke {
    Stroke {
        width,
        // Flat ends look like a marker. A single dot needs a square cap to show at all.
        line_cap: if points == 1 {
            LineCap::Square
        } else {
            LineCap::Butt
        },
        line_join: LineJoin::Round,
        ..Default::default()
    }
}

/// Rasterize a highlighter stroke opaque. tiny-skia fills a stroke's outline once, so
/// where the stroke crosses itself the alpha doesn't double up. The caller applies
/// `HIGHLIGHT_ALPHA` when drawing the result. Returns the pixmap and the board rect it covers.
pub fn rasterize_highlight(
    points: &[Pos2],
    width: f32,
    color: Color32,
    scale: f32,
) -> Option<(Pixmap, Rect)> {
    let bounds = Rect::from_points(points).expand(width * 0.75 + 1.0);
    let w = (bounds.width() * scale).ceil().max(1.0) as u32;
    let h = (bounds.height() * scale).ceil().max(1.0) as u32;
    let mut pixmap = Pixmap::new(w, h)?;
    let path = polyline(points, |p| ((p - bounds.min) * scale).to_pos2())?;
    let [r, g, b, _] = color.to_srgba_unmultiplied();
    let opaque = Color32::from_rgb(r, g, b);
    pixmap.stroke_path(
        &path,
        &paint(opaque),
        &highlighter_stroke(width * scale, points.len()),
        Transform::identity(),
        None,
    );
    Some((pixmap, bounds))
}
