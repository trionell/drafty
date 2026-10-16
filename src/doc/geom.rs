//! Geometry shared by both renderers, hit testing and the tools. Anything that decides
//! where a point ends up lives here once, so the screen and the PNG export agree.

use egui::{Pos2, Rect, Vec2, pos2, vec2};

use super::{Fill, ObjectKind, ShapeKind};

pub fn norm_rect(a: Pos2, b: Pos2) -> Rect {
    Rect::from_two_pos(a, b)
}

/// Apex at `a.y`, base at `b.y`, so dragging upward draws an upside-down triangle.
pub fn triangle_points(a: Pos2, b: Pos2) -> [Pos2; 3] {
    let (x0, x1) = (a.x.min(b.x), a.x.max(b.x));
    [pos2((x0 + x1) * 0.5, a.y), pos2(x1, b.y), pos2(x0, b.y)]
}

pub fn arrow_head_len(width: f32) -> f32 {
    8.0 + width * 2.5
}

/// Arrowhead pointing at `tip`, coming from `from`. Returns the point where the line
/// should stop (so a thick line doesn't poke through the tip) and the head triangle.
pub fn arrowhead(from: Pos2, tip: Pos2, width: f32) -> (Pos2, [Pos2; 3]) {
    let d = tip - from;
    let len = d.length();
    if len < 1e-3 {
        return (tip, [tip; 3]);
    }
    let dir = d / len;
    let head_len = arrow_head_len(width).min(len);
    let half = head_len * 0.5;
    let base = tip - dir * head_len;
    let n = dir.rot90();
    let line_end = tip - dir * head_len * 0.8;
    (line_end, [tip, base + n * half, base - n * half])
}

/// Line segments to draw for a line or arrow, after trimming for the heads.
pub fn arrow_parts(a: Pos2, b: Pos2, width: f32, double: bool) -> (Pos2, Pos2, Vec<[Pos2; 3]>) {
    let (end, head_b) = arrowhead(a, b, width);
    let mut heads = vec![head_b];
    let mut start = a;
    if double {
        let (s, head_a) = arrowhead(b, a, width);
        start = s;
        heads.push(head_a);
    }
    (start, end, heads)
}

pub fn bounds(kind: &ObjectKind) -> Rect {
    match kind {
        ObjectKind::Stroke { points, style, .. } => {
            Rect::from_points(points).expand(style.width * 0.5)
        }
        ObjectKind::Shape { shape, a, b, style } => {
            let pad = match shape {
                ShapeKind::Arrow { .. } => arrow_head_len(style.width) * 0.5,
                _ => style.width * 0.5,
            };
            norm_rect(*a, *b).expand(pad)
        }
        ObjectKind::Text { pos, extent, .. } => Rect::from_min_size(*pos, *extent),
        ObjectKind::Image { rect, .. } => *rect,
    }
}

pub fn union_bounds<'a>(kinds: impl Iterator<Item = &'a ObjectKind>) -> Option<Rect> {
    kinds.map(bounds).reduce(|a, b| a.union(b))
}

pub fn dist_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let len_sq = ab.length_sq();
    if len_sq < 1e-9 {
        return p.distance(a);
    }
    let t = ((p - a).dot(ab) / len_sq).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

fn dist_to_polyline(p: Pos2, points: &[Pos2]) -> f32 {
    match points {
        [] => f32::INFINITY,
        [only] => p.distance(*only),
        _ => points
            .windows(2)
            .map(|w| dist_to_segment(p, w[0], w[1]))
            .fold(f32::INFINITY, f32::min),
    }
}

fn point_in_triangle(p: Pos2, t: [Pos2; 3]) -> bool {
    let sign = |a: Pos2, b: Pos2, c: Pos2| (a.x - c.x) * (b.y - c.y) - (b.x - c.x) * (a.y - c.y);
    let d1 = sign(p, t[0], t[1]);
    let d2 = sign(p, t[1], t[2]);
    let d3 = sign(p, t[2], t[0]);
    let neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(neg && pos)
}

/// Is `p` on the object? `tolerance` is in board units and gets added to half the stroke width.
pub fn hit(kind: &ObjectKind, p: Pos2, tolerance: f32) -> bool {
    if !bounds(kind).expand(tolerance).contains(p) {
        return false;
    }
    match kind {
        ObjectKind::Stroke { points, style, .. } => {
            dist_to_polyline(p, points) <= style.width * 0.5 + tolerance
        }
        ObjectKind::Shape { shape, a, b, style } => {
            let reach = style.width * 0.5 + tolerance;
            let filled = style.fill != Fill::None;
            match shape {
                ShapeKind::Line => dist_to_segment(p, *a, *b) <= reach,
                ShapeKind::Arrow { .. } => {
                    dist_to_segment(p, *a, *b) <= reach.max(arrow_head_len(style.width) * 0.3)
                }
                ShapeKind::Rect => {
                    let r = norm_rect(*a, *b);
                    if filled && r.contains(p) {
                        return true;
                    }
                    let c = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
                    dist_to_polyline(p, &c) <= reach
                }
                ShapeKind::Ellipse => {
                    let r = norm_rect(*a, *b);
                    let radius = (r.size() * 0.5).max(Vec2::splat(0.5));
                    let q = (p - r.center()) / radius;
                    let d = q.length();
                    if filled && d <= 1.0 {
                        return true;
                    }
                    // Scale the normalized distance back by the smaller radius. Exact for
                    // circles and close enough for ellipses.
                    (d - 1.0).abs() * radius.min_elem() <= reach
                }
                ShapeKind::Triangle => {
                    let t = triangle_points(*a, *b);
                    if filled && point_in_triangle(p, t) {
                        return true;
                    }
                    dist_to_polyline(p, &[t[0], t[1], t[2], t[0]]) <= reach
                }
            }
        }
        ObjectKind::Text { .. } | ObjectKind::Image { .. } => true,
    }
}

/// Does the segment `a`-`b` touch the object? Samples along the segment, so a fast eraser
/// drag between two frames doesn't skip objects.
pub fn hit_segment(kind: &ObjectKind, a: Pos2, b: Pos2, tolerance: f32) -> bool {
    let step = tolerance.max(1.0);
    let n = ((a.distance(b) / step).ceil() as usize).clamp(1, 1000);
    (0..=n).any(|i| hit(kind, a.lerp(b, i as f32 / n as f32), tolerance))
}

/// Snap `b` so the line from `a` lands on a multiple of `step_deg`.
pub fn snap_angle(a: Pos2, b: Pos2, step_deg: f32) -> Pos2 {
    let d = b - a;
    let len = d.length();
    if len < 1e-3 {
        return b;
    }
    let step = step_deg.to_radians();
    let angle = (d.angle() / step).round() * step;
    a + Vec2::angled(angle) * len
}

/// Shift-drag constraints: equal sides for boxes, 15° steps for lines.
pub fn constrain(shape: ShapeKind, a: Pos2, b: Pos2) -> Pos2 {
    if shape.is_linear() {
        return snap_angle(a, b, 15.0);
    }
    let d = b - a;
    let side = d.x.abs().max(d.y.abs());
    a + vec2(side * sign(d.x), side * sign(d.y))
}

fn sign(x: f32) -> f32 {
    if x < 0.0 { -1.0 } else { 1.0 }
}

/// Ramer-Douglas-Peucker. Keeps the first and last point.
pub fn simplify(points: &[Pos2], epsilon: f32) -> Vec<Pos2> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    let mut stack = vec![(0, points.len() - 1)];
    while let Some((start, end)) = stack.pop() {
        let mut max_d = 0.0;
        let mut idx = start;
        for i in start + 1..end {
            let d = dist_to_segment(points[i], points[start], points[end]);
            if d > max_d {
                max_d = d;
                idx = i;
            }
        }
        if max_d > epsilon {
            keep[idx] = true;
            stack.push((start, idx));
            stack.push((idx, end));
        }
    }
    points
        .iter()
        .zip(keep)
        .filter_map(|(p, k)| k.then_some(*p))
        .collect()
}

/// Chaikin corner cutting. Keeps the first and last point.
pub fn smooth(points: &[Pos2], iterations: usize) -> Vec<Pos2> {
    let mut pts = points.to_vec();
    for _ in 0..iterations {
        if pts.len() < 3 {
            break;
        }
        let mut out = Vec::with_capacity(pts.len() * 2);
        out.push(pts[0]);
        for w in pts.windows(2) {
            out.push(w[0].lerp(w[1], 0.25));
            out.push(w[0].lerp(w[1], 0.75));
        }
        out.push(*pts.last().unwrap());
        pts = out;
    }
    pts
}

pub fn translate(kind: &ObjectKind, d: Vec2) -> ObjectKind {
    let mut k = kind.clone();
    match &mut k {
        ObjectKind::Stroke { points, .. } => points.iter_mut().for_each(|p| *p += d),
        ObjectKind::Shape { a, b, .. } => {
            *a += d;
            *b += d;
        }
        ObjectKind::Text { pos, .. } => *pos += d,
        ObjectKind::Image { rect, .. } => *rect = rect.translate(d),
    }
    k
}

/// Map an object from one bounding box to another, as when dragging a resize handle.
pub fn transform(kind: &ObjectKind, from: Rect, to: Rect) -> ObjectKind {
    let sx = if from.width() > 1e-3 { to.width() / from.width() } else { 1.0 };
    let sy = if from.height() > 1e-3 { to.height() / from.height() } else { 1.0 };
    let map = |p: Pos2| to.min + vec2((p.x - from.min.x) * sx, (p.y - from.min.y) * sy);
    let mut k = kind.clone();
    match &mut k {
        ObjectKind::Stroke { points, .. } => points.iter_mut().for_each(|p| *p = map(*p)),
        ObjectKind::Shape { a, b, .. } => {
            *a = map(*a);
            *b = map(*b);
        }
        ObjectKind::Text { pos, size, extent, .. } => {
            // Text keeps its aspect ratio and scales with the box height.
            let s = sy.abs().max(0.05);
            *pos = map(*pos);
            *size = (*size * s).clamp(4.0, 400.0);
            *extent *= s;
        }
        ObjectKind::Image { rect, .. } => *rect = Rect::from_two_pos(map(rect.min), map(rect.max)),
    }
    k
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::Style;
    use egui::Color32;

    fn style(fill: Fill) -> Style {
        Style { color: Color32::BLACK, width: 4.0, fill }
    }

    #[test]
    fn rect_outline_vs_fill() {
        let k = ObjectKind::Shape {
            shape: ShapeKind::Rect,
            a: pos2(0.0, 0.0),
            b: pos2(100.0, 100.0),
            style: style(Fill::None),
        };
        assert!(hit(&k, pos2(0.0, 50.0), 1.0));
        assert!(!hit(&k, pos2(50.0, 50.0), 1.0));
        let filled = ObjectKind::Shape {
            shape: ShapeKind::Rect,
            a: pos2(0.0, 0.0),
            b: pos2(100.0, 100.0),
            style: style(Fill::Solid),
        };
        assert!(hit(&filled, pos2(50.0, 50.0), 1.0));
    }

    #[test]
    fn ellipse_and_triangle() {
        let e = ObjectKind::Shape {
            shape: ShapeKind::Ellipse,
            a: pos2(0.0, 0.0),
            b: pos2(100.0, 100.0),
            style: style(Fill::None),
        };
        assert!(hit(&e, pos2(100.0, 50.0), 1.0));
        assert!(!hit(&e, pos2(50.0, 50.0), 1.0));
        assert!(!hit(&e, pos2(2.0, 2.0), 1.0));
        let t = ObjectKind::Shape {
            shape: ShapeKind::Triangle,
            a: pos2(0.0, 0.0),
            b: pos2(100.0, 100.0),
            style: style(Fill::Translucent),
        };
        assert!(hit(&t, pos2(50.0, 60.0), 1.0));
        assert!(!hit(&t, pos2(5.0, 5.0), 1.0));
    }

    #[test]
    fn stroke_hit_and_segment_sweep() {
        let k = ObjectKind::Stroke {
            points: vec![pos2(0.0, 0.0), pos2(100.0, 0.0)],
            style: style(Fill::None),
            highlighter: false,
        };
        assert!(hit(&k, pos2(50.0, 2.0), 1.0));
        assert!(!hit(&k, pos2(50.0, 10.0), 1.0));
        // Crosses the stroke between two samples far apart.
        assert!(hit_segment(&k, pos2(50.0, -50.0), pos2(50.0, 50.0), 2.0));
    }

    #[test]
    fn smoothing_keeps_endpoints() {
        let pts = vec![pos2(0.0, 0.0), pos2(10.0, 10.0), pos2(20.0, 0.0), pos2(30.0, 10.0)];
        let s = smooth(&pts, 2);
        assert_eq!(s.first(), pts.first());
        assert_eq!(s.last(), pts.last());
        assert!(s.len() > pts.len());
    }

    #[test]
    fn simplify_drops_collinear_points() {
        let pts: Vec<_> = (0..50).map(|i| pos2(i as f32, 0.0)).collect();
        assert_eq!(simplify(&pts, 0.1), vec![pos2(0.0, 0.0), pos2(49.0, 0.0)]);
    }

    #[test]
    fn arrowhead_points_at_tip() {
        let (end, head) = arrowhead(pos2(0.0, 0.0), pos2(100.0, 0.0), 4.0);
        assert_eq!(head[0], pos2(100.0, 0.0));
        assert!(end.x < 100.0 && end.x > 80.0);
        assert!((head[1].y + head[2].y).abs() < 1e-4);
    }

    #[test]
    fn constrain_square_and_angle() {
        let b = constrain(ShapeKind::Rect, pos2(0.0, 0.0), pos2(30.0, -10.0));
        assert_eq!(b, pos2(30.0, -30.0));
        let b = constrain(ShapeKind::Line, pos2(0.0, 0.0), pos2(100.0, 3.0));
        assert!(b.y.abs() < 1e-3);
    }

    #[test]
    fn transform_maps_bounds() {
        let k = ObjectKind::Shape {
            shape: ShapeKind::Rect,
            a: pos2(10.0, 10.0),
            b: pos2(20.0, 20.0),
            style: style(Fill::None),
        };
        let from = Rect::from_min_max(pos2(10.0, 10.0), pos2(20.0, 20.0));
        let to = Rect::from_min_max(pos2(0.0, 0.0), pos2(40.0, 20.0));
        let ObjectKind::Shape { a, b, .. } = transform(&k, from, to) else { panic!() };
        assert_eq!((a, b), (pos2(0.0, 0.0), pos2(40.0, 20.0)));
    }
}
