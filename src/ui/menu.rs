//! The board's right-click menu.

use egui::{Button, Ui};

use crate::app::{Action, App, MenuItem};
use crate::doc::{ObjectKind, Order};

/// "Ctrl+X" or "Cmd+X", for the clipboard keys egui handles itself.
fn cmd(key: &str) -> String {
    let m = if cfg!(target_os = "macos") {
        "Cmd"
    } else {
        "Ctrl"
    };
    format!("{m}+{key}")
}

impl App {
    pub fn context_menu_ui(&mut self, ui: &mut Ui) {
        ui.set_min_width(220.0);
        let doc = self.doc();
        let selected = !doc.selection.is_empty();
        let has_objects = !doc.objects.is_empty();
        let text = match doc.selected_objects().as_slice() {
            [o] if matches!(o.kind, ObjectKind::Text { .. }) => Some(o.id),
            _ => None,
        };
        let can = |order| doc.can_reorder(&doc.selection, order);
        let order_enabled = [Order::Front, Order::Forward, Order::Backward, Order::Back].map(can);

        let mut chosen = None;
        let mut item = |ui: &mut Ui, enabled: bool, label: &str, shortcut: String, it: MenuItem| {
            if ui
                .add_enabled(enabled, Button::new(label).shortcut_text(shortcut))
                .clicked()
            {
                chosen = Some(it);
            }
        };

        if selected {
            if let Some(id) = text {
                item(ui, true, "Edit text", String::new(), MenuItem::EditText(id));
                ui.separator();
            }
            item(ui, true, "Cut", cmd("X"), MenuItem::Cut);
            item(ui, true, "Copy", cmd("C"), MenuItem::Copy);
            item(ui, true, "Paste", cmd("V"), MenuItem::Paste);
            item(
                ui,
                true,
                "Duplicate",
                self.shortcut_label(Action::Duplicate),
                MenuItem::Duplicate,
            );
            item(
                ui,
                true,
                "Delete",
                self.shortcut_label(Action::Delete),
                MenuItem::Delete,
            );
            ui.separator();
            let orders = [
                (Order::Front, "Bring to front"),
                (Order::Forward, "Bring forward"),
                (Order::Backward, "Send backward"),
                (Order::Back, "Send to back"),
            ];
            for ((order, label), enabled) in orders.into_iter().zip(order_enabled) {
                let sc = self.shortcut_label(Action::Reorder(order));
                item(ui, enabled, label, sc, MenuItem::Reorder(order));
            }
            ui.separator();
            let sc = self.shortcut_label(Action::CopyPng);
            item(ui, true, "Copy selection as PNG", sc, MenuItem::CopyPng);
            item(
                ui,
                true,
                "Select all",
                self.shortcut_label(Action::SelectAll),
                MenuItem::SelectAll,
            );
        } else {
            item(ui, true, "Paste", cmd("V"), MenuItem::Paste);
            let sc = self.shortcut_label(Action::SelectAll);
            item(ui, has_objects, "Select all", sc, MenuItem::SelectAll);
            ui.separator();
            let sc = self.shortcut_label(Action::CopyPng);
            item(ui, has_objects, "Copy board as PNG", sc, MenuItem::CopyPng);
            let sc = self.shortcut_label(Action::ExportPng);
            item(ui, has_objects, "Export as PNG…", sc, MenuItem::ExportPng);
        }

        if let Some(it) = chosen {
            ui.close();
            self.run_menu(it);
        }
    }
}
