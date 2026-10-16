use egui::{Context, Id, RichText};

use crate::app::App;
use crate::tools::Tool;

impl App {
    pub fn help_overlay(&mut self, ctx: &Context) {
        let modal = egui::Modal::new(Id::unique("drafty-help")).show(ctx, |ui| {
            ui.set_max_width(640.0);
            ui.heading("Keyboard shortcuts");
            ui.add_space(8.0);
            ui.columns(2, |cols| {
                egui::Grid::new("help-tools")
                    .num_columns(2)
                    .spacing([16.0, 4.0])
                    .show(&mut cols[0], |ui| {
                        for t in Tool::ALL {
                            ui.label(RichText::new(t.key().name()).strong());
                            ui.label(t.name());
                            ui.end_row();
                        }
                        let rows = [
                            ("1 – 8", "Pick color"),
                            ("Shift + drag", "Square, circle, 15° lines"),
                            ("Space + drag", "Pan"),
                            ("Middle drag", "Pan"),
                            ("Scroll", "Pan"),
                            ("Ctrl + scroll", "Zoom"),
                            ("Double-click text", "Edit text"),
                            ("Alt+1 – 9", "Go to tab"),
                        ];
                        for (k, v) in rows {
                            ui.label(RichText::new(k).strong());
                            ui.label(v);
                            ui.end_row();
                        }
                    });
                egui::Grid::new("help-keys")
                    .num_columns(2)
                    .spacing([16.0, 4.0])
                    .show(&mut cols[1], |ui| {
                        for s in &self.shortcuts {
                            if let Some(help) = s.help {
                                ui.label(RichText::new(s.label()).strong());
                                ui.label(help);
                                ui.end_row();
                            }
                        }
                        {
                            let (k, v) = ("Ctrl+C / Ctrl+V", "Copy / paste objects or images");
                            ui.label(RichText::new(k).strong());
                            ui.label(v);
                            ui.end_row();
                        }
                    });
            });
            ui.add_space(8.0);
            ui.vertical_centered(|ui| {
                if ui.button("Close").clicked() {
                    ui.close();
                }
            });
        });
        if modal.should_close() {
            self.show_help = false;
        }
    }

    pub fn close_dialog(&mut self, ctx: &Context) {
        let Some(i) = self.confirm_close else { return };
        if i >= self.docs.len() {
            self.confirm_close = None;
            return;
        }
        let title = self.docs[i].title.clone();
        let mut choice = None;
        let modal = egui::Modal::new(Id::unique("drafty-close")).show(ctx, |ui| {
            ui.set_max_width(360.0);
            ui.heading(format!("Close \"{title}\"?"));
            ui.add_space(4.0);
            ui.label("Its unsaved changes will be lost.");
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.button("Close tab").clicked() {
                    choice = Some(true);
                }
                if ui.button("Cancel").clicked() {
                    choice = Some(false);
                }
            });
        });
        if modal.should_close() && choice.is_none() {
            choice = Some(false);
        }
        if let Some(close) = choice {
            self.confirm_close = None;
            if close {
                self.close_tab(i);
            }
        }
    }
}
