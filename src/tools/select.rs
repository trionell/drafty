use std::collections::HashSet;

use egui::{Color32, CursorIcon, Painter, Pos2, Rect, Stroke, StrokeKind, Vec2, vec2};

use super::Input;
use crate::doc::{Camera, Document, Edit, Object, ObjectId, ObjectKind, geom};

/// Screen points around a handle that count as grabbing it.
const HANDLE_REACH: f32 = 7.0;
const HANDLE_SIZE: f32 = 8.0;
pub const ACCENT: Color32 = Color32::from_rgb(0x1e, 0x88, 0xe5);

#[derive(Clone, Copy, PartialEq, Debug)]
enum Handle {
    /// A box handle. -1/0/1 per axis: which side of the bounds it sits on.
    Box(i8, i8),
    /// The `a` (false) or `b` (true) end of a single selected line or arrow.
    End(bool),
}

enum Mode {
    Idle,
    Moving {
        start: Pos2,
        originals: Vec<Object>,
        clicked: ObjectId,
        moved: bool,
    },
    Resizing {
        handle: (i8, i8),
        /// Bounds the handles sit on, including stroke width.
        from: Rect,
        /// Bounds of the bare geometry, which is what gets scaled.
        geo_from: Rect,
        originals: Vec<Object>,
        moved: bool,
    },
    Endpoint {
        b: bool,
        original: Object,
        moved: bool,
    },
    Box {
        start: Pos2,
        end: Pos2,
        additive: bool,
    },
}

pub enum Outcome {
    None,
    EditText(ObjectId),
}

pub struct SelectTool {
    mode: Mode,
}

impl Default for SelectTool {
    fn default() -> Self {
        Self { mode: Mode::Idle }
    }
}

fn selection_bounds(doc: &Document) -> Option<Rect> {
    geom::union_bounds(
        doc.objects
            .iter()
            .filter(|o| doc.selection.contains(&o.id))
            .map(|o| &o.kind),
    )
}

/// The single selected line or arrow, which gets endpoint handles instead of a box.
fn single_linear(doc: &Document) -> Option<&Object> {
    if doc.selection.len() != 1 {
        return None;
    }
    let obj = doc.get(*doc.selection.iter().next()?)?;
    match &obj.kind {
        ObjectKind::Shape { shape, .. } if shape.is_linear() => Some(obj),
        _ => None,
    }
}

fn handles(doc: &Document) -> Vec<(Handle, Pos2)> {
    if let Some(obj) = single_linear(doc)
        && let ObjectKind::Shape { a, b, .. } = obj.kind
    {
        return vec![(Handle::End(false), a), (Handle::End(true), b)];
    }
    let Some(r) = selection_bounds(doc) else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(8);
    for hy in -1i8..=1 {
        for hx in -1i8..=1 {
            if hx == 0 && hy == 0 {
                continue;
            }
            let p = r.center() + vec2(hx as f32 * r.width() * 0.5, hy as f32 * r.height() * 0.5);
            out.push((Handle::Box(hx, hy), p));
        }
    }
    out
}

fn handle_at(doc: &Document, p: Pos2, px: f32) -> Option<Handle> {
    handles(doc)
        .into_iter()
        .find(|(_, hp)| hp.distance(p) <= HANDLE_REACH * px)
        .map(|(h, _)| h)
}

impl SelectTool {
    pub fn is_active(&self) -> bool {
        !matches!(self.mode, Mode::Idle)
    }

    /// Abort a drag. Moves already applied stay, as one undo step.
    pub fn cancel(&mut self) {
        self.mode = Mode::Idle;
    }

    pub fn input(&mut self, doc: &mut Document, input: &Input) -> Outcome {
        if input.pressed {
            return self.press(doc, input);
        }
        if input.down || input.released {
            self.drag(doc, input);
        }
        if input.released {
            self.release(doc, input);
        }
        Outcome::None
    }

    fn press(&mut self, doc: &mut Document, input: &Input) -> Outcome {
        let tol = 4.0 * input.px;
        if input.double_clicked
            && let Some(obj) = doc.hit(input.pos, tol)
            && matches!(obj.kind, ObjectKind::Text { .. })
        {
            self.mode = Mode::Idle;
            return Outcome::EditText(obj.id);
        }
        if let Some(handle) = handle_at(doc, input.pos, input.px) {
            self.mode = match handle {
                Handle::End(b) => Mode::Endpoint {
                    b,
                    original: single_linear(doc)
                        .expect("end handles imply one line")
                        .clone(),
                    moved: false,
                },
                Handle::Box(hx, hy) => {
                    let originals: Vec<Object> = doc
                        .selected_objects()
                        .iter()
                        .map(|o| (**o).clone())
                        .collect();
                    let geo_from = originals
                        .iter()
                        .map(|o| geom::shape_bounds(&o.kind))
                        .reduce(|a, b| a.union(b))
                        .expect("handles imply a selection");
                    Mode::Resizing {
                        handle: (hx, hy),
                        from: selection_bounds(doc).expect("handles imply a selection"),
                        geo_from,
                        originals,
                        moved: false,
                    }
                }
            };
            return Outcome::None;
        }
        if let Some(id) = doc.hit(input.pos, tol).map(|o| o.id) {
            if input.shift {
                if !doc.selection.remove(&id) {
                    doc.selection.insert(id);
                }
            } else if !doc.selection.contains(&id) {
                doc.selection = HashSet::from([id]);
            }
            self.mode = Mode::Moving {
                start: input.pos,
                originals: doc
                    .selected_objects()
                    .iter()
                    .map(|o| (**o).clone())
                    .collect(),
                clicked: id,
                moved: false,
            };
            return Outcome::None;
        }
        if !input.shift {
            doc.selection.clear();
        }
        self.mode = Mode::Box {
            start: input.pos,
            end: input.pos,
            additive: input.shift,
        };
        Outcome::None
    }

    fn drag(&mut self, doc: &mut Document, input: &Input) {
        // The first real movement records the undo step, so plain clicks don't add any.
        let begin = |doc: &mut Document, moved: &mut bool| {
            if !*moved {
                doc.checkpoint();
                *moved = true;
            }
        };
        match &mut self.mode {
            Mode::Idle => {}
            Mode::Moving {
                start,
                originals,
                moved,
                ..
            } => {
                let mut d = input.pos - *start;
                if input.shift {
                    // Lock to the dominant axis.
                    if d.x.abs() > d.y.abs() {
                        d.y = 0.0
                    } else {
                        d.x = 0.0
                    }
                }
                if !*moved && d.length() < 2.0 * input.px {
                    return;
                }
                begin(doc, moved);
                let new = originals
                    .iter()
                    .map(|o| Object {
                        id: o.id,
                        kind: geom::translate(&o.kind, d),
                    })
                    .collect();
                doc.apply_no_history(Edit::Replace(new));
            }
            Mode::Resizing {
                handle,
                from,
                geo_from,
                originals,
                moved,
            } => {
                let to = resized(*from, *handle, input.pos, input.shift);
                if !*moved && to == *from {
                    return;
                }
                begin(doc, moved);
                // Move the geometry's edges by as much as the handle bounds' edges moved.
                let geo_to = Rect {
                    min: geo_from.min + (to.min - from.min),
                    max: geo_from.max + (to.max - from.max),
                };
                let new = originals
                    .iter()
                    .map(|o| Object {
                        id: o.id,
                        kind: geom::transform(&o.kind, *geo_from, geo_to),
                    })
                    .collect();
                doc.apply_no_history(Edit::Replace(new));
            }
            Mode::Endpoint { b, original, moved } => {
                let ObjectKind::Shape {
                    shape,
                    a: oa,
                    b: ob,
                    style,
                } = original.kind.clone()
                else {
                    return;
                };
                let other = if *b { oa } else { ob };
                let p = if input.shift {
                    geom::snap_angle(other, input.pos, 15.0)
                } else {
                    input.pos
                };
                begin(doc, moved);
                let (a, bb) = if *b { (oa, p) } else { (p, ob) };
                doc.apply_no_history(Edit::Replace(vec![Object {
                    id: original.id,
                    kind: ObjectKind::Shape {
                        shape,
                        a,
                        b: bb,
                        style,
                    },
                }]));
            }
            Mode::Box { end, .. } => *end = input.pos,
        }
    }

    fn release(&mut self, doc: &mut Document, input: &Input) {
        match std::mem::replace(&mut self.mode, Mode::Idle) {
            Mode::Box {
                start,
                end,
                additive,
            } => {
                let r = Rect::from_two_pos(start, end);
                if r.width().max(r.height()) < 2.0 * input.px {
                    return;
                }
                if !additive {
                    doc.selection.clear();
                }
                for o in &doc.objects {
                    if r.contains_rect(geom::bounds(&o.kind)) {
                        doc.selection.insert(o.id);
                    }
                }
            }
            Mode::Moving {
                clicked,
                moved: false,
                ..
            } if !input.shift => {
                // Clicking one object of a multi-selection without dragging selects just it.
                doc.selection = HashSet::from([clicked]);
            }
            _ => {}
        }
    }

    pub fn draw(&self, painter: &Painter, doc: &Document, cam: &Camera) {
        let thin = Stroke::new(1.0, ACCENT);
        for o in &doc.objects {
            if doc.selection.contains(&o.id) {
                let r = cam.rect_to_screen(geom::bounds(&o.kind)).expand(2.0);
                painter.rect_stroke(
                    r,
                    0.0,
                    Stroke::new(1.0, ACCENT.gamma_multiply(0.6)),
                    StrokeKind::Outside,
                );
            }
        }
        if doc.selection.len() > 1
            && single_linear(doc).is_none()
            && let Some(r) = selection_bounds(doc)
        {
            painter.rect_stroke(
                cam.rect_to_screen(r).expand(4.0),
                0.0,
                thin,
                StrokeKind::Outside,
            );
        }
        if !matches!(self.mode, Mode::Moving { moved: true, .. }) {
            for (_, p) in handles(doc) {
                let r = Rect::from_center_size(cam.to_screen(p), Vec2::splat(HANDLE_SIZE));
                painter.rect(r, 1.0, Color32::WHITE, thin, StrokeKind::Middle);
            }
        }
        if let Mode::Box { start, end, .. } = self.mode {
            let r = Rect::from_two_pos(cam.to_screen(start), cam.to_screen(end));
            painter.rect(
                r,
                0.0,
                ACCENT.gamma_multiply(0.08),
                thin,
                StrokeKind::Inside,
            );
        }
    }

    pub fn cursor(&self, doc: &Document, input: &Input) -> CursorIcon {
        match &self.mode {
            Mode::Moving { .. } => return CursorIcon::Move,
            Mode::Resizing { handle, .. } => return resize_cursor(*handle),
            Mode::Endpoint { .. } => return CursorIcon::Crosshair,
            _ => {}
        }
        match handle_at(doc, input.pos, input.px) {
            Some(Handle::Box(hx, hy)) => resize_cursor((hx, hy)),
            Some(Handle::End(_)) => CursorIcon::Crosshair,
            None if doc.hit(input.pos, 4.0 * input.px).is_some() => CursorIcon::Move,
            None => CursorIcon::Default,
        }
    }
}

fn resize_cursor((hx, hy): (i8, i8)) -> CursorIcon {
    match (hx, hy) {
        (0, _) => CursorIcon::ResizeVertical,
        (_, 0) => CursorIcon::ResizeHorizontal,
        (x, y) if x == y => CursorIcon::ResizeNwSe,
        _ => CursorIcon::ResizeNeSw,
    }
}

/// New bounds when dragging handle `(hx, hy)` of `from` to `p`. Shift on a corner keeps
/// the aspect ratio.
fn resized(from: Rect, (hx, hy): (i8, i8), p: Pos2, keep_aspect: bool) -> Rect {
    let mut min = from.min;
    let mut max = from.max;
    match hx {
        -1 => min.x = p.x,
        1 => max.x = p.x,
        _ => {}
    }
    match hy {
        -1 => min.y = p.y,
        1 => max.y = p.y,
        _ => {}
    }
    if keep_aspect && hx != 0 && hy != 0 && from.height() > 1e-3 {
        let aspect = from.width() / from.height();
        let w = max.x - min.x;
        let h = max.y - min.y;
        // Follow whichever axis moved further, sized off the fixed corner.
        if (w / aspect).abs() > h.abs() {
            let nh = (w / aspect).abs() * h.signum();
            if hy == -1 {
                min.y = max.y - nh
            } else {
                max.y = min.y + nh
            }
        } else {
            let nw = (h * aspect).abs() * w.signum();
            if hx == -1 {
                min.x = max.x - nw
            } else {
                max.x = min.x + nw
            }
        }
    }
    // Keep a minimum size so objects can't collapse to nothing. Flipping past zero is allowed.
    let w = max.x - min.x;
    let h = max.y - min.y;
    if w.abs() < 1.0 {
        if hx == -1 {
            min.x = max.x - 1.0
        } else if hx == 1 {
            max.x = min.x + 1.0
        }
    }
    if h.abs() < 1.0 {
        if hy == -1 {
            min.y = max.y - 1.0
        } else if hy == 1 {
            max.y = min.y + 1.0
        }
    }
    Rect { min, max }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::pos2;

    #[test]
    fn resize_corner_keeps_aspect() {
        let from = Rect::from_min_max(pos2(0.0, 0.0), pos2(200.0, 100.0));
        let r = resized(from, (1, 1), pos2(400.0, 120.0), true);
        assert_eq!(r.min, pos2(0.0, 0.0));
        assert!((r.width() / r.height() - 2.0).abs() < 1e-3);
    }

    #[test]
    fn resize_edge_moves_one_side() {
        let from = Rect::from_min_max(pos2(0.0, 0.0), pos2(200.0, 100.0));
        let r = resized(from, (-1, 0), pos2(50.0, 999.0), false);
        assert_eq!(r, Rect::from_min_max(pos2(50.0, 0.0), pos2(200.0, 100.0)));
    }
}
