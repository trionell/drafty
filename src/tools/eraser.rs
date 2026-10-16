use std::collections::HashSet;

use egui::Pos2;

use super::Input;
use crate::doc::{Document, Edit, ObjectId, geom};

/// Screen points around the cursor that count as touching.
pub const RADIUS: f32 = 6.0;

/// Deletes whole objects. Touched objects fade while dragging and go away on release,
/// so the whole drag is one undo step.
#[derive(Default)]
pub struct EraserTool {
    last: Option<Pos2>,
    pub hits: HashSet<ObjectId>,
}

impl EraserTool {
    pub fn cancel(&mut self) {
        self.last = None;
        self.hits.clear();
    }

    pub fn input(&mut self, input: &Input, doc: &Document) -> Option<Edit> {
        if input.pressed {
            self.last = Some(input.pos);
            self.hits.clear();
        }
        let last = self.last?;
        let tol = RADIUS * input.px;
        for obj in &doc.objects {
            if !self.hits.contains(&obj.id) && geom::hit_segment(&obj.kind, last, input.pos, tol) {
                self.hits.insert(obj.id);
            }
        }
        self.last = Some(input.pos);
        if input.released {
            self.last = None;
            return Some(Edit::Remove(std::mem::take(&mut self.hits)));
        }
        None
    }
}
