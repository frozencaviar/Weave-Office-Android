//! Touch input: winit's Android backend emits no wheel events, so the shell turns fingers'
//! movement into the wheel events egui's scroll areas and the editors understand, and it turns
//! taps, long presses and drags into the mouse events a finger cannot make.
//!
//! One finger is a mouse in a presentation: a tap clicks (selects), a drag drags (moves a shape,
//! sweeps a marquee), and a long press released in place is a Ctrl+click, so a second tap adds to
//! the selection. In a document one finger scrolls instead, as a reader expects, and a long press
//! starts the drag. Two fingers always scroll (their centroid's movement). Touches that start on
//! the Shortcut Pad are left alone, so its drag and dial keep working.

use std::time::Duration;

use eframe::egui;
use serde::{Deserialize, Serialize};

/// What one finger does on the page.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub enum FingerMode {
    /// A drag scrolls; a long press then drag moves (the default).
    #[default]
    Auto,
    /// A drag drags straight away, like a mouse.
    Mouse,
}

impl<'de> Deserialize<'de> for FingerMode {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // "Select" was this setting's name before a presentation's one finger became a mouse by
        // default; "Scroll" was the old default, which is what Auto does in a document.
        let name = String::deserialize(deserializer)?;
        Ok(if name == "Mouse" || name == "Select" { Self::Mouse } else { Self::Auto })
    }
}

/// How far a finger may move and still be a tap (points).
const TAP_SLOP: f32 = 12.0;
/// How far a finger must move to start a drag (points).
const DRAG_SLOP: f32 = 4.0;
/// How long a finger must rest to arm a drag or a Ctrl+click.
const LONG_PRESS: Duration = Duration::from_millis(400);

/// One finger's gesture, from the touch down to the lift.
struct Single {
    start: egui::Pos2,
    last: egui::Pos2,
    time: std::time::Instant,
    /// Moved far enough that it is no longer a tap.
    moved: bool,
    /// The movement is scrolling the page.
    scrolling: bool,
    /// Rested long enough to drag or to add to the selection.
    armed: bool,
    /// The drag's press has been sent.
    dragging: bool,
    /// The touch started on the Shortcut Pad, so it is the pad's.
    on_pad: bool,
}

/// A click's release, held back one frame.
///
/// The canvas reads a press and a release delivered in the same frame as a single event and never
/// finishes the click, so a tap's release goes out on the next frame.
struct PendingRelease {
    pos: egui::Pos2,
    modifiers: egui::Modifiers,
    /// The click borrowed Ctrl; give it back once the release has landed.
    ctrl: bool,
}

pub struct TouchScroll {
    /// The fingers down, by id, with their last positions.
    fingers: Vec<(egui::TouchId, egui::Pos2)>,
    /// The centroid at the last move.
    last: Option<egui::Vec2>,
    /// The single finger's gesture, while one finger is down.
    single: Option<Single>,
    /// A click's release, waiting for the next frame.
    pending: Option<PendingRelease>,
    /// A Ctrl+click borrowed the modifiers; give them back next frame.
    restore_modifiers: bool,
}

impl TouchScroll {
    pub fn new() -> Self {
        Self {
            fingers: Vec::new(),
            last: None,
            single: None,
            pending: None,
            restore_modifiers: false,
        }
    }

    /// Turn this frame's touches into the pointer and wheel events a finger cannot make.
    ///
    /// `drawing` is whether a drawing tool is armed and `pen` whether the pointer is a stylus:
    /// winit's Android backend reports the pen as another touch, so a stroke would otherwise
    /// scroll the page instead of drawing.
    pub fn apply(
        &mut self,
        ctx: &egui::Context,
        raw: &mut egui::RawInput,
        mode: FingerMode,
        drawing: bool,
        pen: bool,
        modifiers: egui::Modifiers,
        card: Option<egui::Rect>,
        dial: Option<egui::Rect>,
    ) {
        let now = std::time::Instant::now();
        let mouse_like = mode == FingerMode::Mouse || drawing || pen;
        // The Ctrl a long press borrowed for its click is the click's, not the user's.
        if std::mem::take(&mut self.restore_modifiers) {
            raw.events.push(egui::Event::ModifiersChanged(modifiers));
        }
        // The release held back last frame goes out after this frame's events are filtered.
        let deferred = self.pending.take();
        let mut managed = deferred.is_some();

        let mut wheel = egui::Vec2::ZERO;
        let mut press: Option<(egui::Pos2, egui::Modifiers)> = None;
        let mut release: Option<(egui::Pos2, egui::Modifiers, bool)> = None;
        let mut lifted = false;

        for event in &raw.events {
            let egui::Event::Touch { id, phase, pos, .. } = event else { continue };
            match phase {
                egui::TouchPhase::Start => {
                    self.fingers.retain(|(finger, _)| finger != id);
                    self.fingers.push((*id, *pos));
                    if self.fingers.len() == 1 {
                        let on_pad = card.is_some_and(|rect| rect.contains(*pos))
                            || dial.is_some_and(|rect| rect.contains(*pos));
                        self.single = Some(Single {
                            start: *pos,
                            last: *pos,
                            time: now,
                            moved: false,
                            scrolling: false,
                            armed: false,
                            dragging: false,
                            on_pad,
                        });
                    } else {
                        // A second finger is a scroll, never a click or a drag.
                        self.single = None;
                    }
                }
                egui::TouchPhase::Move => {
                    if let Some(finger) = self.fingers.iter_mut().find(|(finger, _)| finger == id) {
                        finger.1 = *pos;
                    }
                    if self.fingers.len() != 1 {
                        continue;
                    }
                    let Some(single) = self.single.as_mut() else { continue };
                    if single.on_pad {
                        continue;
                    }
                    let distance = (*pos - single.start).length();
                    if distance > TAP_SLOP {
                        single.moved = true;
                    }
                    // A finger that has rested long enough drags instead of scrolling; the check
                    // is here as well as after the events, because the frame that carries the
                    // first move may be the first one since the finger came to rest.
                    if !single.scrolling && !single.armed && now.saturating_duration_since(single.time) >= LONG_PRESS {
                        single.armed = true;
                    }
                    if !single.dragging && distance > DRAG_SLOP && (mouse_like || single.armed) {
                        // The press is sent at the touch down, so the drag starts where the
                        // finger did and nothing jumps.
                        single.dragging = true;
                        press = Some((single.start, egui::Modifiers::NONE));
                    }
                    if single.scrolling {
                        wheel += *pos - single.last;
                    } else if !mouse_like && single.moved && !single.dragging {
                        single.scrolling = true;
                        wheel += *pos - single.start;
                    }
                    single.last = *pos;
                }
                egui::TouchPhase::End | egui::TouchPhase::Cancel => {
                    self.fingers.retain(|(finger, _)| finger != id);
                    lifted = true;
                    if !self.fingers.is_empty() {
                        continue;
                    }
                    let Some(single) = self.single.take() else { continue };
                    if single.on_pad {
                        continue;
                    }
                    if single.dragging {
                        release = Some((*pos, egui::Modifiers::NONE, false));
                    } else if single.armed && !single.moved {
                        // A long press released in place: Ctrl+click, so a second tap adds to
                        // the selection instead of replacing it.
                        let ctrl = egui::Modifiers { ctrl: true, command: true, ..egui::Modifiers::NONE };
                        press = Some((single.start, ctrl));
                        release = Some((single.start, ctrl, true));
                    } else if !single.scrolling && !single.moved {
                        press = Some((single.start, egui::Modifiers::NONE));
                        release = Some((single.start, egui::Modifiers::NONE, false));
                    }
                }
            }
        }

        // Two fingers scroll by their centroid.
        if self.fingers.len() >= 2 {
            let sum = self.fingers.iter().fold(egui::Vec2::ZERO, |sum, (_, pos)| sum + pos.to_vec2());
            let centroid = sum / self.fingers.len() as f32;
            if let Some(last) = self.last {
                wheel = centroid - last;
            }
            self.last = Some(centroid);
        } else {
            self.last = None;
        }

        // Arm the long press once the finger has rested, and wake up when it will.
        if let Some(single) = self.single.as_mut() {
            if !single.on_pad && !single.scrolling && !single.dragging {
                let held = now.saturating_duration_since(single.time);
                if !single.armed && held >= LONG_PRESS {
                    single.armed = true;
                }
                if !single.armed {
                    ctx.request_repaint_after(LONG_PRESS.saturating_sub(held));
                }
            }
        }

        // While a finger drives the pointer, the events winit made from it must not also click.
        managed = managed
            || self.fingers.len() >= 2
            || self.single.as_ref().is_some_and(|single| !single.on_pad)
            || press.is_some()
            || release.is_some();
        if managed {
            raw.events.retain(|event| !matches!(event, egui::Event::PointerButton { .. }));
        }
        if let Some((pos, modifiers)) = press {
            if modifiers.ctrl {
                raw.events.push(egui::Event::ModifiersChanged(modifiers));
            }
            raw.events.push(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers,
            });
        }
        if let Some(pending) = deferred {
            raw.events.push(egui::Event::PointerButton {
                pos: pending.pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: pending.modifiers,
            });
            self.restore_modifiers = pending.ctrl;
        }
        if let Some((pos, modifiers, ctrl)) = release {
            self.pending = Some(PendingRelease { pos, modifiers, ctrl });
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
