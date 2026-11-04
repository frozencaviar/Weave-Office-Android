//! The Shortcut Pad: a light strip on the window's edge with touch keys and a wheel, for
//! working without a keyboard. Modelled on the ArtCraft Android set's pad.
//!
//! The strip can be dragged to either edge (it snaps to the nearer one), folded away with its
//! arrow (a thin handle stays at the edge), and its keys run host commands (`edit.undo`,
//! `file.save`) or editor commands (`view.zoomIn`). A long press on the arrow opens the pad's
//! settings, which are saved in `shortcut-pad.json` in the app's files directory.
//!
//! The keys are compact 36 pt squares, 6 pt apart in 8 pt of padding, with Lucide-style 18 pt
//! line icons (1.75 pt stroke, round caps) and their names in a tooltip on hover.

use std::path::{Path, PathBuf};

use eframe::egui;
use serde::{Deserialize, Serialize};

const MIN_KEYS: usize = 2;
const MAX_KEYS: usize = 10;
/// How far the wheel must be turned before it runs one step (one twelfth of a turn).
const WHEEL_STEP: f32 = std::f32::consts::TAU / 12.0;

/// The keys: compact 36 pt squares, 6 pt apart, inside 8 pt of padding.
const KEY: f32 = 36.0;
const KEY_RADIUS: f32 = 9.0;
const KEY_GAP: f32 = 6.0;
const PAD_PAD: f32 = 8.0;
const PAD_W: f32 = KEY + PAD_PAD * 2.0;
/// Icons: 18 pt, drawn in a 24 pt design space with Lucide's 1.75 pt stroke.
const ICON: f32 = 18.0;
const ICON_STROKE: f32 = 1.75 * ICON / 24.0;
const ICON_COLOR: egui::Color32 = egui::Color32::from_rgb(0x4B, 0x55, 0x63);
const WHEEL_SIZE: f32 = 36.0;
const BAR_H: f32 = 20.0;
const COLLAPSED_W: f32 = 20.0;
const COLLAPSED_H: f32 = 88.0;
const EDGE: f32 = 8.0;
const WHEEL_GAP: f32 = 10.0;

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
        let count = self.config.keys.len() as f32;
        BAR_H
            + 4.0
            + count * KEY
            + (count - 1.0).max(0.0) * KEY_GAP
            + WHEEL_GAP
            + WHEEL_SIZE
            + PAD_PAD * 2.0
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
        let visuals = ui.visuals().clone();
        let painter = ui.painter();
        painter.add(
            egui::epaint::Shadow {
                offset: [0, 2],
                blur: 8,
                spread: 0,
                color: egui::Color32::from_black_alpha(24),
            }
            .as_shape(rect, 10.0),
        );
        painter.rect_filled(rect, 10.0, visuals.window_fill);
        // The arrow points away from the edge, the way the strip unfolds.
        let direction = match self.config.side {
            Side::Right => -1.0,
            Side::Left => 1.0,
        };
        let center = rect.center();
        painter.add(egui::Shape::line(
            vec![
                egui::pos2(center.x - direction * 2.0, center.y - 4.0),
                egui::pos2(center.x + direction * 2.0, center.y),
                egui::pos2(center.x - direction * 2.0, center.y + 4.0),
            ],
            egui::Stroke::new(1.4, visuals.weak_text_color()),
        ));
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
            .corner_radius(14.0)
            .inner_margin(PAD_PAD)
            .shadow(egui::epaint::Shadow {
                offset: [0, 2],
                blur: 10,
                spread: 0,
                color: egui::Color32::from_black_alpha(24),
            })
            .show(ui, |ui| {
                ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                    // The gaps below are exact: egui's own item spacing would add to them.
                    ui.spacing_mut().item_spacing.y = 0.0;
                    self.top_bar(ui, drag_delta, drag_stopped);
                    ui.add_space(4.0);
                    for (index, key) in self.config.keys.iter().enumerate() {
                        if index > 0 {
                            ui.add_space(KEY_GAP);
                        }
                        if key_button(ui, key) {
                            self.pending = Some(key.action.clone());
                        }
                    }
                    ui.add_space(WHEEL_GAP);
                    self.wheel(ui);
                });
            });
    }

    /// The strip's top bar: the fold arrow and a drag grip.
    fn top_bar(&mut self, ui: &mut egui::Ui, drag_delta: &mut egui::Vec2, drag_stopped: &mut bool) {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(KEY, BAR_H), egui::Sense::click_and_drag());
        let visuals = ui.visuals().clone();
        let painter = ui.painter();
        let weak = visuals.weak_text_color();
        let direction = match self.config.side {
            Side::Right => 1.0,
            Side::Left => -1.0,
        };
        // The fold arrow, pointing at the edge the strip folds into.
        let center = egui::pos2(rect.left() + 9.0, rect.center().y);
        painter.add(egui::Shape::line(
            vec![
                egui::pos2(center.x - direction * 1.5, center.y - 3.5),
                egui::pos2(center.x + direction * 2.0, center.y),
                egui::pos2(center.x - direction * 1.5, center.y + 3.5),
            ],
            egui::Stroke::new(1.4, weak),
        ));
        // The grip: two columns of three dots.
        for row in 0..3 {
            for column in 0..2 {
                painter.circle_filled(
                    egui::pos2(
                        rect.right() - 12.0 + column as f32 * 3.5,
                        center.y - 4.0 + row as f32 * 4.0,
                    ),
                    0.9,
                    weak,
                );
            }
        }
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
        if held {
            // Keep frames coming while the wheel is held, so its enlargement can animate.
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
        }
        let now = ui.input(|i| i.time);
        if held {
            self.wheel_held.get_or_insert(now);
        } else {
            self.wheel_held = None;
        }
        let long = self.wheel_held.is_some_and(|start| now - start > 0.25);
        let scale = ui.ctx().animate_value_with_time(egui::Id::new("weave-pad-wheel-scale"), if long { 1.18 } else { 1.0 }, 0.15);
        let center = rect.center();
        let radius = 17.0 * scale;
        let visuals = ui.visuals().clone();
        let painter = ui.painter();
        // A soft glow while the wheel is held.
        if scale > 1.02 {
            painter.circle_filled(center, radius + 6.0, egui::Color32::from_rgba_unmultiplied(59, 130, 246, 26));
        }
        // The dial: a light disc, a hairline ring, twelve ticks and a hub.
        painter.circle_filled(center, radius, visuals.widgets.inactive.bg_fill);
        painter.circle_stroke(center, radius, egui::Stroke::new(1.0, visuals.widgets.inactive.bg_stroke.color));
        for i in 0..12 {
            let angle = i as f32 * std::f32::consts::TAU / 12.0;
            let direction = egui::vec2(angle.cos(), angle.sin());
            painter.line_segment(
                [center + direction * (radius - 5.0), center + direction * (radius - 2.0)],
                egui::Stroke::new(1.0, visuals.weak_text_color()),
            );
        }
        painter.circle_filled(center, radius - 7.0, visuals.window_fill);
        painter.circle_stroke(center, radius - 7.0, egui::Stroke::new(1.0, visuals.widgets.inactive.bg_stroke.color));
        painter.circle_filled(center, 4.0 * scale, visuals.selection.bg_fill);
        painter.circle_filled(center, 1.5 * scale, visuals.window_fill);
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

/// One key: a compact square that only shows a fill while it is hovered or pressed, with a
/// Lucide-style line icon and its name in a tooltip to the right.
fn key_button(ui: &mut egui::Ui, key: &KeyConfig) -> bool {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(KEY, KEY), egui::Sense::click());
    let pressed = response.is_pointer_button_down_on();
    let hovered = response.hovered();
    if pressed {
        // Keep frames coming while the key is held, so the long-press tooltip can appear.
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
    }
    // A long press shows the name too, so the pad stays usable without a mouse.
    let now = ui.input(|i| i.time);
    let hold_id = ui.id().with(("weave-pad-hold", &key.label));
    let held_since = ui.ctx().memory_mut(|memory| {
        let start = memory.data.get_temp_mut_or_insert_with(hold_id, || now);
        if !pressed {
            *start = now;
        }
        *start
    });
    let long = pressed && now - held_since > 0.45;
    let visuals = ui.visuals().clone();
    let fill = key_fill(&visuals, hovered, pressed);
    let painter = ui.painter();
    if fill != egui::Color32::TRANSPARENT {
        painter.rect_filled(rect, KEY_RADIUS, fill);
    }
    let color = if pressed || hovered {
        icon_color(&visuals).gamma_multiply(0.72)
    } else {
        icon_color(&visuals)
    };
    paint_icon(
        painter,
        egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(ICON)),
        &key.action,
        color,
    );
    let tip = ui.ctx().animate_bool_with_time(ui.id().with(("weave-pad-tip", &key.label)), hovered || long, 0.12);
    if tip > 0.01 {
        paint_tooltip(ui, rect, &key.label, tip);
    }
    response.clicked()
}

/// The key's fill: nothing at rest, a light grey on hover, a darker one while pressed.
fn key_fill(visuals: &egui::Visuals, hovered: bool, pressed: bool) -> egui::Color32 {
    if pressed {
        if visuals.dark_mode {
            egui::Color32::from_rgb(0x37, 0x41, 0x51)
        } else {
            egui::Color32::from_rgb(0xE5, 0xE7, 0xEB)
        }
    } else if hovered {
        if visuals.dark_mode {
            egui::Color32::from_rgb(0x2B, 0x33, 0x40)
        } else {
            egui::Color32::from_rgb(0xF3, 0xF4, 0xF6)
        }
    } else {
        egui::Color32::TRANSPARENT
    }
}

/// The icon colour: the spec's #4B5563 in the light theme, a light grey in the dark one.
fn icon_color(visuals: &egui::Visuals) -> egui::Color32 {
    if visuals.dark_mode {
        egui::Color32::from_rgb(0xD1, 0xD5, 0xDB)
    } else {
        ICON_COLOR
    }
}

/// The key's name, in a light tooltip beside it (on the left when the strip is at the right edge).
fn paint_tooltip(ui: &egui::Ui, key: egui::Rect, label: &str, t: f32) {
    let visuals = ui.visuals();
    let dark = visuals.dark_mode;
    let text_color = if dark {
        egui::Color32::from_rgb(0xE5, 0xE7, 0xEB)
    } else {
        egui::Color32::from_rgb(0x37, 0x41, 0x51)
    };
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(11.0),
        text_color.gamma_multiply(t),
    );
    let padding = egui::vec2(8.0, 5.0);
    let size = galley.size() + padding * 2.0;
    let content = ui.ctx().content_rect();
    let mut x = key.right() + 8.0;
    if x + size.x > content.right() - 4.0 {
        x = key.left() - 8.0 - size.x;
    }
    let rect = egui::Rect::from_min_size(egui::pos2(x, key.center().y - size.y / 2.0), size);
    let background = if dark {
        egui::Color32::from_rgb(0x1F, 0x29, 0x37)
    } else {
        egui::Color32::WHITE
    };
    let border = if dark {
        egui::Color32::from_rgb(0x37, 0x41, 0x51)
    } else {
        egui::Color32::from_rgb(0xE5, 0xE7, 0xEB)
    };
    let painter = ui.painter();
    painter.add(
        egui::epaint::Shadow {
            offset: [0, 2],
            blur: 6,
            spread: 0,
            color: egui::Color32::from_black_alpha((28.0 * t) as u8),
        }
        .as_shape(rect, 6.0),
    );
    painter.rect_filled(rect, 6.0, background.gamma_multiply(t));
    painter.rect_stroke(rect, 6.0, egui::Stroke::new(1.0, border.gamma_multiply(t)), egui::StrokeKind::Inside);
    painter.galley(rect.min + padding, galley, text_color.gamma_multiply(t));
}

/// A Lucide-style glyph: geometry in a 24 × 24 design space, stroked with round caps and joins.
struct Glyph<'a> {
    painter: &'a egui::Painter,
    rect: egui::Rect,
    stroke: egui::Stroke,
}

impl Glyph<'_> {
    fn p(&self, x: f32, y: f32) -> egui::Pos2 {
        egui::pos2(
            self.rect.min.x + x * self.rect.width() / 24.0,
            self.rect.min.y + y * self.rect.height() / 24.0,
        )
    }

    /// Stroke a polyline, rounding its caps and joins with small discs.
    fn path(&self, points: &[(f32, f32)]) {
        let points: Vec<egui::Pos2> = points.iter().map(|&(x, y)| self.p(x, y)).collect();
        self.painter.add(egui::Shape::line(points.clone(), self.stroke));
        let radius = self.stroke.width / 2.0;
        for point in points {
            self.painter.circle_filled(point, radius, self.stroke.color);
        }
    }

    /// Points along a circular arc, in the design space.
    fn arc(&self, cx: f32, cy: f32, radius: f32, from: f32, to: f32) -> Vec<(f32, f32)> {
        let steps = 12;
        (0..=steps)
            .map(|i| {
                let angle = from + (to - from) * i as f32 / steps as f32;
                (cx + radius * angle.cos(), cy + radius * angle.sin())
            })
            .collect()
    }
}

/// Paint a key's icon: Lucide's undo-2, redo-2 and save, in an 18 pt box.
fn paint_icon(painter: &egui::Painter, rect: egui::Rect, action: &str, color: egui::Color32) {
    let glyph = Glyph { painter, rect, stroke: egui::Stroke::new(ICON_STROKE, color) };
    match action {
        "edit.undo" => {
            glyph.path(&[(9.0, 14.0), (4.0, 9.0), (9.0, 4.0)]);
            let mut curve = vec![(4.0, 9.0), (14.5, 9.0)];
            curve.extend(glyph.arc(14.5, 14.5, 5.5, -std::f32::consts::FRAC_PI_2, std::f32::consts::FRAC_PI_2));
            curve.push((11.0, 20.0));
            glyph.path(&curve);
        }
        "edit.redo" => {
            glyph.path(&[(15.0, 14.0), (20.0, 9.0), (15.0, 4.0)]);
            let mut curve = vec![(20.0, 9.0), (9.5, 9.0)];
            curve.extend(glyph.arc(9.5, 14.5, 5.5, -std::f32::consts::FRAC_PI_2, -3.0 * std::f32::consts::FRAC_PI_2));
            curve.push((13.0, 20.0));
            glyph.path(&curve);
        }
        "file.save" => {
            glyph.path(&[
                (5.0, 3.0),
                (15.2, 3.0),
                (21.0, 8.8),
                (21.0, 19.0),
                (19.0, 21.0),
                (5.0, 21.0),
                (3.0, 19.0),
                (3.0, 5.0),
                (5.0, 3.0),
            ]);
            glyph.path(&[(17.0, 21.0), (17.0, 14.0), (16.0, 13.0), (8.0, 13.0), (7.0, 14.0), (7.0, 21.0)]);
            glyph.path(&[(7.0, 3.0), (7.0, 7.0), (8.0, 8.0), (15.0, 8.0)]);
        }
        _ => {
            painter.circle_filled(rect.center(), 2.0, color);
        }
    }
}
