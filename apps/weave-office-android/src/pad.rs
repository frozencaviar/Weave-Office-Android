//! The Shortcut Pad: a light strip on the window's edge with touch keys and a wheel, for
//! working without a keyboard. Modelled on the ArtCraft Android set's pad.
//!
//! The strip can be dragged to either edge (it snaps to the nearer one), folded away with its
//! arrow (a thin handle stays at the edge), and its keys run host commands (`edit.undo`,
//! `file.save`) or editor commands (`view.zoomIn`). A long press on the arrow opens the pad's
//! settings, which are saved in `shortcut-pad.json` in the app's files directory.

use std::path::{Path, PathBuf};

use eframe::egui;
use serde::{Deserialize, Serialize};

const MIN_KEYS: usize = 2;
const MAX_KEYS: usize = 10;
/// How far the wheel must be turned before it runs one step (one twelfth of a turn).
const WHEEL_STEP: f32 = std::f32::consts::TAU / 12.0;
const KEY_W: f32 = 48.0;
const KEY_H: f32 = 56.0;
const KEY_RADIUS: f32 = 20.0;
const WHEEL_SIZE: f32 = 56.0;
const BAR_H: f32 = 26.0;
const COLLAPSED_W: f32 = 26.0;
const COLLAPSED_H: f32 = 120.0;
const PAD_W: f32 = KEY_W + 16.0;
const MARGIN: f32 = 8.0;
const EDGE: f32 = 8.0;
const KEY_GAP: f32 = 6.0;
const WHEEL_GAP: f32 = 12.0;

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
    pub collapsed: bool,
    pub keys: Vec<KeyConfig>,
    pub wheel: WheelConfig,
}

impl Default for PadConfig {
    fn default() -> Self {
        Self {
            visible: true,
            side: Side::Right,
            collapsed: false,
            keys: vec![
                KeyConfig { label: "Undo".into(), action: "edit.undo".into(), color: [70, 90, 120] },
                KeyConfig { label: "Redo".into(), action: "edit.redo".into(), color: [70, 90, 120] },
                KeyConfig { label: "Save".into(), action: "file.save".into(), color: [60, 110, 80] },
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
    /// The angle the wheel was last dragged to, while it is being turned.
    wheel_angle: Option<f32>,
    /// The angle turned since the wheel last ran a step.
    wheel_turn: f32,
    /// When the wheel was pressed, for its long-press enlargement.
    wheel_held: Option<f64>,
    /// The strip's size as of the last frame, so the next frame can place it against the edge.
    last_size: Option<egui::Vec2>,
    /// How far the strip has been dragged from its edge position.
    drag_offset: egui::Vec2,
}

impl Pad {
    pub fn new(files_dir: &Path) -> Self {
        let path = files_dir.join("shortcut-pad.json");
        let config = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self {
            config,
            path,
            settings_open: false,
            pending: None,
            wheel_angle: None,
            wheel_turn: 0.0,
            wheel_held: None,
            last_size: None,
            drag_offset: egui::Vec2::ZERO,
        }
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
        let content = ctx.content_rect();
        let size = if self.config.collapsed {
            egui::vec2(COLLAPSED_W, COLLAPSED_H)
        } else {
            self.last_size.unwrap_or(egui::vec2(PAD_W, self.estimated_height()))
        };
        let pos = self.position(content, size) + self.drag_offset;
        let mut drag_delta = egui::Vec2::ZERO;
        let mut drag_stopped = false;
        let response = egui::Area::new(egui::Id::new("weave-shortcut-pad"))
            .fixed_pos(pos)
            // The strip is placed against the window's edge by hand; egui's own constraint
            // would move it back inside the content rect on the first (sizing) frame.
            .constrain(false)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                if self.config.collapsed {
                    self.collapsed_handle(ui, &mut drag_delta, &mut drag_stopped);
                } else {
                    self.expanded(ui, &mut drag_delta, &mut drag_stopped);
                }
            });
        self.last_size = Some(response.response.rect.size());
        self.drag_offset += drag_delta;
        if drag_stopped {
            // Snap to the nearer edge, judged by where the strip ended up.
            let x = response.response.rect.center().x;
            self.config.side = if x < content.center().x { Side::Left } else { Side::Right };
            self.drag_offset = egui::Vec2::ZERO;
            self.save();
        }
        self.settings_window(ctx);
    }

    fn estimated_height(&self) -> f32 {
        BAR_H
            + self.config.keys.len() as f32 * (KEY_H + KEY_GAP)
            + WHEEL_GAP
            + WHEEL_SIZE
            + MARGIN * 2.0
            + 8.0
    }

    fn position(&self, content: egui::Rect, size: egui::Vec2) -> egui::Pos2 {
        let x = match self.config.side {
            Side::Left => content.left() + EDGE,
            Side::Right => content.right() - EDGE - size.x,
        };
        egui::pos2(x, content.center().y - size.y / 2.0)
    }

    /// The folded strip: a thin handle with an arrow that unfolds it.
    fn collapsed_handle(&mut self, ui: &mut egui::Ui, drag_delta: &mut egui::Vec2, drag_stopped: &mut bool) {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(COLLAPSED_W, COLLAPSED_H), egui::Sense::click_and_drag());
        let visuals = ui.visuals();
        let painter = ui.painter();
        let shadow = egui::epaint::Shadow { offset: [0, 2], blur: 8, spread: 0, color: egui::Color32::from_black_alpha(36) };
        painter.add(shadow.as_shape(rect, 12.0));
        painter.rect_filled(rect, 12.0, visuals.window_fill);
        painter.rect_stroke(rect, 12.0, visuals.window_stroke, egui::StrokeKind::Inside);
        let arrow = match self.config.side {
            Side::Right => "◀",
            Side::Left => "▶",
        };
        painter.text(rect.center(), egui::Align2::CENTER_CENTER, arrow, egui::FontId::proportional(14.0), visuals.text_color());
        if response.clicked() {
            self.config.collapsed = false;
            self.save();
        }
        if response.long_touched() {
            self.settings_open = true;
        }
        if response.dragged() {
            *drag_delta += response.drag_delta();
        }
        if response.drag_stopped() {
            *drag_stopped = true;
        }
    }

    fn expanded(&mut self, ui: &mut egui::Ui, drag_delta: &mut egui::Vec2, drag_stopped: &mut bool) {
        let visuals = ui.visuals().clone();
        egui::Frame::new()
            .fill(visuals.window_fill)
            .stroke(visuals.window_stroke)
            .corner_radius(16.0)
            .inner_margin(MARGIN)
            .shadow(egui::epaint::Shadow { offset: [0, 3], blur: 14, spread: 0, color: egui::Color32::from_black_alpha(40) })
            .show(ui, |ui| {
                ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                    self.top_bar(ui, drag_delta, drag_stopped);
                    ui.add_space(2.0);
                    for key in &self.config.keys {
                        if key_button(ui, key) {
                            self.pending = Some(key.action.clone());
                        }
                        ui.add_space(KEY_GAP);
                    }
                    ui.add_space(WHEEL_GAP - KEY_GAP);
                    self.wheel(ui);
                });
            });
    }

    /// The strip's top bar: the fold arrow and a drag grip.
    fn top_bar(&mut self, ui: &mut egui::Ui, drag_delta: &mut egui::Vec2, drag_stopped: &mut bool) {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(KEY_W, BAR_H), egui::Sense::click_and_drag());
        let visuals = ui.visuals();
        let painter = ui.painter();
        let arrow = match self.config.side {
            Side::Right => "▶",
            Side::Left => "◀",
        };
        painter.text(
            egui::pos2(rect.left() + 10.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            arrow,
            egui::FontId::proportional(12.0),
            visuals.weak_text_color(),
        );
        painter.text(
            egui::pos2(rect.right() - 10.0, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            "⋮⋮",
            egui::FontId::proportional(12.0),
            visuals.weak_text_color(),
        );
        if response.clicked() {
            self.config.collapsed = true;
            self.save();
        }
        if response.long_touched() {
            self.settings_open = true;
        }
        if response.dragged() {
            *drag_delta += response.drag_delta();
        }
        if response.drag_stopped() {
            *drag_stopped = true;
        }
    }

    /// The wheel: a dial with ticks and a hub. Turning it runs its two actions, one step per
    /// twelfth of a turn; holding it enlarges it.
    fn wheel(&mut self, ui: &mut egui::Ui) {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(WHEEL_SIZE, WHEEL_SIZE), egui::Sense::click_and_drag());
        let held = response.is_pointer_button_down_on();
        let now = ui.input(|i| i.time);
        if held {
            self.wheel_held.get_or_insert(now);
        } else {
            self.wheel_held = None;
        }
        let long = self.wheel_held.is_some_and(|start| now - start > 0.25);
        let scale = ui.ctx().animate_value_with_time(egui::Id::new("weave-pad-wheel-scale"), if long { 1.18 } else { 1.0 }, 0.15);
        let center = rect.center();
        let radius = 30.0 * scale;
        let visuals = ui.visuals();
        let painter = ui.painter();
        // A soft glow while the wheel is held.
        if scale > 1.02 {
            painter.circle_filled(center, radius + 7.0, egui::Color32::from_white_alpha(36));
        }
        // The dial: a light disc, a ring, twelve ticks and a hub.
        painter.circle_filled(center, radius, visuals.widgets.inactive.bg_fill);
        painter.circle_stroke(center, radius, egui::Stroke::new(2.0, visuals.widgets.inactive.bg_stroke.color));
        for i in 0..12 {
            let angle = i as f32 * std::f32::consts::TAU / 12.0;
            let direction = egui::vec2(angle.cos(), angle.sin());
            painter.line_segment(
                [center + direction * (radius - 7.0), center + direction * (radius - 3.0)],
                egui::Stroke::new(1.5, visuals.weak_text_color()),
            );
        }
        painter.circle_filled(center, radius - 10.0, visuals.window_fill);
        painter.circle_stroke(center, radius - 10.0, egui::Stroke::new(1.0, visuals.widgets.inactive.bg_stroke.color));
        painter.circle_filled(center, 6.0 * scale, visuals.selection.bg_fill);
        painter.circle_filled(center, 2.0 * scale, visuals.window_fill);
        // Turning: the angle from the hub to the pointer, one step per twelfth of a turn.
        if response.dragged() {
            if let Some(pointer) = response.interact_pointer_pos() {
                let angle = (pointer.y - center.y).atan2(pointer.x - center.x);
                if let Some(last) = self.wheel_angle {
                    let mut delta = angle - last;
                    while delta > std::f32::consts::PI {
                        delta -= std::f32::consts::TAU;
                    }
                    while delta < -std::f32::consts::PI {
                        delta += std::f32::consts::TAU;
                    }
                    self.wheel_turn += delta;
                    while self.wheel_turn >= WHEEL_STEP {
                        self.wheel_turn -= WHEEL_STEP;
                        self.pending = Some(self.config.wheel.down.clone());
                    }
                    while self.wheel_turn <= -WHEEL_STEP {
                        self.wheel_turn += WHEEL_STEP;
                        self.pending = Some(self.config.wheel.up.clone());
                    }
                }
                self.wheel_angle = Some(angle);
            }
        } else {
            self.wheel_angle = None;
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

/// One key: a round button with an icon, its label underneath, growing a little while it is
/// pressed.
fn key_button(ui: &mut egui::Ui, key: &KeyConfig) -> bool {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(KEY_W, KEY_H), egui::Sense::click());
    let pressed = response.is_pointer_button_down_on();
    let grow = ui.ctx().animate_bool_with_time(ui.id().with(&key.label), pressed, 0.08);
    let visuals = ui.visuals();
    let painter = ui.painter();
    let radius = KEY_RADIUS * (1.0 + 0.08 * grow);
    let center = egui::pos2(rect.center().x, rect.top() + KEY_RADIUS);
    let circle = egui::Rect::from_center_size(center, egui::Vec2::splat(radius * 2.0));
    let shadow = egui::epaint::Shadow { offset: [0, 2], blur: 8, spread: 0, color: egui::Color32::from_black_alpha(30) };
    painter.add(shadow.as_shape(circle, radius));
    let fill = if pressed { visuals.widgets.active.bg_fill } else { visuals.widgets.inactive.bg_fill };
    painter.circle_filled(center, radius, fill);
    painter.circle_stroke(center, radius, egui::Stroke::new(1.0, visuals.widgets.inactive.bg_stroke.color));
    let icon = match key.action.as_str() {
        "edit.undo" => weave_office::icons::Icon::Undo,
        "edit.redo" => weave_office::icons::Icon::Redo,
        "file.save" => weave_office::icons::Icon::Save,
        _ => weave_office::icons::Icon::More,
    };
    let icon_rect = egui::Rect::from_center_size(center, egui::Vec2::splat(20.0));
    weave_office::icons::paint(painter, icon_rect, icon, visuals.text_color());
    painter.text(
        egui::pos2(rect.center().x, rect.bottom() - 6.0),
        egui::Align2::CENTER_CENTER,
        &key.label,
        egui::FontId::proportional(10.0),
        visuals.weak_text_color(),
    );
    response.clicked()
}
