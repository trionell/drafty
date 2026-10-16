# Drafty plan

Drafty is a small desktop whiteboard for sketching quick explanations for coworkers. It covers the gap between MS Paint and a screenshot annotator. You open it, draw boxes and arrows, paste a screenshot, scribble on it, copy the result and paste it into chat.

This file covers scope, tech choice, UX and milestones. `ARCHITECTURE.md` covers the code design.

## Goals

1. Draw a diagram and get it into Slack/Teams/email in under 30 seconds.
2. Native app on Linux and macOS, built from one codebase with `cargo build`.
3. Light. Small binary, fast start, zero CPU when idle.
4. Several boards open at once as tabs.

## Non-goals for v1

- Pixel editing. No fill bucket, no pixel eraser, no filters. Everything on the board is an object you can select and move.
- Layers, rotation of objects, pressure-sensitive pens.
- Collaboration, sync, accounts.
- PDF/SVG export. PNG covers the use case.
- Windows. It will likely compile, but nobody tests it.

## Feature list

### Tools

| Tool | Key | Behavior |
|---|---|---|
| Select | `V` | Click to select, drag for box select, shift-click to add. Move by dragging, resize with handles. `Delete` removes. |
| Pen | `P` | Freehand stroke. Color and width from the style bar. Points get smoothed when the stroke ends. |
| Highlighter | `H` | Wide, flat-capped, semi-transparent stroke. Holding shift snaps it to a straight line, which is how you mark a line of text in a screenshot. |
| Text | `T` | Click to place a text box, type, `Esc` or click outside to finish. Double-click existing text with the select tool to edit it. Multi-line. |
| Rectangle | `R` | Drag a rectangle. Shift makes a square. |
| Ellipse | `O` | Drag an ellipse. Shift makes a circle. |
| Triangle | `G` | Drag an isosceles triangle in its bounding box. |
| Line | `L` | Straight line. Shift snaps to 15° steps. |
| Arrow | `A` | Line with an arrowhead. Style bar toggles single or double head. Shift snaps angle. |
| Eraser | `E` | Drag over objects to delete them whole. Simple, predictable, and what annotators do. |
| Pan | `Space` held, or middle mouse | Moves the view. |

Shapes have stroke color, stroke width and a fill option: none, solid, or translucent. Translucent fill with a red stroke is the usual "look here" box.

### Style bar

- 8 preset colors plus a custom picker. Presets: black, white, red, orange, yellow, green, blue, purple. Keys `1` to `8` pick them.
- Stroke width: 3 presets (thin, medium, thick) plus a slider. `[` and `]` step it.
- Fill toggle for shapes.
- Font size for text: S, M, L, XL.
- With objects selected, changing a style applies to the selection. Without a selection it sets the style for the next object. One control, two jobs, no separate "properties" panel.

### Canvas

- Infinite board with pan and zoom. Scroll pans, ctrl/cmd+scroll and pinch zoom, `Ctrl+0` resets to 100%, `Ctrl+1` zooms to fit content.
- White background by default. Optional dot grid, toggled with `Ctrl+'`. No snapping to grid in v1.

### Images

- `Ctrl+V` with an image on the clipboard pastes it at the view center, or under the cursor if the cursor is on the board. The image is a normal object, so you can move, resize and draw over it.
- `Ctrl+Shift+V` pastes the clipboard image into a new tab. This is the screenshot annotator flow: take a screenshot with the OS tool, hit `Ctrl+Shift+V` in Drafty, mark it up.
- Drag and drop image files onto the window.
- `drafty some.png` on the command line opens the image in a new board.
- Formats: PNG and JPEG. Others are out until someone needs them.

### Getting things out

- `Ctrl+Shift+C` copies the whole board as a PNG to the clipboard. This is the main way out of the app, so it gets the shortest path.
- With a selection, `Ctrl+Shift+C` copies only the selection as PNG.
- `Ctrl+E` exports to a PNG file.
- Export crops to the content bounds plus 16 px padding, renders at 2x for sharp text on HiDPI screens, and uses the board background color. A transparent background is an export option.
- `Ctrl+C`/`Ctrl+V` on selected objects copies objects, not pixels, so you can move things between tabs.

### Tabs

- `Ctrl+T` new board, `Ctrl+W` close, `Ctrl+Tab`/`Ctrl+Shift+Tab` cycle, `Alt+1..9` jump.
- Double-click a tab to rename it. New tabs get "Board 1", "Board 2" and so on.
- Each tab has its own undo history and view position.
- Middle-click closes a tab. Drag to reorder is a later nice-to-have.

### Saving

Most boards are throwaway, so Drafty behaves like a scratchpad editor, not like a document editor that nags on close.

- Session restore. All open tabs, saved or not, get written to the app data directory on quit and every 30 seconds while something changed. Next launch reopens them. Closing a tab with unsaved content asks once. Quitting the app never asks.
- `Ctrl+S` saves a board to a `.drafty` file. `Ctrl+O` opens one. Once a board has a path, `Ctrl+S` writes to it without a dialog.

### Undo

`Ctrl+Z`, `Ctrl+Shift+Z` and `Ctrl+Y`. 200 steps per tab. Every finished action is one step: a stroke, a move, a style change, a paste.

On macOS every `Ctrl` shortcut above is `Cmd`.

## Tech choice

**Rust with egui (eframe).**

Reasons:

- One `cargo build --release` produces a single binary on both Linux and macOS. No system UI toolkit to install.
- egui ships window creation (winit), OpenGL rendering (glow), text layout and input, and a widget set for the toolbar. The canvas is custom drawing in egui's painter, which is the part of this app that matters.
- egui only repaints on input or animation. Idle CPU is zero.
- A release build of an egui app with the glow backend lands around 5 to 10 MB stripped. Start-up is well under half a second.
- Rust has the pieces I need as small crates: `arboard` for clipboard images, `image` for PNG/JPEG decode, `tiny-skia` for CPU rendering on export, `rfd` for file dialogs.

What egui costs: widgets don't look like GTK or Cocoa widgets, and screen reader support is weaker than a native toolkit's. For a drawing tool with a toolbar of icons, I think that trade is fine.

Alternatives considered:

| Option | Why not |
|---|---|
| Tauri, Electron | Web view. Ruled out by the brief. |
| Qt (C++ or via bindings) | Native look and a good canvas, but a large dependency, slow builds, and more friction building on macOS. Heavier than the app needs. |
| GTK4 | Great on Linux. On macOS it means Homebrew GTK and bundling dylibs into the `.app`. That breaks the "easy to build on both" goal. |
| Iced | Close second. Elm-style architecture fits less well with a tool-heavy canvas, and text input has historically lagged egui. |
| Slint | Good for forms, weaker for a free-form drawing canvas. |
| C/Zig with SDL or raylib | Smallest binary, but I would write text editing, IME, clipboard images and file dialogs by hand. |

## Performance budgets

These go into the M7 checklist and get measured on both platforms.

| Metric | Target |
|---|---|
| Release binary, stripped | under 12 MB |
| Cold start to usable window | under 300 ms |
| CPU while idle | 0% (no repaint loop) |
| Memory, 3 tabs with one 4K screenshot each | under 200 MB |
| Frame time while drawing with 5,000 objects on the board | under 8 ms |

## Milestones

Each milestone ends with something runnable. Tabs are a UI concern only. The `Document` type exists from M0 so nothing assumes a single board.

### M0. Skeleton

- Cargo project, eframe window titled "Drafty", glow backend.
- `App` holds `Vec<Document>` with one document.
- Canvas widget with camera: pan with space-drag, middle mouse, scroll. Zoom with ctrl+scroll and pinch, centered on the cursor.
- Dot grid rendering, so pan/zoom can be checked by eye.

Done when the window opens on Linux (Wayland and X11) and macOS, and pan/zoom feel right.

### M1. Pen, styles, undo

- Pen tool with point capture, distance filtering and smoothing on release.
- Style bar with colors and widths.
- Undo/redo.
- Object list rendered with viewport culling.

Done when you can draw, change color, undo and redo.

### M2. Shapes, highlighter, eraser

- Rectangle, ellipse, triangle, line, arrow. Shift constraints. Live preview while dragging.
- Fill options.
- Highlighter with straight-line snap.
- Object eraser.

### M3. Selection

- Hit testing for every object type.
- Click select, box select, shift-add.
- Move, resize with 8 handles, delete, duplicate with `Ctrl+D`.
- Bring to front and send to back with `Ctrl+]` and `Ctrl+[`.
- Style bar edits the selection.
- Arrow keys nudge 1 px, shift+arrow 10 px.

### M4. Text

- Text tool places a box backed by egui's text editing.
- Edit existing text by double-click.
- Font sizes. One bundled sans font so text looks the same on every machine and in exports.

### M5. Images and export

- Paste image from clipboard, `Ctrl+Shift+V` paste as new tab, drag-and-drop, CLI argument.
- Export renderer with tiny-skia.
- Copy board or selection as PNG to the clipboard. Export PNG file.

After M5 the core loop works end to end: paste screenshot, annotate, copy out. This is the point to start using Drafty for real and adjust the plan.

### M6. Tabs and files

- Tab bar, shortcuts, rename, close prompt for unsaved content.
- `.drafty` save/open.
- Session autosave and restore.
- Object copy/paste across tabs.

### M7. Packaging and polish

- App icon, `.desktop` file, macOS `.app` bundle.
- Build scripts for both platforms. See "Build and install" in `ARCHITECTURE.md`.
- `?` opens a shortcut cheat sheet overlay.
- Measure the performance budgets and fix what misses.
- Manual test pass on Arch (Wayland and X11) and macOS (Apple Silicon).

## Risks

- **Clipboard images on Wayland.** `arboard` needs its `wayland-data-control` feature, and some compositors don't implement the data-control protocol. Fallback is shelling out to `wl-copy`/`wl-paste` when present. Test on the compositor actually in use early, in M5 at the latest, ideally with a throwaway spike in M0.
- **Highlighter overlap.** egui draws a semi-transparent polyline as one mesh, and where the stroke crosses itself the alpha doubles, so you get dark blotches. Fix is to rasterize each highlighter stroke opaque into a texture with tiny-skia and draw that texture with alpha. Start with the simple version and switch if it looks bad.
- **Screen and export renderers drift apart.** Two renderers draw the same objects, egui on screen and tiny-skia for PNG. Golden-image tests on the export side plus a shared geometry module, where arrowheads, triangle points and smoothing live once, keep them in line.
- **OpenGL on macOS.** Deprecated by Apple but still working. If it breaks, switching eframe to the wgpu backend is a feature flag change at the cost of a bigger binary.
- **Text editing edge cases.** IME and dead keys go through egui/winit. Worth testing Swedish keyboard input (å, ä, ö) on both platforms in M4.

## Open questions

1. Infinite board, or a fixed page size like Paint? The plan assumes infinite with export cropped to content. A fixed canvas would make exports predictable but you'd have to resize it.
2. Should `Ctrl+Shift+C` also flash a short "Copied" toast? I'd say yes. It's cheap and you otherwise can't tell it worked.
3. Default font. Plan assumes Inter. Any preference for a hand-drawn style like Excalidraw's?
4. Name. "Drafty" comes from the repo name.
