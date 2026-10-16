# Drafty architecture

How the code is laid out and why. Scope, milestones and measured results are in `PLAN.md`.

## Dependencies

Keep this list short. Every crate added should replace code I'd otherwise write and test.

| Crate | Use | Notes |
|---|---|---|
| `eframe` / `egui` 0.37 | Window, input, GPU drawing, widgets, text layout | `default-features = false`; features `glow`, `wayland`, `x11`, `clipboard`. No default fonts: Inter is bundled. |
| `arboard` | Clipboard images | Features `image-data`, `wayland-data-control` |
| `image` | PNG/JPEG decode and PNG encode | Features `png`, `jpeg` only |
| `tiny-skia` | CPU rasterizer for PNG export and highlighter textures | |
| `ab_glyph` | Glyph outlines for text in export | Reads the same `Inter-Regular.ttf` egui uses |
| `rfd` | Open/save dialogs | `xdg-portal` on Linux, so no GTK |
| `serde`, `serde_json` | `.drafty` and session files | |
| `base64` | Images inside `.drafty` JSON | |
| `directories` | Platform data dir for the session | |

No async runtime and no logging framework. Errors the user should see become toasts. The rest goes to stderr.

## Module layout

```
src/
  main.rs            CLI args, window options, eframe::run_native
  app.rs             App: tabs, tools, shortcuts, clipboard, files, canvas input and painting
  app_tests.rs       Drives App through egui with synthetic input
  clipboard.rs       arboard wrapper with wl-copy/wl-paste fallback, object clipboard
  session.rs         Autosave and restore of open tabs
  doc/
    mod.rs           Document, Object, ObjectKind, Style, ImageData, Camera, Edit
    geom.rs          Bounds, hit testing, arrowheads, triangles, smoothing, transforms
    history.rs       Snapshot undo/redo
    io.rs            .drafty read/write
  tools/
    mod.rs           Tool enum, Settings (style for new objects), Input
    pen.rs           Pen and highlighter
    shape.rs         Rect, ellipse, triangle, line, arrow
    select.rs        Select, move, resize, endpoints, box select
    eraser.rs        Object eraser
  render/
    mod.rs           tiny-skia helpers shared by both renderers
    screen.rs        Draws a Document with egui, plus the texture and mesh cache
    export.rs        Draws a Document into a tiny_skia::Pixmap
  ui/
    toolbar.rs       Tool buttons and style bar, painted icons
    tabs.rs          Tab strip, painted by hand
    menu.rs          Right-click menu
    help.rs          Shortcut overlay and close prompt
assets/              Inter font and license, icons, .desktop and MIME files
scripts/             bundle-macos.sh
tests/golden/        Expected export PNGs
```

`doc/` uses egui only for its math and color types, so it's testable without a window. Text editing has no module of its own: it's a few functions in `app.rs` around an egui `TextEdit`.

## Data model

```rust
pub struct Document {
    pub title: String,
    pub path: Option<PathBuf>,
    pub session_id: String,                      // names this tab's autosave file
    pub objects: Vec<Arc<Object>>,               // back to front
    pub images: HashMap<ImageId, Arc<ImageData>>,
    pub camera: Camera,
    pub history: History,
    pub selection: HashSet<ObjectId>,
    pub background: Color32,
    pub rev: u64,                                // bumped on every change
    pub file_rev: u64,                           // rev at last save to `path`
    pub session_rev: u64,                        // rev at last autosave
}

pub enum ObjectKind {
    Stroke { points: Vec<Pos2>, style: Style, highlighter: bool },
    Shape  { shape: ShapeKind, a: Pos2, b: Pos2, style: Style },
    Text   { pos: Pos2, text: String, size: f32, color: Color32, extent: Vec2 },
    Image  { image: ImageId, rect: Rect },
}
```

All positions are board coordinates in logical pixels. Only the camera knows about the screen.

Shapes store the two drag points `a` and `b` instead of a normalized rect. Lines and arrows need the direction, and triangles use it too: dragging upward draws one pointing down.

`Text::extent` is the laid-out size at zoom 1. It's measured with egui's fonts whenever text or size changes, and re-measured after loading, since files don't store it.

Revision counters replace a `dirty` flag. `is_dirty()` is `rev != file_rev`, plus "never saved and not empty". Autosave writes tabs whose `rev != session_rev`. The render cache keys on `rev`.

### Images

`ImageData` holds an image's encoded bytes and, only while needed, its decoded RGBA:

- Opened from a file: the header gives the size, the id is a hash of the bytes, nothing gets decoded until the image is first drawn.
- Pasted: starts as RGBA, the id is a hash of the pixels. A background thread encodes it to PNG right away, so the next autosave doesn't stall the UI.
- Once encoded and uploaded to the GPU, the RGBA gets dropped. Export decodes again when it needs pixels.

A 4K screenshot is 33 MB as RGBA, so this matters with a few tabs open.

## Camera

```rust
pub struct Camera { pub offset: Vec2, pub zoom: f32, pub origin: Pos2 }
// screen = origin + board * zoom + offset
```

`origin` is the canvas' top-left on screen, set every frame. Zoom clamps to 10%..800% and keeps the board point under the cursor fixed. Stroke widths scale with zoom; selection handles and hit tolerances don't.

## Rendering

### On screen

`render/screen.rs` turns objects into egui shapes:

- Pen strokes are polylines with a filled circle at each end, since egui only draws butt caps.
- Highlighter strokes are rasterized opaque with tiny-skia into a texture, then drawn at 40% alpha. tiny-skia fills a stroke's outline once, so a self-crossing stroke doesn't get darker where it crosses. Textures are cached per object at power-of-two scales and re-rasterized when zoom crosses a step.
- Rect, ellipse and triangle draw fill then stroke. Arrows are a line plus filled heads from `geom::arrow_parts`; head size grows with stroke width.
- Text is an egui `Galley` laid out at `size * zoom`.
- Images draw a mipmapped texture into `rect`.

egui tessellates every shape every frame, which was most of the frame time on big boards. So the board is cached as a list of runs: consecutive strokes and shapes tessellated into one `Mesh`, with images, text and highlighter strokes drawn individually between them to keep z-order. The cache key is the document id and `rev`, the camera, the clip rect, pixels per point, and the hidden and dimmed object sets. Drawing a new stroke changes none of those, so only the stroke in progress gets tessellated. Panning and zooming rebuild the cache every frame, which costs the same as before caching.

After the board, the active tool draws its preview and the select tool draws handles.

### Export

`render/export.rs` takes a `Document`, an optional set of objects, and a scale, and returns a `tiny_skia::Pixmap`:

1. Union the bounds of the exported objects and add 16 px padding.
2. Scale down if the result would pass 16384 px a side or 100 million pixels.
3. Draw each object with tiny-skia paths, using the same `geom` helpers as the screen.
4. For text, lay out with egui's `Galley` so line breaks and spacing match the screen, then fill each glyph's outline from `ab_glyph`. egui's font size and ab_glyph's scale aren't defined the same way, so the scale is calibrated from a glyph's advance width.
5. Images are premultiplied once per export and drawn with a bicubic pattern.

Export renders the selection if there is one, else the whole board, at 2x.

## Tools

Each tool has a small state struct and an `input()` method taking an `Input`: pointer position in board units, pressed, down, released, double-clicked, shift, and board units per screen point for tolerances. Tools never see screen coordinates.

Pen, shape and eraser return an `Edit` when an action finishes, and `App` applies it. Select is different: dragging moves objects live, so it calls `Document::checkpoint()` on the first real movement and then `apply_no_history` every frame. The whole drag is one undo step, and a click without movement adds none.

A press and release can land in the same frame on a quick click. The canvas feeds them to the tool as two steps.

### Freehand capture

- New points closer than half a screen pixel to the last one get dropped.
- On release: Chaikin smoothing with 2 iterations, then Ramer-Douglas-Peucker at 0.15 screen px. Smoothing first gives round curves. Simplifying after it drops the points smoothing added on straight parts.
- Shift with the highlighter keeps only the first point and the cursor, snapped to 15°.

### Selecting and resizing

- Click selects the top object. Shift-click toggles. Dragging empty space draws a box that selects objects fully inside it.
- With several objects selected, clicking one without dragging selects just that one.
- Eight handles resize the selection's bounds. Shift on a corner keeps the aspect ratio. The handles sit on bounds including stroke width, but the geometry gets mapped between bounds without stroke width, so a shape's corner stays exactly where it was drawn.
- A single selected line or arrow gets two endpoint handles instead.
- Double-clicking text edits it.

### Context menu

A right-click press on the board commits any text being typed, then hit-tests. Hitting an object selects it, unless it's already part of the selection, and switches to the select tool so the selection is visible. Missing everything clears the selection. egui opens the menu on release.

Menu entries are a `MenuItem` enum and `App::run_menu` does the work, so tests can trigger entries without finding buttons on screen. Stacking moves are `Edit::Reorder(ids, Order)`. Forward and backward move each selected object one step past its unselected neighbor, so a selected block moves together. `Document::can_reorder` greys out moves that would change nothing, and `apply` skips them so they don't add undo steps.

While any popup is open, a click on the board only closes it. That covers the context menu and the color picker.

## Undo and history

Snapshot-based. Before an edit, the current `Vec<Arc<Object>>` goes onto the undo stack. That copies pointers, not objects: a snapshot of 5,000 objects is 40 KB, and unchanged objects are shared by every snapshot. 200 steps per tab.

Images live outside the snapshots and stay for the session, so undoing a delete brings the image back. Selection isn't part of history; undo drops selected ids that no longer exist.

## File format

`.drafty` is UTF-8 JSON:

```json
{
  "format": "drafty",
  "version": 1,
  "title": "Board 1",
  "background": "#ffffff",
  "view": { "offset": [0, 0], "zoom": 1 },
  "objects": [
    { "type": "stroke", "points": [[10,20],[12,24]], "color": "#e53935", "width": 4, "highlighter": false },
    { "type": "shape", "shape": "arrow", "a": [0,0], "b": [100,40], "color": "#000000", "width": 3, "fill": "none", "double": true },
    { "type": "text", "pos": [50,60], "text": "this one", "size": 24, "color": "#000000" },
    { "type": "image", "image": "9f2c41d07e3a5b18", "rect": [0,0,1920,1080] }
  ],
  "images": { "9f2c41d07e3a5b18": "<base64 png or jpeg>" }
}
```

- JSON keeps files readable. base64 adds a third to image size; a board with a few screenshots is a few MB. If that becomes a problem, a zip container can be `version: 2`.
- Unknown fields and unknown object types are skipped. A newer `version` is an error, not a guess.
- Points round to 2 decimals.
- Writes go to `file.drafty.tmp`, then a rename, so a crash mid-save never truncates the file.

## Session

`session.rs` keeps state in the platform data dir: `~/.local/share/drafty/` on Linux (respects `XDG_DATA_HOME`), `~/Library/Application Support/drafty/` on macOS.

```
session.json           tab order, titles, paths, active tab, grid on/off
tabs/<session_id>.drafty   content of each open tab
```

- Autosave runs every 30 s while something changed, and on quit. When idle with unsaved changes, the app asks egui for one repaint at the next deadline instead of polling.
- Only changed tabs get rewritten. Files of closed tabs get removed.
- Startup restores the session, then opens command-line files as extra tabs. Anything that needs fonts, like measuring text, waits for the first frame, because egui has no fonts before then.

## Clipboard

egui's clipboard handles text editing, and turns Ctrl+C/X/V into `Copy`, `Cut`, `Paste(text)` and `PasteImage` events. `App` handles those when no text box has focus:

- **Copy/Cut** puts the selected objects in an in-process buffer and writes a marker string to the system clipboard.
- **Paste(text)** checks for an image on the clipboard first, since a browser image copy can also carry a URL. Then, if the text is still our marker, it pastes the objects. Otherwise it pastes the text as a text object.
- **PasteImage** inserts the image.

`clipboard.rs` owns one `arboard::Clipboard` for the whole run, because on Linux the owner has to stay alive for other apps to read what we copied. It handles image reads for `Ctrl+Shift+V` and image writes for `Ctrl+Shift+C`. On Wayland, when arboard fails, it falls back to `wl-paste`/`wl-copy` if they're installed.

## Keyboard handling

One table in `app.rs` maps `(Key, modifiers)` to an `Action`. `command` means Ctrl on Linux and Cmd on macOS. Matching is exact, so `Ctrl+Z` doesn't also fire on `Ctrl+Shift+Z`. The help overlay renders from the same table.

- Nothing fires while a text box has focus.
- With the help overlay or close prompt open, only `?` and `Escape` work.
- egui's own Ctrl+Plus/Minus/0 UI zoom is off, since those keys zoom the board.

## Build and install

### Linux

```sh
make                  # cargo build --release
make install          # into ~/.local
sudo make install PREFIX=/usr/local
make uninstall
```

`make install` puts the binary in `bin/`, plus the `.desktop` file, icons in the hicolor theme, and a MIME type for `.drafty`, then refreshes the desktop caches. With `DESTDIR` set, it skips the cache refresh, which is what packagers want. `install` never builds, so `sudo make install` never runs cargo as root.

Build dependencies are a Rust toolchain (1.88+) and a C compiler. On Arch, a desktop install already has the libraries winit needs at runtime.

### macOS

```sh
./scripts/bundle-macos.sh               # target/Drafty.app for this Mac
./scripts/bundle-macos.sh --universal   # Apple Silicon + Intel
cp -r target/Drafty.app /Applications/
```

The script builds, lays out the `.app` by hand, makes `drafty.icns` from `assets/icon-1024.png` with `sips` and `iconutil`, writes `Info.plist`, and signs ad hoc with `codesign --sign -`. Apple Silicon won't run unsigned binaries. There's no notarization; apps built locally aren't quarantined, so Gatekeeper doesn't complain.

### CI

`.github/workflows/ci.yml` runs fmt, clippy with `-D warnings`, tests and a release build on Linux, and tests plus a universal `.app` on macOS. Both upload their build as an artifact.

## Testing

`cargo test` runs everything without a window:

- **Unit tests** in `doc/`, `tools/select.rs` and `session.rs`: geometry, hit testing, history, file round trips, image memory release, session save and restore.
- **App tests** in `app_tests.rs` run the real `App::show` inside `egui::Context::run_ui` with synthetic pointer and keyboard events, then check the document. They cover drawing with each tool, shift constraints, select/move/resize, undo, eraser, typing and editing text, paste, tabs, the close prompt, shortcuts behind dialogs, scroll and zoom, and session restore.
- **Golden images** in `render/export.rs` compare exports against `tests/golden/*.png`, allowing a few pixels of drift. `UPDATE_GOLDEN=1 cargo test` rewrites them.
- **Frame time**: `cargo test --release -- --ignored frame_time --nocapture` prints CPU time per frame with 1,000 and 5,000 strokes.

The app tests don't reach the system clipboard, file dialogs, drag-and-drop or the GPU. Check those by hand before a release: paste a screenshot, annotate it with each tool, copy it out into a chat app, save, quit, relaunch, check the session came back, type å/ä/ö.
