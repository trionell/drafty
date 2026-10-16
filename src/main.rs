// Hide the console window if someone builds this on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod clipboard;
mod doc;
mod render;
mod session;
mod tools;
mod ui;

#[cfg(test)]
mod app_tests;

use std::path::PathBuf;

pub const INTER: &[u8] = include_bytes!("../assets/Inter-Regular.ttf");
const ICON_PNG: &[u8] = include_bytes!("../assets/icon.png");

fn main() -> eframe::Result {
    let mut files = Vec::new();
    for arg in std::env::args_os().skip(1) {
        match arg.to_str() {
            Some("-h" | "--help") => {
                println!("usage: drafty [FILE...]\n\nOpens .drafty boards and PNG/JPEG images as tabs.");
                return Ok(());
            }
            Some("-V" | "--version") => {
                println!("drafty {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            _ => files.push(PathBuf::from(arg)),
        }
    }

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Drafty")
        .with_app_id("drafty")
        .with_inner_size([1200.0, 800.0])
        .with_min_inner_size([520.0, 320.0])
        .with_drag_and_drop(true);
    if let Ok(icon) = eframe::icon_data::from_png_bytes(ICON_PNG) {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native(
        "Drafty",
        options,
        Box::new(|cc| {
            let app = app::App::new(&cc.egui_ctx, files, session::Session::new(), clipboard::Clipboard::new());
            Ok(Box::new(app))
        }),
    )
}
