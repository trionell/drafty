use egui::Pos2;

use super::{Input, Settings};
use crate::doc::{Edit, ObjectKind, ShapeKind, geom};

#[derive(Default)]
pub struct ShapeTool {
    start: Option<Pos2>,
    end: Pos2,
}

impl ShapeTool {
    pub fn is_active(&self) -> bool {
        self.start.is_some()
    }

    pub fn cancel(&mut self) {
        self.start = None;
    }

    pub fn input(&mut self, input: &Input, shape: ShapeKind, settings: &Settings) -> Option<Edit> {
        if input.pressed {
            self.start = Some(input.pos);
            self.end = input.pos;
            return None;
        }
        let start = self.start?;
        self.end = if input.shift {
            geom::constrain(shape, start, input.pos)
        } else {
            input.pos
        };
        if !input.released {
            return None;
        }
        self.start = None;
        // A click without a drag shouldn't leave an invisible shape behind.
        if start.distance(self.end) < 3.0 * input.px {
            return None;
        }
        Some(Edit::Add(vec![self.kind(start, shape, settings)]))
    }

    fn kind(&self, start: Pos2, shape: ShapeKind, settings: &Settings) -> ObjectKind {
        ObjectKind::Shape {
            shape,
            a: start,
            b: self.end,
            style: settings.shape_style(),
        }
    }

    pub fn preview(&self, shape: ShapeKind, settings: &Settings) -> Option<ObjectKind> {
        self.start.map(|s| self.kind(s, shape, settings))
    }
}
