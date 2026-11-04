//! The soft keyboard's text, as egui events.
//!
//! winit 0.30 opens the soft keyboard but drops GameActivity's text events, so the shell polls
//! the keyboard's text state each frame while a text field has focus and replays the changes as
//! egui text, Backspace and Enter events.

use eframe::egui;

/// One change between two keyboard buffers: `removed` characters taken back, `inserted` added.
#[derive(Debug, PartialEq, Eq)]
pub struct Edit {
    pub removed: usize,
    pub inserted: String,
}

/// The change that turns `old` into `new` (a common prefix and suffix are kept).
pub fn diff(old: &str, new: &str) -> Edit {
    let old: Vec<char> = old.chars().collect();
    let new: Vec<char> = new.chars().collect();
    let mut start = 0;
    while start < old.len() && start < new.len() && old[start] == new[start] {
        start += 1;
    }
    let mut end_old = old.len();
    let mut end_new = new.len();
    while end_old > start && end_new > start && old[end_old - 1] == new[end_new - 1] {
        end_old -= 1;
        end_new -= 1;
    }
    Edit {
        removed: end_old - start,
        inserted: new[start..end_new].iter().collect(),
    }
}

/// The egui events that replay `edit` into the focused text field.
pub fn events(edit: &Edit) -> Vec<egui::Event> {
    let mut events = Vec::new();
    for _ in 0..edit.removed {
        events.push(egui::Event::Key {
            key: egui::Key::Backspace,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
        events.push(egui::Event::Key {
            key: egui::Key::Backspace,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
    }
    if !edit.inserted.is_empty() {
        events.push(egui::Event::Text(edit.inserted.clone()));
    }
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_appends() {
        assert_eq!(diff("", "a"), Edit { removed: 0, inserted: "a".into() });
        assert_eq!(diff("ab", "abc"), Edit { removed: 0, inserted: "c".into() });
    }

    #[test]
    fn backspace_removes() {
        assert_eq!(diff("abc", "ab"), Edit { removed: 1, inserted: String::new() });
    }

    #[test]
    fn autocorrect_replaces() {
        assert_eq!(diff("teh", "the"), Edit { removed: 2, inserted: "he".into() });
    }

    #[test]
    fn composing_text_is_replayed() {
        let events = events(&diff("", "你好"));
        assert_eq!(events, vec![egui::Event::Text("你好".into())]);
    }
}
