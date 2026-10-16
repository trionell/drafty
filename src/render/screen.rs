//! Draws a document with egui's painter.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use egui::{
    Color32, ColorImage, Context, FontId, Painter, Pos2, Rect, Shape, Stroke, StrokeKind,
    TextureHandle, TextureOptions, pos2, vec2,
};

use super::{HIGHLIGHT_ALPHA, fill_color, rasterize_highlight};
use crate::doc::{Camera, Document, ImageId, Object, ObjectId, ObjectKind, ShapeKind, geom};

const MAX_HIGHLIGHT_TEXTURE: f32 = 4096.0;

struct HighlightTexture {
    source: Arc<Object>,
    scale: f32,
    rect: Rect,
    texture: TextureHandle,
}

/// GPU textures for images and highlighter strokes, kept across frames.
#[derive(Default)]
pub struct TextureCache {
    images: HashMap<ImageId, TextureHandle>,
    highlights: HashMap<ObjectId, HighlightTexture>,
    used_highlights: HashSet<ObjectId>,
}

impl TextureCache {
    /// Free highlighter textures that weren't drawn this frame.
    pub fn end_frame(&mut self) {
        let used = std::mem::take(&mut self.used_highlights);
        self.highlights.retain(|id, _| used.contains(id));
    }

    /// Free image textures no open document refers to.
    pub fn retain_images(&mut self, keep: &HashSet<ImageId>) {
        self.images.retain(|id, _| keep.contains(id));
    }

    fn image(&mut self, ctx: &Context, doc: &Document, id: ImageId) -> Option<TextureHandle> {
        if let Some(t) = self.images.get(&id) {
            return Some(t.clone());
        }
        let data = doc.images.get(&id)?;
        let size = [data.size[0] as usize, data.size[1] as usize];
        let image = ColorImage::from_rgba_unmultiplied(size, &data.rgba());
        let options = TextureOptions::LINEAR.with_mipmap_mode(Some(egui::TextureFilter::Linear));
        let tex = ctx.load_texture(format!("image-{}", id.to_hex()), image, options);
        self.images.insert(id, tex.clone());
        Some(tex)
    }
}

pub struct Renderer<'a> {
    pub ctx: &'a Context,
    pub painter: &'a Painter,
    pub cam: Camera,
    pub textures: &'a mut TextureCache,
}

impl Renderer<'_> {
    pub fn draw_document(&mut self, doc: &Document, hidden: &HashSet<ObjectId>, dimmed: &HashSet<ObjectId>) {
        let clip = self.painter.clip_rect();
        for obj in &doc.objects {
            if hidden.contains(&obj.id) {
                continue;
            }
            let screen_bounds = self.cam.rect_to_screen(geom::bounds(&obj.kind));
            if !screen_bounds.intersects(clip) {
                continue;
            }
            let opacity = if dimmed.contains(&obj.id) { 0.25 } else { 1.0 };
            self.draw(doc, &obj.kind, Some(obj), opacity);
        }
    }

    /// Draw one object. `cached` is the committed object, used to key the highlighter
    /// texture cache. Previews pass `None` and get rasterized fresh each frame.
    pub fn draw(&mut self, doc: &Document, kind: &ObjectKind, cached: Option<&Arc<Object>>, opacity: f32) {
        let zoom = self.cam.zoom;
        let fade = |c: Color32| if opacity < 1.0 { c.gamma_multiply(opacity) } else { c };
        match kind {
            ObjectKind::Stroke { points, style, highlighter: true } => {
                self.draw_highlight(points, style.width, style.color, cached, opacity);
            }
            ObjectKind::Stroke { points, style, highlighter: false } => {
                let color = fade(style.color);
                let w = style.width * zoom;
                let pts: Vec<Pos2> = points.iter().map(|p| self.cam.to_screen(*p)).collect();
                if pts.len() > 1 {
                    self.painter.add(Shape::line(pts.clone(), Stroke::new(w, color)));
                }
                // egui draws butt caps; round dots on the ends make it look like a pen.
                self.painter.circle_filled(pts[0], w * 0.5, color);
                if pts.len() > 1 {
                    self.painter.circle_filled(*pts.last().unwrap(), w * 0.5, color);
                }
            }
            ObjectKind::Shape { shape, a, b, style } => {
                let color = fade(style.color);
                let stroke = Stroke::new(style.width * zoom, color);
                let fill = fill_color(style).map(fade).unwrap_or(Color32::TRANSPARENT);
                let (sa, sb) = (self.cam.to_screen(*a), self.cam.to_screen(*b));
                match shape {
                    ShapeKind::Rect => {
                        let r = Rect::from_two_pos(sa, sb);
                        self.painter.rect(r, 0.0, fill, stroke, StrokeKind::Middle);
                    }
                    ShapeKind::Ellipse => {
                        let r = Rect::from_two_pos(sa, sb);
                        let radius = r.size() * 0.5;
                        if fill != Color32::TRANSPARENT {
                            self.painter.add(Shape::ellipse_filled(r.center(), radius, fill));
                        }
                        self.painter.add(Shape::ellipse_stroke(r.center(), radius, stroke));
                    }
                    ShapeKind::Triangle => {
                        let t = geom::triangle_points(sa, sb);
                        self.painter.add(Shape::convex_polygon(t.to_vec(), fill, stroke));
                    }
                    ShapeKind::Line => {
                        self.painter.line_segment([sa, sb], stroke);
                        self.painter.circle_filled(sa, stroke.width * 0.5, color);
                        self.painter.circle_filled(sb, stroke.width * 0.5, color);
                    }
                    ShapeKind::Arrow { double } => {
                        let (start, end, heads) = geom::arrow_parts(*a, *b, style.width, *double);
                        let (s, e) = (self.cam.to_screen(start), self.cam.to_screen(end));
                        self.painter.line_segment([s, e], stroke);
                        if !double {
                            self.painter.circle_filled(sa, stroke.width * 0.5, color);
                        }
                        for head in heads {
                            let pts = head.iter().map(|p| self.cam.to_screen(*p)).collect();
                            self.painter.add(Shape::convex_polygon(pts, color, Stroke::NONE));
                        }
                    }
                }
            }
            ObjectKind::Text { pos, text, size, color, .. } => {
                let galley = layout_text(self.ctx, text, *size * zoom, fade(*color));
                self.painter.galley(self.cam.to_screen(*pos), galley, fade(*color));
            }
            ObjectKind::Image { image, rect } => {
                if let Some(tex) = self.textures.image(self.ctx, doc, *image) {
                    let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                    let tint = Color32::WHITE.gamma_multiply(opacity);
                    self.painter.image(tex.id(), self.cam.rect_to_screen(*rect), uv, tint);
                }
            }
        }
    }

    fn draw_highlight(
        &mut self,
        points: &[Pos2],
        width: f32,
        color: Color32,
        cached: Option<&Arc<Object>>,
        opacity: f32,
    ) {
        if points.is_empty() {
            return;
        }
        // Rasterize at a power-of-two scale so zooming only re-rasterizes when the
        // zoom level crosses a step, and cap the texture size.
        let target = self.cam.zoom * self.ctx.pixels_per_point();
        let mut scale = 2f32.powf(target.log2().ceil()).clamp(0.125, 16.0);
        let board = Rect::from_points(points).expand(width);
        let max_side = board.width().max(board.height()) * scale;
        if max_side > MAX_HIGHLIGHT_TEXTURE {
            scale *= MAX_HIGHLIGHT_TEXTURE / max_side;
        }

        let tint = Color32::WHITE.gamma_multiply(HIGHLIGHT_ALPHA * opacity);
        let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));

        if let Some(obj) = cached {
            self.textures.used_highlights.insert(obj.id);
            if let Some(h) = self.textures.highlights.get(&obj.id)
                && Arc::ptr_eq(&h.source, obj) && h.scale == scale {
                    self.painter.image(h.texture.id(), self.cam.rect_to_screen(h.rect), uv, tint);
                    return;
                }
        }

        let Some((pixmap, rect)) = rasterize_highlight(points, width, color, scale) else {
            return;
        };
        let size = [pixmap.width() as usize, pixmap.height() as usize];
        let image = ColorImage::from_rgba_premultiplied(size, pixmap.data());
        let texture = self.ctx.load_texture("highlight", image, TextureOptions::LINEAR);
        self.painter.image(texture.id(), self.cam.rect_to_screen(rect), uv, tint);
        if let Some(obj) = cached {
            self.textures.highlights.insert(
                obj.id,
                HighlightTexture { source: obj.clone(), scale, rect, texture },
            );
        }
    }
}

/// Lay out text the way the board shows it. Shared with export and text measuring so
/// all three agree on line breaks and spacing.
pub fn layout_text(ctx: &Context, text: &str, size: f32, color: Color32) -> Arc<egui::Galley> {
    ctx.fonts_mut(|f| f.layout_no_wrap(text.to_owned(), FontId::proportional(size), color))
}

/// Size of a text object at zoom 1.
pub fn measure_text(ctx: &Context, text: &str, size: f32) -> egui::Vec2 {
    let shown = if text.is_empty() { " " } else { text };
    layout_text(ctx, shown, size, Color32::BLACK).size()
}

pub fn draw_grid(painter: &Painter, cam: &Camera, color: Color32) {
    let mut spacing = 24.0;
    while spacing * cam.zoom < 12.0 {
        spacing *= 2.0;
    }
    let clip = painter.clip_rect();
    let board = cam.rect_to_board(clip);
    let dot = (1.5 * cam.zoom.sqrt()).clamp(1.0, 2.5);
    let x0 = (board.min.x / spacing).floor() as i64;
    let x1 = (board.max.x / spacing).ceil() as i64;
    let y0 = (board.min.y / spacing).floor() as i64;
    let y1 = (board.max.y / spacing).ceil() as i64;
    for gx in x0..=x1 {
        for gy in y0..=y1 {
            let p = cam.to_screen(pos2(gx as f32 * spacing, gy as f32 * spacing));
            painter.rect_filled(Rect::from_center_size(p, vec2(dot, dot)), 0.0, color);
        }
    }
}
