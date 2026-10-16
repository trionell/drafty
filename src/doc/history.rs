use std::sync::Arc;

use super::Object;

pub const MAX_STEPS: usize = 200;

/// Snapshot-based undo. A snapshot is a vector of `Arc`s, so it copies pointers, not
/// objects. Unchanged objects are shared across every snapshot.
#[derive(Default)]
pub struct History {
    undo: Vec<Vec<Arc<Object>>>,
    redo: Vec<Vec<Arc<Object>>>,
}

impl History {
    pub fn push(&mut self, current: &[Arc<Object>]) {
        self.undo.push(current.to_vec());
        if self.undo.len() > MAX_STEPS {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// Drop the last checkpoint, for a drag that ended without changing anything.
    pub fn discard_last(&mut self) {
        self.undo.pop();
    }

    pub fn undo(&mut self, current: &[Arc<Object>]) -> Option<Vec<Arc<Object>>> {
        let prev = self.undo.pop()?;
        self.redo.push(current.to_vec());
        Some(prev)
    }

    pub fn redo(&mut self, current: &[Arc<Object>]) -> Option<Vec<Arc<Object>>> {
        let next = self.redo.pop()?;
        self.undo.push(current.to_vec());
        Some(next)
    }

    #[cfg(test)]
    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::{ObjectId, ObjectKind};
    use egui::{Pos2, Rect};

    fn obj(id: u64) -> Arc<Object> {
        Arc::new(Object {
            id: ObjectId(id),
            kind: ObjectKind::Image {
                image: crate::doc::ImageId(0),
                rect: Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            },
        })
    }

    #[test]
    fn caps_at_max_steps() {
        let mut h = History::default();
        let state = vec![obj(1)];
        for _ in 0..MAX_STEPS + 50 {
            h.push(&state);
        }
        assert_eq!(h.undo_len(), MAX_STEPS);
    }

    #[test]
    fn snapshots_share_objects() {
        let mut h = History::default();
        let state = vec![obj(1), obj(2)];
        h.push(&state);
        let restored = h.undo(&[]).unwrap();
        assert!(Arc::ptr_eq(&restored[0], &state[0]));
    }

    #[test]
    fn push_clears_redo() {
        let mut h = History::default();
        h.push(&[]);
        assert!(h.undo(&[obj(1)]).is_some());
        h.push(&[]);
        assert!(h.redo(&[]).is_none());
    }
}
