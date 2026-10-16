pub mod geom;
pub mod history;
pub mod io;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use egui::{Color32, Pos2, Rect, Vec2};

use history::History;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct ObjectId(pub u64);

/// Content hash of an image's pixels, so pasting the same screenshot twice stores it once.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ImageId(pub u64);

impl ImageId {
    pub fn to_hex(self) -> String {
        format!("{:016x}", self.0)
    }

    pub fn from_hex(s: &str) -> Option<Self> {
        u64::from_str_radix(s, 16).ok().map(Self)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fill {
    None,
    Solid,
    Translucent,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Style {
    pub color: Color32,
    pub width: f32,
    pub fill: Fill,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShapeKind {
    Rect,
    Ellipse,
    Triangle,
    Line,
    Arrow { double: bool },
}

impl ShapeKind {
    pub fn is_linear(self) -> bool {
        matches!(self, ShapeKind::Line | ShapeKind::Arrow { .. })
    }
}

#[derive(Clone, PartialEq, Debug)]
pub enum ObjectKind {
    Stroke {
        points: Vec<Pos2>,
        style: Style,
        highlighter: bool,
    },
    /// `a` and `b` are the drag start and end. Lines and arrows need the direction,
    /// the other shapes normalize when drawn.
    Shape {
        shape: ShapeKind,
        a: Pos2,
        b: Pos2,
        style: Style,
    },
    /// `pos` is the top-left corner. `extent` is the laid-out size at zoom 1, measured
    /// with egui's fonts whenever the text or size changes.
    Text {
        pos: Pos2,
        text: String,
        size: f32,
        color: Color32,
        extent: Vec2,
    },
    Image {
        image: ImageId,
        rect: Rect,
    },
}

#[derive(Clone, PartialEq, Debug)]
pub struct Object {
    pub id: ObjectId,
    pub kind: ObjectKind,
}

/// An image's encoded bytes (PNG/JPEG) and, while needed, its decoded pixels.
///
/// Decoded pixels are big (33 MB for a 4K screenshot), so they're only kept while
/// something needs them: a pasted image keeps them until it has been both encoded for
/// saving and uploaded to the GPU. After that, `rgba()` decodes again on demand.
pub struct ImageData {
    pub size: [u32; 2],
    encoded: OnceLock<Arc<Vec<u8>>>,
    rgba: Mutex<Option<Arc<Vec<u8>>>>,
    uploaded: AtomicBool,
}

impl std::fmt::Debug for ImageData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ImageData({}x{})", self.size[0], self.size[1])
    }
}

impl ImageData {
    pub fn from_rgba(size: [u32; 2], rgba: Vec<u8>) -> (ImageId, Self) {
        let id = hash_bytes(b"rgba", &rgba, size);
        let data = Self {
            size,
            encoded: OnceLock::new(),
            rgba: Mutex::new(Some(Arc::new(rgba))),
            uploaded: AtomicBool::new(false),
        };
        (id, data)
    }

    /// Reads only the header, so opening a big image is instant. Decoding waits until
    /// the image is first drawn.
    pub fn from_encoded(bytes: Vec<u8>) -> Result<(ImageId, Self), String> {
        let data = Self::from_encoded_lazy(bytes)?;
        let id = hash_bytes(b"file", data.encoded.get().expect("just set"), data.size);
        Ok((id, data))
    }

    /// For loading from a `.drafty` file, where the id is already known.
    pub fn from_encoded_lazy(bytes: Vec<u8>) -> Result<Self, String> {
        let dims = image::ImageReader::new(std::io::Cursor::new(&bytes))
            .with_guessed_format()
            .map_err(|e| e.to_string())?
            .into_dimensions()
            .map_err(|e| e.to_string())?;
        Ok(Self {
            size: [dims.0, dims.1],
            encoded: OnceLock::from(Arc::new(bytes)),
            rgba: Mutex::new(None),
            uploaded: AtomicBool::new(false),
        })
    }

    fn pixels(&self) -> std::sync::MutexGuard<'_, Option<Arc<Vec<u8>>>> {
        self.rgba.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// RGBA pixels with straight alpha. Decodes if they were released.
    pub fn rgba(&self) -> Arc<Vec<u8>> {
        if let Some(p) = self.pixels().as_ref() {
            return p.clone();
        }
        let bytes = self.encoded.get().expect("image has neither pixels nor bytes");
        match image::load_from_memory(bytes) {
            Ok(img) => Arc::new(img.into_rgba8().into_raw()),
            Err(e) => {
                eprintln!("drafty: failed to decode image: {e}");
                Arc::new(vec![255; (self.size[0] * self.size[1] * 4) as usize])
            }
        }
    }

    pub fn encoded(&self) -> Arc<Vec<u8>> {
        let bytes = self
            .encoded
            .get_or_init(|| {
                let rgba = self.pixels().clone().expect("image has neither pixels nor bytes");
                let mut out = Vec::new();
                let encoder = image::codecs::png::PngEncoder::new_with_quality(
                    &mut out,
                    image::codecs::png::CompressionType::Fast,
                    image::codecs::png::FilterType::Adaptive,
                );
                use image::ImageEncoder as _;
                if let Err(e) = encoder.write_image(
                    &rgba,
                    self.size[0],
                    self.size[1],
                    image::ExtendedColorType::Rgba8,
                ) {
                    eprintln!("drafty: failed to encode image: {e}");
                }
                Arc::new(out)
            })
            .clone();
        self.release_if_done();
        bytes
    }

    /// The renderer calls this once the pixels are on the GPU.
    pub fn mark_uploaded(&self) {
        self.uploaded.store(true, Ordering::Relaxed);
        self.release_if_done();
    }

    fn release_if_done(&self) {
        if self.uploaded.load(Ordering::Relaxed) && self.encoded.get().is_some() {
            *self.pixels() = None;
        }
    }

    #[cfg(test)]
    pub fn has_pixels(&self) -> bool {
        self.pixels().is_some()
    }
}

fn hash_bytes(kind: &[u8], bytes: &[u8], size: [u32; 2]) -> ImageId {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    kind.hash(&mut h);
    size.hash(&mut h);
    bytes.hash(&mut h);
    ImageId(h.finish())
}

/// Maps board coordinates to screen coordinates: `screen = origin + board * zoom + offset`.
/// `origin` is the canvas' top-left on screen and gets set every frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub offset: Vec2,
    pub zoom: f32,
    pub origin: Pos2,
}

pub const MIN_ZOOM: f32 = 0.1;
pub const MAX_ZOOM: f32 = 8.0;

impl Default for Camera {
    fn default() -> Self {
        Self {
            offset: Vec2::ZERO,
            zoom: 1.0,
            origin: Pos2::ZERO,
        }
    }
}

impl Camera {
    pub fn to_screen(self, p: Pos2) -> Pos2 {
        self.origin + p.to_vec2() * self.zoom + self.offset
    }

    pub fn to_board(self, p: Pos2) -> Pos2 {
        ((p - self.origin - self.offset) / self.zoom).to_pos2()
    }

    pub fn rect_to_screen(self, r: Rect) -> Rect {
        Rect::from_min_max(self.to_screen(r.min), self.to_screen(r.max))
    }

    pub fn rect_to_board(self, r: Rect) -> Rect {
        Rect::from_min_max(self.to_board(r.min), self.to_board(r.max))
    }

    /// Zoom by `factor`, keeping the board point under `anchor` (screen) fixed.
    pub fn zoom_around(&mut self, anchor: Pos2, factor: f32) {
        let before = self.to_board(anchor);
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let after = self.to_screen(before);
        self.offset += anchor - after;
    }

    /// Center `board_rect` in a viewport of `view_size`, zooming out if needed but never above `max_zoom`.
    pub fn fit(&mut self, board_rect: Rect, view_size: Vec2, max_zoom: f32) {
        let margin = 40.0;
        let avail = (view_size - Vec2::splat(margin * 2.0)).max(Vec2::splat(1.0));
        let size = board_rect.size().max(Vec2::splat(1.0));
        self.zoom = (avail.x / size.x)
            .min(avail.y / size.y)
            .min(max_zoom)
            .clamp(MIN_ZOOM, MAX_ZOOM);
        self.offset = view_size * 0.5 - board_rect.center().to_vec2() * self.zoom;
    }
}

/// One board, shown as one tab.
pub struct Document {
    pub title: String,
    pub path: Option<PathBuf>,
    /// Stable name for this tab's session file.
    pub session_id: String,
    pub objects: Vec<Arc<Object>>,
    pub images: HashMap<ImageId, Arc<ImageData>>,
    pub camera: Camera,
    pub history: History,
    pub selection: HashSet<ObjectId>,
    pub background: Color32,
    /// Bumped on every change. Compared with the revs below to know what needs saving.
    pub rev: u64,
    pub file_rev: u64,
    pub session_rev: u64,
    next_id: u64,
}

/// A change to a document. Tools build these, `Document::apply` runs them and records history.
#[derive(Debug)]
pub enum Edit {
    Add(Vec<ObjectKind>),
    Remove(HashSet<ObjectId>),
    /// Replace objects with the same ids.
    Replace(Vec<Object>),
    ToFront(HashSet<ObjectId>),
    ToBack(HashSet<ObjectId>),
}

impl Document {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            path: None,
            session_id: new_session_id(),
            objects: Vec::new(),
            images: HashMap::new(),
            camera: Camera::default(),
            history: History::default(),
            selection: HashSet::new(),
            background: Color32::WHITE,
            rev: 0,
            file_rev: 0,
            session_rev: 0,
            next_id: 1,
        }
    }

    /// Unsaved changes relative to the file on disk, or content that has never been saved.
    pub fn is_dirty(&self) -> bool {
        self.rev != self.file_rev || (self.path.is_none() && !self.objects.is_empty())
    }

    pub fn get(&self, id: ObjectId) -> Option<&Arc<Object>> {
        self.objects.iter().find(|o| o.id == id)
    }

    fn alloc_id(&mut self) -> ObjectId {
        let id = ObjectId(self.next_id);
        self.next_id += 1;
        id
    }

    /// Used by the loader so new ids don't collide with loaded ones.
    pub(crate) fn set_objects(&mut self, objects: Vec<Arc<Object>>) {
        self.next_id = objects.iter().map(|o| o.id.0 + 1).max().unwrap_or(1);
        self.objects = objects;
    }

    pub fn add_image(&mut self, id: ImageId, data: ImageData) {
        self.images.entry(id).or_insert_with(|| Arc::new(data));
    }

    pub fn touch(&mut self) {
        self.rev += 1;
    }

    /// Save the current state as an undo step. Call before changing `objects` directly.
    pub fn checkpoint(&mut self) {
        self.history.push(&self.objects);
    }

    /// Apply an edit as one undo step. Returns the ids of added objects.
    pub fn apply(&mut self, edit: Edit) -> Vec<ObjectId> {
        if is_noop(&edit) {
            return Vec::new();
        }
        self.checkpoint();
        self.apply_no_history(edit)
    }

    /// Apply without recording history. Used for live previews between a `checkpoint`
    /// and the end of a drag.
    pub fn apply_no_history(&mut self, edit: Edit) -> Vec<ObjectId> {
        let mut added = Vec::new();
        match edit {
            Edit::Add(kinds) => {
                for kind in kinds {
                    let id = self.alloc_id();
                    added.push(id);
                    self.objects.push(Arc::new(Object { id, kind }));
                }
            }
            Edit::Remove(ids) => {
                self.objects.retain(|o| !ids.contains(&o.id));
                self.selection.retain(|id| !ids.contains(id));
            }
            Edit::Replace(new) => {
                let mut by_id: HashMap<ObjectId, Object> =
                    new.into_iter().map(|o| (o.id, o)).collect();
                for slot in &mut self.objects {
                    if let Some(o) = by_id.remove(&slot.id) {
                        *slot = Arc::new(o);
                    }
                }
            }
            Edit::ToFront(ids) => {
                let (moved, rest): (Vec<_>, Vec<_>) =
                    self.objects.drain(..).partition(|o| ids.contains(&o.id));
                self.objects = rest;
                self.objects.extend(moved);
            }
            Edit::ToBack(ids) => {
                let (mut moved, rest): (Vec<_>, Vec<_>) =
                    self.objects.drain(..).partition(|o| ids.contains(&o.id));
                moved.extend(rest);
                self.objects = moved;
            }
        }
        self.touch();
        added
    }

    pub fn undo(&mut self) -> bool {
        match self.history.undo(&self.objects) {
            Some(objects) => {
                self.objects = objects;
                self.after_history_change();
                true
            }
            None => false,
        }
    }

    pub fn redo(&mut self) -> bool {
        match self.history.redo(&self.objects) {
            Some(objects) => {
                self.objects = objects;
                self.after_history_change();
                true
            }
            None => false,
        }
    }

    fn after_history_change(&mut self) {
        let existing: HashSet<ObjectId> = self.objects.iter().map(|o| o.id).collect();
        self.selection.retain(|id| existing.contains(id));
        self.touch();
    }

    pub fn selected_objects(&self) -> Vec<Arc<Object>> {
        self.objects
            .iter()
            .filter(|o| self.selection.contains(&o.id))
            .cloned()
            .collect()
    }

    /// Bounds of all objects, or `None` for an empty board.
    pub fn content_bounds(&self) -> Option<Rect> {
        geom::union_bounds(self.objects.iter().map(|o| &o.kind))
    }

    /// Topmost object under `p`.
    pub fn hit(&self, p: Pos2, tolerance: f32) -> Option<&Arc<Object>> {
        self.objects
            .iter()
            .rev()
            .find(|o| geom::hit(&o.kind, p, tolerance))
    }
}

fn is_noop(edit: &Edit) -> bool {
    match edit {
        Edit::Add(v) => v.is_empty(),
        Edit::Replace(v) => v.is_empty(),
        Edit::Remove(s) | Edit::ToFront(s) | Edit::ToBack(s) => s.is_empty(),
    }
}

fn new_session_id() -> String {
    use std::sync::atomic::AtomicU64;
    use std::time::{SystemTime, UNIX_EPOCH};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{:x}-{:x}", nanos, COUNTER.fetch_add(1, Ordering::Relaxed))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect_kind(x: f32) -> ObjectKind {
        ObjectKind::Shape {
            shape: ShapeKind::Rect,
            a: Pos2::new(x, 0.0),
            b: Pos2::new(x + 10.0, 10.0),
            style: Style {
                color: Color32::BLACK,
                width: 2.0,
                fill: Fill::None,
            },
        }
    }

    #[test]
    fn apply_undo_redo() {
        let mut doc = Document::new("t");
        let ids = doc.apply(Edit::Add(vec![rect_kind(0.0), rect_kind(20.0)]));
        assert_eq!(ids.len(), 2);
        doc.apply(Edit::Remove(HashSet::from([ids[0]])));
        assert_eq!(doc.objects.len(), 1);
        assert!(doc.undo());
        assert_eq!(doc.objects.len(), 2);
        assert!(doc.undo());
        assert!(doc.objects.is_empty());
        assert!(!doc.undo());
        assert!(doc.redo());
        assert!(doc.redo());
        assert_eq!(doc.objects.len(), 1);
        assert!(!doc.redo());
    }

    #[test]
    fn z_order() {
        let mut doc = Document::new("t");
        let ids = doc.apply(Edit::Add(vec![rect_kind(0.0), rect_kind(1.0), rect_kind(2.0)]));
        doc.apply(Edit::ToFront(HashSet::from([ids[0]])));
        assert_eq!(doc.objects.last().unwrap().id, ids[0]);
        doc.apply(Edit::ToBack(HashSet::from([ids[2]])));
        assert_eq!(doc.objects[0].id, ids[2]);
    }

    #[test]
    fn undo_drops_missing_selection() {
        let mut doc = Document::new("t");
        let ids = doc.apply(Edit::Add(vec![rect_kind(0.0)]));
        doc.selection.insert(ids[0]);
        doc.undo();
        assert!(doc.selection.is_empty());
    }

    #[test]
    fn pasted_pixels_released_after_encode_and_upload() {
        let (_, data) = ImageData::from_rgba([1, 1], vec![1, 2, 3, 255]);
        data.mark_uploaded();
        assert!(data.has_pixels(), "not saved yet, keep the pixels");
        let png = data.encoded();
        assert!(!data.has_pixels());
        assert_eq!(&*data.rgba(), &[1, 2, 3, 255], "decodes again from the PNG");
        assert!(!png.is_empty());
    }

    #[test]
    fn camera_roundtrip_and_zoom_anchor() {
        let mut cam = Camera {
            origin: Pos2::new(10.0, 50.0),
            ..Default::default()
        };
        let anchor = Pos2::new(200.0, 300.0);
        let board = cam.to_board(anchor);
        cam.zoom_around(anchor, 2.0);
        assert!((cam.to_screen(board) - anchor).length() < 1e-3);
        assert!((cam.to_board(cam.to_screen(Pos2::new(3.0, 4.0))) - Pos2::new(3.0, 4.0)).length() < 1e-4);
    }
}
