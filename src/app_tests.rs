//! Drives the whole app through egui with synthetic input, no window needed.

use std::path::PathBuf;

use egui::{Context, Event, Key, Modifiers, PointerButton, Pos2, RawInput, Rect, pos2, vec2};

use crate::app::App;
use crate::clipboard::Clipboard;
use crate::doc::{ObjectKind, ShapeKind};
use crate::session::Session;
use crate::tools::Tool;

struct Harness {
    ctx: Context,
    app: App,
    time: f64,
    mods: Modifiers,
    sent_mods: Modifiers,
    dir: PathBuf,
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("drafty-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

impl Harness {
    fn new(name: &str) -> Self {
        Self::with_dir(temp_dir(name))
    }

    fn with_dir(dir: PathBuf) -> Self {
        let ctx = Context::default();
        let app = App::new(
            &ctx,
            Vec::new(),
            Session::at(dir.clone()),
            Clipboard::disabled(),
        );
        let mut h = Self {
            ctx,
            app,
            time: 0.0,
            mods: Modifiers::NONE,
            sent_mods: Modifiers::NONE,
            dir,
        };
        h.frame(vec![]);
        h.frame(vec![]);
        h
    }

    fn frame(&mut self, mut events: Vec<Event>) {
        if self.mods != self.sent_mods {
            events.insert(0, Event::ModifiersChanged(self.mods));
            self.sent_mods = self.mods;
        }
        // Space frames out so egui doesn't read separate clicks as a double-click.
        self.time += 0.6;
        let raw = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 800.0))),
            time: Some(self.time),
            events,
            focused: true,
            ..Default::default()
        };
        let app = &mut self.app;
        let mut out = self.ctx.run_ui(raw, |ui| app.show(ui));
        // Tessellate like eframe does, so the frame does the full CPU work.
        let shapes = std::mem::take(&mut out.shapes);
        let _ = self.ctx.tessellate(shapes, out.pixels_per_point);
        // No GPU here; drop the texture uploads.
        out.textures_delta.clear();
    }

    /// Screen position of a board point.
    fn screen(&self, p: Pos2) -> Pos2 {
        self.app.doc().camera.to_screen(p)
    }

    fn button(&self, pos: Pos2, pressed: bool) -> Event {
        Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: self.mods,
        }
    }

    /// Drag between two board points.
    fn drag(&mut self, from: Pos2, to: Pos2) {
        let (a, b) = (self.screen(from), self.screen(to));
        self.frame(vec![Event::PointerMoved(a)]);
        self.frame(vec![self.button(a, true)]);
        for i in 1..=8 {
            let p = a.lerp(b, i as f32 / 8.0);
            self.frame(vec![Event::PointerMoved(p)]);
        }
        self.frame(vec![self.button(b, false)]);
        self.frame(vec![]);
    }

    fn click(&mut self, at: Pos2) {
        let p = self.screen(at);
        self.frame(vec![Event::PointerMoved(p)]);
        self.frame(vec![self.button(p, true), self.button(p, false)]);
        self.frame(vec![]);
    }

    fn double_click(&mut self, at: Pos2) {
        let p = self.screen(at);
        self.frame(vec![Event::PointerMoved(p)]);
        // Two clicks inside egui's double-click window.
        self.time -= 0.55;
        self.frame(vec![self.button(p, true), self.button(p, false)]);
        self.time -= 0.55;
        self.frame(vec![self.button(p, true), self.button(p, false)]);
        self.frame(vec![]);
    }

    fn key_with(&mut self, key: Key, mods: Modifiers) {
        let ev = |pressed| Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: mods,
        };
        let old = self.mods;
        self.mods = mods;
        self.frame(vec![ev(true)]);
        self.frame(vec![ev(false)]);
        self.mods = old;
        self.frame(vec![]);
    }

    fn key(&mut self, key: Key) {
        self.key_with(key, Modifiers::NONE);
    }

    fn cmd(&mut self, key: Key) {
        self.key_with(key, Modifiers::COMMAND);
    }

    fn cmd_shift(&mut self, key: Key) {
        self.key_with(key, Modifiers::COMMAND | Modifiers::SHIFT);
    }

    fn kinds(&self) -> Vec<ObjectKind> {
        self.app
            .doc()
            .objects
            .iter()
            .map(|o| o.kind.clone())
            .collect()
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn pen_draws_a_stroke() {
    let mut h = Harness::new("pen");
    assert_eq!(h.app.tool, Tool::Pen);
    h.drag(pos2(100.0, 100.0), pos2(300.0, 200.0));
    let kinds = h.kinds();
    assert_eq!(kinds.len(), 1);
    let ObjectKind::Stroke {
        points,
        highlighter,
        ..
    } = &kinds[0]
    else {
        panic!("{kinds:?}")
    };
    assert!(!highlighter);
    assert!(points.len() >= 2);
    assert!(points[0].distance(pos2(100.0, 100.0)) < 1.0);
    assert!(points.last().unwrap().distance(pos2(300.0, 200.0)) < 1.0);
}

#[test]
fn click_leaves_a_dot() {
    let mut h = Harness::new("dot");
    h.click(pos2(50.0, 60.0));
    assert_eq!(h.kinds().len(), 1);
}

#[test]
fn shapes_and_shift_constraint() {
    let mut h = Harness::new("shapes");
    h.key(Key::R);
    assert_eq!(h.app.tool, Tool::Rect);
    h.drag(pos2(100.0, 100.0), pos2(200.0, 150.0));
    let ObjectKind::Shape {
        shape: ShapeKind::Rect,
        a,
        b,
        ..
    } = h.kinds()[0]
    else {
        panic!()
    };
    assert!(a.distance(pos2(100.0, 100.0)) < 1.0 && b.distance(pos2(200.0, 150.0)) < 1.0);

    h.key(Key::O);
    h.mods = Modifiers::SHIFT;
    h.drag(pos2(300.0, 100.0), pos2(400.0, 130.0));
    h.mods = Modifiers::NONE;
    let ObjectKind::Shape {
        shape: ShapeKind::Ellipse,
        a,
        b,
        ..
    } = h.kinds()[1]
    else {
        panic!()
    };
    let d = b - a;
    assert!((d.x - d.y).abs() < 1.0, "shift should make a circle: {d:?}");

    h.key(Key::A);
    h.drag(pos2(100.0, 300.0), pos2(250.0, 320.0));
    assert!(matches!(
        h.kinds()[2],
        ObjectKind::Shape {
            shape: ShapeKind::Arrow { double: false },
            ..
        }
    ));
}

#[test]
fn tiny_shape_drag_is_ignored() {
    let mut h = Harness::new("tiny");
    h.key(Key::R);
    h.click(pos2(100.0, 100.0));
    assert!(h.kinds().is_empty());
}

#[test]
fn undo_redo_shortcuts() {
    let mut h = Harness::new("undo");
    h.drag(pos2(10.0, 10.0), pos2(100.0, 100.0));
    h.drag(pos2(10.0, 200.0), pos2(100.0, 300.0));
    assert_eq!(h.kinds().len(), 2);
    h.cmd(Key::Z);
    assert_eq!(h.kinds().len(), 1);
    h.cmd(Key::Z);
    assert_eq!(h.kinds().len(), 0);
    h.cmd_shift(Key::Z);
    assert_eq!(h.kinds().len(), 1);
    h.cmd(Key::Y);
    assert_eq!(h.kinds().len(), 2);
}

#[test]
fn select_move_and_undo() {
    let mut h = Harness::new("move");
    h.key(Key::R);
    h.drag(pos2(100.0, 100.0), pos2(200.0, 200.0));
    h.key(Key::V);
    // Grab the left edge, drag 50 right.
    h.drag(pos2(100.0, 150.0), pos2(150.0, 150.0));
    let ObjectKind::Shape { a, .. } = h.kinds()[0] else {
        panic!()
    };
    assert!(a.distance(pos2(150.0, 100.0)) < 1.0, "{a:?}");
    assert_eq!(h.app.doc().selection.len(), 1);
    h.cmd(Key::Z);
    let ObjectKind::Shape { a, .. } = h.kinds()[0] else {
        panic!()
    };
    assert!(
        a.distance(pos2(100.0, 100.0)) < 1.0,
        "one undo restores the position: {a:?}"
    );
}

#[test]
fn resize_handle() {
    let mut h = Harness::new("resize");
    h.key(Key::R);
    h.drag(pos2(100.0, 100.0), pos2(200.0, 200.0));
    h.key(Key::V);
    h.click(pos2(100.0, 150.0));
    // Bottom-right handle sits at the stroke-expanded corner.
    let w = h.app.settings.shape_width * 0.5;
    h.drag(pos2(200.0 + w, 200.0 + w), pos2(300.0 + w, 300.0 + w));
    let ObjectKind::Shape { a, b, .. } = h.kinds()[0] else {
        panic!()
    };
    assert!(a.distance(pos2(100.0, 100.0)) < 0.01, "{a:?}");
    assert!(b.distance(pos2(300.0, 300.0)) < 0.01, "{b:?}");
}

#[test]
fn box_select_delete_duplicate() {
    let mut h = Harness::new("box");
    h.drag(pos2(100.0, 100.0), pos2(150.0, 120.0));
    h.drag(pos2(100.0, 200.0), pos2(150.0, 220.0));
    h.drag(pos2(600.0, 600.0), pos2(650.0, 620.0));
    h.key(Key::V);
    h.drag(pos2(50.0, 50.0), pos2(300.0, 300.0));
    assert_eq!(h.app.doc().selection.len(), 2);
    h.cmd(Key::D);
    assert_eq!(h.kinds().len(), 5);
    assert_eq!(h.app.doc().selection.len(), 2);
    h.key(Key::Delete);
    assert_eq!(h.kinds().len(), 3);
    h.cmd(Key::A);
    assert_eq!(h.app.doc().selection.len(), 3);
    h.key(Key::Escape);
    assert!(h.app.doc().selection.is_empty());
}

#[test]
fn eraser_removes_touched_objects_in_one_step() {
    let mut h = Harness::new("eraser");
    h.drag(pos2(100.0, 100.0), pos2(300.0, 100.0));
    h.drag(pos2(100.0, 200.0), pos2(300.0, 200.0));
    h.drag(pos2(100.0, 400.0), pos2(300.0, 400.0));
    h.key(Key::E);
    h.drag(pos2(200.0, 50.0), pos2(200.0, 250.0));
    assert_eq!(h.kinds().len(), 1);
    h.cmd(Key::Z);
    assert_eq!(h.kinds().len(), 3);
}

#[test]
fn text_tool_types_and_edits() {
    let mut h = Harness::new("text");
    h.key(Key::T);
    h.click(pos2(200.0, 200.0));
    assert!(h.app.text.is_some());
    h.frame(vec![Event::Text("Hello".into())]);
    // Tool letters must type, not switch tools.
    h.frame(vec![Event::Text(" pv åäö".into())]);
    h.key(Key::Escape);
    h.frame(vec![]);
    assert!(h.app.text.is_none());
    let kinds = h.kinds();
    let ObjectKind::Text {
        text, extent, pos, ..
    } = &kinds[0]
    else {
        panic!("{kinds:?}")
    };
    assert_eq!(text, "Hello pv åäö");
    assert!(extent.x > 20.0 && extent.y > 10.0, "{extent:?}");
    assert_eq!(h.app.tool, Tool::Text);

    let mid = *pos + *extent * 0.5;
    h.key(Key::V);
    h.double_click(mid);
    assert!(h.app.text.is_some(), "double-click should edit the text");
    h.frame(vec![Event::Key {
        key: Key::End,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    }]);
    h.frame(vec![Event::Text("!".into())]);
    // Clicking the board finishes editing.
    h.click(pos2(800.0, 600.0));
    let kinds = h.kinds();
    assert_eq!(kinds.len(), 1);
    let ObjectKind::Text { text, .. } = &kinds[0] else {
        panic!()
    };
    assert_eq!(text, "Hello pv åäö!");
}

#[test]
fn empty_text_is_discarded() {
    let mut h = Harness::new("emptytext");
    h.key(Key::T);
    h.click(pos2(200.0, 200.0));
    h.key(Key::Escape);
    h.frame(vec![]);
    assert!(h.kinds().is_empty());
}

#[test]
fn paste_text_and_image() {
    let mut h = Harness::new("paste");
    h.frame(vec![Event::Paste("from clipboard".into())]);
    assert!(matches!(&h.kinds()[0], ObjectKind::Text { text, .. } if text == "from clipboard"));
    let img = egui::ColorImage::new([40, 20], vec![egui::Color32::RED; 800]);
    h.frame(vec![Event::PasteImage(std::sync::Arc::new(img))]);
    let ObjectKind::Image { rect, .. } = h.kinds()[1] else {
        panic!()
    };
    assert_eq!(rect.size(), vec2(40.0, 20.0));
    assert_eq!(h.app.tool, Tool::Select);
    assert_eq!(h.app.doc().images.len(), 1);
}

#[test]
fn copy_paste_objects_between_tabs() {
    let mut h = Harness::new("objclip");
    h.drag(pos2(100.0, 100.0), pos2(200.0, 100.0));
    h.cmd(Key::A);
    h.frame(vec![Event::Copy]);
    h.cmd(Key::T);
    assert_eq!(h.app.docs.len(), 2);
    assert!(h.kinds().is_empty());
    // The disabled test clipboard can't hold the marker, so hand egui's paste event the
    // marker text directly, as the system clipboard would.
    let marker = h.app.object_clip_marker().expect("objects copied");
    h.frame(vec![Event::Paste(marker)]);
    assert_eq!(h.kinds().len(), 1);
    assert!(matches!(h.kinds()[0], ObjectKind::Stroke { .. }));
}

#[test]
fn tabs_open_close_and_confirm() {
    let mut h = Harness::new("tabs");
    h.cmd(Key::T);
    h.cmd(Key::T);
    assert_eq!(h.app.docs.len(), 3);
    assert_eq!(h.app.active, 2);
    assert_eq!(h.app.doc().title, "Board 3");
    h.key_with(Key::Num1, Modifiers::ALT);
    assert_eq!(h.app.active, 0);
    h.key_with(Key::Tab, Modifiers::COMMAND);
    assert_eq!(h.app.active, 1);
    // Empty tab closes without asking.
    h.cmd(Key::W);
    assert_eq!(h.app.docs.len(), 2);
    // A tab with drawings asks first.
    h.drag(pos2(10.0, 10.0), pos2(50.0, 50.0));
    h.cmd(Key::W);
    assert!(h.app.confirm_close.is_some());
    assert_eq!(h.app.docs.len(), 2);
    h.key(Key::Escape);
    h.frame(vec![]);
    // Closing the last tab leaves a fresh board.
    h.app.close_tab(0);
    h.app.close_tab(0);
    assert_eq!(h.app.docs.len(), 1);
    assert!(h.kinds().is_empty());
}

#[test]
fn keys_dont_reach_the_board_behind_a_dialog() {
    let mut h = Harness::new("dialog");
    h.drag(pos2(100.0, 100.0), pos2(200.0, 100.0));
    h.cmd(Key::A);
    h.key_with(Key::Questionmark, Modifiers::SHIFT);
    assert!(h.app.show_help);
    h.key(Key::Delete);
    h.key(Key::R);
    assert_eq!(h.kinds().len(), 1);
    assert_eq!(h.app.tool, Tool::Select);
    h.key(Key::Escape);
    assert!(!h.app.show_help);
    h.key(Key::Delete);
    assert!(h.kinds().is_empty());
}

#[test]
fn style_bar_edits_selection() {
    let mut h = Harness::new("style");
    h.key(Key::R);
    h.drag(pos2(100.0, 100.0), pos2(200.0, 200.0));
    h.key(Key::V);
    h.click(pos2(100.0, 150.0));
    h.key(Key::Num7);
    h.key(Key::CloseBracket);
    let ObjectKind::Shape { style, .. } = h.kinds()[0] else {
        panic!()
    };
    assert_eq!(style.color, crate::tools::PALETTE[6]);
    assert!(style.width > 4.0);
    // Two style edits, two undo steps.
    h.cmd(Key::Z);
    h.cmd(Key::Z);
    let ObjectKind::Shape { style, .. } = h.kinds()[0] else {
        panic!()
    };
    assert_eq!(style.color, crate::tools::PALETTE[2]);
}

#[test]
fn zoom_and_pan_keep_drawing_in_board_space() {
    let mut h = Harness::new("zoom");
    h.cmd(Key::Equals);
    h.cmd(Key::Equals);
    let zoom = h.app.doc().camera.zoom;
    assert!((zoom - 1.5625).abs() < 1e-3, "{zoom}");
    // Pick board points that are on screen at this zoom.
    let cam = h.app.doc().camera;
    let start = cam.to_board(pos2(500.0, 400.0));
    let end = start + vec2(100.0, 0.0);
    h.drag(start, end);
    let ObjectKind::Stroke { points, style, .. } = &h.kinds()[0] else {
        panic!()
    };
    assert!(points[0].distance(start) < 1.0);
    assert!(points.last().unwrap().distance(end) < 1.0);
    assert_eq!(
        style.width, 4.0,
        "width is in board units, not screen units"
    );
    h.cmd(Key::Minus);
    assert_eq!(h.ctx.zoom_factor(), 1.0, "the UI itself must not zoom");
    assert!((h.app.doc().camera.zoom - 1.25).abs() < 1e-3);
    h.cmd(Key::Num0);
    assert!((h.app.doc().camera.zoom - 1.0).abs() < 1e-4);
}

#[test]
fn scroll_pans_and_ctrl_scroll_zooms() {
    let mut h = Harness::new("scroll");
    let wheel = |delta, modifiers| Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta,
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Unknown,
        modifiers,
    };
    let p = pos2(600.0, 400.0);
    h.frame(vec![Event::PointerMoved(p)]);
    let before = h.app.doc().camera;
    h.frame(vec![wheel(vec2(0.0, -50.0), Modifiers::NONE)]);
    for _ in 0..20 {
        h.frame(vec![]);
    }
    let after = h.app.doc().camera;
    assert_eq!(after.zoom, before.zoom);
    assert!(
        after.offset.y < before.offset.y - 10.0,
        "scroll pans: {:?} -> {:?}",
        before.offset,
        after.offset
    );

    let board = after.to_board(p);
    h.mods = Modifiers::COMMAND;
    h.frame(vec![wheel(vec2(0.0, 50.0), Modifiers::COMMAND)]);
    h.mods = Modifiers::NONE;
    for _ in 0..20 {
        h.frame(vec![]);
    }
    let cam = h.app.doc().camera;
    assert!(cam.zoom > 1.0, "ctrl+scroll zooms in: {}", cam.zoom);
    assert!(
        (cam.to_screen(board) - p).length() < 0.5,
        "zoom keeps the point under the cursor"
    );
}

#[test]
fn session_survives_restart() {
    let dir = temp_dir("restart");
    {
        let mut h = Harness::with_dir(dir.clone());
        h.drag(pos2(100.0, 100.0), pos2(200.0, 100.0));
        h.cmd(Key::T);
        h.key(Key::T);
        h.click(pos2(300.0, 300.0));
        h.frame(vec![Event::Text("unsaved text".into())]);
        // Quit while typing: the text still gets kept.
        h.app.shutdown();
        h.dir = temp_dir("restart-unused");
    }
    let h = Harness::with_dir(dir);
    assert_eq!(h.app.docs.len(), 2);
    assert_eq!(h.app.active, 1);
    assert_eq!(h.app.docs[0].objects.len(), 1);
    let ObjectKind::Text { text, extent, .. } = &h.app.docs[1].objects[0].kind else {
        panic!()
    };
    assert_eq!(text, "unsaved text");
    assert!(extent.x > 0.0, "restored text gets measured");
}

/// CPU time per frame while drawing on a full board, tessellation included. Run with
/// `cargo test --release -- --ignored frame_time --nocapture`.
#[test]
#[ignore]
fn frame_time_on_big_boards() {
    for count in [1000, 5000] {
        let per_frame = frame_time(count);
        println!("frame time with {count} objects: {per_frame:?}");
    }
}

fn frame_time(count: usize) -> std::time::Duration {
    use crate::doc::{Edit, Fill, Style};
    let mut h = Harness::new(&format!("bench{count}"));
    let style = Style {
        color: egui::Color32::BLACK,
        width: 3.0,
        fill: Fill::None,
    };
    let kinds = (0..count)
        .map(|i| {
            // Short wiggly 30-point strokes packed onto the visible area.
            let x = (i % 100) as f32 * 11.0;
            let y = (i / 100) as f32 * (650.0 / (count / 100) as f32) + 10.0;
            let points = (0..30)
                .map(|k| pos2(x + k as f32 * 0.4, y + (k as f32 * 0.7).sin() * 4.0))
                .collect();
            ObjectKind::Stroke {
                points,
                style,
                highlighter: false,
            }
        })
        .collect();
    h.app.doc_mut().apply(Edit::Add(kinds));
    // Draw while the board is full, like a pen stroke in progress.
    let p = h.screen(pos2(300.0, 300.0));
    h.frame(vec![Event::PointerMoved(p)]);
    h.frame(vec![h.button(p, true)]);
    let n = 60;
    let start = std::time::Instant::now();
    for i in 0..n {
        h.frame(vec![Event::PointerMoved(p + vec2(i as f32, 0.0))]);
    }
    start.elapsed() / n
}
