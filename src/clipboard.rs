//! System clipboard for images, plus Drafty's in-process clipboard for objects.

use std::borrow::Cow;
use std::collections::HashMap;
use std::io::Write as _;
use std::process::{Command, Stdio};
use std::sync::Arc;

use crate::doc::{ImageData, ImageId, ObjectKind};

/// Owns one `arboard::Clipboard` for the whole run. On Linux the owner has to stay alive
/// for other apps to read what we copied.
pub struct Clipboard {
    inner: Option<arboard::Clipboard>,
    /// Allow shelling out to wl-copy/wl-paste when arboard fails.
    fallback: bool,
}

pub struct RgbaImage {
    pub size: [u32; 2],
    pub rgba: Vec<u8>,
}

fn on_wayland() -> bool {
    cfg!(target_os = "linux") && std::env::var_os("WAYLAND_DISPLAY").is_some()
}

impl Clipboard {
    pub fn new() -> Self {
        let inner = arboard::Clipboard::new()
            .inspect_err(|e| eprintln!("drafty: clipboard unavailable: {e}"))
            .ok();
        Self { inner, fallback: on_wayland() }
    }

    /// A clipboard that never touches the system one, for tests.
    #[cfg(test)]
    pub fn disabled() -> Self {
        Self { inner: None, fallback: false }
    }

    pub fn get_image(&mut self) -> Option<RgbaImage> {
        if let Some(cb) = &mut self.inner
            && let Ok(img) = cb.get_image() {
                return Some(RgbaImage {
                    size: [img.width as u32, img.height as u32],
                    rgba: img.bytes.into_owned(),
                });
            }
        if self.fallback {
            return wl_paste_image();
        }
        None
    }

    pub fn set_image(&mut self, size: [u32; 2], rgba: Vec<u8>, png: impl FnOnce() -> Vec<u8>) -> Result<(), String> {
        let mut err = String::from("no clipboard");
        if let Some(cb) = &mut self.inner {
            let data = arboard::ImageData {
                width: size[0] as usize,
                height: size[1] as usize,
                bytes: Cow::Owned(rgba),
            };
            match cb.set_image(data) {
                Ok(()) => return Ok(()),
                Err(e) => err = e.to_string(),
            }
        }
        if self.fallback && wl_copy_png(&png()) {
            return Ok(());
        }
        Err(err)
    }

    pub fn set_text(&mut self, text: &str) {
        if let Some(cb) = &mut self.inner {
            let _ = cb.set_text(text);
        }
    }
}

/// Fallback for compositors without the data-control protocol arboard needs.
fn wl_paste_image() -> Option<RgbaImage> {
    let out = Command::new("wl-paste")
        .args(["--no-newline", "--type", "image/png"])
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() || out.stdout.is_empty() {
        return None;
    }
    let img = image::load_from_memory(&out.stdout).ok()?.into_rgba8();
    Some(RgbaImage { size: [img.width(), img.height()], rgba: img.into_raw() })
}

fn wl_copy_png(png: &[u8]) -> bool {
    let Ok(mut child) = Command::new("wl-copy")
        .args(["--type", "image/png"])
        .stdin(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    let wrote = child.stdin.take().is_some_and(|mut s| s.write_all(png).is_ok());
    wrote && child.wait().is_ok_and(|s| s.success())
}

/// Objects copied with Ctrl+C. Lives in the process so it can carry images between tabs.
/// `marker` goes on the system clipboard as text; if it's still there at paste time, the
/// user hasn't copied anything else since.
pub struct ObjectClip {
    pub objects: Vec<ObjectKind>,
    pub images: HashMap<ImageId, Arc<ImageData>>,
    pub marker: String,
}
