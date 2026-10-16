use egui::{Id, Ui};

use crate::app::App;

const RENAME_ID: &str = "drafty-tab-rename";

impl App {
    pub fn tab_bar(&mut self, ui: &mut Ui) {
        let mut switch = None;
        let mut close = None;
        let mut rename_start = None;
        let mut rename_done = false;
        egui::ScrollArea::horizontal().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for i in 0..self.docs.len() {
                    if let Some((ri, buf)) = &mut self.renaming
                        && *ri == i
                    {
                        let r = ui.add(
                            egui::TextEdit::singleline(buf)
                                .id(Id::unique(RENAME_ID))
                                .desired_width(140.0),
                        );
                        if r.lost_focus() {
                            rename_done = true;
                        }
                        continue;
                    }
                    let doc = &self.docs[i];
                    let mut title = doc.title.clone();
                    if doc.path.is_some() && doc.is_dirty() {
                        title.push_str(" •");
                    }
                    let tab = ui
                        .add(egui::Button::selectable(i == self.active, title))
                        .on_hover_text(match &doc.path {
                            Some(p) => p.display().to_string(),
                            None => "Not saved to a file. Double-click to rename.".into(),
                        });
                    if tab.clicked() {
                        switch = Some(i);
                    }
                    if tab.double_clicked() {
                        rename_start = Some(i);
                    }
                    if tab.middle_clicked() {
                        close = Some(i);
                    }
                    if ui.small_button("×").on_hover_text("Close tab").clicked() {
                        close = Some(i);
                    }
                    ui.add_space(6.0);
                }
                if ui.button("+").on_hover_text("New tab (Ctrl+T)").clicked() {
                    self.new_tab();
                }
            });
        });

        if rename_done && let Some((i, name)) = self.renaming.take() {
            let name = name.trim();
            if !name.is_empty() && i < self.docs.len() {
                self.docs[i].title = name.to_owned();
                // The title is saved in the session file, so count it as a change.
                self.docs[i].touch();
            }
        }
        if let Some(i) = switch {
            self.switch_tab(i);
        }
        if let Some(i) = rename_start {
            self.renaming = Some((i, self.docs[i].title.clone()));
            ui.memory_mut(|m| m.request_focus(Id::unique(RENAME_ID)));
        }
        if let Some(i) = close {
            self.request_close(i);
        }
    }
}
