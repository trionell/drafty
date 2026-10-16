pub mod eraser;
pub mod pen;
pub mod select;
pub mod shape;

use egui::{Color32, Pos2};

use crate::doc::{Fill, ShapeKind, Style};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Select,
    Pen,
    Highlighter,
    Text,
    Rect,
    Ellipse,
    Triangle,
    Line,
    Arrow,
    Eraser,
}

impl Tool {
    pub const ALL: [Tool; 10] = [
        Tool::Select,
        Tool::Pen,
        Tool::Highlighter,
        Tool::Text,
        Tool::Rect,
        Tool::Ellipse,
        Tool::Triangle,
        Tool::Line,
        Tool::Arrow,
        Tool::Eraser,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Tool::Select => "Select",
            Tool::Pen => "Pen",
            Tool::Highlighter => "Highlighter",
            Tool::Text => "Text",
            Tool::Rect => "Rectangle",
            Tool::Ellipse => "Ellipse",
            Tool::Triangle => "Triangle",
            Tool::Line => "Line",
            Tool::Arrow => "Arrow",
            Tool::Eraser => "Eraser",
        }
    }

    pub fn key(self) -> egui::Key {
        use egui::Key;
        match self {
            Tool::Select => Key::V,
            Tool::Pen => Key::P,
            Tool::Highlighter => Key::H,
            Tool::Text => Key::T,
            Tool::Rect => Key::R,
            Tool::Ellipse => Key::O,
            Tool::Triangle => Key::G,
            Tool::Line => Key::L,
            Tool::Arrow => Key::A,
            Tool::Eraser => Key::E,
        }
    }

    pub fn shape(self, arrow_double: bool) -> Option<ShapeKind> {
        match self {
            Tool::Rect => Some(ShapeKind::Rect),
            Tool::Ellipse => Some(ShapeKind::Ellipse),
            Tool::Triangle => Some(ShapeKind::Triangle),
            Tool::Line => Some(ShapeKind::Line),
            Tool::Arrow => Some(ShapeKind::Arrow { double: arrow_double }),
            _ => None,
        }
    }
}

pub const PALETTE: [Color32; 8] = [
    Color32::from_rgb(0x1f, 0x1f, 0x1f),
    Color32::from_rgb(0xff, 0xff, 0xff),
    Color32::from_rgb(0xe5, 0x39, 0x35),
    Color32::from_rgb(0xfb, 0x8c, 0x00),
    Color32::from_rgb(0xfd, 0xd8, 0x35),
    Color32::from_rgb(0x43, 0xa0, 0x47),
    Color32::from_rgb(0x1e, 0x88, 0xe5),
    Color32::from_rgb(0x8e, 0x24, 0xaa),
];

pub const PEN_WIDTHS: [f32; 3] = [2.0, 4.0, 8.0];
pub const HIGHLIGHT_WIDTHS: [f32; 3] = [12.0, 20.0, 32.0];
pub const FONT_SIZES: [(&str, f32); 4] = [("S", 16.0), ("M", 24.0), ("L", 36.0), ("XL", 56.0)];

/// Style for the next object. The highlighter keeps its own color and width so switching
/// between pen and highlighter doesn't turn a red pen into a red highlighter.
pub struct Settings {
    pub color: Color32,
    pub highlight_color: Color32,
    pub pen_width: f32,
    pub shape_width: f32,
    pub highlight_width: f32,
    pub fill: Fill,
    pub font_size: f32,
    pub arrow_double: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            color: PALETTE[2],
            highlight_color: PALETTE[4],
            pen_width: PEN_WIDTHS[1],
            shape_width: PEN_WIDTHS[1],
            highlight_width: HIGHLIGHT_WIDTHS[1],
            fill: Fill::None,
            font_size: FONT_SIZES[1].1,
            arrow_double: false,
        }
    }
}

impl Settings {
    pub fn pen_style(&self) -> Style {
        Style { color: self.color, width: self.pen_width, fill: Fill::None }
    }

    pub fn highlight_style(&self) -> Style {
        Style { color: self.highlight_color, width: self.highlight_width, fill: Fill::None }
    }

    pub fn shape_style(&self) -> Style {
        Style { color: self.color, width: self.shape_width, fill: self.fill }
    }

    pub fn color_for(&self, tool: Tool) -> Color32 {
        if tool == Tool::Highlighter { self.highlight_color } else { self.color }
    }

    pub fn set_color_for(&mut self, tool: Tool, c: Color32) {
        if tool == Tool::Highlighter {
            self.highlight_color = c;
        } else {
            self.color = c;
        }
    }

    pub fn width_for(&self, tool: Tool) -> f32 {
        match tool {
            Tool::Highlighter => self.highlight_width,
            Tool::Pen => self.pen_width,
            _ => self.shape_width,
        }
    }

    pub fn set_width_for(&mut self, tool: Tool, w: f32) {
        match tool {
            Tool::Highlighter => self.highlight_width = w,
            Tool::Pen => self.pen_width = w,
            _ => self.shape_width = w,
        }
    }
}

/// Pointer state for one frame, in board coordinates. Tools never see screen coordinates.
#[derive(Clone, Copy, Debug)]
pub struct Input {
    pub pos: Pos2,
    pub pressed: bool,
    pub down: bool,
    pub released: bool,
    pub double_clicked: bool,
    pub shift: bool,
    /// Board units per screen point, for zoom-independent tolerances.
    pub px: f32,
}
