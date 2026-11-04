//! One clipboard for the workspace. Engine payloads are valid only while the operating
//! system still owns the exact copy we published; matching plain text is not an identity.

use std::{
    cell::RefCell,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};

const OBJECTS_MARKER: &str = "\u{2060}";
static COPY_SERIAL: AtomicU64 = AtomicU64::new(1);

/// The shell's system clipboard on Android: the platform has no arboard backend, so the shell
/// answers through its file bridge (the Activity owns the real clipboard).
#[cfg(target_os = "android")]
pub mod android {
    use std::sync::OnceLock;

    /// The shell's clipboard: put text on the system clipboard (returning a revision that
    /// changes with it), read its text, and report the current revision.
    pub struct Clipboard {
        pub set: Box<dyn Fn(&str) -> i64 + Send + Sync>,
        pub get: Box<dyn Fn() -> Option<String> + Send + Sync>,
        pub revision: Box<dyn Fn() -> i64 + Send + Sync>,
    }

    static CLIPBOARD: OnceLock<Clipboard> = OnceLock::new();

    /// Install the shell's clipboard, once, before the app starts.
    pub fn install(clipboard: Clipboard) {
        let _ = CLIPBOARD.set(clipboard);
    }

    pub(crate) fn get() -> Option<&'static Clipboard> {
        CLIPBOARD.get()
    }
}

#[derive(Clone, Default)]
pub struct Clipboard(Rc<RefCell<State>>);

#[derive(Default)]
struct State {
    #[cfg(not(target_os = "android"))]
    system: Option<arboard::Clipboard>,
    copy: Option<Copy>,
    sheet_transfers: Vec<crate::sheet_transfer::Request>,
    #[cfg(test)]
    memory: Option<Memory>,
}

struct Copy {
    identity: Identity,
    generation: u64,
    owner: Rc<()>,
    payload: Payload,
}

enum Identity {
    Native(i64),
    /// A unique HTML comment identifies our copy when no change serial is available.
    /// Comparing both complete flavours also detects changes to formatting with equal text.
    Portable {
        html: String,
        text: String,
    },
}

enum Payload {
    Word(wordcraft_doc::edit::Fragment),
    Sheet(gridcraft_engine::Clipboard),
    Slides(deckcraft_engine::Clipboard),
}

impl Clipboard {
    pub fn text(&self) -> Option<String> {
        self.0.borrow_mut().text()
    }

    pub fn html(&self) -> Option<String> {
        self.0.borrow_mut().html()
    }

    pub fn rtf(&self) -> Option<String> {
        #[cfg(test)]
        if self.0.borrow().memory.is_some() {
            return None;
        }
        #[cfg(target_os = "macos")]
        {
            use objc2_app_kit::NSPasteboard;
            use objc2_foundation::ns_string;
            NSPasteboard::generalPasteboard()
                .stringForType(ns_string!("public.rtf"))
                .map(|s| s.to_string())
        }
        #[cfg(not(target_os = "macos"))]
        None
    }

    pub fn write_rich(&self, html: &str, text: &str, mathml: &str) -> bool {
        let mut state = self.0.borrow_mut();
        state.copy = None;
        state.write(html, text, mathml).is_ok()
    }

    pub fn write_word(
        &self,
        owner: &Rc<()>,
        fragment: &wordcraft_doc::edit::Fragment,
        html: &str,
        text: &str,
        mathml: &str,
    ) -> bool {
        self.write_payload(owner, Payload::Word(fragment.clone()), html, text, mathml)
    }

    pub fn word(&self, owner: &Rc<()>) -> Option<(wordcraft_doc::edit::Fragment, bool)> {
        let mut state = self.0.borrow_mut();
        let copy = state.current()?;
        match &copy.payload {
            Payload::Word(fragment) => Some((fragment.clone(), Rc::ptr_eq(owner, &copy.owner))),
            _ => None,
        }
    }

    pub fn write_sheet(
        &self,
        owner: &Rc<()>,
        cells: &gridcraft_engine::Clipboard,
        html: &str,
        text: &str,
    ) -> Result<(), String> {
        if self.write_payload(owner, Payload::Sheet(cells.clone()), html, text, "") {
            Ok(())
        } else {
            Err("无法写入系统剪贴板".into())
        }
    }

    pub fn sheet(&self) -> Option<gridcraft_engine::Clipboard> {
        self.read_sheet().map(|(_, cells)| cells)
    }

    pub fn read_sheet(&self) -> Option<(u64, gridcraft_engine::Clipboard)> {
        let mut state = self.0.borrow_mut();
        let copy = state.current()?;
        match &copy.payload {
            Payload::Sheet(cells) => Some((copy.generation, cells.clone())),
            _ => None,
        }
    }

    pub fn request_sheet_transfer(
        &self,
        generation: Option<u64>,
        command: &str,
        params: serde_json::Value,
        supplied: &gridcraft_engine::Clipboard,
        target: &gridcraft_engine::DocState,
    ) -> Result<serde_json::Value, String> {
        let mut state = self.0.borrow_mut();
        let copy = state
            .current()
            .ok_or("系统剪贴板已变化，请重新剪切后粘贴")?;
        if generation != Some(copy.generation) {
            return Err("剪切副本已变化，请重新剪切后粘贴".into());
        }
        let Payload::Sheet(cells) = &copy.payload else {
            return Err("当前剪贴板不是表格剪切内容".into());
        };
        if !cells.cut
            || cells.doc_uid == target.uid
            || cells.doc_uid != supplied.doc_uid
            || cells.sheet != supplied.sheet
            || cells.range != supplied.range
            || !std::sync::Arc::ptr_eq(&cells.wb, &supplied.wb)
        {
            return Err("剪切内容与来源不匹配，移动未执行".into());
        }
        let request = crate::sheet_transfer::Request::new(
            copy.generation,
            command,
            params,
            cells.clone(),
            target,
        );
        if state
            .sheet_transfers
            .iter()
            .any(|pending| pending.generation == request.generation)
        {
            return Err("这份剪切内容正在移动，请等待完成".into());
        }
        state.sheet_transfers.push(request);
        Ok(serde_json::json!({"pending": "host", "operation": "moveCells"}))
    }

    pub(crate) fn take_sheet_transfers(&self) -> Vec<crate::sheet_transfer::Request> {
        std::mem::take(&mut self.0.borrow_mut().sheet_transfers)
    }

    pub(crate) fn sheet_transfer_is_current(
        &self,
        request: &crate::sheet_transfer::Request,
    ) -> bool {
        self.0.borrow_mut().current().is_some_and(|copy| {
            copy.generation == request.generation && matches!(&copy.payload, Payload::Sheet(cells) if cells.cut && cells.doc_uid == request.clipboard.doc_uid)
        })
    }

    pub(crate) fn has_sheet_transfer(&self, uid: u64) -> bool {
        self.0
            .borrow()
            .sheet_transfers
            .iter()
            .any(|request| request.involves(uid))
    }

    pub(crate) fn finish_sheet_transfer(&self, generation: u64) {
        let mut state = self.0.borrow_mut();
        if state
            .current()
            .is_some_and(|copy| copy.generation == generation)
        {
            state.copy = None;
            let _ = state.write("", "", "");
        }
    }

    pub fn update_sheet(
        &self,
        owner: &Rc<()>,
        generation: Option<u64>,
        cells: Option<&gridcraft_engine::Clipboard>,
    ) {
        let mut state = self.0.borrow_mut();
        let Some(copy) = state.current() else { return };
        if !Rc::ptr_eq(owner, &copy.owner) && generation != Some(copy.generation) {
            return;
        }
        let Payload::Sheet(previous) = &copy.payload else {
            return;
        };
        if let Some(cells) = cells {
            copy.payload = Payload::Sheet(cells.clone());
        } else {
            let was_cut = previous.cut;
            let generation = copy.generation;
            // A cut is consumed once. Do not import its old snapshot on the next paste,
            // including when that paste is in a different workbook tab.
            state.copy = None;
            state
                .sheet_transfers
                .retain(|request| request.generation != generation);
            if was_cut {
                let _ = state.write("", "", "");
            }
        }
    }

    pub fn write_slides(
        &self,
        owner: &Rc<()>,
        content: &deckcraft_engine::Clipboard,
        html: &str,
        text: &str,
    ) -> bool {
        self.write_payload(owner, Payload::Slides(content.clone()), html, text, "")
    }

    pub fn slides(&self) -> Option<deckcraft_engine::Clipboard> {
        let mut state = self.0.borrow_mut();
        match &state.current()?.payload {
            Payload::Slides(content) => Some(content.clone()),
            _ => None,
        }
    }

    pub fn update_slides(&self, content: &deckcraft_engine::Clipboard) {
        let mut state = self.0.borrow_mut();
        if let Some(copy) = state.current()
            && matches!(copy.payload, Payload::Slides(_))
        {
            copy.payload = Payload::Slides(content.clone());
        }
    }

    fn write_payload(
        &self,
        owner: &Rc<()>,
        payload: Payload,
        html: &str,
        text: &str,
        mathml: &str,
    ) -> bool {
        let mut state = self.0.borrow_mut();
        let previous = state.copy.take();
        let generation = COPY_SERIAL.fetch_add(1, Ordering::Relaxed);
        let text = if text.is_empty() {
            OBJECTS_MARKER
        } else {
            text
        };
        let token = state
            .revision()
            .is_none()
            .then(|| format!("<!--weave-office-{}-{}-->", std::process::id(), generation));
        let tagged_html = token.as_ref().map(|token| format!("{html}{token}"));
        match state.write(tagged_html.as_deref().unwrap_or(html), text, mathml) {
            Ok(Some(revision)) => {
                state.copy = Some(Copy {
                    identity: Identity::Native(revision),
                    generation,
                    owner: owner.clone(),
                    payload,
                });
                true
            }
            Ok(None) => {
                let Some(actual_html) = state.html() else {
                    return false;
                };
                let Some(actual_text) = state.text() else {
                    return false;
                };
                if token
                    .as_ref()
                    .is_none_or(|token| !actual_html.contains(token))
                    || actual_text != text
                {
                    return false;
                }
                state.copy = Some(Copy {
                    identity: Identity::Portable {
                        html: actual_html,
                        text: actual_text,
                    },
                    generation,
                    owner: owner.clone(),
                    payload,
                });
                true
            }
            Err(_) => {
                // A refused write may leave the previous system copy untouched. Keep it
                // only if its identity still matches; a partial native write invalidates it.
                state.copy = previous;
                let _ = state.current();
                false
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn memory() -> Self {
        Self(Rc::new(RefCell::new(State {
            memory: Some(Memory {
                serial: true,
                ..Default::default()
            }),
            ..Default::default()
        })))
    }

    #[cfg(test)]
    pub(crate) fn external_copy(&self, text: &str, html: &str) {
        let _ = self.0.borrow_mut().write(html, text, "");
    }
}

impl State {
    #[cfg(not(target_os = "android"))]
    fn system(&mut self) -> Option<&mut arboard::Clipboard> {
        if self.system.is_none() {
            self.system = arboard::Clipboard::new().ok();
        }
        self.system.as_mut()
    }

    fn revision(&self) -> Option<i64> {
        #[cfg(test)]
        if let Some(memory) = &self.memory {
            return memory.serial.then_some(memory.revision);
        }
        #[cfg(target_os = "macos")]
        return Some(objc2_app_kit::NSPasteboard::generalPasteboard().changeCount() as i64);
        #[cfg(target_os = "android")]
        return Some(android::get().map(|clipboard| (clipboard.revision)()).unwrap_or(0));
        #[cfg(not(any(target_os = "macos", target_os = "android")))]
        None
    }

    fn current(&mut self) -> Option<&mut Copy> {
        let current = match self.copy.as_ref().map(|copy| &copy.identity) {
            Some(Identity::Native(revision)) => self.revision() == Some(*revision),
            Some(Identity::Portable { html, text }) => {
                let (html, text) = (html.clone(), text.clone());
                self.html().as_ref() == Some(&html) && self.text().as_ref() == Some(&text)
            }
            None => false,
        };
        if !current {
            self.copy = None;
        }
        self.copy.as_mut()
    }

    fn text(&mut self) -> Option<String> {
        #[cfg(test)]
        if let Some(memory) = &self.memory {
            return memory.text.clone();
        }
        #[cfg(not(target_os = "android"))]
        {
            self.system()?.get_text().ok()
        }
        #[cfg(target_os = "android")]
        {
            android::get().and_then(|clipboard| (clipboard.get)())
        }
    }

    fn html(&mut self) -> Option<String> {
        #[cfg(test)]
        if let Some(memory) = &self.memory {
            return memory.html.clone();
        }
        #[cfg(not(target_os = "android"))]
        {
            self.system()?.get().html().ok()
        }
        #[cfg(target_os = "android")]
        {
            None
        }
    }

    fn write(&mut self, html: &str, text: &str, mathml: &str) -> Result<Option<i64>, String> {
        #[cfg(test)]
        if let Some(memory) = &mut self.memory {
            if memory.reject_writes {
                return Err("test clipboard write refused".into());
            }
            memory.revision += 1;
            memory.text = Some(text.into());
            memory.html = (!html.is_empty()).then(|| html.into());
            return Ok(memory.serial.then_some(memory.revision));
        }
        #[cfg(target_os = "macos")]
        {
            write_macos(
                &objc2_app_kit::NSPasteboard::generalPasteboard(),
                html,
                text,
                mathml,
            )
            .map(Some)
        }
        #[cfg(target_os = "android")]
        {
            let _ = (html, mathml);
            let clipboard = android::get().ok_or("无法访问系统剪贴板")?;
            Ok(Some((clipboard.set)(text)))
        }
        #[cfg(all(not(target_os = "macos"), not(target_os = "android")))]
        {
            let _ = mathml;
            let clipboard = self.system().ok_or("无法访问系统剪贴板")?;
            if html.is_empty() {
                clipboard.set_text(text)
            } else {
                clipboard.set_html(html, Some(text))
            }
            .map_err(|error| error.to_string())?;
            Ok(None)
        }
    }
}

#[cfg(target_os = "macos")]
fn write_macos(
    board: &objc2_app_kit::NSPasteboard,
    html: &str,
    text: &str,
    mathml: &str,
) -> Result<i64, String> {
    use objc2_foundation::{NSString, ns_string};
    let revision = board.clearContents();
    let mut written = board.setString_forType(
        &NSString::from_str(text),
        ns_string!("public.utf8-plain-text"),
    );
    if !html.is_empty() {
        written &= board.setString_forType(&NSString::from_str(html), ns_string!("public.html"));
    }
    if !mathml.is_empty() {
        written &= board.setString_forType(
            &NSString::from_str(mathml),
            ns_string!("application/mathml+xml"),
        );
    }
    if written && board.changeCount() == revision {
        Ok(revision as i64)
    } else {
        Err("无法写入系统剪贴板".into())
    }
}

#[cfg(test)]
#[derive(Default)]
struct Memory {
    revision: i64,
    serial: bool,
    reject_writes: bool,
    text: Option<String>,
    html: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::Editor;
    use eframe::egui;
    use serde_json::{Value, json};
    use weave_workspace::DocumentKind;

    #[test]
    fn failed_clipboard_write_keeps_word_and_slide_sources_and_the_previous_copy() {
        let clipboard = Clipboard::memory();
        let mut word = editor(DocumentKind::Text, &clipboard);
        command(&mut word, "text.insert", json!({"text": "保留文字"}));
        command(&mut word, "select.all", json!({}));
        command(&mut word, "edit.copy", json!({}));
        let Editor::Word(app) = &word else {
            unreachable!()
        };
        let before = app.session.doc.clone();
        let revision = app.session.rev();
        let history = (app.session.can_undo(), app.session.can_redo());
        clipboard
            .0
            .borrow_mut()
            .memory
            .as_mut()
            .unwrap()
            .reject_writes = true;
        assert!(word.command("edit.cut", json!({})).is_err());
        let Editor::Word(app) = &word else {
            unreachable!()
        };
        assert!(app.session.doc.same_content_without_save_stamp(&before));
        assert_eq!(app.session.rev(), revision);
        assert_eq!((app.session.can_undo(), app.session.can_redo()), history);
        assert!(clipboard.0.borrow().copy.is_some());
        clipboard
            .0
            .borrow_mut()
            .memory
            .as_mut()
            .unwrap()
            .reject_writes = false;
        let mut slides = editor(DocumentKind::Presentation, &clipboard);
        command(
            &mut slides,
            "shape.insert",
            json!({"preset": "rect", "rect": [50,50,100,60]}),
        );
        command(&mut slides, "edit.copy", json!({}));
        let before = selected_slide_shape(&slides);
        let Editor::Slides(app) = &slides else {
            unreachable!()
        };
        let journal = app.session.journal.clone();
        clipboard
            .0
            .borrow_mut()
            .memory
            .as_mut()
            .unwrap()
            .reject_writes = true;
        assert!(slides.command("edit.cut", json!({})).is_err());
        assert_eq!(selected_slide_shape(&slides), before);
        let Editor::Slides(app) = &slides else {
            unreachable!()
        };
        assert_eq!(app.session.journal, journal);
        assert!(clipboard.slides().is_some());
    }

    #[test]
    fn portable_identity_requires_our_token_and_unchanged_html_as_well_as_text() {
        let clipboard = Clipboard::memory();
        clipboard.0.borrow_mut().memory.as_mut().unwrap().serial = false;
        let owner = Rc::new(());
        let content = deckcraft_engine::Clipboard {
            plain: "same".into(),
            ..Default::default()
        };
        assert!(clipboard.write_slides(&owner, &content, "<b>same</b>", "same"));
        assert!(clipboard.slides().is_some());
        let tagged_html = clipboard.html().unwrap();
        assert!(tagged_html.contains("<!--weave-office-"));
        // Even if another application retains the comment, edited formatting is external.
        clipboard.external_copy("same", &tagged_html.replace("<b>same</b>", "<i>same</i>"));
        assert!(clipboard.slides().is_none());
        assert!(clipboard.write_slides(&owner, &content, "<b>same</b>", "same"));
        clipboard.external_copy("same", "<b>same</b>");
        assert!(
            clipboard.slides().is_none(),
            "equal visible HTML without our copy token is external"
        );
    }

    #[test]
    fn cancelled_sheet_cut_cannot_restore_an_old_snapshot_over_new_content() {
        let clipboard = Clipboard::memory();
        let mut sheet = editor(DocumentKind::Spreadsheet, &clipboard);
        let mut other = editor(DocumentKind::Spreadsheet, &clipboard);
        command(
            &mut other,
            "cell.set",
            json!({"cell": "A1", "input": "older copy"}),
        );
        command(&mut other, "edit.copy", json!({}));
        command(
            &mut sheet,
            "cell.set",
            json!({"cell": "A1", "input": "old"}),
        );
        command(&mut sheet, "edit.cut", json!({"range": "A1"}));
        command(&mut other, "home.insertRows", json!({"rows": "1:1"}));
        assert!(
            clipboard.sheet().is_some_and(|copy| copy.cut),
            "another tab releasing an old snapshot must not clear the new copy"
        );
        command(&mut other, "edit.clearClipboard", json!({}));
        assert!(clipboard.sheet().is_none());
        command(
            &mut sheet,
            "cell.set",
            json!({"cell": "A1", "input": "new"}),
        );
        assert!(sheet.command("edit.paste", json!({"at": "B1"})).is_err());
        assert_eq!(
            command(&mut sheet, "cell.get", json!({"cell": "A1"}))["text"],
            "new"
        );
    }

    #[test]
    fn cross_workbook_cut_waits_for_host_before_mutating_either_editor() {
        let clipboard = Clipboard::memory();
        let mut source = editor(DocumentKind::Spreadsheet, &clipboard);
        let mut target = editor(DocumentKind::Spreadsheet, &clipboard);
        command(
            &mut source,
            "cell.set",
            json!({"cell": "A1", "input": "move"}),
        );
        command(&mut source, "edit.cut", json!({"range": "A1"}));
        let result = command(&mut target, "edit.paste", json!({"at": "B1"}));
        assert_eq!(result["pending"], "host");
        assert!(clipboard.sheet().is_some_and(|copy| copy.cut));
        assert_eq!(
            command(&mut source, "cell.get", json!({"cell": "A1"}))["text"],
            "move"
        );
        assert_eq!(
            command(&mut target, "cell.get", json!({"cell": "B1"}))["text"],
            ""
        );
        assert_eq!(clipboard.take_sheet_transfers().len(), 1);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn native_private_pasteboard_keeps_all_flavours_and_changes_identity_for_equal_text() {
        use objc2_app_kit::NSPasteboard;
        use objc2_foundation::ns_string;
        // A separate named pasteboard tests AppKit without replacing the user's clipboard.
        let board = NSPasteboard::pasteboardWithUniqueName();
        let revision = write_macos(&board, "<b>same</b>", "same", "<math/>").unwrap();
        assert_eq!(board.changeCount() as i64, revision);
        assert_eq!(
            board
                .stringForType(ns_string!("public.utf8-plain-text"))
                .unwrap()
                .to_string(),
            "same"
        );
        assert_eq!(
            board
                .stringForType(ns_string!("public.html"))
                .unwrap()
                .to_string(),
            "<b>same</b>"
        );
        assert_eq!(
            board
                .stringForType(ns_string!("application/mathml+xml"))
                .unwrap()
                .to_string(),
            "<math/>"
        );
        let external_revision = write_macos(&board, "", "same", "").unwrap();
        assert_ne!(external_revision, revision);
        assert!(board.stringForType(ns_string!("public.html")).is_none());
    }

    fn editor(kind: DocumentKind, clipboard: &Clipboard) -> Editor {
        let mut editor = Editor::create(kind, None, clipboard).unwrap();
        if let Editor::Word(app) = &mut editor {
            let ctx = egui::Context::default();
            ctx.run_ui(egui::RawInput::default(), |ui| app.hosted_logic(ui.ctx()))
                .drop_without_applying_deltas();
        }
        editor
    }

    fn command(editor: &mut Editor, id: &str, params: Value) -> Value {
        editor
            .command(id, params)
            .unwrap_or_else(|error| panic!("{id}: {error}"))
    }

    fn word_bold(editor: &Editor) -> bool {
        let Editor::Word(app) = editor else {
            panic!("Word editor")
        };
        app.session
            .doc
            .para_at(&app.session.sel.focus)
            .unwrap()
            .props_of_char(0)
            .bold
            .unwrap_or(false)
    }

    #[test]
    fn shared_word_copy_keeps_formatting_and_external_identical_text_invalidates_it() {
        let clipboard = Clipboard::memory();
        let mut source = editor(DocumentKind::Text, &clipboard);
        let mut target = editor(DocumentKind::Text, &clipboard);
        command(&mut source, "text.insert", json!({"text": "共同文字"}));
        command(&mut source, "select.all", json!({}));
        command(&mut source, "format.bold", json!({"on": true}));
        command(&mut source, "edit.copy", json!({}));
        assert!(clipboard.html().is_some());
        command(&mut target, "edit.paste", json!({}));
        assert!(word_bold(&target));
        command(&mut target, "edit.undo", json!({}));
        assert!(word_bold(&source), "target undo must not alter the source");
        clipboard.external_copy("共同文字", "");
        command(&mut target, "edit.paste", json!({}));
        assert!(
            !word_bold(&target),
            "equal external text is a new copy, not the old bold fragment"
        );
    }

    fn selected_slide_shape(editor: &Editor) -> Value {
        let Editor::Slides(app) = editor else {
            panic!("slide editor")
        };
        let doc = app.session.active().unwrap();
        let shape = doc.shape(*doc.selection.shapes.first().unwrap()).unwrap();
        serde_json::to_value(shape).unwrap()
    }

    #[test]
    fn shared_slides_copy_pastes_the_source_shape_instead_of_the_targets_old_copy() {
        let clipboard = Clipboard::memory();
        let mut source = editor(DocumentKind::Presentation, &clipboard);
        let mut target = editor(DocumentKind::Presentation, &clipboard);
        command(
            &mut target,
            "shape.insert",
            json!({"preset": "ellipse", "rect": [200, 100, 70, 70]}),
        );
        command(&mut target, "edit.copy", json!({}));
        let old_shape = selected_slide_shape(&target);
        command(
            &mut source,
            "shape.insert",
            json!({"preset": "rect", "rect": [100, 100, 120, 50]}),
        );
        command(&mut source, "edit.copy", json!({}));
        let source_shape = selected_slide_shape(&source);
        assert_eq!(source_shape["geom"]["name"], "rect");
        assert_eq!(old_shape["geom"]["name"], "ellipse");
        assert_eq!(clipboard.text().as_deref(), Some(OBJECTS_MARKER));
        command(&mut target, "edit.paste", json!({}));
        let pasted = selected_slide_shape(&target);
        assert_eq!(pasted["geom"], source_shape["geom"]);
        assert_ne!(pasted["geom"], old_shape["geom"]);
        command(&mut target, "edit.undo", json!({}));
        assert_eq!(selected_slide_shape(&source), source_shape);
        // The same invisible marker copied by another application carries no local object.
        clipboard.external_copy(OBJECTS_MARKER, "");
        let Editor::Slides(app) = &mut target else {
            unreachable!()
        };
        let before = app.session.active().unwrap().doc.slides[0].shapes.len();
        let _ = app.hosted_command("edit.paste", json!({}));
        assert!(app.session.clipboard.shapes.is_empty());
        assert_eq!(
            app.session.active().unwrap().doc.slides[0].shapes.len(),
            before
        );
    }

    #[test]
    fn shared_sheet_copy_keeps_formula_and_style_with_independent_undo() {
        let clipboard = Clipboard::memory();
        let mut source = editor(DocumentKind::Spreadsheet, &clipboard);
        let mut target = editor(DocumentKind::Spreadsheet, &clipboard);
        command(
            &mut source,
            "cell.set",
            json!({"cell": "B2", "input": "=1+2"}),
        );
        command(&mut source, "home.bold", json!({"range": "B2", "on": true}));
        command(&mut source, "selection.set", json!({"range": "A1"}));
        command(&mut source, "edit.copy", json!({"range": "B2"}));
        command(&mut target, "edit.paste", json!({}));
        let original = command(&mut source, "cell.get", json!({"cell": "B2"}));
        let pasted = command(&mut target, "cell.get", json!({"cell": "A1"}));
        assert_eq!(pasted["formula"], original["formula"]);
        assert_eq!(pasted["style"], original["style"]);
        assert!(pasted["formula"].is_string());
        command(&mut target, "edit.undo", json!({}));
        assert_eq!(
            command(&mut source, "cell.get", json!({"cell": "B2"})),
            original
        );
        clipboard.external_copy("3", "");
        command(&mut target, "edit.paste", json!({}));
        let external = command(&mut target, "cell.get", json!({"cell": "A1"}));
        assert!(
            external["formula"].is_null(),
            "same displayed text cannot resurrect the old formula"
        );
        assert_ne!(external["style"], pasted["style"]);
    }

    #[test]
    fn shared_sheet_cut_is_consumed_once_across_host_clipboard_reads() {
        let clipboard = Clipboard::memory();
        let mut sheet = editor(DocumentKind::Spreadsheet, &clipboard);
        command(
            &mut sheet,
            "cell.set",
            json!({"cell": "A1", "input": "cut once"}),
        );
        command(&mut sheet, "selection.set", json!({"range": "A1"}));
        command(&mut sheet, "edit.cut", json!({}));
        command(&mut sheet, "edit.paste", json!({"at": "B1"}));
        assert!(clipboard.sheet().is_none());
        assert_eq!(clipboard.text().as_deref(), Some(""));
        assert!(sheet.command("edit.paste", json!({"at": "C1"})).is_err());
        let again = command(&mut sheet, "cell.get", json!({"cell": "C1"}));
        assert!(again["formula"].is_null());
        assert!(again["text"].as_str().unwrap_or_default().is_empty());
    }

    #[test]
    fn shared_html_keeps_word_emphasis_when_pasted_into_a_presentation() {
        let clipboard = Clipboard::memory();
        let mut word = editor(DocumentKind::Text, &clipboard);
        let mut slides = editor(DocumentKind::Presentation, &clipboard);
        command(&mut word, "text.insert", json!({"text": "跨类型内容"}));
        command(&mut word, "select.all", json!({}));
        command(&mut word, "format.bold", json!({"on": true}));
        command(&mut word, "edit.copy", json!({}));
        command(&mut slides, "edit.paste", json!({}));
        let pasted = selected_slide_shape(&slides);
        let body = pasted["text"].to_string();
        assert!(body.contains("跨类型内容"));
        assert!(body.contains("\"bold\":true"), "{body}");
    }
}
