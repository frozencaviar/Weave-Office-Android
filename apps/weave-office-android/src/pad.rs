//! The Shortcut Pad: a light strip on the window's edge with touch keys and a dial, for
//! working without a keyboard. Modelled on the ArtCraft Android set's pad.
//!
//! The strip can be dragged to either edge (it snaps to the nearer one), folded away with its
//! arrow, and its keys run host commands (`edit.undo`, `file.save`) or editor commands
//! (`view.zoomIn`). A long press on the arrow opens the pad's settings, which are saved in
//! `shortcut-pad.json` in the app's files directory.
//!
//! The keys are compact 36 pt squares, 6 pt apart in 8 pt of padding, with Lucide-style 18 pt
//! line icons (1.75 pt stroke, round caps) and their names in a tooltip on hover. The wheel is
//! a minimal 36 pt ring; pressing it opens a 172 pt dial anchored at the strip's edge, with an
//! orange progress arc and the current zoom in its middle.

use std::path::{Path, PathBuf};

use eframe::egui;
use serde::{Deserialize, Serialize};

const MIN_KEYS: usize = 2;
const MAX_KEYS: usize = 10;
/// How far the dial must be turned before it runs one step (one twelfth of a turn).
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
const EDGE: f32 = 8.0;
const WHEEL_GAP: f32 = 10.0;

/// The dial that opens from the wheel: 172 pt across, anchored at the strip's edge.
const DIAL: f32 = 172.0;
const DIAL_TICKS: usize = 36;
const ACCENT: egui::Color32 = egui::Color32::from_rgb(0xF5, 0x9E, 0x0B);
const DIAL_LABEL: egui::Color32 = egui::Color32::from_rgb(0x6B, 0x72, 0x80);
const DIAL_VALUE: egui::Color32 = egui::Color32::from_rgb(0x11, 0x18, 0x27);
const RING: egui::Color32 = egui::Color32::from_rgb(0xE5, 0xE7, 0xEB);
const TICK: egui::Color32 = egui::Color32::from_rgb(0x9C, 0xA3, 0xAF);

/// The fold and dial animations: ease-out-quint, and a slight overshoot for the dial.
const FOLD_CURVE: [f32; 4] = [0.22, 1.0, 0.36, 1.0];
const DIAL_CURVE: [f32; 4] = [0.34, 1.56, 0.64, 1.0];
const FOLD_TIME: f32 = 0.3;
const DIAL_TIME: f32 = 0.28;

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

/// A time-based animation with a cubic-bezier curve.
struct Anim {
    value: f32,
    from: f32,
    to: f32,
    start: f64,
    duration: f32,
    curve: [f32; 4],
}

impl Anim {
    fn new(value: f32) -> Self {
        Self { value, from: value, to: value, start: 0.0, duration: 0.0, curve: FOLD_CURVE }
    }

    fn set(&mut self, target: f32, now: f64, duration: f32, curve: [f32; 4]) {
        if (target - self.to).abs() > 1e-4 {
            self.from = self.value;
            self.to = target;
            self.start = now;
            self.duration = duration;
            self.curve = curve;
        }
    }

    fn update(&mut self, now: f64) -> f32 {
        if self.duration <= 0.0 {
            self.value = self.to;
            return self.value;
        }
        let t = (((now - self.start) / self.duration as f64) as f32).clamp(0.0, 1.0);
        self.value = self.from + (self.to - self.from) * bezier(self.curve, t);
        self.value
    }

    fn running(&self, now: f64) -> bool {
        self.duration > 0.0 && now < self.start + self.duration as f64
    }
}

/// A CSS-style cubic-bezier easing: solve x(u) = t, then return y(u).
fn bezier(curve: [f32; 4], t: f32) -> f32 {
    let [x1, y1, x2, y2] = curve;
    let mut u = t;
    for _ in 0..6 {
        let x = bezier_axis(x1, x2, u) - t;
        if x.abs() < 1e-4 {
            break;
        }
        let dx = bezier_derivative(x1, x2, u);
        if dx.abs() < 1e-6 {
            break;
        }
        u = (u - x / dx).clamp(0.0, 1.0);
    }
    bezier_axis(y1, y2, u)
}

fn bezier_axis(a: f32, b: f32, u: f32) -> f32 {
    let v = 1.0 - u;
    3.0 * v * v * u * a + 3.0 * v * u * u * b + u * u * u
}

fn bezier_derivative(a: f32, b: f32, u: f32) -> f32 {
    let v = 1.0 - u;
    3.0 * v * v * a + 6.0 * v * u * (b - a) + 3.0 * u * u * (1.0 - b)
}

/// Where the strip's parts sit for a given card rect.
struct Layout {
    card: egui::Rect,
    header: egui::Rect,
    keys: Vec<egui::Rect>,
    wheel: egui::Rect,
}

impl Layout {
    fn new(card: egui::Rect, count: usize) -> Self {
        let header = egui::Rect::from_min_size(
            egui::pos2(card.center().x - KEY / 2.0, card.top() + PAD_PAD),
            egui::vec2(KEY, BAR_H),
        );
        let mut keys = Vec::with_capacity(count);
        let mut y = header.bottom() + 4.0;
        for _ in 0..count {
            keys.push(egui::Rect::from_min_size(
                egui::pos2(card.center().x - KEY / 2.0, y),
                egui::Vec2::splat(KEY),
            ));
            y += KEY + KEY_GAP;
        }
        let wheel = egui::Rect::from_min_size(
            egui::pos2(card.center().x - WHEEL_SIZE / 2.0, y - KEY_GAP + WHEEL_GAP),
            egui::Vec2::splat(WHEEL_SIZE),
        );
        Self { card, header, keys, wheel }
    }
}

pub struct Pad {
    config: PadConfig,
    path: PathBuf,
    settings_open: bool,
    /// The action a key or the dial asked for, with its parameters, run on the next frame.
    pending: Option<(String, serde_json::Value)>,
    /// The pointer's last angle while the dial is being turned.
    wheel_angle: Option<f32>,
    /// The rotation since the dial opened, for its arc and ticks.
    wheel_turn: f32,
    /// The rotation since the last step, which runs the wheel's actions.
    wheel_step: f32,
    /// The zoom the dial started from: a zoom wheel then follows the turn continuously.
    dial_base_zoom: Option<f32>,
    /// The zoom last asked for, so the dial only sends a command when it moves.
    last_zoom: f32,
    /// The wheel is held down.
    wheel_pressed: bool,
    /// The dial stays open after a tap.
    wheel_pinned: bool,
    /// 0 = expanded, 1 = folded.
    fold: Anim,
    /// 0 = the wheel, 1 = the dial.
    dial: Anim,
    /// How far the strip has been dragged from its edge position.
    drag_offset: egui::Vec2,
}

impl Pad {
    pub fn new(files_dir: &Path) -> Self {
        let path = files_dir.join("shortcut-pad.json");
        let config: PadConfig = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let collapsed = config.collapsed;
        Self {
            config,
            path,
            settings_open: false,
            pending: None,
            wheel_angle: None,
            wheel_turn: 0.0,
            wheel_step: 0.0,
            dial_base_zoom: None,
            last_zoom: 100.0,
            wheel_pressed: false,
            wheel_pinned: false,
            fold: Anim::new(if collapsed { 1.0 } else { 0.0 }),
            dial: Anim::new(0.0),
            drag_offset: egui::Vec2::ZERO,
        }
    }

    fn save(&self) {
        if let Ok(bytes) = serde_json::to_vec_pretty(&self.config) {
            let _ = std::fs::write(&self.path, bytes);
        }
    }

    /// The action a key or the dial asked for, with its parameters, if any (the shell
    /// dispatches it).
    pub fn take_pending(&mut self) -> Option<(String, serde_json::Value)> {
        self.pending.take()
    }

    /// Draw the strip over the window's edge. `zoom` is the active document's zoom in percent,
    /// shown in the dial's middle.
    pub fn show(&mut self, ctx: &egui::Context, zoom: Option<f32>) {
        if !self.config.visible {
            return;
        }
        let now = ctx.input(|i| i.time);
        let content = ctx.content_rect();
        self.fold.set(if self.config.collapsed { 1.0 } else { 0.0 }, now, FOLD_TIME, FOLD_CURVE);
        let fold = self.fold.update(now);
        let active = self.wheel_pinned || self.wheel_pressed;
        self.dial.set(if active { 1.0 } else { 0.0 }, now, DIAL_TIME, DIAL_CURVE);
        let dial_t = self.dial.update(now);
        if active && self.dial_base_zoom.is_none() {
            // A zoom wheel follows the turn from the zoom the dial opened at.
            self.dial_base_zoom = zoom;
            self.last_zoom = zoom.unwrap_or(100.0);
        }
        if !active && dial_t < 0.01 {
            self.wheel_turn = 0.0;
            self.wheel_step = 0.0;
            self.wheel_angle = None;
            self.dial_base_zoom = None;
        }

        let expanded_h = self.estimated_height();
        let collapsed_h = BAR_H + PAD_PAD * 2.0;
        let height = expanded_h + (collapsed_h - expanded_h) * fold;
        let size = egui::vec2(PAD_W, height);
        let pos = self.position(content, size) + self.drag_offset;
        let card = egui::Rect::from_min_size(pos, size);
        let layout = Layout::new(card, self.config.keys.len());
        let dial_rect = self.dial_rect(&layout, dial_t);

        // A press outside the dial and the strip closes it.
        if self.wheel_pinned && ctx.input(|i| i.pointer.any_pressed()) {
            if let Some(pointer) = ctx.input(|i| i.pointer.interact_pos()) {
                if !dial_rect.contains(pointer) && !card.contains(pointer) {
                    self.wheel_pinned = false;
                }
            }
        }

        let mut drag_delta = egui::Vec2::ZERO;
        let mut drag_stopped = false;
        let response = egui::Area::new(egui::Id::new("weave-shortcut-pad"))
            .fixed_pos(pos)
            // The strip is placed against the window's edge by hand; egui's own constraint
            // would move it back inside the content rect on the first (sizing) frame.
            .constrain(false)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                ui.set_min_size(size);
                self.draw_card(ui, &layout);
                self.draw_header(ui, &layout, fold, &mut drag_delta, &mut drag_stopped);
                let (wheel_response, tooltips) = self.draw_content(ui, &layout, fold);
                for (rect, label, tip) in tooltips {
                    if tip > 0.01 && fold < 0.01 {
                        paint_tooltip(ui, rect, &label, tip);
                    }
                }
                let dial_response = if dial_t > 0.05 {
                    let response = ui.interact(
                        dial_rect,
                        ui.scope_id().with("weave-pad-dial"),
                        egui::Sense::click_and_drag(),
                    );
                    self.draw_dial(ui, &layout, dial_t, zoom);
                    Some(response)
                } else {
                    None
                };
                // Turning: the pointer's angle around the dial's middle.
                let dragged = dial_response.as_ref().is_some_and(|r| r.dragged()) || wheel_response.dragged();
                let pointer = dial_response
                    .as_ref()
                    .and_then(|r| r.interact_pointer_pos())
                    .or_else(|| wheel_response.interact_pointer_pos());
                if dragged {
                    if let Some(pointer) = pointer {
                        let center = if dial_t > 0.5 { dial_rect.center() } else { layout.wheel.center() };
                        self.turn(pointer, center);
                    }
                } else {
                    self.wheel_angle = None;
                }
                // A tap on the wheel opens the dial; a tap on the dial closes it.
                if wheel_response.clicked() {
                    self.wheel_pinned = !self.wheel_pinned;
                }
                if dial_response.as_ref().is_some_and(|r| r.clicked()) {
                    self.wheel_pinned = false;
                }
                self.wheel_pressed = wheel_response.is_pointer_button_down_on();
            });
        self.drag_offset += drag_delta;
        if drag_stopped {
            // Snap to the nearer edge, judged by where the strip ended up.
            let x = response.response.rect.center().x;
            self.config.side = if x < content.center().x { Side::Left } else { Side::Right };
            self.drag_offset = egui::Vec2::ZERO;
            self.save();
        }
        if self.fold.running(now) || self.dial.running(now) {
            ctx.request_repaint();
        }
        self.settings_window(ctx);
    }

    fn estimated_height(&self) -> f32 {
        let count = self.config.keys.len() as f32;
        PAD_PAD
            + BAR_H
            + 4.0
            + count * KEY
            + (count - 1.0).max(0.0) * KEY_GAP
            + WHEEL_GAP
            + WHEEL_SIZE
            + PAD_PAD
    }

    fn position(&self, content: egui::Rect, size: egui::Vec2) -> egui::Pos2 {
        let x = match self.config.side {
            Side::Left => content.left() + EDGE,
            Side::Right => content.right() - EDGE - size.x,
        };
        egui::pos2(x, content.center().y - size.y / 2.0)
    }

    /// The dial's bounding box: anchored at the strip's edge, centred on the wheel.
    fn dial_rect(&self, layout: &Layout, t: f32) -> egui::Rect {
        let radius = DIAL / 2.0 * t;
        let center = match self.config.side {
            Side::Left => egui::pos2(layout.card.left() + radius, layout.wheel.center().y),
            Side::Right => egui::pos2(layout.card.right() - radius, layout.wheel.center().y),
        };
        egui::Rect::from_center_size(center, egui::Vec2::splat(radius * 2.0))
    }

    fn draw_card(&self, ui: &egui::Ui, layout: &Layout) {
        let visuals = ui.visuals();
        let painter = ui.painter();
        painter.add(
            egui::epaint::Shadow {
                offset: [0, 2],
                blur: 10,
                spread: 0,
                color: egui::Color32::from_black_alpha(24),
            }
            .as_shape(layout.card, 14.0),
        );
        painter.rect_filled(layout.card, 14.0, visuals.window_fill);
    }

    /// The strip's top bar: the fold arrow (which turns as the strip folds) and a drag grip.
    fn draw_header(
        &mut self,
        ui: &mut egui::Ui,
        layout: &Layout,
        fold: f32,
        drag_delta: &mut egui::Vec2,
        drag_stopped: &mut bool,
    ) {
        let response = ui.interact(layout.header, ui.scope_id().with("weave-pad-header"), egui::Sense::click_and_drag());
        let visuals = ui.visuals();
        let painter = ui.painter();
        let weak = visuals.weak_text_color();
        let direction = match self.config.side {
            Side::Right => 1.0,
            Side::Left => -1.0,
        };
        let center = layout.header.center();
        let angle = std::f32::consts::PI * fold;
        let rotate = |point: egui::Pos2| center + egui::emath::Rot2::from_angle(angle) * (point - center);
        let chevron = [
            egui::pos2(center.x - direction * 1.5, center.y - 3.5),
            egui::pos2(center.x + direction * 2.0, center.y),
            egui::pos2(center.x - direction * 1.5, center.y + 3.5),
        ];
        painter.add(egui::Shape::line(
            chevron.iter().map(|&point| rotate(point)).collect(),
            egui::Stroke::new(1.4, weak),
        ));
        // The grip: two columns of three dots.
        for row in 0..3 {
            for column in 0..2 {
                painter.circle_filled(
                    egui::pos2(
                        layout.header.right() - 12.0 + column as f32 * 3.5,
                        center.y - 4.0 + row as f32 * 4.0,
                    ),
                    0.9,
                    weak,
                );
            }
        }
        if response.clicked() {
            self.config.collapsed = !self.config.collapsed;
            self.wheel_pinned = false;
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

    /// The keys and the wheel, clipped to the card and faded as the strip folds.
    fn draw_content(
        &mut self,
        ui: &mut egui::Ui,
        layout: &Layout,
        fold: f32,
    ) -> (egui::Response, Vec<(egui::Rect, String, f32)>) {
        let top = layout.header.bottom() + 2.0;
        let bottom = (layout.card.bottom() - PAD_PAD).max(top);
        let clip = egui::Rect::from_min_max(
            egui::pos2(layout.card.left() + PAD_PAD, top),
            egui::pos2(layout.card.right() - PAD_PAD, bottom),
        );
        let opacity = 1.0 - fold;
        let scale = 1.0 - 0.05 * fold;
        let lift = egui::vec2(0.0, -8.0 * fold);
        ui.scope(|ui| {
            ui.multiply_opacity(opacity);
            ui.set_clip_rect(clip);
            let mut tooltips = Vec::new();
            for (index, key) in self.config.keys.iter().enumerate() {
                let rect = shrink(layout.keys[index], scale).translate(lift);
                let response = ui.interact(rect, ui.scope_id().with(("weave-pad-key", index)), egui::Sense::click());
                let pressed = response.is_pointer_button_down_on();
                let hovered = response.hovered();
                let fill = key_fill(ui.visuals(), hovered, pressed);
                let painter = ui.painter();
                if fill != egui::Color32::TRANSPARENT {
                    painter.rect_filled(rect, KEY_RADIUS, fill);
                }
                let color = if pressed || hovered {
                    icon_color(ui.visuals()).gamma_multiply(0.72)
                } else {
                    icon_color(ui.visuals())
                };
                paint_icon(
                    painter,
                    egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(ICON * scale)),
                    &key.action,
                    color,
                );
                let tip = ui.ctx().animate_bool_with_time(
                    ui.scope_id().with(("weave-pad-tip", &key.label)),
                    hovered,
                    0.12,
                );
                if tip > 0.01 {
                    tooltips.push((rect, key.label.clone(), tip));
                }
                if response.clicked() {
                    self.pending = Some((key.action.clone(), serde_json::json!({})));
                }
            }
            // The wheel: a minimal ring until it is pressed.
            let wheel_rect = shrink(layout.wheel, scale).translate(lift);
            let response = ui.interact(wheel_rect, ui.scope_id().with("weave-pad-wheel"), egui::Sense::click_and_drag());
            let pressed = response.is_pointer_button_down_on();
            let hovered = response.hovered();
            let fill = key_fill(ui.visuals(), hovered, pressed);
            let painter = ui.painter();
            if fill != egui::Color32::TRANSPARENT {
                painter.circle_filled(wheel_rect.center(), WHEEL_SIZE / 2.0 * scale, fill);
            }
            draw_wheel(painter, wheel_rect, scale);
            (response, tooltips)
        })
        .inner
    }

    /// The dial: a frosted white disc with an orange ring, rotating ticks, a progress arc and
    /// the current zoom in its middle.
    fn draw_dial(&self, ui: &egui::Ui, layout: &Layout, t: f32, zoom: Option<f32>) {
        let radius = DIAL / 2.0 * t;
        let center = match self.config.side {
            Side::Left => egui::pos2(layout.card.left() + radius, layout.wheel.center().y),
            Side::Right => egui::pos2(layout.card.right() - radius, layout.wheel.center().y),
        };
        let rect = egui::Rect::from_center_size(center, egui::Vec2::splat(radius * 2.0));
        let alpha = t.clamp(0.0, 1.0);
        let fade = |color: egui::Color32| color.gamma_multiply(alpha);
        let painter = ui.painter();
        painter.add(
            egui::epaint::Shadow {
                offset: [0, 8],
                blur: 24,
                spread: 0,
                color: egui::Color32::from_black_alpha((30.0 * alpha) as u8),
            }
            .as_shape(rect, radius),
        );
        painter.circle_filled(
            center,
            radius,
            egui::Color32::from_rgba_unmultiplied(255, 255, 255, (242.0 * alpha) as u8),
        );
        painter.circle_stroke(center, radius - 0.75, egui::Stroke::new(1.5, fade(ACCENT)));
        for i in 0..DIAL_TICKS {
            let angle = i as f32 * std::f32::consts::TAU / DIAL_TICKS as f32 + self.wheel_turn;
            let direction = egui::vec2(angle.cos(), angle.sin());
            painter.line_segment(
                [center + direction * (radius - 14.0), center + direction * (radius - 6.0)],
                egui::Stroke::new(1.0, fade(ACCENT.gamma_multiply(0.55))),
            );
        }
        // The progress arc: from the top, as far as the dial has been turned.
        let sweep = self.wheel_turn.clamp(-std::f32::consts::TAU, std::f32::consts::TAU);
        if sweep.abs() > 0.02 {
            let start = -std::f32::consts::FRAC_PI_2;
            let steps = 48;
            let points: Vec<egui::Pos2> = (0..=steps)
                .map(|i| {
                    let angle = start + sweep * i as f32 / steps as f32;
                    center + egui::vec2(angle.cos(), angle.sin()) * (radius - 0.75)
                })
                .collect();
            painter.add(egui::Shape::line(points, egui::Stroke::new(3.0, fade(ACCENT))));
            let end = start + sweep;
            let dot = center + egui::vec2(end.cos(), end.sin()) * (radius - 0.75);
            painter.circle_filled(dot, 5.0, fade(ACCENT));
            painter.circle_filled(dot, 2.0, fade(egui::Color32::WHITE));
        }
        painter.text(
            center + egui::vec2(0.0, -10.0),
            egui::Align2::CENTER_CENTER,
            self.wheel_label(),
            egui::FontId::proportional(12.0),
            fade(DIAL_LABEL),
        );
        let value = zoom.map(|zoom| format!("{zoom:.0}%")).unwrap_or_else(|| "—".to_owned());
        painter.text(
            center + egui::vec2(0.0, 10.0),
            egui::Align2::CENTER_CENTER,
            value,
            egui::FontId::proportional(17.0),
            fade(DIAL_VALUE),
        );
    }

    /// The dial's name: "Zoom" when its actions zoom, "Dial" otherwise.
    fn wheel_label(&self) -> &'static str {
        let up = self.config.wheel.up.as_str();
        let down = self.config.wheel.down.as_str();
        if up.contains("zoom") || down.contains("zoom") {
            "Zoom"
        } else {
            "Dial"
        }
    }

    /// Turn the dial: the pointer's angle around its middle. A zoom wheel follows the turn
    /// continuously (a full turn doubles the zoom, like spreading two fingers); any other wheel
    /// runs one step per twelfth of a turn.
    fn turn(&mut self, pointer: egui::Pos2, center: egui::Pos2) {
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
            if let Some(base) = self.dial_base_zoom {
                let target = (base * (self.wheel_turn / std::f32::consts::TAU).exp2()).clamp(10.0, 500.0);
                if (target - self.last_zoom).abs() > 0.5 {
                    self.last_zoom = target;
                    self.pending = Some((
                        "view.zoom".into(),
                        serde_json::json!({"value": target, "percent": target}),
                    ));
                }
            } else {
                self.wheel_step += delta;
                // Clockwise turns up (zoom in), counter-clockwise down.
                while self.wheel_step >= WHEEL_STEP {
                    self.wheel_step -= WHEEL_STEP;
                    self.pending = Some((self.config.wheel.up.clone(), serde_json::json!({})));
                }
                while self.wheel_step <= -WHEEL_STEP {
                    self.wheel_step += WHEEL_STEP;
                    self.pending = Some((self.config.wheel.down.clone(), serde_json::json!({})));
                }
            }
        }
        self.wheel_angle = Some(angle);
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

/// Shrink a rect around its middle.
fn shrink(rect: egui::Rect, scale: f32) -> egui::Rect {
    egui::Rect::from_center_size(rect.center(), rect.size() * scale)
}

/// The wheel at rest: a hairline ring, twelve short ticks and a small hollow hub.
fn draw_wheel(painter: &egui::Painter, rect: egui::Rect, scale: f32) {
    let center = rect.center();
    let radius = WHEEL_SIZE / 2.0 * scale - 1.0;
    painter.circle_stroke(center, radius, egui::Stroke::new(1.0, RING));
    for i in 0..12 {
        let angle = i as f32 * std::f32::consts::TAU / 12.0;
        let direction = egui::vec2(angle.cos(), angle.sin());
        painter.line_segment(
            [center + direction * (radius - 4.0), center + direction * (radius - 1.0)],
            egui::Stroke::new(1.0, TICK),
        );
    }
    painter.circle_stroke(center, 2.0 * scale, egui::Stroke::new(1.2, ICON_COLOR));
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
