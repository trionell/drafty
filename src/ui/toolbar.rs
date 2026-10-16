//! Tool buttons and the style bar, in one row. Icons are painted, not font glyphs, so
//! they look the same everywhere and need no icon font.

use egui::{
    Align, Color32, Layout, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui,
    Vec2, pos2, vec2,
};

use crate::app::{Action, App};
use crate::doc::Fill;
use crate::tools::{FONT_SIZES, HIGHLIGHT_WIDTHS, PALETTE, PEN_WIDTHS, Tool};

const BUTTON: f32 = 30.0;

/// A square button with a painted icon. `paint` gets the icon area and the foreground color.
fn icon_button(ui: &mut Ui, selected: bool, paint: impl FnOnce(&Painter, Rect, Color32)) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(BUTTON), Sense::click());
    if ui.is_rect_visible(rect) {
        let v = ui.style().interact_selectable(&response, selected);
        if selected || response.hovered() {
            ui.painter().rect(rect, 5.0, v.weak_bg_fill, v.bg_stroke, StrokeKind::Inside);
        }
        paint(ui.painter(), rect.shrink(7.0), v.fg_stroke.color);
    }
    response
}

fn paint_tool_icon(p: &Painter, r: Rect, tool: Tool, c: Color32) {
    let s = Stroke::new(1.6, c);
    let at = |x: f32, y: f32| r.min + vec2(x * r.width(), y * r.height());
    match tool {
        Tool::Select => {
            let pts = vec![at(0.2, 0.0), at(0.2, 0.85), at(0.42, 0.65), at(0.58, 1.0), at(0.7, 0.94), at(0.55, 0.6), at(0.85, 0.6)];
            p.add(Shape::closed_line(pts, s));
        }
        Tool::Pen => {
            p.line_segment([at(0.15, 0.85), at(0.8, 0.2)], Stroke::new(3.0, c));
            p.add(Shape::convex_polygon(vec![at(0.0, 1.0), at(0.08, 0.72), at(0.28, 0.92)], c, Stroke::NONE));
        }
        Tool::Highlighter => {
            let y = Color32::from_rgb(0xfd, 0xd8, 0x35);
            p.rect_filled(Rect::from_min_max(at(0.0, 0.55), at(1.0, 0.9)), 1.0, y.gamma_multiply(0.7));
            p.line_segment([at(0.3, 0.6), at(0.85, 0.05)], Stroke::new(4.0, c));
        }
        Tool::Text => {
            p.line_segment([at(0.1, 0.1), at(0.9, 0.1)], Stroke::new(2.2, c));
            p.line_segment([at(0.5, 0.1), at(0.5, 0.95)], Stroke::new(2.2, c));
        }
        Tool::Rect => {
            p.rect_stroke(Rect::from_min_max(at(0.0, 0.15), at(1.0, 0.85)), 1.0, s, StrokeKind::Middle);
        }
        Tool::Ellipse => {
            p.add(Shape::ellipse_stroke(r.center(), vec2(r.width() * 0.5, r.height() * 0.4), s));
        }
        Tool::Triangle => {
            p.add(Shape::closed_line(vec![at(0.5, 0.05), at(1.0, 0.95), at(0.0, 0.95)], s));
        }
        Tool::Line => {
            p.line_segment([at(0.05, 0.95), at(0.95, 0.05)], s);
        }
        Tool::Arrow => paint_arrow(p, at(0.05, 0.95), at(0.95, 0.05), c, false),
        Tool::Eraser => {
            let pts = vec![at(0.35, 0.25), at(0.65, 0.0), at(1.0, 0.4), at(0.7, 0.65)];
            p.add(Shape::convex_polygon(pts, c, Stroke::NONE));
            let pts = vec![at(0.35, 0.25), at(0.7, 0.65), at(0.45, 0.9), at(0.1, 0.5)];
            p.add(Shape::closed_line(pts, s));
            p.line_segment([at(0.0, 1.0), at(1.0, 1.0)], s);
        }
    }
}

fn paint_arrow(p: &Painter, a: Pos2, b: Pos2, c: Color32, double: bool) {
    let s = Stroke::new(1.6, c);
    p.line_segment([a, b], s);
    let head = |from: Pos2, tip: Pos2| {
        let d = (tip - from).normalized();
        let base = tip - d * 6.0;
        let n = d.rot90() * 3.5;
        p.add(Shape::convex_polygon(vec![tip, base + n, base - n], c, Stroke::NONE));
    };
    head(a, b);
    if double {
        head(b, a);
    }
}

fn swatch(ui: &mut Ui, color: Color32, selected: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(20.0, BUTTON), Sense::click());
    let center = rect.center();
    let outline = ui.visuals().widgets.noninteractive.bg_stroke.color;
    ui.painter().circle(center, 8.0, color, Stroke::new(1.0, outline));
    if selected {
        ui.painter().circle_stroke(center, 10.5, Stroke::new(2.0, crate::tools::select::ACCENT));
    } else if response.hovered() {
        ui.painter().circle_stroke(center, 10.5, Stroke::new(1.0, outline));
    }
    response
}

impl App {
    pub fn toolbar(&mut self, ui: &mut Ui) {
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            for tool in Tool::ALL {
                let tip = format!("{} ({})", tool.name(), tool.key().name());
                if icon_button(ui, self.tool == tool, |p, r, c| paint_tool_icon(p, r, tool, c))
                    .on_hover_text(tip)
                    .clicked()
                {
                    self.set_tool(tool);
                }
                if matches!(tool, Tool::Text | Tool::Arrow) {
                    ui.separator();
                }
            }
            ui.separator();
            self.style_bar(ui);

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button(" ? ").on_hover_text("Keyboard shortcuts (?)").clicked() {
                    self.show_help = !self.show_help;
                }
                let zoom = self.doc().camera.zoom;
                if ui
                    .button(format!("{:.0}%", zoom * 100.0))
                    .on_hover_text("Reset zoom (Ctrl+0)")
                    .clicked()
                {
                    self.run(Action::ZoomReset);
                }
                let grid = self.show_grid;
                if icon_button(ui, grid, |p, r, c| {
                    for i in 0..3 {
                        for j in 0..3 {
                            let q = r.min + vec2(i as f32, j as f32) * r.width() * 0.5;
                            p.circle_filled(q, 1.5, c);
                        }
                    }
                })
                .on_hover_text("Dot grid (Ctrl+')")
                .clicked()
                {
                    self.show_grid = !grid;
                }
            });
        });
        ui.add_space(2.0);
    }

    fn style_bar(&mut self, ui: &mut Ui) {
        let ctx_tool = self.style_context();
        let current = self.settings.color_for(ctx_tool);
        for (i, c) in PALETTE.iter().enumerate() {
            if swatch(ui, *c, *c == current).on_hover_text(format!("Color ({})", i + 1)).clicked() {
                self.apply_color(*c);
            }
        }
        let mut c = current;
        if egui::color_picker::color_edit_button_srgba(ui, &mut c, egui::color_picker::Alpha::Opaque)
            .on_hover_text("Custom color")
            .changed()
        {
            self.apply_color(c);
        }
        ui.separator();

        if ctx_tool == Tool::Text {
            let size = self.text.as_ref().map(|t| t.size).unwrap_or(self.settings.font_size);
            for (label, s) in FONT_SIZES {
                if ui.selectable_label(size == s, label).on_hover_text(format!("Font size {s}")).clicked() {
                    self.apply_font_size(s);
                }
            }
            return;
        }

        let presets = if ctx_tool == Tool::Highlighter { HIGHLIGHT_WIDTHS } else { PEN_WIDTHS };
        let width = self.current_width();
        for w in presets {
            let dot = (w / presets[2] * 12.0).max(2.5);
            if icon_button(ui, width == w, |p, r, c| {
                p.circle_filled(r.center(), dot * 0.5, c);
            })
            .on_hover_text(format!("Width {w} ([ and ])"))
            .clicked()
            {
                self.apply_width(w, false);
            }
        }
        let mut w = width;
        ui.spacing_mut().slider_width = 70.0;
        let resp = ui.add(egui::Slider::new(&mut w, 1.0..=64.0).logarithmic(true).fixed_decimals(0));
        if resp.changed() {
            self.apply_width(w.round(), resp.dragged());
        }
        if resp.drag_stopped() {
            self.style_drag = false;
        }

        if matches!(ctx_tool, Tool::Rect | Tool::Ellipse | Tool::Triangle) {
            ui.separator();
            let fills = [
                (Fill::None, "No fill"),
                (Fill::Translucent, "Translucent fill"),
                (Fill::Solid, "Solid fill"),
            ];
            for (f, tip) in fills {
                let col = self.settings.color;
                if icon_button(ui, self.settings.fill == f, |p, r, c| {
                    let r = r.shrink(1.0);
                    let fill = match f {
                        Fill::None => Color32::TRANSPARENT,
                        Fill::Translucent => col.gamma_multiply(0.3),
                        Fill::Solid => col,
                    };
                    p.rect(r, 2.0, fill, Stroke::new(1.5, c), StrokeKind::Middle);
                })
                .on_hover_text(tip)
                .clicked()
                {
                    self.apply_fill(f);
                }
            }
        }
        if self.tool == Tool::Arrow || (self.tool == Tool::Select && self.selection_has_arrow()) {
            ui.separator();
            for (double, tip) in [(false, "Single arrowhead"), (true, "Double arrowhead")] {
                if icon_button(ui, self.settings.arrow_double == double, |p, r, c| {
                    paint_arrow(p, r.left_center(), r.right_center(), c, double);
                })
                .on_hover_text(tip)
                .clicked()
                {
                    self.apply_arrow_double(double);
                }
            }
        }
        let _ = pos2;
    }

    fn selection_has_arrow(&self) -> bool {
        self.doc().selected_objects().iter().any(|o| {
            matches!(o.kind, crate::doc::ObjectKind::Shape { shape: crate::doc::ShapeKind::Arrow { .. }, .. })
        })
    }
}
