use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{
    Color32, Context, CursorIcon, Event, FontData, FontDefinitions, FontFamily, Id, Key, Modifiers,
    Pos2, Rect, Sense, Stroke, Ui, Vec2, ViewportCommand, vec2,
};

use crate::clipboard::{Clipboard, ObjectClip};
use crate::doc::{Document, Edit, ImageData, ImageId, Object, ObjectId, ObjectKind, Order, io};
use crate::render::export::{self, ExportOptions};
use crate::render::screen::{self, RenderCache, Renderer};
use crate::session::{self, Session};
use crate::tools::eraser::{self, EraserTool};
use crate::tools::pen::PenTool;
use crate::tools::select::{self, SelectTool};
use crate::tools::shape::ShapeTool;
use crate::tools::{Input, PALETTE, Settings, Tool};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Action {
    Tool(Tool),
    Color(usize),
    WidthDown,
    WidthUp,
    Undo,
    Redo,
    Delete,
    Duplicate,
    SelectAll,
    Reorder(Order),
    CopyPng,
    ExportPng,
    NewTab,
    CloseTab,
    NextTab,
    PrevTab,
    GotoTab(usize),
    Save,
    SaveAs,
    Open,
    PasteNewTab,
    ZoomReset,
    ZoomFit,
    ZoomIn,
    ZoomOut,
    ToggleGrid,
    Help,
    Escape,
    Nudge(f32, f32),
}

pub struct Shortcut {
    pub key: Key,
    pub command: bool,
    /// `None` accepts either. Used for keys like `?` that need shift on most layouts.
    pub shift: Option<bool>,
    pub alt: bool,
    pub action: Action,
    /// Shown in the help overlay. `None` hides it, for entries covered by a group line.
    pub help: Option<&'static str>,
}

impl Shortcut {
    fn new(key: Key, action: Action, help: Option<&'static str>) -> Self {
        Self {
            key,
            command: false,
            shift: Some(false),
            alt: false,
            action,
            help,
        }
    }
    fn cmd(mut self) -> Self {
        self.command = true;
        self
    }
    fn shift(mut self) -> Self {
        self.shift = Some(true);
        self
    }
    fn any_shift(mut self) -> Self {
        self.shift = None;
        self
    }
    fn alt(mut self) -> Self {
        self.alt = true;
        self
    }

    fn matches(&self, key: Key, m: Modifiers) -> bool {
        key == self.key
            && m.command == self.command
            && m.alt == self.alt
            && self.shift.is_none_or(|s| s == m.shift)
            // On macOS Ctrl is separate from Cmd; don't let Ctrl+V act as plain V.
            && (!cfg!(target_os = "macos") || !m.ctrl)
    }

    pub fn label(&self) -> String {
        let mut s = String::new();
        if self.command {
            s.push_str(if cfg!(target_os = "macos") {
                "Cmd+"
            } else {
                "Ctrl+"
            });
        }
        if self.alt {
            s.push_str(if cfg!(target_os = "macos") {
                "Option+"
            } else {
                "Alt+"
            });
        }
        if self.shift == Some(true) {
            s.push_str("Shift+");
        }
        s.push_str(match self.key {
            Key::OpenBracket => "[",
            Key::CloseBracket => "]",
            Key::Quote => "'",
            Key::Questionmark => "?",
            Key::Equals => "+",
            Key::Minus => "-",
            k => k.name(),
        });
        s
    }
}

fn shortcuts() -> Vec<Shortcut> {
    use Action as A;
    use Shortcut as S;
    let mut v = Vec::new();
    for t in Tool::ALL {
        v.push(S::new(t.key(), A::Tool(t), None));
    }
    let nums = [
        Key::Num1,
        Key::Num2,
        Key::Num3,
        Key::Num4,
        Key::Num5,
        Key::Num6,
        Key::Num7,
        Key::Num8,
        Key::Num9,
    ];
    for (i, k) in nums.iter().take(PALETTE.len()).enumerate() {
        v.push(S::new(*k, A::Color(i), None));
    }
    for (i, k) in nums.iter().enumerate() {
        v.push(S::new(*k, A::GotoTab(i), None).alt());
    }
    v.extend([
        S::new(Key::OpenBracket, A::WidthDown, Some("Thinner")),
        S::new(Key::CloseBracket, A::WidthUp, Some("Thicker")),
        S::new(Key::Z, A::Undo, Some("Undo")).cmd(),
        S::new(Key::Z, A::Redo, Some("Redo")).cmd().shift(),
        S::new(Key::Y, A::Redo, None).cmd(),
        S::new(Key::Delete, A::Delete, Some("Delete selection")),
        S::new(Key::Backspace, A::Delete, None),
        S::new(Key::D, A::Duplicate, Some("Duplicate")).cmd(),
        S::new(Key::A, A::SelectAll, Some("Select all")).cmd(),
        S::new(
            Key::CloseBracket,
            A::Reorder(Order::Forward),
            Some("Bring forward"),
        )
        .cmd(),
        S::new(
            Key::CloseBracket,
            A::Reorder(Order::Front),
            Some("Bring to front"),
        )
        .cmd()
        .shift(),
        // Shift+] arrives as } on most layouts.
        S::new(Key::CloseCurlyBracket, A::Reorder(Order::Front), None)
            .cmd()
            .any_shift(),
        S::new(
            Key::OpenBracket,
            A::Reorder(Order::Backward),
            Some("Send backward"),
        )
        .cmd(),
        S::new(
            Key::OpenBracket,
            A::Reorder(Order::Back),
            Some("Send to back"),
        )
        .cmd()
        .shift(),
        S::new(Key::OpenCurlyBracket, A::Reorder(Order::Back), None)
            .cmd()
            .any_shift(),
        S::new(Key::C, A::CopyPng, Some("Copy board (or selection) as PNG"))
            .cmd()
            .shift(),
        S::new(Key::V, A::PasteNewTab, Some("Paste image into a new tab"))
            .cmd()
            .shift(),
        S::new(Key::E, A::ExportPng, Some("Export PNG file")).cmd(),
        S::new(Key::T, A::NewTab, Some("New tab")).cmd(),
        S::new(Key::W, A::CloseTab, Some("Close tab")).cmd(),
        S::new(Key::Tab, A::NextTab, Some("Next tab")).cmd(),
        S::new(Key::Tab, A::PrevTab, Some("Previous tab"))
            .cmd()
            .shift(),
        S::new(Key::S, A::Save, Some("Save")).cmd(),
        S::new(Key::S, A::SaveAs, Some("Save as")).cmd().shift(),
        S::new(Key::O, A::Open, Some("Open")).cmd(),
        S::new(Key::Num0, A::ZoomReset, Some("Zoom to 100%")).cmd(),
        S::new(Key::Num1, A::ZoomFit, Some("Zoom to fit")).cmd(),
        S::new(Key::Equals, A::ZoomIn, Some("Zoom in"))
            .cmd()
            .any_shift(),
        S::new(Key::Plus, A::ZoomIn, None).cmd().any_shift(),
        S::new(Key::Minus, A::ZoomOut, Some("Zoom out")).cmd(),
        S::new(Key::Quote, A::ToggleGrid, Some("Toggle dot grid")).cmd(),
        S::new(Key::Questionmark, A::Help, Some("Show shortcuts")).any_shift(),
        S::new(Key::F1, A::Help, None),
        S::new(Key::Escape, A::Escape, Some("Cancel / deselect")),
        S::new(Key::ArrowLeft, A::Nudge(-1.0, 0.0), None),
        S::new(Key::ArrowRight, A::Nudge(1.0, 0.0), None),
        S::new(Key::ArrowUp, A::Nudge(0.0, -1.0), None),
        S::new(Key::ArrowDown, A::Nudge(0.0, 1.0), None),
        S::new(Key::ArrowLeft, A::Nudge(-10.0, 0.0), None).shift(),
        S::new(Key::ArrowRight, A::Nudge(10.0, 0.0), None).shift(),
        S::new(Key::ArrowUp, A::Nudge(0.0, -10.0), None).shift(),
        S::new(Key::ArrowDown, A::Nudge(0.0, 10.0), None).shift(),
    ]);
    v
}

/// Entries of the board's right-click menu.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum MenuItem {
    EditText(ObjectId),
    Cut,
    Copy,
    Paste,
    Duplicate,
    Delete,
    Reorder(Order),
    CopyPng,
    ExportPng,
    SelectAll,
}

/// Text being typed. Drawn as an egui `TextEdit` over the canvas, committed as an object
/// when it loses focus.
pub struct TextEditState {
    pub target: Option<ObjectId>,
    pub doc_id: String,
    pub pos: Pos2,
    pub text: String,
    pub size: f32,
    pub color: Color32,
    pub focus_requested: bool,
}

pub struct App {
    pub ctx: Context,
    pub docs: Vec<Document>,
    pub active: usize,
    pub tool: Tool,
    pub settings: Settings,
    pub shortcuts: Vec<Shortcut>,
    pen: PenTool,
    shape: ShapeTool,
    eraser: EraserTool,
    select: SelectTool,
    pub text: Option<TextEditState>,
    render_cache: RenderCache,
    clipboard: Clipboard,
    object_clip: Option<ObjectClip>,
    pub show_grid: bool,
    pub show_help: bool,
    toast: Option<(String, Instant)>,
    pub confirm_close: Option<usize>,
    pub renaming: Option<(usize, String)>,
    session: Session,
    canvas_rect: Rect,
    capturing: bool,
    swallow_press: bool,
    panning: bool,
    last_pointer: Pos2,
    /// A width slider drag in progress edits the selection without an undo step per frame.
    pub style_drag: bool,
    last_title: String,
    /// Board point of the last right-click. The menu's Paste goes there.
    pub menu_at: Option<Pos2>,
    startup_files: Option<Vec<PathBuf>>,
}

impl App {
    pub fn new(ctx: &Context, files: Vec<PathBuf>, session: Session, clipboard: Clipboard) -> Self {
        let ctx = ctx.clone();
        install_fonts(&ctx);
        // Ctrl+Plus/Minus/0 zoom the board, not the whole UI.
        ctx.options_mut(|o| o.zoom_with_keyboard = false);

        let restored = session.load();
        let (docs, active, grid) = match restored {
            Some(r) => (r.docs, r.active, r.grid),
            None => (vec![Document::new("Board 1")], 0, false),
        };

        let mut app = Self {
            ctx: ctx.clone(),
            docs,
            active,
            tool: Tool::Pen,
            settings: Settings::default(),
            shortcuts: shortcuts(),
            pen: PenTool::default(),
            shape: ShapeTool::default(),
            eraser: EraserTool::default(),
            select: SelectTool::default(),
            text: None,
            render_cache: RenderCache::default(),
            clipboard,
            object_clip: None,
            show_grid: grid,
            show_help: false,
            toast: None,
            confirm_close: None,
            renaming: None,
            session,
            canvas_rect: Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 700.0)),
            capturing: false,
            swallow_press: false,
            panning: false,
            last_pointer: Pos2::ZERO,
            style_drag: false,
            last_title: String::new(),
            menu_at: None,
            startup_files: None,
        };
        app.startup_files = Some(files);
        app
    }

    /// Work that needs fonts, which egui only has once the first frame starts.
    fn first_frame(&mut self) {
        let Some(files) = self.startup_files.take() else {
            return;
        };
        // Text extents aren't stored in files; measure them with our fonts.
        for doc in &mut self.docs {
            remeasure_text(&self.ctx, doc);
        }
        for f in files {
            self.open_path(&f, None);
        }
    }

    pub fn doc(&self) -> &Document {
        &self.docs[self.active]
    }

    pub fn doc_mut(&mut self) -> &mut Document {
        &mut self.docs[self.active]
    }

    pub fn toast(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), Instant::now()));
    }

    fn next_board_title(&self) -> String {
        let n = self
            .docs
            .iter()
            .filter_map(|d| d.title.strip_prefix("Board ")?.parse::<usize>().ok())
            .max()
            .unwrap_or(0);
        format!("Board {}", n + 1)
    }

    pub fn set_tool(&mut self, tool: Tool) {
        self.commit_text();
        self.cancel_ops();
        if tool != Tool::Select {
            self.doc_mut().selection.clear();
        }
        self.tool = tool;
    }

    fn cancel_ops(&mut self) {
        self.pen.cancel();
        self.shape.cancel();
        self.eraser.cancel();
        self.select.cancel();
        self.capturing = false;
    }

    pub fn switch_tab(&mut self, i: usize) {
        if i < self.docs.len() && i != self.active {
            self.commit_text();
            self.cancel_ops();
            self.active = i;
        }
    }

    pub fn new_tab(&mut self) {
        self.commit_text();
        self.cancel_ops();
        let title = self.next_board_title();
        self.docs.push(Document::new(title));
        self.active = self.docs.len() - 1;
    }

    /// Ask before closing a tab whose content would be lost.
    pub fn request_close(&mut self, i: usize) {
        if self.docs[i].is_dirty() {
            self.confirm_close = Some(i);
        } else {
            self.close_tab(i);
        }
    }

    pub fn close_tab(&mut self, i: usize) {
        self.commit_text();
        self.cancel_ops();
        self.docs.remove(i);
        if self.docs.is_empty() {
            self.docs.push(Document::new("Board 1"));
        }
        if self.active >= self.docs.len() || self.active > i {
            self.active = self.active.saturating_sub(1).min(self.docs.len() - 1);
        }
        let keep: HashSet<ImageId> = self
            .docs
            .iter()
            .flat_map(|d| d.images.keys().copied())
            .collect();
        self.render_cache.retain_images(&keep);
        // Write now so the closed tab's autosave file goes away too.
        self.session
            .save(&mut self.docs, self.active, self.show_grid);
    }

    /// Reuse the current tab if it's an untouched blank board, otherwise open a new one.
    fn blank_tab(&mut self, title: String) {
        let d = self.doc();
        if !(d.objects.is_empty() && d.path.is_none() && d.history_is_empty()) {
            self.new_tab();
        }
        self.doc_mut().title = title;
    }

    // ---- Shortcuts and clipboard events ----

    fn handle_events(&mut self, ctx: &Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        // A dialog is up: the board behind it shouldn't react to keys.
        let dialog = self.show_help || self.confirm_close.is_some();
        let events = ctx.input(|i| i.events.clone());
        for ev in events {
            match ev {
                Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    let action = self
                        .shortcuts
                        .iter()
                        .find(|s| s.matches(key, modifiers))
                        .map(|s| s.action);
                    match action {
                        Some(a @ (Action::Help | Action::Escape)) => self.run(a),
                        Some(a) if !dialog => self.run(a),
                        _ => {}
                    }
                }
                _ if dialog => {}
                Event::Copy => self.copy_objects(false),
                Event::Cut => self.copy_objects(true),
                Event::Paste(text) => self.paste(Some(text), None),
                Event::PasteImage(img) => {
                    let rgba: Vec<u8> = img
                        .pixels
                        .iter()
                        .flat_map(|c| c.to_srgba_unmultiplied())
                        .collect();
                    self.insert_rgba([img.size[0] as u32, img.size[1] as u32], rgba, false, None);
                }
                _ => {}
            }
        }
    }

    pub fn run(&mut self, action: Action) {
        use Action as A;
        match action {
            A::Tool(t) => self.set_tool(t),
            A::Color(i) => self.apply_color(PALETTE[i]),
            A::WidthDown | A::WidthUp => {
                let w = self.current_width();
                let next = if action == A::WidthUp {
                    (w * 1.5).max(w + 1.0)
                } else {
                    (w / 1.5).min(w - 1.0)
                };
                self.apply_width(next.round().clamp(1.0, 64.0), false);
            }
            A::Undo | A::Redo => {
                self.commit_text();
                self.cancel_ops();
                let doc = self.doc_mut();
                if action == A::Undo {
                    doc.undo()
                } else {
                    doc.redo()
                };
            }
            A::Delete => {
                let sel = self.doc().selection.clone();
                self.doc_mut().apply(Edit::Remove(sel));
            }
            A::Duplicate => {
                let copies: Vec<ObjectKind> = self
                    .doc()
                    .selected_objects()
                    .iter()
                    .map(|o| crate::doc::geom::translate(&o.kind, vec2(16.0, 16.0)))
                    .collect();
                let ids = self.doc_mut().apply(Edit::Add(copies));
                self.doc_mut().selection = ids.into_iter().collect();
            }
            A::SelectAll => {
                self.set_tool(Tool::Select);
                let doc = self.doc_mut();
                doc.selection = doc.objects.iter().map(|o| o.id).collect();
            }
            A::Reorder(order) => {
                let sel = self.doc().selection.clone();
                self.doc_mut().apply(Edit::Reorder(sel, order));
            }
            A::CopyPng => self.copy_png(),
            A::ExportPng => self.export_png(),
            A::NewTab => self.new_tab(),
            A::CloseTab => self.request_close(self.active),
            A::NextTab => self.switch_tab((self.active + 1) % self.docs.len()),
            A::PrevTab => self.switch_tab((self.active + self.docs.len() - 1) % self.docs.len()),
            A::GotoTab(i) => self.switch_tab(i),
            A::Save => self.save(false),
            A::SaveAs => self.save(true),
            A::Open => self.open_dialog(),
            A::PasteNewTab => match self.clipboard.get_image() {
                Some(img) => self.insert_rgba(img.size, img.rgba, true, None),
                None => self.toast("No image on the clipboard"),
            },
            A::ZoomReset => {
                let c = self.canvas_rect.center();
                let doc = self.doc_mut();
                let f = 1.0 / doc.camera.zoom;
                doc.camera.zoom_around(c, f);
            }
            A::ZoomFit => self.zoom_to_fit(1.0),
            A::ZoomIn | A::ZoomOut => {
                let c = self.canvas_rect.center();
                let f = if action == A::ZoomIn { 1.25 } else { 0.8 };
                self.doc_mut().camera.zoom_around(c, f);
            }
            A::ToggleGrid => self.show_grid = !self.show_grid,
            A::Help => self.show_help = !self.show_help,
            A::Escape => {
                if self.show_help {
                    self.show_help = false;
                } else if self.pen.is_active() || self.shape.is_active() || self.select.is_active()
                {
                    self.cancel_ops();
                } else {
                    self.doc_mut().selection.clear();
                }
            }
            A::Nudge(dx, dy) => {
                let moved: Vec<Object> = self
                    .doc()
                    .selected_objects()
                    .iter()
                    .map(|o| Object {
                        id: o.id,
                        kind: crate::doc::geom::translate(&o.kind, vec2(dx, dy)),
                    })
                    .collect();
                self.doc_mut().apply(Edit::Replace(moved));
            }
        }
    }

    pub fn zoom_to_fit(&mut self, max_zoom: f32) {
        let size = self.canvas_rect.size();
        let doc = self.doc_mut();
        match doc.content_bounds() {
            Some(b) => doc.camera.fit(b, size, max_zoom),
            None => {
                doc.camera = crate::doc::Camera {
                    origin: doc.camera.origin,
                    ..Default::default()
                }
            }
        }
    }

    // ---- Styles ----

    /// The tool whose settings the style bar edits: the selection's type when selecting.
    pub fn style_context(&self) -> Tool {
        if self.tool != Tool::Select {
            return self.tool;
        }
        // Images have no style, so skip them when picking which controls to show.
        let sel = self.doc().selected_objects();
        let first = sel
            .iter()
            .map(|o| &o.kind)
            .find(|k| !matches!(k, ObjectKind::Image { .. }));
        match first {
            Some(ObjectKind::Stroke {
                highlighter: true, ..
            }) => Tool::Highlighter,
            Some(ObjectKind::Stroke { .. }) => Tool::Pen,
            Some(ObjectKind::Text { .. }) => Tool::Text,
            Some(ObjectKind::Shape { shape, .. }) if shape.is_linear() => Tool::Line,
            Some(ObjectKind::Shape { .. }) => Tool::Rect,
            _ => Tool::Select,
        }
    }

    pub fn current_width(&self) -> f32 {
        if self.tool == Tool::Select {
            for o in self.doc().selected_objects() {
                match &o.kind {
                    ObjectKind::Stroke { style, .. } | ObjectKind::Shape { style, .. } => {
                        return style.width;
                    }
                    _ => {}
                }
            }
        }
        self.settings.width_for(self.style_context())
    }

    /// Change selected objects with `f`. With `coalesce`, consecutive calls during one
    /// slider drag share a single undo step.
    fn edit_selection(&mut self, coalesce: bool, f: impl Fn(&ObjectKind) -> Option<ObjectKind>) {
        if self.tool != Tool::Select {
            return;
        }
        let changed: Vec<Object> = self
            .doc()
            .selected_objects()
            .iter()
            .filter_map(|o| f(&o.kind).map(|kind| Object { id: o.id, kind }))
            .collect();
        if changed.is_empty() {
            return;
        }
        if coalesce {
            if !self.style_drag {
                self.doc_mut().checkpoint();
                self.style_drag = true;
            }
            self.doc_mut().apply_no_history(Edit::Replace(changed));
        } else {
            self.doc_mut().apply(Edit::Replace(changed));
        }
    }

    pub fn apply_color(&mut self, c: Color32) {
        let ctx = self.style_context();
        self.settings.set_color_for(ctx, c);
        if let Some(t) = &mut self.text {
            t.color = c;
        }
        self.edit_selection(false, |k| {
            let mut k = k.clone();
            match &mut k {
                ObjectKind::Stroke { style, .. } | ObjectKind::Shape { style, .. } => {
                    style.color = c
                }
                ObjectKind::Text { color, .. } => *color = c,
                ObjectKind::Image { .. } => return None,
            }
            Some(k)
        });
    }

    pub fn apply_width(&mut self, w: f32, coalesce: bool) {
        let ctx = self.style_context();
        self.settings.set_width_for(ctx, w);
        self.edit_selection(coalesce, |k| {
            let mut k = k.clone();
            match &mut k {
                ObjectKind::Stroke { style, .. } | ObjectKind::Shape { style, .. } => {
                    style.width = w
                }
                _ => return None,
            }
            Some(k)
        });
    }

    pub fn apply_fill(&mut self, f: crate::doc::Fill) {
        self.settings.fill = f;
        self.edit_selection(false, |k| match k {
            ObjectKind::Shape { shape, a, b, style } if !shape.is_linear() => {
                Some(ObjectKind::Shape {
                    shape: *shape,
                    a: *a,
                    b: *b,
                    style: crate::doc::Style { fill: f, ..*style },
                })
            }
            _ => None,
        });
    }

    pub fn apply_font_size(&mut self, size: f32) {
        self.settings.font_size = size;
        if let Some(t) = &mut self.text {
            t.size = size;
        }
        let ctx = self.ctx.clone();
        self.edit_selection(false, |k| match k {
            ObjectKind::Text {
                pos, text, color, ..
            } => Some(ObjectKind::Text {
                pos: *pos,
                text: text.clone(),
                size,
                color: *color,
                extent: screen::measure_text(&ctx, text, size),
            }),
            _ => None,
        });
    }

    pub fn apply_arrow_double(&mut self, double: bool) {
        self.settings.arrow_double = double;
        self.edit_selection(false, |k| match k {
            ObjectKind::Shape {
                shape: crate::doc::ShapeKind::Arrow { .. },
                a,
                b,
                style,
            } => Some(ObjectKind::Shape {
                shape: crate::doc::ShapeKind::Arrow { double },
                a: *a,
                b: *b,
                style: *style,
            }),
            _ => None,
        });
    }

    // ---- Text ----

    pub(crate) fn begin_text(&mut self, target: Option<ObjectId>, pos: Pos2) {
        let doc_id = self.doc().session_id.clone();
        let (text, size, color, pos) =
            match target.and_then(|id| self.doc().get(id)).map(|o| &o.kind) {
                Some(ObjectKind::Text {
                    text,
                    size,
                    color,
                    pos,
                    ..
                }) => (text.clone(), *size, *color, *pos),
                _ => (
                    String::new(),
                    self.settings.font_size,
                    self.settings.color,
                    pos,
                ),
            };
        self.text = Some(TextEditState {
            target,
            doc_id,
            pos,
            text,
            size,
            color,
            focus_requested: false,
        });
    }

    pub fn commit_text(&mut self) {
        let Some(t) = self.text.take() else { return };
        let Some(doc) = self.docs.iter_mut().find(|d| d.session_id == t.doc_id) else {
            return;
        };
        let empty = t.text.trim().is_empty();
        let kind = ObjectKind::Text {
            pos: t.pos,
            extent: screen::measure_text(&self.ctx, &t.text, t.size),
            text: t.text,
            size: t.size,
            color: t.color,
        };
        match (t.target, empty) {
            (Some(id), true) => {
                doc.apply(Edit::Remove(HashSet::from([id])));
            }
            (Some(id), false) => {
                if doc.get(id).is_some_and(|o| o.kind != kind) {
                    doc.apply(Edit::Replace(vec![Object { id, kind }]));
                }
            }
            (None, true) => {}
            (None, false) => {
                doc.apply(Edit::Add(vec![kind]));
            }
        }
    }

    fn text_overlay(&mut self, ctx: &Context) {
        let cam = self.doc().camera;
        let Some(t) = &mut self.text else { return };
        let zoom = cam.zoom;
        let screen_pos = cam.to_screen(t.pos);
        let size = t.size * zoom;
        let color = t.color;
        let mut lost = false;
        egui::Area::new(Id::unique("drafty-text-area"))
            .fixed_pos(screen_pos)
            .order(egui::Order::Foreground)
            .constrain(false)
            .show(ctx, |ui| {
                let mut layouter = |ui: &Ui, buf: &dyn egui::TextBuffer, _wrap: f32| {
                    screen::layout_text(ui.ctx(), buf.as_str(), size, color)
                };
                let measured = screen::measure_text(ctx, &t.text, size);
                let out = egui::TextEdit::multiline(&mut t.text)
                    .id(Id::unique("drafty-text-edit"))
                    .frame(egui::Frame::NONE)
                    .margin(egui::Margin::ZERO)
                    .desired_rows(1)
                    .desired_width(measured.x + size)
                    .layouter(&mut layouter)
                    .show(ui);
                if !t.focus_requested {
                    out.response.request_focus();
                    t.focus_requested = true;
                } else if out.response.lost_focus() {
                    lost = true;
                }
                let r = out.response.rect.expand(4.0);
                ui.painter().rect_stroke(
                    r,
                    2.0,
                    Stroke::new(1.0, select::ACCENT),
                    egui::StrokeKind::Outside,
                );
            });
        if lost {
            self.commit_text();
        }
    }

    // ---- Context menu ----

    /// Right-click on the board: select what's under the pointer, like a left click with
    /// the select tool would, so the menu acts on it.
    fn open_context_menu(&mut self, pos: Pos2, px: f32) {
        self.commit_text();
        self.cancel_ops();
        self.menu_at = Some(pos);
        let hit = self.doc().hit(pos, 4.0 * px).map(|o| o.id);
        match hit {
            Some(id) => {
                if self.tool != Tool::Select {
                    self.set_tool(Tool::Select);
                }
                if !self.doc().selection.contains(&id) {
                    self.doc_mut().selection = HashSet::from([id]);
                }
            }
            None => self.doc_mut().selection.clear(),
        }
    }

    pub fn run_menu(&mut self, item: MenuItem) {
        let at = self.menu_at;
        match item {
            MenuItem::EditText(id) => {
                let pos = at.unwrap_or_default();
                self.begin_text(Some(id), pos);
            }
            MenuItem::Cut => self.copy_objects(true),
            MenuItem::Copy => self.copy_objects(false),
            MenuItem::Paste => {
                let text = self.clipboard.get_text();
                self.paste(text, at);
            }
            MenuItem::Duplicate => self.run(Action::Duplicate),
            MenuItem::Delete => self.run(Action::Delete),
            MenuItem::Reorder(order) => self.run(Action::Reorder(order)),
            MenuItem::CopyPng => self.copy_png(),
            MenuItem::ExportPng => self.export_png(),
            MenuItem::SelectAll => self.run(Action::SelectAll),
        }
    }

    /// The label of the first shortcut bound to `action`, for menus.
    pub fn shortcut_label(&self, action: Action) -> String {
        self.shortcuts
            .iter()
            .find(|s| s.action == action)
            .map(|s| s.label())
            .unwrap_or_default()
    }

    // ---- Clipboard ----

    pub(crate) fn copy_objects(&mut self, cut: bool) {
        let doc = self.doc();
        if doc.selection.is_empty() {
            return;
        }
        let objects: Vec<ObjectKind> = doc
            .selected_objects()
            .iter()
            .map(|o| o.kind.clone())
            .collect();
        let images = objects
            .iter()
            .filter_map(|k| match k {
                ObjectKind::Image { image, .. } => Some((*image, doc.images.get(image)?.clone())),
                _ => None,
            })
            .collect();
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let marker = format!(
            "drafty-objects:{}:{}",
            std::process::id(),
            COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        self.clipboard.set_text(&marker);
        self.object_clip = Some(ObjectClip {
            objects,
            images,
            marker,
        });
        if cut {
            self.run(Action::Delete);
        }
    }

    #[cfg(test)]
    pub fn object_clip_marker(&self) -> Option<String> {
        self.object_clip.as_ref().map(|c| c.marker.clone())
    }

    /// Where pasted things go: `at` if given, else under the cursor if it's on the board,
    /// else the view center.
    fn paste_point(&self, at: Option<Pos2>) -> Pos2 {
        if let Some(p) = at {
            return p;
        }
        let cam = self.doc().camera;
        let p = if self.canvas_rect.contains(self.last_pointer) {
            self.last_pointer
        } else {
            self.canvas_rect.center()
        };
        cam.to_board(p)
    }

    /// Paste at `at` (board coordinates) or the default spot. `text` is the clipboard's
    /// text, or `None` if it couldn't be read.
    fn paste(&mut self, text: Option<String>, at: Option<Pos2>) {
        // Prefer an image when the clipboard has one, e.g. a browser image copy with a URL.
        if let Some(img) = self.clipboard.get_image() {
            self.insert_rgba(img.size, img.rgba, false, at);
            return;
        }
        // Our objects, if the marker from copying them is still on the clipboard. If the
        // clipboard can't be read at all, assume nothing replaced them.
        let ours = |c: &&ObjectClip| text.as_ref().is_none_or(|t| *t == c.marker);
        if let Some(clip) = self.object_clip.as_ref().filter(ours) {
            let Some(bounds) = crate::doc::geom::union_bounds(clip.objects.iter()) else {
                return;
            };
            let d = self.paste_point(at) - bounds.center();
            let kinds: Vec<_> = clip
                .objects
                .iter()
                .map(|k| crate::doc::geom::translate(k, d))
                .collect();
            let images: Vec<_> = clip
                .images
                .iter()
                .map(|(id, data)| (*id, data.clone()))
                .collect();
            let doc = self.doc_mut();
            for (id, data) in images {
                doc.images.entry(id).or_insert(data);
            }
            let ids = doc.apply(Edit::Add(kinds));
            self.set_tool(Tool::Select);
            self.doc_mut().selection = ids.into_iter().collect();
            return;
        }
        let Some(text) = text
            .map(|t| t.trim_end().to_owned())
            .filter(|t| !t.is_empty())
        else {
            return;
        };
        let size = self.settings.font_size;
        let extent = screen::measure_text(&self.ctx, &text, size);
        let pos = self.paste_point(at) - extent * 0.5;
        let kind = ObjectKind::Text {
            pos,
            text,
            size,
            color: self.settings.color,
            extent,
        };
        let ids = self.doc_mut().apply(Edit::Add(vec![kind]));
        self.set_tool(Tool::Select);
        self.doc_mut().selection = ids.into_iter().collect();
    }

    fn insert_rgba(&mut self, size: [u32; 2], rgba: Vec<u8>, new_tab: bool, at: Option<Pos2>) {
        let (id, data) = ImageData::from_rgba(size, rgba);
        self.insert_image(id, data, new_tab, at);
    }

    /// Add an image at its native size in screen pixels, so a HiDPI screenshot doesn't
    /// show up twice as large.
    fn insert_image(&mut self, id: ImageId, data: ImageData, new_tab: bool, at: Option<Pos2>) {
        let ppp = self.ctx.pixels_per_point();
        let size = vec2(data.size[0] as f32, data.size[1] as f32) / ppp;
        if new_tab {
            let title = self.next_board_title();
            self.blank_tab(title);
        }
        let center = if new_tab {
            Pos2::ZERO + size * 0.5
        } else {
            self.paste_point(at)
        };
        let doc = self.doc_mut();
        doc.add_image(id, data);
        // Encode pasted pixels in the background, so the next autosave doesn't stall the
        // UI on a 4K PNG. A no-op for images that came from files.
        if let Some(d) = doc.images.get(&id).cloned() {
            std::thread::spawn(move || {
                d.encoded();
            });
        }
        let rect = Rect::from_center_size(center, size);
        let ids = doc.apply(Edit::Add(vec![ObjectKind::Image { image: id, rect }]));
        if new_tab {
            self.zoom_to_fit(1.0);
        }
        self.set_tool(Tool::Select);
        self.doc_mut().selection = ids.into_iter().collect();
    }

    fn render_export(&self) -> Option<tiny_skia::Pixmap> {
        let doc = self.doc();
        let only = (!doc.selection.is_empty()).then_some(&doc.selection);
        export::render(&self.ctx, doc, only, ExportOptions::default())
    }

    pub(crate) fn copy_png(&mut self) {
        self.commit_text();
        let Some(pixmap) = self.render_export() else {
            self.toast("Nothing to copy");
            return;
        };
        let what = if self.doc().selection.is_empty() {
            "board"
        } else {
            "selection"
        };
        let rgba = export::to_rgba(&pixmap);
        let size = [pixmap.width(), pixmap.height()];
        match self.clipboard.set_image(size, rgba, || {
            export::encode_png(&pixmap).unwrap_or_default()
        }) {
            Ok(()) => self.toast(format!("Copied {what} as PNG")),
            Err(e) => self.toast(format!("Copy failed: {e}")),
        }
    }

    pub(crate) fn export_png(&mut self) {
        self.commit_text();
        let Some(pixmap) = self.render_export() else {
            self.toast("Nothing to export");
            return;
        };
        let Some(path) = rfd::FileDialog::new()
            .add_filter("PNG image", &["png"])
            .set_file_name(format!("{}.png", self.doc().title))
            .save_file()
        else {
            return;
        };
        let path = with_extension(path, "png");
        let result = export::encode_png(&pixmap)
            .and_then(|png| io::write_atomic(&path, &png).map_err(|e| e.to_string()));
        match result {
            Ok(()) => self.toast(format!("Exported {}", file_name(&path))),
            Err(e) => self.toast(format!("Export failed: {e}")),
        }
    }

    // ---- Files ----

    fn save(&mut self, save_as: bool) {
        self.commit_text();
        let path = match (&self.doc().path, save_as) {
            (Some(p), false) => p.clone(),
            _ => {
                let Some(p) = rfd::FileDialog::new()
                    .add_filter("Drafty board", &["drafty"])
                    .set_file_name(format!("{}.drafty", self.doc().title))
                    .save_file()
                else {
                    return;
                };
                with_extension(p, "drafty")
            }
        };
        let doc = self.doc_mut();
        doc.title = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        match io::save(doc, &path) {
            Ok(()) => {
                doc.path = Some(path.clone());
                doc.file_rev = doc.rev;
                self.toast(format!("Saved {}", file_name(&path)));
            }
            Err(e) => self.toast(format!("Save failed: {e}")),
        }
    }

    fn open_dialog(&mut self) {
        let Some(paths) = rfd::FileDialog::new()
            .add_filter(
                "Drafty boards and images",
                &["drafty", "png", "jpg", "jpeg"],
            )
            .pick_files()
        else {
            return;
        };
        for p in paths {
            self.open_path(&p, None);
        }
    }

    /// Open a board in a new tab. Images go into a new tab, or onto the current board at
    /// `drop_at` when dropped onto it.
    pub fn open_path(&mut self, path: &Path, drop_at: Option<Pos2>) {
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if ext == "drafty" {
            if let Some(i) = self
                .docs
                .iter()
                .position(|d| d.path.as_deref() == Some(path))
            {
                self.switch_tab(i);
                return;
            }
            match io::load(path) {
                Ok(mut doc) => {
                    remeasure_text(&self.ctx, &mut doc);
                    doc.title = path
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let d = self.doc();
                    if d.objects.is_empty() && d.path.is_none() && d.history_is_empty() {
                        self.docs[self.active] = doc;
                    } else {
                        self.commit_text();
                        self.docs.push(doc);
                        self.active = self.docs.len() - 1;
                    }
                }
                Err(e) => self.toast(format!("Can't open {}: {e}", file_name(path))),
            }
            return;
        }
        let loaded = std::fs::read(path)
            .map_err(|e| e.to_string())
            .and_then(ImageData::from_encoded);
        match loaded {
            Ok((id, data)) => {
                if drop_at.is_some() {
                    self.insert_image(id, data, false, drop_at);
                } else {
                    let title = path
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    self.insert_image(id, data, true, None);
                    self.doc_mut().title = title;
                }
            }
            Err(e) => self.toast(format!("Can't open {}: {e}", file_name(path))),
        }
    }

    fn handle_dropped_files(&mut self, ctx: &Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        if dropped.is_empty() {
            return;
        }
        let at = ctx
            .input(|i| i.pointer.latest_pos())
            .unwrap_or(self.canvas_rect.center());
        let board = self.doc().camera.to_board(at);
        for (n, f) in dropped.iter().enumerate() {
            // Fan out several dropped images so they don't stack exactly.
            let offset = vec2(24.0, 24.0) * n as f32;
            self.open_path(f.path(), Some(board + offset));
        }
    }

    // ---- Canvas ----

    fn canvas(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        self.canvas_rect = rect;
        let typing = ctx.egui_wants_keyboard_input();
        let (
            pointer,
            pressed,
            down,
            released,
            double,
            shift,
            scroll,
            zoom,
            middle_pressed,
            middle_down,
            delta,
            space,
            secondary,
        ) = ctx.input(|i| {
            (
                i.pointer.latest_pos(),
                i.pointer.primary_pressed(),
                i.pointer.primary_down(),
                i.pointer.primary_released(),
                i.pointer
                    .button_double_clicked(egui::PointerButton::Primary),
                i.modifiers.shift,
                i.smooth_scroll_delta,
                i.zoom_delta(),
                i.pointer.button_pressed(egui::PointerButton::Middle),
                i.pointer.middle_down(),
                i.pointer.delta(),
                i.key_down(Key::Space) && !typing,
                i.pointer.secondary_pressed(),
            )
        });
        if let Some(p) = pointer {
            self.last_pointer = p;
        }
        let hovered = response.hovered();
        self.doc_mut().camera.origin = rect.min;

        // Pan and zoom.
        if hovered && !self.capturing {
            let cam = &mut self.docs[self.active].camera;
            if zoom != 1.0 {
                cam.zoom_around(pointer.unwrap_or(rect.center()), zoom);
            }
            cam.offset += scroll;
        }
        if hovered && !self.capturing && (middle_pressed || (space && pressed)) {
            self.panning = true;
        }
        if self.panning {
            self.docs[self.active].camera.offset += delta;
            if !middle_down && !down {
                self.panning = false;
            }
        }

        // A click on the board while typing only finishes the text. A click that closes a
        // menu or color picker only closes it.
        let popup_open = egui::Popup::is_any_open(&ctx);
        if pressed && hovered && (self.text.is_some() || popup_open) && !self.panning {
            self.commit_text();
            self.swallow_press = true;
        }

        let cam = self.doc().camera;
        let px = 1.0 / cam.zoom;
        let pos = cam.to_board(pointer.unwrap_or(self.last_pointer));
        if secondary && hovered && !self.capturing && !self.panning {
            self.open_context_menu(pos, px);
        }
        if !self.panning && !self.swallow_press {
            let start = pressed && hovered;
            if start {
                self.capturing = true;
            }
            if self.capturing || hovered {
                let base = Input {
                    pos,
                    pressed: false,
                    down,
                    released: false,
                    double_clicked: false,
                    shift,
                    px,
                };
                // Press and release can land in the same frame on a quick click. Feed them
                // as two steps so tools see both.
                if start {
                    self.tool_input(Input {
                        pressed: true,
                        double_clicked: double,
                        ..base
                    });
                }
                let released_now = released && self.capturing;
                self.tool_input(Input {
                    released: released_now,
                    ..base
                });
                if released_now {
                    self.capturing = false;
                    self.style_drag = false;
                }
            }
        }
        if self.swallow_press && !down {
            self.swallow_press = false;
        }

        if hovered {
            let icon = if self.panning {
                CursorIcon::Grabbing
            } else if space {
                CursorIcon::Grab
            } else {
                match self.tool {
                    Tool::Select => self.select.cursor(
                        self.doc(),
                        &Input {
                            pos,
                            pressed: false,
                            down,
                            released: false,
                            double_clicked: false,
                            shift,
                            px,
                        },
                    ),
                    Tool::Text => CursorIcon::Text,
                    _ => CursorIcon::Crosshair,
                }
            };
            ctx.set_cursor_icon(icon);
        }

        self.paint_canvas(&ctx, ui, rect, pointer.filter(|_| hovered));
        response.context_menu(|ui| self.context_menu_ui(ui));
    }

    fn tool_input(&mut self, input: Input) {
        let doc = &mut self.docs[self.active];
        let edit = match self.tool {
            Tool::Select => {
                if let select::Outcome::EditText(id) = self.select.input(doc, &input) {
                    self.begin_text(Some(id), input.pos);
                }
                None
            }
            Tool::Pen => self.pen.input(&input, &self.settings, false),
            Tool::Highlighter => self.pen.input(&input, &self.settings, true),
            Tool::Eraser => self.eraser.input(&input, doc),
            Tool::Text => {
                if input.pressed {
                    let hit = doc
                        .hit(input.pos, 4.0 * input.px)
                        .filter(|o| matches!(o.kind, ObjectKind::Text { .. }))
                        .map(|o| o.id);
                    // Put the click roughly in the middle of the first line.
                    let pos = input.pos - vec2(0.0, self.settings.font_size * 0.6);
                    self.begin_text(hit, pos);
                }
                None
            }
            t => {
                let shape = t
                    .shape(self.settings.arrow_double)
                    .expect("remaining tools are shapes");
                self.shape.input(&input, shape, &self.settings)
            }
        };
        if let Some(edit) = edit {
            self.docs[self.active].apply(edit);
        }
    }

    fn paint_canvas(&mut self, ctx: &Context, ui: &Ui, rect: Rect, hover: Option<Pos2>) {
        let painter = ui.painter_at(rect);
        let doc = &self.docs[self.active];
        painter.rect_filled(rect, 0.0, doc.background);
        if self.show_grid {
            screen::draw_grid(&painter, &doc.camera, Color32::from_gray(200));
        }

        let mut hidden = HashSet::new();
        if let Some(t) = &self.text
            && let Some(id) = t.target
        {
            hidden.insert(id);
        }
        let dimmed = if self.tool == Tool::Eraser {
            self.eraser.hits.clone()
        } else {
            HashSet::new()
        };

        let mut r = Renderer {
            ctx,
            painter: &painter,
            cam: doc.camera,
            cache: &mut self.render_cache,
        };
        r.draw_document(doc, &hidden, &dimmed);

        let preview = match self.tool {
            Tool::Pen => self.pen.preview(&self.settings, false),
            Tool::Highlighter => self.pen.preview(&self.settings, true),
            t => t
                .shape(self.settings.arrow_double)
                .and_then(|s| self.shape.preview(s, &self.settings)),
        };
        if let Some(kind) = preview {
            r.draw(doc, &kind, None, 1.0);
        }
        self.render_cache.end_frame();

        if self.tool == Tool::Select {
            self.select.draw(&painter, doc, &doc.camera);
        }
        if self.tool == Tool::Eraser
            && let Some(p) = hover
        {
            painter.circle_stroke(p, eraser::RADIUS, Stroke::new(1.0, Color32::from_gray(90)));
        }
        self.paint_toast(ctx, &painter, rect);
    }

    fn paint_toast(&mut self, ctx: &Context, painter: &egui::Painter, rect: Rect) {
        const SHOW: Duration = Duration::from_millis(1800);
        let Some((msg, at)) = &self.toast else { return };
        let age = at.elapsed();
        if age > SHOW {
            self.toast = None;
            return;
        }
        let galley = painter.layout_no_wrap(
            msg.clone(),
            egui::FontId::proportional(14.0),
            Color32::WHITE,
        );
        let size = galley.size() + vec2(24.0, 14.0);
        let r = Rect::from_center_size(rect.center_bottom() - vec2(0.0, 40.0), size);
        painter.rect_filled(r, 8.0, Color32::from_black_alpha(200));
        painter.galley(r.center() - galley.size() * 0.5, galley, Color32::WHITE);
        ctx.request_repaint_after(SHOW - age);
    }

    fn update_title(&mut self, ctx: &Context) {
        let doc = self.doc();
        let title = format!(
            "{}{} - Drafty",
            doc.title,
            if doc.is_dirty() && doc.path.is_some() {
                " *"
            } else {
                ""
            }
        );
        if title != self.last_title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.last_title = title;
        }
    }

    fn autosave(&mut self, ctx: &Context) {
        if !self.session.needs_save(&self.docs) {
            return;
        }
        let since = self.session.last_save.elapsed();
        if since >= session::INTERVAL && !self.capturing {
            self.session
                .save(&mut self.docs, self.active, self.show_grid);
        } else {
            // Wake up for the save even if the user stops touching the app.
            ctx.request_repaint_after(
                session::INTERVAL
                    .saturating_sub(since)
                    .max(Duration::from_secs(1)),
            );
        }
    }
}

impl App {
    /// One frame. Separate from `eframe::App::ui` so tests can drive it without a window.
    pub fn show(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        self.first_frame();
        self.handle_dropped_files(&ctx);
        self.handle_events(&ctx);

        let tab_frame = crate::ui::tabs::bar_frame(ui.visuals());
        egui::Panel::top("tabs")
            .frame(tab_frame)
            .show_separator_line(false)
            .show(ui, |ui| self.tab_bar(ui));
        egui::Panel::top("toolbar").show(ui, |ui| self.toolbar(ui));
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| self.canvas(ui));

        self.text_overlay(&ctx);
        self.close_dialog(&ctx);
        if self.show_help {
            self.help_overlay(&ctx);
        }
        self.update_title(&ctx);
        self.autosave(&ctx);
    }

    /// Finish typing and write the session. Called on quit.
    pub fn shutdown(&mut self) {
        self.commit_text();
        self.session
            .save(&mut self.docs, self.active, self.show_grid);
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.shutdown();
    }
}

impl Document {
    pub fn history_is_empty(&self) -> bool {
        self.rev == 0
    }
}

pub fn install_fonts(ctx: &Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "inter".into(),
        Arc::new(FontData::from_static(crate::INTER)),
    );
    fonts
        .families
        .insert(FontFamily::Proportional, vec!["inter".into()]);
    fonts
        .families
        .insert(FontFamily::Monospace, vec!["inter".into()]);
    ctx.set_fonts(fonts);
}

/// Fill in text extents, which files don't store.
fn remeasure_text(ctx: &Context, doc: &mut Document) {
    for slot in &mut doc.objects {
        if let ObjectKind::Text {
            text, size, extent, ..
        } = &slot.kind
            && *extent == Vec2::ZERO
        {
            let e = screen::measure_text(ctx, text, *size);
            let mut o = (**slot).clone();
            if let ObjectKind::Text { extent, .. } = &mut o.kind {
                *extent = e;
            }
            *slot = Arc::new(o);
        }
    }
}

fn with_extension(p: PathBuf, ext: &str) -> PathBuf {
    if p.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext)) {
        p
    } else {
        p.with_extension(ext)
    }
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}
