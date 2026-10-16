//! `.drafty` files: UTF-8 JSON with images embedded as base64.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use base64::Engine as _;
use egui::{Color32, Pos2, Rect, Vec2, pos2, vec2};
use serde::{Deserialize, Serialize};

use super::{Document, Fill, ImageData, ImageId, Object, ObjectId, ObjectKind, ShapeKind, Style};

pub const FORMAT: &str = "drafty";
pub const VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct FileDoc {
    format: String,
    version: u32,
    #[serde(default)]
    title: String,
    #[serde(default)]
    background: Option<String>,
    #[serde(default)]
    view: Option<FileView>,
    #[serde(default)]
    objects: Vec<FileObject>,
    #[serde(default)]
    images: BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize)]
struct FileView {
    offset: [f32; 2],
    zoom: f32,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum FileObject {
    Stroke {
        points: Vec<[f32; 2]>,
        color: String,
        width: f32,
        #[serde(default)]
        highlighter: bool,
    },
    Shape {
        shape: String,
        a: [f32; 2],
        b: [f32; 2],
        color: String,
        width: f32,
        #[serde(default = "fill_none")]
        fill: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        double: bool,
    },
    Text {
        pos: [f32; 2],
        text: String,
        size: f32,
        color: String,
    },
    Image {
        image: String,
        rect: [f32; 4],
    },
    /// Object types from a newer version get skipped instead of failing the load.
    #[serde(other)]
    Unknown,
}

fn fill_none() -> String {
    "none".into()
}

fn r2(x: f32) -> f32 {
    (x * 100.0).round() / 100.0
}

fn p2(p: Pos2) -> [f32; 2] {
    [r2(p.x), r2(p.y)]
}

fn to_pos(a: [f32; 2]) -> Pos2 {
    pos2(a[0], a[1])
}

pub fn color_to_hex(c: Color32) -> String {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    if a == 255 {
        format!("#{r:02x}{g:02x}{b:02x}")
    } else {
        format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
    }
}

pub fn color_from_hex(s: &str) -> Option<Color32> {
    let s = s.strip_prefix('#')?;
    let byte = |i: usize| u8::from_str_radix(s.get(i..i + 2)?, 16).ok();
    match s.len() {
        6 => Some(Color32::from_rgb(byte(0)?, byte(2)?, byte(4)?)),
        8 => Some(Color32::from_rgba_unmultiplied(byte(0)?, byte(2)?, byte(4)?, byte(6)?)),
        _ => None,
    }
}

fn fill_str(f: Fill) -> &'static str {
    match f {
        Fill::None => "none",
        Fill::Solid => "solid",
        Fill::Translucent => "translucent",
    }
}

fn shape_str(s: ShapeKind) -> &'static str {
    match s {
        ShapeKind::Rect => "rect",
        ShapeKind::Ellipse => "ellipse",
        ShapeKind::Triangle => "triangle",
        ShapeKind::Line => "line",
        ShapeKind::Arrow { .. } => "arrow",
    }
}

/// Serialize a document.
pub fn to_json(doc: &Document) -> String {
    let mut used_images = BTreeMap::new();
    let objects = doc
        .objects
        .iter()
        .map(|o| object_to_file(&o.kind, doc, &mut used_images))
        .collect();
    let file = FileDoc {
        format: FORMAT.into(),
        version: VERSION,
        title: doc.title.clone(),
        background: Some(color_to_hex(doc.background)),
        view: Some(FileView {
            offset: [r2(doc.camera.offset.x), r2(doc.camera.offset.y)],
            zoom: doc.camera.zoom,
        }),
        objects,
        images: used_images,
    };
    serde_json::to_string(&file).expect("serializing a document can't fail")
}

fn object_to_file(
    kind: &ObjectKind,
    doc: &Document,
    images: &mut BTreeMap<String, String>,
) -> FileObject {
    match kind {
        ObjectKind::Stroke { points, style, highlighter } => FileObject::Stroke {
            points: points.iter().map(|p| p2(*p)).collect(),
            color: color_to_hex(style.color),
            width: style.width,
            highlighter: *highlighter,
        },
        ObjectKind::Shape { shape, a, b, style } => FileObject::Shape {
            shape: shape_str(*shape).into(),
            a: p2(*a),
            b: p2(*b),
            color: color_to_hex(style.color),
            width: style.width,
            fill: fill_str(style.fill).into(),
            double: matches!(shape, ShapeKind::Arrow { double: true }),
        },
        ObjectKind::Text { pos, text, size, color, .. } => FileObject::Text {
            pos: p2(*pos),
            text: text.clone(),
            size: *size,
            color: color_to_hex(*color),
        },
        ObjectKind::Image { image, rect } => {
            let key = image.to_hex();
            if !images.contains_key(&key)
                && let Some(data) = doc.images.get(image) {
                    let b64 = base64::engine::general_purpose::STANDARD.encode(&*data.encoded());
                    images.insert(key.clone(), b64);
                }
            FileObject::Image {
                image: key,
                rect: [r2(rect.min.x), r2(rect.min.y), r2(rect.max.x), r2(rect.max.y)],
            }
        }
    }
}

/// Parse a document. Text extents come back zero; the app re-measures them with its fonts.
pub fn from_json(json: &str) -> Result<Document, String> {
    let file: FileDoc = serde_json::from_str(json).map_err(|e| format!("not a Drafty file: {e}"))?;
    if file.format != FORMAT {
        return Err("not a Drafty file".into());
    }
    if file.version > VERSION {
        return Err(format!(
            "this file was written by a newer Drafty (format version {}), please update",
            file.version
        ));
    }

    let mut doc = Document::new(if file.title.is_empty() { "Board".into() } else { file.title });
    if let Some(bg) = file.background.as_deref().and_then(color_from_hex) {
        doc.background = bg;
    }
    if let Some(v) = file.view {
        doc.camera.offset = vec2(v.offset[0], v.offset[1]);
        doc.camera.zoom = v.zoom.clamp(super::MIN_ZOOM, super::MAX_ZOOM);
    }

    for (key, b64) in &file.images {
        let Some(id) = ImageId::from_hex(key) else { continue };
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| format!("broken image data: {e}"))?;
        doc.images.insert(id, Arc::new(ImageData::from_encoded_lazy(bytes)?));
    }

    let black = Color32::BLACK;
    let mut objects = Vec::new();
    for (i, o) in file.objects.into_iter().enumerate() {
        let kind = match o {
            FileObject::Stroke { points, color, width, highlighter } => {
                if points.is_empty() {
                    continue;
                }
                ObjectKind::Stroke {
                    points: points.into_iter().map(to_pos).collect(),
                    style: Style {
                        color: color_from_hex(&color).unwrap_or(black),
                        width,
                        fill: Fill::None,
                    },
                    highlighter,
                }
            }
            FileObject::Shape { shape, a, b, color, width, fill, double } => {
                let shape = match shape.as_str() {
                    "rect" => ShapeKind::Rect,
                    "ellipse" => ShapeKind::Ellipse,
                    "triangle" => ShapeKind::Triangle,
                    "line" => ShapeKind::Line,
                    "arrow" => ShapeKind::Arrow { double },
                    _ => continue,
                };
                let fill = match fill.as_str() {
                    "solid" => Fill::Solid,
                    "translucent" => Fill::Translucent,
                    _ => Fill::None,
                };
                ObjectKind::Shape {
                    shape,
                    a: to_pos(a),
                    b: to_pos(b),
                    style: Style {
                        color: color_from_hex(&color).unwrap_or(black),
                        width,
                        fill,
                    },
                }
            }
            FileObject::Text { pos, text, size, color } => ObjectKind::Text {
                pos: to_pos(pos),
                text,
                size,
                color: color_from_hex(&color).unwrap_or(black),
                extent: Vec2::ZERO,
            },
            FileObject::Image { image, rect } => {
                let Some(id) = ImageId::from_hex(&image) else { continue };
                if !doc.images.contains_key(&id) {
                    continue;
                }
                ObjectKind::Image {
                    image: id,
                    rect: Rect::from_min_max(pos2(rect[0], rect[1]), pos2(rect[2], rect[3])),
                }
            }
            FileObject::Unknown => continue,
        };
        objects.push(Arc::new(Object { id: ObjectId(i as u64 + 1), kind }));
    }
    doc.set_objects(objects);
    Ok(doc)
}

/// Write via a temp file and rename, so a crash mid-save never truncates the old file.
pub fn write_atomic(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = std::path::PathBuf::from(tmp);
    std::fs::write(&tmp, contents)?;
    std::fs::rename(&tmp, path)
}

pub fn save(doc: &Document, path: &Path) -> std::io::Result<()> {
    write_atomic(path, to_json(doc).as_bytes())
}

pub fn load(path: &Path) -> Result<Document, String> {
    let json = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut doc = from_json(&json)?;
    doc.path = Some(path.to_path_buf());
    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::Edit;

    fn sample() -> Document {
        let mut doc = Document::new("Sample");
        let (id, data) = ImageData::from_rgba([2, 1], vec![255, 0, 0, 255, 0, 0, 255, 255]);
        doc.add_image(id, data);
        let style = Style { color: Color32::from_rgb(229, 57, 53), width: 4.0, fill: Fill::Translucent };
        doc.apply(Edit::Add(vec![
            ObjectKind::Stroke {
                points: vec![pos2(1.5, 2.25), pos2(10.0, 20.0)],
                style: Style { fill: Fill::None, ..style },
                highlighter: true,
            },
            ObjectKind::Shape { shape: ShapeKind::Arrow { double: true }, a: pos2(0.0, 0.0), b: pos2(50.0, 5.0), style },
            ObjectKind::Shape { shape: ShapeKind::Triangle, a: pos2(0.0, 0.0), b: pos2(50.0, 50.0), style },
            ObjectKind::Text {
                pos: pos2(5.0, 6.0),
                text: "hej å ä ö\nline 2".into(),
                size: 24.0,
                color: Color32::from_rgba_unmultiplied(0, 0, 0, 128),
                extent: Vec2::ZERO,
            },
            ObjectKind::Image { image: id, rect: Rect::from_min_max(pos2(0.0, 0.0), pos2(20.0, 10.0)) },
        ]));
        doc
    }

    #[test]
    fn roundtrip() {
        let doc = sample();
        let loaded = from_json(&to_json(&doc)).unwrap();
        assert_eq!(loaded.title, "Sample");
        assert_eq!(loaded.objects.len(), doc.objects.len());
        for (a, b) in doc.objects.iter().zip(&loaded.objects) {
            assert_eq!(a.kind, b.kind);
        }
        let img = loaded.images.values().next().unwrap();
        assert_eq!(img.size, [2, 1]);
        assert_eq!(&*img.rgba(), &[255, 0, 0, 255, 0, 0, 255, 255]);
    }

    #[test]
    fn rejects_newer_version() {
        let json = r#"{"format":"drafty","version":99,"objects":[]}"#;
        assert!(from_json(json).err().unwrap().contains("newer"));
    }

    #[test]
    fn ignores_unknown_fields_and_types() {
        let json = r##"{"format":"drafty","version":1,"future":true,"objects":[
            {"type":"hologram","x":1},
            {"type":"text","pos":[1,2],"text":"hi","size":20,"color":"#000000","glow":3}
        ]}"##;
        let doc = from_json(json).unwrap();
        assert_eq!(doc.objects.len(), 1);
    }

    #[test]
    fn hex_colors() {
        let c = Color32::from_rgba_unmultiplied(1, 2, 3, 200);
        assert_eq!(color_from_hex(&color_to_hex(c)), Some(c));
        assert_eq!(color_to_hex(Color32::from_rgb(255, 0, 16)), "#ff0010");
        assert_eq!(color_from_hex("nope"), None);
    }
}
