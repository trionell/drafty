//! Tab strip, painted by hand. The active tab has the toolbar's color and curves into it at
//! the bottom corners, so it reads as the front card of a stack.

use egui::text::{LayoutJob, TextWrapping};
use egui::{
    Align, Color32, CornerRadius, FontId, Id, Rect, Sense, Stroke, TextFormat, Ui, Visuals, pos2,
    vec2,
};

use crate::app::App;

const RENAME_ID: &str = "drafty-tab-rename";
const TAB_HEIGHT: f32 = 32.0;
const TAB_MIN: f32 = 96.0;
const TAB_MAX: f32 = 210.0;
const GAP: f32 = 2.0;
const RADIUS: u8 = 8;
/// Radius of the outward curves where the active tab meets the toolbar.
const FLARE: f32 = 6.0;
const PAD_LEFT: f32 = 12.0;
/// Room on the right for the close button.
const PAD_RIGHT: f32 = 30.0;

/// Background of the tab strip: a shade darker than the toolbar.
pub fn bar_color(v: &Visuals) -> Color32 {
    let t = if v.dark_mode { 0.4 } else { 0.08 };
    v.panel_fill.lerp_to_gamma(Color32::BLACK, t)
}

/// Frame for the tab strip's panel. No bottom margin, so tabs touch the toolbar.
pub fn bar_frame(v: &Visuals) -> egui::Frame {
    egui::Frame::NONE
        .fill(bar_color(v))
        .inner_margin(egui::Margin {
            left: 4,
            right: 4,
            top: 6,
            bottom: 0,
        })
}

struct TabPaint {
    rect: Rect,
    active: bool,
    hovered: bool,
}

impl App {
    pub fn tab_bar(&mut self, ui: &mut Ui) {
        let v = ui.visuals().clone();
        let bar = bar_color(&v);
        let active_fill = v.panel_fill;
        let hover_fill = active_fill.lerp_to_gamma(bar, 0.5);

        // Equal widths that shrink as tabs get added, down to a minimum. Past that the
        // strip scrolls.
        let n = self.docs.len();
        let room = ui.available_width() - 40.0 - 2.0 * FLARE;
        let tab_w = (room / n as f32 - GAP).clamp(TAB_MIN, TAB_MAX);

        let mut switch = None;
        let mut close = None;
        let mut rename_start = None;
        let mut rename_done = false;
        let mut new_tab = false;

        // Scroll to the active tab when it changes, e.g. after Ctrl+Tab.
        let last_id = Id::unique("drafty-last-active-tab");
        let active_changed = ui.data(|d| d.get_temp::<usize>(last_id)) != Some(self.active);
        ui.data_mut(|d| d.insert_temp(last_id, self.active));

        egui::ScrollArea::horizontal()
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = GAP;
                    ui.add_space(FLARE);
                    let mut painted = Vec::with_capacity(n);
                    for i in 0..n {
                        let active = i == self.active;
                        let (rect, resp) =
                            ui.allocate_exact_size(vec2(tab_w, TAB_HEIGHT), Sense::click());
                        let close_rect = Rect::from_center_size(
                            pos2(rect.right() - PAD_RIGHT * 0.5 - 2.0, rect.center().y),
                            vec2(20.0, 20.0),
                        );
                        let close_resp = ui.interact(
                            close_rect,
                            Id::unique(("drafty-tab-close", i)),
                            Sense::click(),
                        );
                        let hovered = resp.hovered() || close_resp.hovered();

                        // Background.
                        let p = ui.painter();
                        if active {
                            let r = CornerRadius {
                                nw: RADIUS,
                                ne: RADIUS,
                                sw: 0,
                                se: 0,
                            };
                            p.rect_filled(rect, r, active_fill);
                            for (x, side) in [(rect.left(), -1.0), (rect.right(), 1.0)] {
                                // A square of tab color with a circle of bar color carved
                                // out of it leaves a concave curve.
                                let corner = Rect::from_two_pos(
                                    pos2(x, rect.bottom() - FLARE),
                                    pos2(x + side * FLARE, rect.bottom()),
                                );
                                p.rect_filled(corner, 0.0, active_fill);
                                let c = pos2(x + side * FLARE, rect.bottom() - FLARE);
                                p.circle_filled(c, FLARE, bar);
                            }
                        } else if hovered {
                            let r = rect.shrink2(vec2(1.0, 3.0)).translate(vec2(0.0, -1.0));
                            p.rect_filled(r, RADIUS - 2, hover_fill);
                        }

                        // Title, cut off with an ellipsis when it doesn't fit.
                        let doc = &self.docs[i];
                        let title_rect = Rect::from_min_max(
                            pos2(rect.left() + PAD_LEFT, rect.top()),
                            pos2(rect.right() - PAD_RIGHT, rect.bottom()),
                        );
                        let renaming = matches!(self.renaming, Some((ri, _)) if ri == i);
                        if !renaming {
                            let color = if active {
                                v.strong_text_color()
                            } else if hovered {
                                v.text_color()
                            } else {
                                v.text_color().gamma_multiply(0.7)
                            };
                            let mut job = LayoutJob::single_section(
                                doc.title.clone(),
                                TextFormat::simple(FontId::proportional(13.5), color),
                            );
                            job.wrap = TextWrapping {
                                max_width: title_rect.width(),
                                max_rows: 1,
                                break_anywhere: true,
                                overflow_character: Some('…'),
                            };
                            let galley = ui.fonts_mut(|f| f.layout_job(job));
                            let y = rect.center().y - galley.size().y * 0.5;
                            p.galley(pos2(title_rect.left(), y), galley, color);
                        }

                        // Close button, or a dot for unsaved changes to a saved file.
                        let unsaved = doc.path.is_some() && doc.is_dirty();
                        let c = close_rect.center();
                        if hovered || (active && !unsaved) {
                            if close_resp.hovered() {
                                p.circle_filled(c, 10.0, v.widgets.hovered.weak_bg_fill);
                            }
                            let s = Stroke::new(1.4, v.text_color());
                            let d = 3.5;
                            p.line_segment([c + vec2(-d, -d), c + vec2(d, d)], s);
                            p.line_segment([c + vec2(-d, d), c + vec2(d, -d)], s);
                        } else if unsaved {
                            p.circle_filled(c, 3.5, v.text_color());
                        }

                        if renaming && let Some((_, buf)) = &mut self.renaming {
                            let edit = egui::TextEdit::singleline(buf)
                                .id(Id::unique(RENAME_ID))
                                .font(FontId::proportional(13.5))
                                .desired_width(title_rect.width());
                            let field = Rect::from_center_size(
                                title_rect.center(),
                                vec2(title_rect.width() + 8.0, 22.0),
                            );
                            if ui.place(field, edit).lost_focus() {
                                rename_done = true;
                            }
                        }

                        if close_resp.clicked() || resp.middle_clicked() {
                            close = Some(i);
                        } else if resp.double_clicked() {
                            rename_start = Some(i);
                        } else if resp.clicked() {
                            switch = Some(i);
                        }
                        if active && active_changed {
                            resp.scroll_to_me(Some(Align::Center));
                        }
                        let tip = match &doc.path {
                            Some(p) => p.display().to_string(),
                            None => "Not saved to a file. Double-click to rename.".into(),
                        };
                        resp.on_hover_text(tip);
                        close_resp.on_hover_text("Close tab");
                        painted.push(TabPaint {
                            rect,
                            active,
                            hovered,
                        });
                    }

                    // Thin dividers between idle tabs. None next to the active or hovered
                    // tab, whose own background already separates it.
                    let line = Stroke::new(1.0, v.text_color().gamma_multiply(0.2));
                    for w in painted.windows(2) {
                        let quiet = |t: &TabPaint| !t.active && !t.hovered;
                        if quiet(&w[0]) && quiet(&w[1]) {
                            let x = w[0].rect.right() + GAP * 0.5;
                            let y = w[0].rect.center().y;
                            ui.painter()
                                .line_segment([pos2(x, y - 8.0), pos2(x, y + 8.0)], line);
                        }
                    }

                    ui.add_space(FLARE);
                    let (rect, resp) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::click());
                    let p = ui.painter();
                    if resp.hovered() {
                        p.rect_filled(rect, 6, hover_fill);
                    }
                    let c = rect.center();
                    let s = Stroke::new(1.5, v.text_color());
                    p.line_segment([c - vec2(5.5, 0.0), c + vec2(5.5, 0.0)], s);
                    p.line_segment([c - vec2(0.0, 5.5), c + vec2(0.0, 5.5)], s);
                    if resp.on_hover_text("New tab (Ctrl+T)").clicked() {
                        new_tab = true;
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
        if new_tab {
            self.new_tab();
        }
    }
}
