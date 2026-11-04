//! Touch input: winit's Android backend emits no wheel events, so the shell turns fingers'
//! movement into the wheel events egui's scroll areas and the editors understand.
//!
//! Two fingers always scroll (their centroid's movement). One finger scrolls too, unless the
//! Touch Input setting says it selects text like a mouse; a tap still puts the caret either way.
//! Touches that start on the Shortcut Pad are left alone, so its drag and dial keep working.

use eframe::egui;
use serde::{Deserialize, Serialize};

/// What one finger does on the page.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FingerMode {
    /// A drag scrolls the page; a tap still puts the caret.
    #[default]
    Scroll,
    /// A drag selects text, like a mouse.
    Select,
}

/// How far a finger must move before it scrolls rather than taps (points).
const SLOP: f32 = 8.0;

pub struct TouchScroll {
    /// The fingers down, by id, with their last positions.
    fingers: Vec<(egui::TouchId, egui::Pos2)>,
    /// The centroid at the last move.
    last: Option<egui::Vec2>,
    /// The single finger's start, while it may still be a tap.
    single_start: Option<egui::Pos2>,
    /// The single finger's last position, while it scrolls.
    single_last: Option<egui::Pos2>,
    /// The single finger has moved far enough to scroll.
    scrolling: bool,
    /// The pointer the touch made has been released, so the canvas does not drag a selection.
    released: bool,
    /// The touch started on the Shortcut Pad, so it is the pad's, not a scroll.
    on_pad: bool,
}

impl TouchScroll {
    pub fn new() -> Self {
        Self {
            fingers: Vec::new(),
            last: None,
            single_start: None,
            single_last: None,
            scrolling: false,
            released: false,
            on_pad: false,
        }
    }

    pub fn apply(
        &mut self,
        raw: &mut egui::RawInput,
        mode: FingerMode,
        card: Option<egui::Rect>,
        dial: Option<egui::Rect>,
    ) {
        let mut wheel = egui::Vec2::ZERO;
        let mut lifted = false;
        for event in &raw.events {
            if let egui::Event::Touch { id, phase, pos, .. } = event {
                match phase {
                    egui::TouchPhase::Start => {
                        self.fingers.retain(|(finger, _)| finger != id);
                        self.fingers.push((*id, *pos));
                        if self.fingers.len() == 1 {
                            self.single_start = Some(*pos);
                            self.single_last = Some(*pos);
                            self.scrolling = false;
                            self.released = false;
                            self.on_pad = card.is_some_and(|rect| rect.contains(*pos))
                                || dial.is_some_and(|rect| rect.contains(*pos));
                        } else {
                            self.single_start = None;
                            self.single_last = None;
                            self.scrolling = false;
                            self.on_pad = false;
                        }
                    }
                    egui::TouchPhase::Move => {
                        if let Some(finger) = self.fingers.iter_mut().find(|(finger, _)| finger == id) {
                            finger.1 = *pos;
                        }
                        if self.fingers.len() == 1 && mode == FingerMode::Scroll && !self.on_pad {
                            if let (Some(start), Some(last)) = (self.single_start, self.single_last) {
                                if !self.scrolling && (*pos - start).length() > SLOP {
                                    self.scrolling = true;
                                }
                                if self.scrolling {
                                    wheel += *pos - last;
                                }
                                self.single_last = Some(*pos);
                            }
                        }
                    }
                    egui::TouchPhase::End | egui::TouchPhase::Cancel => {
                        self.fingers.retain(|(finger, _)| finger != id);
                        lifted = true;
                        if self.fingers.is_empty() {
                            self.single_start = None;
                            self.single_last = None;
                            self.scrolling = false;
                            self.released = false;
                            self.on_pad = false;
                        }
                    }
                }
            }
        }
        if self.fingers.len() >= 2 {
            let sum = self
                .fingers
                .iter()
                .fold(egui::Vec2::ZERO, |sum, (_, pos)| sum + pos.to_vec2());
            let centroid = sum / self.fingers.len() as f32;
            if let Some(last) = self.last {
                wheel = centroid - last;
            }
            self.last = Some(centroid);
            // While two fingers scroll, the pointer egui-winit made from the first finger must
            // not drag a selection or a slider.
            raw.events.retain(|event| !matches!(event, egui::Event::PointerButton { .. }));
        } else {
            self.last = None;
            if self.scrolling {
                // The finger scrolls: release the pointer the touch made so the canvas does not
                // drag a selection with it. Its moves stay, so the canvas still knows where the
                // finger is (it only scrolls the area under the pointer).
                raw.events.retain(|event| !matches!(event, egui::Event::PointerButton { .. }));
                if !self.released {
                    self.released = true;
                    raw.events.push(egui::Event::PointerButton {
                        pos: self.single_last.unwrap_or_default(),
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: egui::Modifiers::NONE,
                    });
                }
            }
        }
        if wheel != egui::Vec2::ZERO {
            raw.events.push(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: wheel,
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Trackpad,
                modifiers: egui::Modifiers::NONE,
            });
        }
        if lifted && self.fingers.is_empty() {
            // A final event so a flicked list coasts.
            raw.events.push(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::Vec2::ZERO,
                phase: egui::TouchPhase::End,
                source: egui::MouseWheelSource::Trackpad,
                modifiers: egui::Modifiers::NONE,
            });
        }
    }
}
