# Drafty architecture

How the code is laid out and why. Scope and milestones are in `PLAN.md`.

## Dependencies

Keep this list short. Every crate added should replace code I'd otherwise write and test.

| Crate | Use | Notes |
|---|---|---|
| `eframe` / `egui` | Window, input, GPU drawing, widgets | `default-features = false`, enable `glow`, `wayland`, `x11`, `default_fonts` off since we bundle our own font |
| `arboard` | Clipboard text and images | Features `image-data`, `wayland-data-control` |
| `image` | PNG/JPEG decode and PNG encode | `default-features = false`, features `png`, `jpeg` only |
| `tiny-skia` | CPU rasterizer for PNG export and highlighter textures | |
| `ab_glyph` | Glyph outlines for text in export | Same font file egui uses |
| `rfd` | Open/save dialogs | Use the `xdg-portal` feature on Linux so it doesn't pull in GTK |
| `serde`, `serde_json` | `.drafty` and session files | |
| `base64` | Images inside `.drafty` JSON | |
| `directories` | Platform data dir for the session | |

No async runtime, no logging framework beyond `eprintln!` until there's a reason.

## Module layout

```
src/
  main.rs            CLI args, eframe::run_native, font setup
  app.rs             App: tabs, active tool, current style, top-level update()
  doc/
    mod.rs           Document, Object, Style, ObjectId
    geom.rs          Bounds, hit testing, arrowhead/triangle points, smoothing
    history.rs       Undo/redo
    io.rs            .drafty read/write, versioning
  tools/
    mod.rs           Tool enum and the ToolState trait
    pen.rs           Pen and highlighter (same code, different style)
    shape.rs         Rect, ellipse, triangle, line, arrow
    text.rs          Text placement and editing
    select.rs        Selection, move, resize, box select
    eraser.rs
  render/
    screen.rs        Draws a Document with egui::Painter
    export.rs        Draws a Document into a tiny_skia::Pixmap
  ui/
    toolbar.rs       Tool buttons
    stylebar.rs      Colors, widths, fill, font size
    tabs.rs          Tab strip
    help.rs          Shortcut overlay
  clipboard.rs       Image/object clipboard, Wayland fallback
  session.rs         Autosave and restore
assets/
  Inter-Regular.ttf
  icon.png, icon.icns, drafty.desktop
```

`doc/` has no egui dependency beyond `egui::Pos2`/`Rect`/`Color32` as plain math types. That keeps it unit-testable without a window.

## Data model

```rust
pub struct ObjectId(u64);

pub struct Document {
    pub title: String,
    pub path: Option<PathBuf>,
    pub objects: Vec<Arc<Object>>,        // back to front
    pub images: HashMap<ImageId, Arc<ImageData>>,
    pub camera: Camera,
    pub history: History,
    pub selection: HashSet<ObjectId>,
    pub dirty: bool,
    next_id: u64,
}

pub struct Object {
    pub id: ObjectId,
    pub kind: ObjectKind,
}

pub enum ObjectKind {
    Stroke { points: Vec<Pos2>, style: Style, highlighter: bool },
    Shape  { shape: ShapeKind, a: Pos2, b: Pos2, style: Style },
    Text   { pos: Pos2, text: String, size: f32, color: Color32 },
    Image  { image: ImageId, rect: Rect },
}

pub enum ShapeKind { Rect, Ellipse, Triangle, Line, Arrow { double: bool } }

pub struct Style {
    pub color: Color32,
    pub width: f32,
    pub fill: Fill,                       // None | Solid | Translucent
}

pub struct ImageData {
    pub png_bytes: Vec<u8>,               // original encoded bytes, for saving
    pub size: [u32; 2],
    pub texture: OnceCell<TextureHandle>, // uploaded lazily on first draw
}
```

All positions are board coordinates in logical pixels. Only the camera knows about the screen.

Shapes store the two drag points `a` and `b` instead of a normalized rect. Lines and arrows need the direction, and rects/ellipses normalize when drawn. Resizing a shape moves `a` and `b`.

`ImageId` is a hash of the encoded bytes, so pasting the same screenshot twice stores it once.

## Camera

```rust
pub struct Camera { pub offset: Vec2, pub zoom: f32 }   // screen = board * zoom + offset
```

Zoom clamps to 10%..800%. Zooming keeps the board point under the cursor fixed. Stroke widths scale with zoom, since they belong to the board, not the screen. Selection handles and hit tolerance stay a constant size on screen.

## Rendering

### On screen

`render/screen.rs` walks `objects` back to front, skips anything whose bounds miss the visible rect, and emits egui shapes:

- Strokes become `PathShape` polylines with round joins. A single-point stroke becomes a filled circle so a click leaves a dot.
- Highlighter strokes start as `PathShape` with alpha. If self-overlap blotches look bad, switch to a cached tiny-skia texture per stroke, re-rasterized when the zoom level changes by more than 2x (see Risks in `PLAN.md`).
- Rect, ellipse and triangle become `RectShape`/`EllipseShape`/convex `PathShape` with fill then stroke.
- Arrows are a line plus a filled triangle head from `geom::arrowhead(a, b, width)`. Head size grows with stroke width so thick arrows don't get tiny heads.
- Text uses an egui `Galley` laid out with the bundled font at `size * zoom`.
- Images draw their texture into `rect`.

Then the active tool draws its preview (the shape being dragged, the stroke in progress) and the select tool draws handles.

egui re-tessellates every frame. With culling that is fine up to thousands of objects. If profiling shows otherwise, cache tessellated meshes per object keyed on `(ObjectId, zoom)`. Don't build that before measuring.

### Export

`render/export.rs` takes a `Document`, an optional selection, a scale and a background, and returns a `tiny_skia::Pixmap`:

1. Union the bounds of the objects to export and add 16 px padding.
2. Create a pixmap of `bounds.size() * scale`, fill the background.
3. Draw each object with tiny-skia paths using the same `geom` helpers as the screen renderer.
4. For text, get glyph positions from egui's `Galley` (the `egui::Context` is available at export time) and rasterize each glyph outline with `ab_glyph`. Layout happens once, in egui, so line breaks and spacing match the screen exactly.
5. Images get decoded once to RGBA and drawn with a scaled pattern.

The resulting `Pixmap` goes to `image` for PNG encoding, or to `arboard` as raw RGBA for the clipboard.

## Tools

```rust
pub trait ToolState {
    fn on_input(&mut self, ctx: &mut ToolCtx, input: &CanvasInput) -> Option<Edit>;
    fn draw_preview(&self, painter: &Painter, camera: &Camera);
    fn cursor(&self) -> CursorIcon;
}
```

`CanvasInput` is pointer position in board coordinates, press/drag/release, modifiers and scroll. The canvas widget converts egui input into it, after pan/zoom have taken their share. Tools never see screen coordinates.

Tools return an `Edit` when an action finishes. They don't mutate the document directly. The document applies the edit and records history. That gives one place where changes happen and one place that sets `dirty`.

The text tool is the exception that needs live state. While editing, it owns an egui `TextEdit` positioned over the canvas and commits an `Edit` when editing ends.

### Freehand capture

- Drop new points closer than `0.5 / zoom` board px to the last one.
- On release, run Ramer-Douglas-Peucker with tolerance `0.3 / zoom`, then Chaikin smoothing with 2 iterations.
- While drawing, show the raw points. Smoothing on release avoids lag while the pen moves.

### Hit testing

`geom::hit(object, point, tolerance) -> bool`:

- Stroke and line/arrow: distance to any segment ≤ `width / 2 + tolerance`.
- Rect/ellipse/triangle: inside if filled, else near the outline.
- Text and image: inside the bounds.

Hit testing goes front to back so clicks pick the top object. The eraser uses the same test along the segment between the last and current pointer positions so fast drags don't skip objects.

## Undo and history

Snapshot-based:

```rust
pub struct History {
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
}
struct Snapshot { objects: Vec<Arc<Object>> }
```

Before applying an `Edit`, push the current `objects` vector onto `undo`. Cloning `Vec<Arc<Object>>` copies pointers only, so a snapshot of a board with 5,000 objects costs 40 KB. Changed objects get replaced with a new `Arc`, unchanged ones are shared across every snapshot. Cap at 200 entries.

This is simpler than a command pattern with inverse operations and harder to get wrong. Images live outside the snapshots in `Document::images` and are never removed during a session, so undoing a delete brings the image back.

Selection is not part of history. After undo/redo, drop selected ids that no longer exist.

## File format

`.drafty` is UTF-8 JSON:

```json
{
  "format": "drafty",
  "version": 1,
  "title": "Board 1",
  "background": "#ffffff",
  "objects": [
    { "type": "stroke", "points": [[10,20],[12,24]], "color": "#e53935", "width": 4, "highlighter": false },
    { "type": "shape", "shape": "arrow", "a": [0,0], "b": [100,40], "color": "#000000", "width": 3, "fill": "none", "double": false },
    { "type": "text", "pos": [50,60], "text": "this one", "size": 24, "color": "#000000" },
    { "type": "image", "image": "9f2c...", "rect": [0,0,1920,1080] }
  ],
  "images": { "9f2c...": "<base64 png or jpeg>" }
}
```

- JSON keeps files readable and diffable, and base64 adds a third to image size. A board with a couple of screenshots ends up a few MB. Acceptable. If it isn't, switch to a zip container in `version: 2`.
- Unknown fields are ignored on load. A higher `version` than the app knows shows an error instead of guessing.
- Points round to 2 decimals on save.
- Write to `file.drafty.tmp`, then rename, so a crash mid-save never truncates the file.

## Session

`session.rs` stores state in the platform data dir from `directories`: `~/.local/share/drafty/` on Linux and `~/Library/Application Support/drafty/` on macOS.

```
session.json         tab order, active tab, each tab's path if saved
tabs/<uuid>.drafty   content of each open tab
```

- Autosave runs on the UI thread every 30 s if any tab is dirty, and on exit. Serializing a board takes milliseconds. If big boards make that noticeable, move the write onto a thread with a cloned `Vec<Arc<Object>>`.
- Restore runs on launch. CLI file arguments open as extra tabs after the restored ones.
- Closing a tab deletes its session file.

## Clipboard

`clipboard.rs` wraps `arboard`:

- `read_image() -> Option<RgbaImage>`
- `write_image(&Pixmap)`
- Object copy/paste goes in a private in-process buffer as `Vec<Object>` plus referenced images. Pasting into another tab copies the images over. Cross-process object paste isn't needed.

On Wayland, if `arboard` fails, fall back to `wl-paste --type image/png` and `wl-copy --type image/png` when they're on `PATH`.

Arboard on Linux needs the app to keep the clipboard owner alive after a copy. Keep one `arboard::Clipboard` in `App` for the whole run instead of creating one per copy.

## Keyboard handling

All shortcuts go through one table in `app.rs` mapping `(Modifiers, Key)` to an `Action`, using egui's `Modifiers::command` so `Ctrl` on Linux becomes `Cmd` on macOS. The help overlay renders from the same table, so it can't drift out of date.

Single-letter tool keys are ignored while a text box has focus.

## Build and install

### Linux

```sh
cargo build --release
make install          # copies binary, .desktop and icon into ~/.local
```

`make install` puts:
- `target/release/drafty` → `~/.local/bin/drafty`
- `assets/drafty.desktop` → `~/.local/share/applications/`
- `assets/icon.png` → `~/.local/share/icons/hicolor/256x256/apps/drafty.png`

`PREFIX=/usr/local sudo make install` for a system install. A `PKGBUILD` for the AUR fits in a few lines once there's a tagged release.

Build deps on Linux are only what winit and glow need: a C toolchain, `libxkbcommon`, `wayland`, and X11 libs. On Arch those are already present on any desktop install.

### macOS

```sh
cargo build --release
./scripts/bundle-macos.sh      # creates target/Drafty.app
cp -r target/Drafty.app /Applications/
```

`bundle-macos.sh` builds the `.app` layout by hand: `Contents/MacOS/drafty`, `Contents/Info.plist`, `Contents/Resources/icon.icns`. Then it runs `codesign --force --sign - Drafty.app` for an ad-hoc signature, which Apple Silicon requires for the binary to run. No notarization. Apps built locally aren't quarantined, so Gatekeeper doesn't complain.

Twenty lines of shell beat adding `cargo-bundle` as a required tool. Universal binaries via `lipo` are an option in the script for sharing a build with Intel Mac users.

### Release profile

```toml
[profile.release]
opt-level = 3
lto = "thin"
codegen-units = 1
strip = true
panic = "abort"
```

### CI

One GitHub Actions workflow with a Linux and a macOS job. Each runs `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`, and a release build. It uploads the Linux binary and the zipped `.app` as artifacts. Optional, set up in M7.

## Testing

- **Unit tests** in `doc/`: hit testing per object type, bounds, arrowhead geometry, smoothing keeps endpoints, history undo/redo/cap, snapshot sharing.
- **Round-trip tests** in `doc/io.rs`: save then load equals the original, version mismatch errors, unknown fields ignored.
- **Golden image tests** for `render/export.rs`: render small fixture boards and compare against PNGs in `tests/golden/` with a small per-pixel tolerance. tiny-skia is deterministic, so these stay stable across machines. `UPDATE_GOLDEN=1 cargo test` rewrites them.
- **Manual checklist** per release, on Wayland, X11 and macOS: paste screenshot, annotate with each tool, copy out to a chat app, save, quit, relaunch, check session restore, type å/ä/ö in a text box.

No UI automation. The UI is small and the manual checklist covers it.
