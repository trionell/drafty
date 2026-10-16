use egui::Pos2;

use super::{Input, Settings};
use crate::doc::{Edit, ObjectKind, geom};

/// Pen and highlighter. Same capture, different style. Shift on the highlighter draws a
/// straight line, for marking a line of text in a screenshot.
#[derive(Default)]
pub struct PenTool {
    points: Vec<Pos2>,
    active: bool,
}

impl PenTool {
    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn cancel(&mut self) {
        self.active = false;
        self.points.clear();
    }

    pub fn input(&mut self, input: &Input, settings: &Settings, highlighter: bool) -> Option<Edit> {
        if input.pressed {
            self.active = true;
            self.points = vec![input.pos];
            return None;
        }
        if !self.active {
            return None;
        }
        if input.down || input.released {
            if highlighter && input.shift {
                self.points.truncate(1);
                self.points.push(geom::snap_angle(self.points[0], input.pos, 15.0));
            } else if self.points.last().is_none_or(|l| l.distance(input.pos) > 0.5 * input.px) {
                self.points.push(input.pos);
            }
        }
        if !input.released {
            return None;
        }
        self.active = false;
        let raw = std::mem::take(&mut self.points);
        // Smooth on release so the line doesn't lag behind the cursor while drawing.
        let points = if raw.len() > 2 {
            geom::smooth(&geom::simplify(&raw, 0.3 * input.px), 2)
        } else {
            raw
        };
        let style = if highlighter { settings.highlight_style() } else { settings.pen_style() };
        Some(Edit::Add(vec![ObjectKind::Stroke { points, style, highlighter }]))
    }

    pub fn preview(&self, settings: &Settings, highlighter: bool) -> Option<ObjectKind> {
        if !self.active || self.points.is_empty() {
            return None;
        }
        let style = if highlighter { settings.highlight_style() } else { settings.pen_style() };
        Some(ObjectKind::Stroke { points: self.points.clone(), style, highlighter })
    }
}
