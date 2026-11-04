//! The Shortcut Pad: a strip on the left or right edge of the window with touch keys and a
//! wheel, for working without a keyboard. Modelled on the ArtCraft Android set's pad.
//!
//! A key runs a host command (`edit.undo`, `file.save`, `file.print`) or an editor command
//! (`view.zoomIn`); the wheel turns two more. The settings window edits the keys, the side and
//! the wheel, and everything is saved in `shortcut-pad.json` in the app's files directory.

use std::path::{Path, PathBuf};

use eframe::egui;
use serde::{Deserialize, Serialize};

const MIN_KEYS: usize = 2;
const MAX_KEYS: usize = 10;
/// How far the wheel must be dragged before it runs one step.
const WHEEL_STEP: f32 = 18.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Side {
    Left,
    Right,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KeyConfig {
    pub label: String,
    pub action: String,
    pub color: [u8; 3],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WheelConfig {
    pub up: String,
    pub down: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PadConfig {
    pub visible: bool,
    pub side: Side,
    pub keys: Vec<KeyConfig>,
    pub wheel: WheelConfig,
}

impl Default for PadConfig {
    fn default() -> Self {
        Self {
            visible: true,
            side: Side::Right,
            keys: vec![
                KeyConfig { label: "Undo".into(), action: "edit.undo".into(), color: [70, 90, 120] },
                KeyConfig { label: "Redo".into(), action: "edit.redo".into(), color: [70, 90, 120] },
                KeyConfig { label: "Save".into(), action: "file.save".into(), color: [60, 110, 80] },
                KeyConfig { label: "Print".into(), action: "android.print".into(), color: [110, 90, 60] },
            ],
            wheel: WheelConfig { up: "view.zoomIn".into(), down: "view.zoomOut".into() },
        }
    }
}

pub struct Pad {
    config: PadConfig,
    path: PathBuf,
    settings_open: bool,
    /// The action a key or the wheel asked for, run on the next frame.
    pending: Option<String>,
    /// The wheel's drag since it last ran a step.
    wheel_drag: f32,
}

impl Pad {
    pub fn new(files_dir: &Path) -> Self {
        let path = files_dir.join("shortcut-pad.json");
        let config = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self { config, path, settings_open: false, pending: None, wheel_drag: 0.0 }
    }

    fn save(&self) {
        if let Ok(bytes) = serde_json::to_vec_pretty(&self.config) {
            let _ = std::fs::write(&self.path, bytes);
        }
    }

    /// The action a key or the wheel asked for, if any (the shell dispatches it).
    pub fn take_pending(&mut self) -> Option<String> {
        self.pending.take()
    }

    /// Draw the strip over the window's edge.
    pub fn show(&mut self, ctx: &egui::Context) {
        if !self.config.visible {
            return;
        }
        let (anchor, offset) = match self.config.side {
            Side::Left => (egui::Align2::LEFT_CENTER, egui::vec2(8.0, 0.0)),
            Side::Right => (egui::Align2::RIGHT_CENTER, egui::vec2(-8.0, 0.0)),
        };
        egui::Area::new(egui::Id::new("weave-shortcut-pad"))
            .anchor(anchor, offset)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(egui::Color32::from_black_alpha(190))
                    .corner_radius(12.0)
                    .inner_margin(6.0)
                    .show(ui, |ui| {
                        ui.vertical(|ui| {
                            for key in &self.config.keys {
                                let color = egui::Color32::from_rgb(key.color[0], key.color[1], key.color[2]);
                                let button = egui::Button::new(
                                    egui::RichText::new(&key.label).size(13.0).color(egui::Color32::WHITE),
                                )
                                .fill(color)
                                .min_size(egui::vec2(58.0, 44.0));
                                if ui.add(button).clicked() {
                                    self.pending = Some(key.action.clone());
                                }
                            }
                            ui.add_space(4.0);
                            self.wheel(ui);
                            ui.add_space(4.0);
                            if ui
                                .add(egui::Button::new(egui::RichText::new("⚙").size(16.0)).min_size(egui::vec2(58.0, 30.0)))
                                .clicked()
                            {
                                self.settings_open = true;
                            }
                        });
                    });
            });
        self.settings_window(ctx);
    }

    /// The wheel: drag up or down to run its two actions, one step per `WHEEL_STEP`.
    fn wheel(&mut self, ui: &mut egui::Ui) {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(58.0, 58.0), egui::Sense::click_and_drag());
        let painter = ui.painter();
        painter.circle_filled(rect.center(), 27.0, egui::Color32::from_rgb(48, 50, 58));
        painter.circle_stroke(rect.center(), 27.0, egui::Stroke::new(2.0, egui::Color32::from_gray(120)));
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "◎",
            egui::FontId::proportional(22.0),
            egui::Color32::from_gray(210),
        );
        if response.dragged() {
            self.wheel_drag += response.drag_delta().y;
            while self.wheel_drag <= -WHEEL_STEP {
                self.wheel_drag += WHEEL_STEP;
                self.pending = Some(self.config.wheel.up.clone());
            }
            while self.wheel_drag >= WHEEL_STEP {
                self.wheel_drag -= WHEEL_STEP;
                self.pending = Some(self.config.wheel.down.clone());
            }
        } else {
            self.wheel_drag = 0.0;
        }
    }

    fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            return;
        }
        let mut open = true;
        egui::Window::new("Shortcut Pad")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Side:");
                    ui.selectable_value(&mut self.config.side, Side::Left, "Left");
                    ui.selectable_value(&mut self.config.side, Side::Right, "Right");
                });
                ui.separator();
                ui.label("Keys:");
                let mut remove = None;
                let count = self.config.keys.len();
                for (index, key) in self.config.keys.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut key.label).desired_width(80.0));
                        ui.add(egui::TextEdit::singleline(&mut key.action).desired_width(170.0));
                        if count > MIN_KEYS && ui.button("−").clicked() {
                            remove = Some(index);
                        }
                    });
                }
                if let Some(index) = remove {
                    self.config.keys.remove(index);
                }
                if self.config.keys.len() < MAX_KEYS && ui.button("Add Key").clicked() {
                    self.config.keys.push(KeyConfig {
                        label: "Key".into(),
                        action: "edit.undo".into(),
                        color: [80, 80, 90],
                    });
                }
                ui.separator();
                ui.label("Wheel:");
                ui.horizontal(|ui| {
                    ui.label("Up:");
                    ui.add(egui::TextEdit::singleline(&mut self.config.wheel.up).desired_width(170.0));
                });
                ui.horizontal(|ui| {
                    ui.label("Down:");
                    ui.add(egui::TextEdit::singleline(&mut self.config.wheel.down).desired_width(170.0));
                });
                ui.separator();
                if ui.button("Restore Defaults").clicked() {
                    self.config = PadConfig::default();
                }
                if ui.button("Close").clicked() {
                    self.settings_open = false;
                }
            });
        if !open {
            self.settings_open = false;
        }
        self.save();
    }
}
