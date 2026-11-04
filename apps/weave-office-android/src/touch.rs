//! Two-finger scrolling: winit's Android backend emits no wheel events, so the shell turns two
//! fingers' movement into the wheel events egui's scroll areas and the editors understand.

use eframe::egui;

pub struct TouchScroll {
    /// The fingers down, by id, with their last positions.
    fingers: Vec<(egui::TouchId, egui::Pos2)>,
    /// The centroid at the last move.
    last: Option<egui::Vec2>,
}

impl TouchScroll {
    pub fn new() -> Self {
        Self { fingers: Vec::new(), last: None }
    }

    pub fn apply(&mut self, raw: &mut egui::RawInput) {
        let mut wheel = egui::Vec2::ZERO;
        let mut lifted = false;
        for event in &raw.events {
            if let egui::Event::Touch { id, phase, pos, .. } = event {
                match phase {
                    egui::TouchPhase::Start => {
                        self.fingers.retain(|(finger, _)| finger != id);
                        self.fingers.push((*id, *pos));
                    }
                    egui::TouchPhase::Move => {
                        if let Some(finger) = self.fingers.iter_mut().find(|(finger, _)| finger == id) {
                            finger.1 = *pos;
                        }
                    }
                    egui::TouchPhase::End | egui::TouchPhase::Cancel => {
                        self.fingers.retain(|(finger, _)| finger != id);
                        lifted = true;
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
