//! Renders a document to a PNG-ready pixmap on the CPU with tiny-skia.

use std::collections::{HashMap, HashSet};

use ab_glyph::{Font as _, FontRef, OutlineCurve};
use egui::{Color32, Context, Pos2, Rect, Vec2};
use tiny_skia::{
    FillRule, FilterQuality, LineCap, LineJoin, PathBuilder, Pattern, Pixmap, SpreadMode,
    Stroke, Transform,
};

use super::{HIGHLIGHT_ALPHA, fill_color, highlighter_stroke, paint, polyline};
use crate::doc::{Document, ImageId, ObjectId, ObjectKind, ShapeKind, geom};

pub const PADDING: f32 = 16.0;
const MAX_SIDE: f32 = 16384.0;
const MAX_PIXELS: f32 = 100_000_000.0;

#[derive(Clone, Copy)]
pub struct ExportOptions {
    pub scale: f32,
    pub transparent: bool,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self { scale: 2.0, transparent: false }
    }
}

/// Render `only` (or everything) cropped to content plus padding. `None` for an empty board.
pub fn render(
    ctx: &Context,
    doc: &Document,
    only: Option<&HashSet<ObjectId>>,
    opts: ExportOptions,
) -> Option<Pixmap> {
    let objects: Vec<_> = doc
        .objects
        .iter()
        .filter(|o| only.is_none_or(|s| s.contains(&o.id)))
        .collect();
    let bounds = geom::union_bounds(objects.iter().map(|o| &o.kind))?.expand(PADDING);

    let mut scale = opts.scale;
    let longest = bounds.width().max(bounds.height()) * scale;
    if longest > MAX_SIDE {
        scale *= MAX_SIDE / longest;
    }
    let area = bounds.area() * scale * scale;
    if area > MAX_PIXELS {
        scale *= (MAX_PIXELS / area).sqrt();
    }

    let w = (bounds.width() * scale).ceil() as u32;
    let h = (bounds.height() * scale).ceil() as u32;
    let mut pixmap = Pixmap::new(w.max(1), h.max(1))?;
    if !opts.transparent {
        pixmap.fill(super::skia_color(doc.background));
    }

    let mut r = Rasterizer {
        ctx,
        doc,
        origin: bounds.min,
        scale,
        pixmap: &mut pixmap,
        images: HashMap::new(),
        font: FontRef::try_from_slice(crate::INTER).expect("bundled font is valid"),
    };
    for obj in objects {
        r.draw(&obj.kind);
    }
    Some(pixmap)
}

/// RGBA bytes with straight alpha, as PNG encoders and clipboards want them.
pub fn to_rgba(pixmap: &Pixmap) -> Vec<u8> {
    pixmap
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect()
}

pub fn encode_png(pixmap: &Pixmap) -> Result<Vec<u8>, String> {
    pixmap.encode_png().map_err(|e| e.to_string())
}

struct Rasterizer<'a> {
    ctx: &'a Context,
    doc: &'a Document,
    origin: Pos2,
    scale: f32,
    pixmap: &'a mut Pixmap,
    images: HashMap<ImageId, Pixmap>,
    font: FontRef<'static>,
}

impl Rasterizer<'_> {
    fn map(&self, p: Pos2) -> Pos2 {
        ((p - self.origin) * self.scale).to_pos2()
    }

    fn stroke(&self, width: f32, cap: LineCap, join: LineJoin) -> Stroke {
        Stroke {
            width: width * self.scale,
            line_cap: cap,
            line_join: join,
            miter_limit: 4.0,
            ..Default::default()
        }
    }

    fn draw(&mut self, kind: &ObjectKind) {
        let id = Transform::identity();
        match kind {
            ObjectKind::Stroke { points, style, highlighter } => {
                let Some(path) = polyline(points, |p| self.map(p)) else { return };
                if *highlighter {
                    let c = style.color.gamma_multiply(HIGHLIGHT_ALPHA);
                    let s = highlighter_stroke(style.width * self.scale, points.len());
                    self.pixmap.stroke_path(&path, &paint(c), &s, id, None);
                } else {
                    let s = self.stroke(style.width, LineCap::Round, LineJoin::Round);
                    self.pixmap.stroke_path(&path, &paint(style.color), &s, id, None);
                }
            }
            ObjectKind::Shape { shape, a, b, style } => {
                let (sa, sb) = (self.map(*a), self.map(*b));
                let fill = fill_color(style);
                let stroke_paint = paint(style.color);
                let path = match shape {
                    ShapeKind::Rect => {
                        let r = Rect::from_two_pos(sa, sb);
                        tiny_skia::Rect::from_ltrb(r.min.x, r.min.y, r.max.x, r.max.y)
                            .map(PathBuilder::from_rect)
                    }
                    ShapeKind::Ellipse => {
                        let r = Rect::from_two_pos(sa, sb);
                        tiny_skia::Rect::from_ltrb(r.min.x, r.min.y, r.max.x, r.max.y)
                            .and_then(PathBuilder::from_oval)
                    }
                    ShapeKind::Triangle => {
                        let t = geom::triangle_points(sa, sb);
                        let mut pb = PathBuilder::new();
                        pb.move_to(t[0].x, t[0].y);
                        pb.line_to(t[1].x, t[1].y);
                        pb.line_to(t[2].x, t[2].y);
                        pb.close();
                        pb.finish()
                    }
                    ShapeKind::Line => {
                        let s = self.stroke(style.width, LineCap::Round, LineJoin::Round);
                        if let Some(p) = polyline(&[*a, *b], |p| self.map(p)) {
                            self.pixmap.stroke_path(&p, &stroke_paint, &s, id, None);
                        }
                        return;
                    }
                    ShapeKind::Arrow { double } => {
                        let (start, end, heads) = geom::arrow_parts(*a, *b, style.width, *double);
                        let cap = if *double { LineCap::Butt } else { LineCap::Round };
                        let s = self.stroke(style.width, cap, LineJoin::Round);
                        if let Some(p) = polyline(&[start, end], |p| self.map(p)) {
                            self.pixmap.stroke_path(&p, &stroke_paint, &s, id, None);
                        }
                        for head in heads {
                            if let Some(p) = polyline(&head, |p| self.map(p)) {
                                let mut pb = PathBuilder::new();
                                pb.push_path(&p);
                                pb.close();
                                if let Some(p) = pb.finish() {
                                    self.pixmap.fill_path(&p, &stroke_paint, FillRule::Winding, id, None);
                                }
                            }
                        }
                        return;
                    }
                };
                let Some(path) = path else { return };
                if let Some(f) = fill {
                    self.pixmap.fill_path(&path, &paint(f), FillRule::Winding, id, None);
                }
                let s = self.stroke(style.width, LineCap::Butt, LineJoin::Miter);
                self.pixmap.stroke_path(&path, &stroke_paint, &s, id, None);
            }
            ObjectKind::Text { pos, text, size, color, .. } => self.draw_text(*pos, text, *size, *color),
            ObjectKind::Image { image, rect } => self.draw_image(*image, *rect),
        }
    }

    /// Glyph positions come from egui's layout so lines and spacing match the screen.
    /// ab_glyph supplies the outlines, which tiny-skia fills.
    fn draw_text(&mut self, pos: Pos2, text: &str, size: f32, color: Color32) {
        let galley = super::screen::layout_text(self.ctx, text, size, color);
        let mut pb = PathBuilder::new();
        let mut units_to_board: Option<f32> = None;
        for row in &galley.rows {
            for glyph in &row.glyphs {
                let gid = self.font.glyph_id(glyph.chr);
                let advance = self.font.h_advance_unscaled(gid);
                // egui's font size isn't defined the same way as ab_glyph's scale, so
                // calibrate from a glyph's advance width instead of guessing.
                let k = *units_to_board.get_or_insert_with(|| {
                    if advance > 0.0 && glyph.advance_width > 0.0 {
                        glyph.advance_width / advance
                    } else {
                        size / self.font.units_per_em().unwrap_or(2048.0)
                    }
                });
                let Some(outline) = self.font.outline(gid) else { continue };
                let baseline = pos + row.pos.to_vec2() + glyph.pos.to_vec2();
                let map = |p: ab_glyph::Point| {
                    let b = baseline + Vec2::new(p.x * k, -p.y * k);
                    self.map(b)
                };
                let mut last: Option<ab_glyph::Point> = None;
                for curve in &outline.curves {
                    let start = match curve {
                        OutlineCurve::Line(a, _) | OutlineCurve::Quad(a, _, _) | OutlineCurve::Cubic(a, _, _, _) => *a,
                    };
                    if last != Some(start) {
                        if last.is_some() {
                            pb.close();
                        }
                        let s = map(start);
                        pb.move_to(s.x, s.y);
                    }
                    match curve {
                        OutlineCurve::Line(_, b) => {
                            let b2 = map(*b);
                            pb.line_to(b2.x, b2.y);
                            last = Some(*b);
                        }
                        OutlineCurve::Quad(_, c, b) => {
                            let (c2, b2) = (map(*c), map(*b));
                            pb.quad_to(c2.x, c2.y, b2.x, b2.y);
                            last = Some(*b);
                        }
                        OutlineCurve::Cubic(_, c1, c2, b) => {
                            let (c1, c2, b2) = (map(*c1), map(*c2), map(*b));
                            pb.cubic_to(c1.x, c1.y, c2.x, c2.y, b2.x, b2.y);
                            last = Some(*b);
                        }
                    }
                }
                if last.is_some() {
                    pb.close();
                }
            }
        }
        if let Some(path) = pb.finish() {
            self.pixmap.fill_path(&path, &paint(color), FillRule::Winding, Transform::identity(), None);
        }
    }

    fn draw_image(&mut self, image: ImageId, rect: Rect) {
        if !self.images.contains_key(&image) {
            let Some(data) = self.doc.images.get(&image) else { return };
            let rgba = data.rgba();
            let mut premul = Vec::with_capacity(rgba.len());
            for px in rgba.as_chunks::<4>().0 {
                let a = px[3] as u16;
                premul.extend_from_slice(&[
                    ((px[0] as u16 * a + 127) / 255) as u8,
                    ((px[1] as u16 * a + 127) / 255) as u8,
                    ((px[2] as u16 * a + 127) / 255) as u8,
                    px[3],
                ]);
            }
            let Some(size) = tiny_skia::IntSize::from_wh(data.size[0], data.size[1]) else { return };
            let Some(pm) = Pixmap::from_vec(premul, size) else { return };
            self.images.insert(image, pm);
        }
        let img = &self.images[&image];
        let dst = Rect::from_min_max(self.map(rect.min), self.map(rect.max));
        let sx = dst.width() / img.width() as f32;
        let sy = dst.height() / img.height() as f32;
        let shader = Pattern::new(
            img.as_ref(),
            SpreadMode::Pad,
            FilterQuality::Bicubic,
            1.0,
            Transform::from_row(sx, 0.0, 0.0, sy, dst.min.x, dst.min.y),
        );
        let p = tiny_skia::Paint { shader, anti_alias: true, ..Default::default() };
        if let Some(r) = tiny_skia::Rect::from_ltrb(dst.min.x, dst.min.y, dst.max.x, dst.max.y) {
            self.pixmap.fill_rect(r, &p, Transform::identity(), None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::{Edit, Fill, ImageData, ShapeKind, Style};
    use egui::{RawInput, pos2, vec2};

    fn style(color: Color32, width: f32, fill: Fill) -> Style {
        Style { color, width, fill }
    }

    /// One of everything, laid out like a small diagram on a screenshot.
    fn sample(ctx: &Context) -> Document {
        let red = Color32::from_rgb(0xe5, 0x39, 0x35);
        let blue = Color32::from_rgb(0x1e, 0x88, 0xe5);
        let mut doc = Document::new("sample");

        // A fake screenshot: grey bars on a light background.
        let (w, h) = (240u32, 120u32);
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                let bar = (y / 20) % 2 == 1 && x > 16 && x < 200 - (y % 60);
                let v = if bar { 150 } else { 235 };
                rgba.extend_from_slice(&[v, v, v + 10, 255]);
            }
        }
        let (img, data) = ImageData::from_rgba([w, h], rgba);
        doc.add_image(img, data);

        let text = |pos, s: &str, size, color| ObjectKind::Text {
            pos,
            text: s.into(),
            size,
            color,
            extent: crate::render::screen::measure_text(ctx, s, size),
        };
        doc.apply(Edit::Add(vec![
            ObjectKind::Image { image: img, rect: Rect::from_min_size(pos2(0.0, 0.0), vec2(240.0, 120.0)) },
            ObjectKind::Stroke {
                points: vec![pos2(20.0, 30.0), pos2(150.0, 30.0)],
                style: style(Color32::from_rgb(0xfd, 0xd8, 0x35), 16.0, Fill::None),
                highlighter: true,
            },
            // A highlighter loop that crosses itself; the crossing must not get darker.
            ObjectKind::Stroke {
                points: vec![pos2(30.0, 100.0), pos2(90.0, 70.0), pos2(90.0, 110.0), pos2(30.0, 80.0)],
                style: style(Color32::from_rgb(0x43, 0xa0, 0x47), 14.0, Fill::None),
                highlighter: true,
            },
            ObjectKind::Shape {
                shape: ShapeKind::Rect,
                a: pos2(160.0, 60.0),
                b: pos2(230.0, 110.0),
                style: style(red, 3.0, Fill::Translucent),
            },
            ObjectKind::Shape {
                shape: ShapeKind::Arrow { double: false },
                a: pos2(330.0, 40.0),
                b: pos2(235.0, 80.0),
                style: style(red, 4.0, Fill::None),
            },
            ObjectKind::Shape {
                shape: ShapeKind::Ellipse,
                a: pos2(280.0, 120.0),
                b: pos2(360.0, 180.0),
                style: style(blue, 2.0, Fill::Solid),
            },
            ObjectKind::Shape {
                shape: ShapeKind::Triangle,
                a: pos2(20.0, 140.0),
                b: pos2(90.0, 200.0),
                style: style(Color32::BLACK, 2.0, Fill::None),
            },
            ObjectKind::Shape {
                shape: ShapeKind::Arrow { double: true },
                a: pos2(110.0, 180.0),
                b: pos2(250.0, 180.0),
                style: style(Color32::BLACK, 2.0, Fill::None),
            },
            ObjectKind::Stroke {
                points: crate::doc::geom::smooth(
                    &[pos2(120.0, 150.0), pos2(150.0, 130.0), pos2(180.0, 160.0), pos2(210.0, 135.0), pos2(240.0, 150.0)],
                    2,
                ),
                style: style(blue, 3.0, Fill::None),
                highlighter: false,
            },
            text(pos2(260.0, 8.0), "Click here", 20.0, red),
            text(pos2(110.0, 200.0), "Två rader\nåäö Q@#", 16.0, Color32::BLACK),
        ]));
        doc
    }

    fn render_sample(opts: ExportOptions, only_first: bool) -> Pixmap {
        let ctx = Context::default();
        crate::app::install_fonts(&ctx);
        let mut out = None;
        let mut full = ctx.run_ui(RawInput::default(), |ui| {
            let doc = sample(ui.ctx());
            let only: Option<HashSet<ObjectId>> = only_first.then(|| HashSet::from([doc.objects[3].id]));
            out = render(ui.ctx(), &doc, only.as_ref(), opts);
        });
        full.textures_delta.clear();
        out.expect("sample has content")
    }

    /// Compare with `tests/golden/<name>.png`. `UPDATE_GOLDEN=1` rewrites the file.
    fn check_golden(name: &str, pixmap: &Pixmap) {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden").join(format!("{name}.png"));
        if std::env::var_os("UPDATE_GOLDEN").is_some() || !path.exists() {
            std::fs::write(&path, encode_png(pixmap).unwrap()).unwrap();
            return;
        }
        let golden = Pixmap::load_png(&path).unwrap();
        assert_eq!((golden.width(), golden.height()), (pixmap.width(), pixmap.height()), "{name}: size changed");
        let off = golden
            .data()
            .as_chunks::<4>()
            .0
            .iter()
            .zip(pixmap.data().as_chunks::<4>().0)
            .filter(|(a, b)| a.iter().zip(b.iter()).any(|(x, y)| x.abs_diff(*y) > 8))
            .count();
        let total = (golden.width() * golden.height()) as usize;
        assert!(off * 1000 <= total, "{name}: {off} of {total} pixels differ; UPDATE_GOLDEN=1 if intended");
    }

    #[test]
    fn golden_board() {
        let p = render_sample(ExportOptions::default(), false);
        check_golden("board", &p);
    }

    #[test]
    fn golden_selection_transparent() {
        let p = render_sample(ExportOptions { scale: 1.0, transparent: true }, true);
        // Just the rectangle plus padding, at 1x.
        assert_eq!((p.width(), p.height()), (70 + 3 + 32, 50 + 3 + 32));
        assert_eq!(p.pixel(0, 0).unwrap().alpha(), 0, "corner is transparent");
        check_golden("selection", &p);
    }

    #[test]
    fn highlighter_crossing_is_not_darker() {
        let p = render_sample(ExportOptions { scale: 1.0, transparent: false }, false);
        // Content bounds start at the image's corner, 0,0, so board = pixel - padding.
        let at = |x: f32, y: f32| {
            let px = p.pixel((x + PADDING) as u32, (y + PADDING) as u32).unwrap().demultiply();
            (px.red(), px.green(), px.blue())
        };
        let crossing = at(60.0, 90.0);
        let single = at(45.0, 92.5);
        assert_eq!(crossing, single, "self-overlap doubled the highlighter alpha");
    }
}
