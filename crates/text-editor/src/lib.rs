//! The Markdown and plain-text editor Weave Office hosts beside its office editors.
//!
//! One [`TextApp`] edits one file. In [`Kind::Markdown`] it shows Obsidian-style live preview
//! (formatting applied in place, syntax shown only on the lines being edited), a source view
//! and a reading view. In [`Kind::PlainText`] it is a code editor with syntax colors, line
//! numbers and the usual line commands. The file's bytes are written back in the encoding,
//! byte-order mark and line endings they were read with.

pub mod edit;
pub mod file;
mod i18n;
pub mod layout;
pub mod markdown;
pub mod palette;
pub mod sidebar;
pub mod syntax;
mod tables;
pub mod tools;
mod ui;

use edit::{Edit, Selection};
use egui::text::{CCursor, CCursorRange};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

pub use file::Format;
pub use syntax::Language;
/// Installs the bold and monospace families other editors (the rich-text editor) share.
pub use ui::ensure_fonts;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Markdown,
    PlainText,
}

/// The Markdown views.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    /// Formatting shown in place; syntax appears on the lines being edited.
    Live,
    /// All syntax visible.
    Source,
    /// Read-only, no syntax.
    Reading,
}

#[derive(Default)]
struct Find {
    open: bool,
    replace: bool,
    query: String,
    replacement: String,
    match_case: bool,
    whole_word: bool,
    /// Matches in the current text (byte ranges), and which one is selected.
    matches: Vec<std::ops::Range<usize>>,
    current: Option<usize>,
    /// The text and options the matches were computed for.
    searched: Option<(u64, String, bool, bool)>,
    focus: bool,
    goto: Option<String>,
}

pub struct TextApp {
    pub kind: Kind,
    text: String,
    saved: String,
    format: Format,
    path: Option<PathBuf>,
    language: &'static Language,
    indent: String,
    zh: bool,
    view: View,
    wrap: bool,
    line_numbers: bool,
    sidebar: sidebar::Sidebar,
    palette: Option<palette::Palette>,
    /// Hide everything but the text, as in Obsidian's and iA Writer's focus modes.
    focus_mode: bool,
    /// Keep the caret's line in the middle of the window.
    typewriter: bool,
    /// Where typewriter scrolling last centred: `(caret, generation)`.
    typewriter_at: Option<(usize, u64)>,
    font_size: f32,
    ribbon_tab: Option<String>,
    find: Find,
    id: egui::Id,
    ctx: Option<egui::Context>,
    host_commands: Vec<(String, Value)>,
    /// A message for the status bar (a failed command).
    notice: Option<String>,
    /// Where the next frame should scroll to (byte offset), and focus the text.
    scroll_to: Option<usize>,
    focus_text: bool,
    /// Bumped on every change, to key caches.
    generation: u64,
    /// Undo history of the text. The editor keeps its own (rather than the text widget's) so
    /// the host's undo works without a window, and every edit path shares one history.
    undoer: egui::util::undoer::Undoer<String>,
    cache: ui::Cache,
}

static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

impl TextApp {
    /// An empty document.
    pub fn new(kind: Kind) -> Self {
        let language = match kind {
            Kind::Markdown => Language::by_name("markdown").unwrap_or(&syntax::PLAIN),
            Kind::PlainText => &syntax::PLAIN,
        };
        Self {
            kind,
            text: String::new(),
            saved: String::new(),
            format: Format::default(),
            path: None,
            language,
            indent: if kind == Kind::Markdown {
                "    ".into()
            } else {
                "    ".into()
            },
            zh: true,
            view: View::Live,
            wrap: kind == Kind::Markdown,
            line_numbers: kind == Kind::PlainText,
            sidebar: Default::default(),
            palette: None,
            focus_mode: false,
            typewriter: false,
            typewriter_at: None,
            font_size: if kind == Kind::Markdown { 15.0 } else { 13.0 },
            ribbon_tab: Some("home".into()),
            find: Find::default(),
            id: egui::Id::unique((
                "weave-text-editor",
                NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            )),
            ctx: None,
            host_commands: Vec::new(),
            notice: None,
            scroll_to: None,
            focus_text: true,
            generation: 0,
            undoer: Default::default(),
            cache: ui::Cache::default(),
        }
    }

    /// A document read from `bytes`; `name` (a file name) picks the language.
    pub fn from_bytes(kind: Kind, name: &str, bytes: &[u8]) -> Result<Self, String> {
        let (text, format) = file::decode(bytes)?;
        let mut app = Self::new(kind);
        if kind == Kind::PlainText {
            app.language = Language::for_path(name);
            // Long lines in data files read better unwrapped; prose files wrap.
            app.wrap = app.language.is_plain();
        }
        app.indent = edit::detect_indent(&text);
        app.saved = text.clone();
        app.undoer.add_undo(&text);
        app.text = text;
        app.format = format;
        if app.text.len() > ui::HIGHLIGHT_LIMIT {
            app.notice = Some("文件较大,已关闭语法高亮".into());
        }
        Ok(app)
    }

    pub fn open(kind: Kind, path: &Path) -> Result<Self, String> {
        let bytes =
            std::fs::read(path).map_err(|error| format!("无法打开 {}:{error}", path.display()))?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut app = Self::from_bytes(kind, &name, &bytes)?;
        app.path = Some(path.to_path_buf());
        Ok(app)
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// The file this document belongs to; also picks the language of a restored copy.
    pub fn set_path(&mut self, path: Option<PathBuf>) {
        if self.kind == Kind::PlainText
            && let Some(name) = path.as_deref().and_then(Path::file_name)
        {
            self.language = Language::for_path(&name.to_string_lossy());
        }
        self.path = path;
    }

    /// Highlight plain text as the language a file called `name` would have.
    pub fn set_language_for(&mut self, name: &str) {
        if self.kind == Kind::PlainText {
            self.language = Language::for_path(name);
        }
    }

    /// The name of the language the text is highlighted as.
    pub fn language_name(&self) -> &'static str {
        self.language.name
    }

    /// Put the caret at the start of zero-based `line` and scroll it into view.
    pub fn jump_to_line(&mut self, line: usize) {
        let at = edit::offset_of_line(&self.text, line + 1);
        self.select(Selection::caret(at));
        self.focus_text = true;
    }

    /// Which sidebar pane is showing, if any.
    pub fn sidebar_pane(&self) -> Option<sidebar::Pane> {
        self.sidebar.pane
    }

    /// Mark the document as changed since it was saved (a restored copy).
    pub fn mark_unsaved(&mut self) {
        self.saved = String::from("\u{0}unsaved");
    }

    pub fn language(&self) -> &'static Language {
        self.language
    }

    pub fn set_language_code(&mut self, code: &str) {
        self.zh = !code.eq_ignore_ascii_case("en") && !code.to_ascii_lowercase().starts_with("en-");
    }

    /// Whether the interface is in Chinese.
    pub fn interface_zh(&self) -> bool {
        self.zh
    }

    pub fn format(&self) -> Format {
        self.format
    }

    /// The bytes [`Self::save`] would write.
    pub fn bytes(&self) -> Result<Vec<u8>, String> {
        file::encode(&self.text, self.format)
    }

    pub fn save(&mut self, path: &Path) -> Result<(), String> {
        let bytes = self.bytes()?;
        file::write_atomically(path, &bytes)?;
        self.saved = self.text.clone();
        self.set_path(Some(path.to_path_buf()));
        Ok(())
    }

    pub fn dirty(&self) -> bool {
        self.text != self.saved
    }

    pub fn take_host_commands(&mut self) -> Vec<(String, Value)> {
        std::mem::take(&mut self.host_commands)
    }

    fn text_id(&self) -> egui::Id {
        self.id.with("text")
    }

    fn tr<'a>(&self, text: &'a str) -> &'a str {
        i18n::tr(self.zh, text)
    }

    /// The current selection, as byte offsets.
    pub fn selection(&self) -> Selection {
        let Some(ctx) = &self.ctx else {
            return Selection::caret(0);
        };
        let range =
            egui::TextEdit::load_state(ctx, self.text_id()).and_then(|s| s.cursor.char_range());
        match range {
            Some(range) => Selection::new(
                byte_of_char(&self.text, range.secondary.index.0),
                byte_of_char(&self.text, range.primary.index.0),
            ),
            None => Selection::caret(0),
        }
    }

    fn char_range(&self, selection: Selection) -> CCursorRange {
        CCursorRange {
            primary: CCursor::new(char_of_byte(&self.text, selection.head)),
            secondary: CCursor::new(char_of_byte(&self.text, selection.anchor)),
            h_pos: None,
        }
    }

    /// Select `selection` and scroll it into view.
    pub fn select(&mut self, selection: Selection) {
        self.set_cursor(selection);
    }

    fn set_cursor(&mut self, selection: Selection) {
        if let Some(ctx) = self.ctx.clone() {
            let mut state = egui::TextEdit::load_state(&ctx, self.text_id()).unwrap_or_default();
            state
                .cursor
                .set_char_range(Some(self.char_range(selection)));
            state.store(&ctx, self.text_id());
        }
        self.scroll_to = Some(selection.head);
    }

    /// Apply an edit as one undo step.
    /// Apply `edit` if there is one; a command with nothing to change does nothing.
    fn apply_some(&mut self, edit: Option<Edit>) {
        if let Some(edit) = edit {
            self.apply(edit);
        }
    }

    pub fn apply(&mut self, edit: Edit) {
        self.undoer.add_undo(&self.text);
        edit.apply(&mut self.text);
        self.undoer.add_undo(&self.text);
        self.set_cursor(edit.selection);
        self.focus_text = true;
        self.changed();
    }

    /// Apply an edit made by typing: a burst of these is one undo step, like typing in the
    /// text view.
    pub fn apply_typing(&mut self, edit: Edit, time: f64) {
        edit.apply(&mut self.text);
        self.typed(time);
        self.set_cursor(edit.selection);
        self.changed();
    }

    fn changed(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }

    /// Record typing in the undo history; the history groups a burst of typing into one step.
    fn typed(&mut self, time: f64) {
        self.undoer.feed_state(time, &self.text);
    }

    fn undo(&mut self, redo: bool) -> bool {
        // Typing still in flux becomes its own step first.
        if !redo {
            self.undoer.add_undo(&self.text);
        }
        let restored = if redo {
            self.undoer.redo(&self.text)
        } else {
            self.undoer.undo(&self.text)
        }
        .cloned();
        let Some(text) = restored else {
            return false;
        };
        // Put the caret at the end of what changed.
        let prefix = common_prefix(&self.text, &text);
        let suffix = common_suffix(&self.text[prefix..], &text[prefix..]);
        let caret = text.len() - suffix;
        self.text = text;
        self.set_cursor(Selection::caret(caret.max(prefix)));
        self.changed();
        true
    }

    fn undo_redo_available(&self) -> (bool, bool) {
        (
            self.undoer.has_undo(&self.text),
            self.undoer.has_redo(&self.text),
        )
    }

    fn markdown(&self) -> bool {
        self.kind == Kind::Markdown
    }

    /// Run a command by id: the host's quick controls, the ribbon and shortcuts all use these.
    pub fn command(&mut self, id: &str, params: Value) -> Result<Value, String> {
        self.notice = None;
        let selection = self.selection();
        let text = self.text.clone();
        let markdown = self.markdown();
        let wrap = |app: &mut Self, before: &str, after: &str| {
            app.apply(edit::toggle_wrap(&text, selection, before, after));
        };
        match id {
            "edit.undo" => {
                self.undo(false);
            }
            "edit.redo" => {
                self.undo(true);
            }
            "edit.selectAll" => self.select(Selection::new(0, self.text.len())),
            "edit.find" => {
                self.find.open = true;
                self.find.replace = false;
                self.find.goto = None;
                self.find.focus = true;
                let selected = &self.text[selection.range()];
                if !selected.is_empty() && !selected.contains('\n') {
                    self.find.query = selected.to_owned();
                }
            }
            "edit.replace" => {
                self.command("edit.find", json!({}))?;
                self.find.replace = true;
            }
            "edit.findNext" | "edit.findPrevious" => self.find_step(id == "edit.findNext"),
            "edit.goToLine" => {
                if let Some(line) = params.get("line").and_then(Value::as_u64) {
                    let at = edit::offset_of_line(&self.text, line as usize);
                    self.select(Selection::caret(at));
                } else {
                    self.find.open = true;
                    self.find.goto = Some(String::new());
                    self.find.focus = true;
                }
            }
            "edit.indent" => self.apply(edit::indent(&text, selection, &self.indent.clone())),
            "edit.outdent" => self.apply(edit::outdent(&text, selection, &self.indent.clone())),
            "edit.toggleComment" => {
                let marker = if markdown {
                    Some("<!--")
                } else {
                    self.language.comment
                };
                match marker {
                    Some("<!--") => wrap(self, "<!-- ", " -->"),
                    Some(marker) => self.apply(edit::toggle_comment(&text, selection, marker)),
                    None => return Err(self.tr("此语言没有行注释").into()),
                }
            }
            "edit.moveLineUp" | "edit.moveLineDown" => {
                if let Some(edit) = edit::move_lines(
                    &text,
                    selection,
                    if id == "edit.moveLineUp" { -1 } else { 1 },
                ) {
                    self.apply(edit);
                }
            }
            "edit.duplicateLine" => self.apply(edit::duplicate_lines(&text, selection)),
            "edit.deleteLine" => self.apply(edit::delete_lines(&text, selection)),
            "format.bold" => wrap(self, "**", "**"),
            "format.italic" => wrap(self, "*", "*"),
            "format.strike" => wrap(self, "~~", "~~"),
            "format.highlight" => wrap(self, "==", "=="),
            "format.code" => wrap(self, "`", "`"),
            "format.link" => self.apply(edit::insert_link(&text, selection)),
            "format.wikilink" => wrap(self, "[[", "]]"),
            "format.heading" => {
                let level = params
                    .get("level")
                    .and_then(Value::as_u64)
                    .unwrap_or(1)
                    .clamp(0, 6) as u8;
                let block = if level == 0 {
                    edit::Block::Paragraph
                } else {
                    edit::Block::Heading(level)
                };
                self.apply(edit::set_block(&text, selection, block));
            }
            "format.paragraph" => {
                self.apply(edit::set_block(&text, selection, edit::Block::Paragraph))
            }
            "format.bullet" => self.apply(edit::set_block(&text, selection, edit::Block::Bullet)),
            "format.numbered" => {
                self.apply(edit::set_block(&text, selection, edit::Block::Numbered))
            }
            "format.task" => self.apply(edit::set_block(&text, selection, edit::Block::Task)),
            "format.quote" => self.apply(edit::set_block(&text, selection, edit::Block::Quote)),
            "insert.codeBlock" => {
                let range = selection.range();
                let inner = &text[range.clone()];
                let lead = if edit::line_start(&text, range.start) == range.start {
                    ""
                } else {
                    "\n"
                };
                let block = format!(
                    "{lead}```\n{inner}{}```\n",
                    if inner.is_empty() || inner.ends_with('\n') {
                        ""
                    } else {
                        "\n"
                    }
                );
                let caret = range.start + lead.len() + 3;
                self.apply(Edit {
                    range,
                    text: block,
                    selection: Selection::caret(caret),
                });
            }
            "insert.table" => {
                let range = selection.range();
                let lead = if edit::line_start(&text, range.start) == range.start {
                    ""
                } else {
                    "\n"
                };
                let table =
                    format!("{lead}| 列 1 | 列 2 | 列 3 |\n| --- | --- | --- |\n|  |  |  |\n");
                let caret = range.start + lead.len() + 2;
                self.apply(Edit {
                    range,
                    text: table,
                    selection: Selection::new(caret, caret + "列 1".len()),
                });
            }
            "insert.rule" => {
                let range = selection.range();
                let lead = if edit::line_start(&text, range.start) == range.start {
                    ""
                } else {
                    "\n"
                };
                let rule = format!("{lead}\n---\n\n");
                let caret = range.start + rule.len();
                self.apply(Edit {
                    range,
                    text: rule,
                    selection: Selection::caret(caret),
                });
            }
            "format.json" | "format.jsonMinify" => {
                let indent = (id == "format.json")
                    .then_some(self.indent.as_str())
                    .map(str::to_owned);
                let formatted = edit::format_json(&text, indent.as_deref())?;
                self.apply(Edit {
                    range: 0..text.len(),
                    text: formatted,
                    selection: Selection::caret(0),
                });
            }
            "view.mode" => {
                self.view = match params.get("value").and_then(Value::as_str) {
                    Some("source") => View::Source,
                    Some("reading") => View::Reading,
                    _ => View::Live,
                };
            }
            "view.wrap" => {
                self.wrap = params
                    .get("value")
                    .and_then(Value::as_bool)
                    .unwrap_or(!self.wrap)
            }
            "view.lineNumbers" => {
                self.line_numbers = params
                    .get("value")
                    .and_then(Value::as_bool)
                    .unwrap_or(!self.line_numbers)
            }
            "view.outline" => {
                let show = params
                    .get("value")
                    .and_then(Value::as_bool)
                    .unwrap_or(self.sidebar.pane != Some(sidebar::Pane::Outline));
                let pane = if show {
                    Some(sidebar::Pane::Outline)
                } else {
                    self.sidebar.pane.filter(|p| *p != sidebar::Pane::Outline)
                };
                sidebar::set_pane(self, pane);
            }
            "view.sidebar" => {
                // `{"pane": "files"}` shows a pane, `{"pane": null}` closes the sidebar, and no
                // pane toggles the last one.
                let pane = match params.get("pane") {
                    Some(Value::String(name)) => {
                        Some(sidebar::Pane::from_name(name).ok_or(self.tr("未知侧栏"))?)
                    }
                    Some(Value::Null) => None,
                    _ => match self.sidebar.pane {
                        Some(_) => None,
                        None => Some(sidebar::Pane::Files),
                    },
                };
                if params.get("toggle").and_then(Value::as_bool) == Some(true)
                    && pane.is_some()
                    && pane == self.sidebar.pane
                {
                    sidebar::set_pane(self, None);
                } else {
                    sidebar::set_pane(self, pane);
                }
            }
            "edit.sortLines" => {
                let descending = params.get("descending").and_then(Value::as_bool) == Some(true);
                self.apply_some(tools::sort_lines(&text, selection, descending));
            }
            "edit.uniqueLines" => self.apply_some(tools::unique_lines(&text, selection)),
            "edit.reverseLines" => self.apply_some(tools::reverse_lines(&text, selection)),
            "edit.joinLines" => self.apply_some(tools::join_lines(&text, selection)),
            "edit.upperCase" => {
                self.apply_some(tools::change_case(&text, selection, tools::Case::Upper))
            }
            "edit.lowerCase" => {
                self.apply_some(tools::change_case(&text, selection, tools::Case::Lower))
            }
            "edit.titleCase" => {
                self.apply_some(tools::change_case(&text, selection, tools::Case::Title))
            }
            "edit.trimTrailing" => {
                self.apply_some(tools::trim_trailing(&text, selection, markdown))
            }
            "edit.indentToSpaces" | "edit.indentToTabs" => {
                let width = if self.indent == "\t" {
                    4
                } else {
                    self.indent.len().max(1)
                };
                let to_tabs = id == "edit.indentToTabs";
                self.apply_some(tools::convert_indent(&text, selection, to_tabs, width));
                self.indent = if to_tabs {
                    "\t".into()
                } else {
                    " ".repeat(width)
                };
            }
            "insert.footnote" => self.apply(tools::insert_footnote(&text, selection)),
            "insert.callout" => {
                let kind = params.get("kind").and_then(Value::as_str).unwrap_or("note");
                let title = params.get("title").and_then(Value::as_str).unwrap_or("");
                self.apply(tools::insert_callout(&text, selection, kind, title));
            }
            "insert.math" => self.apply(edit::toggle_wrap(&text, selection, "$", "$")),
            "insert.mathBlock" => self.apply(tools::insert_math_block(&text, selection)),
            "insert.image" => {
                let placeholder = self.tr("图片路径");
                self.apply(tools::insert_image(&text, selection, placeholder));
            }
            "insert.date" | "insert.time" | "insert.dateTime" => {
                let now = chrono::Local::now();
                let format = match id {
                    "insert.date" => "%Y-%m-%d",
                    "insert.time" => "%H:%M",
                    _ => "%Y-%m-%d %H:%M",
                };
                let stamp = now.format(format).to_string();
                let range = selection.range();
                let caret = range.start + stamp.len();
                self.apply(Edit {
                    range,
                    text: stamp,
                    selection: Selection::caret(caret),
                });
            }
            "insert.properties" => {
                let title = self
                    .path
                    .as_deref()
                    .and_then(Path::file_stem)
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let date = chrono::Local::now().format("%Y-%m-%d").to_string();
                let edit = tools::properties(&text, &title, &date);
                if edit.text.is_empty() {
                    self.select(edit.selection);
                } else {
                    self.apply(edit);
                }
            }
            "view.focus" => {
                self.focus_mode = params
                    .get("value")
                    .and_then(Value::as_bool)
                    .unwrap_or(!self.focus_mode)
            }
            "view.typewriter" => {
                self.typewriter = params
                    .get("value")
                    .and_then(Value::as_bool)
                    .unwrap_or(!self.typewriter)
            }
            "palette.commands" => palette::open(self, palette::Mode::Commands),
            "palette.files" => palette::open(self, palette::Mode::Files),
            "edit.searchFolder" => {
                if let Some(query) = params.get("query").and_then(Value::as_str) {
                    sidebar::set_query(self, query.to_owned());
                } else if !selection.is_empty()
                    && let Some(selected) =
                        text.get(selection.range()).filter(|t| !t.contains('\n'))
                {
                    sidebar::set_query(self, selected.to_owned());
                }
                sidebar::set_pane(self, Some(sidebar::Pane::Search));
            }
            "view.zoomIn" => self.font_size = (self.font_size + 1.0).min(32.0),
            "view.zoomOut" => self.font_size = (self.font_size - 1.0).max(9.0),
            "view.zoomReset" => self.font_size = if markdown { 15.0 } else { 13.0 },
            "file.lineEnding" => {
                self.format.line_ending = match params.get("value").and_then(Value::as_str) {
                    Some("crlf") => file::LineEnding::CrLf,
                    _ => file::LineEnding::Lf,
                };
                // A format change is a change to save.
                self.saved.push('\u{0}');
            }
            "file.encoding" => {
                let previous = self.format;
                self.format.encoding = match params.get("value").and_then(Value::as_str) {
                    Some("utf-16le") => file::TextEncoding::Utf16Le,
                    Some("utf-16be") => file::TextEncoding::Utf16Be,
                    Some(label) if !label.eq_ignore_ascii_case("utf-8") => {
                        file::TextEncoding::Legacy(
                            encoding_rs::Encoding::for_label(label.as_bytes())
                                .ok_or(self.tr("不支持此编码"))?,
                        )
                    }
                    _ => file::TextEncoding::Utf8,
                };
                self.format.bom = matches!(
                    self.format.encoding,
                    file::TextEncoding::Utf16Le | file::TextEncoding::Utf16Be
                ) || params.get("bom").and_then(Value::as_bool).unwrap_or(false);
                if let Err(error) = self.bytes() {
                    self.format = previous;
                    return Err(error);
                }
                self.saved.push('\u{0}');
            }
            "language.set" => {
                let name = params.get("value").and_then(Value::as_str).unwrap_or("");
                self.language = Language::by_name(name).ok_or(self.tr("不支持此语言"))?;
            }
            "file.open" | "file.new" | "file.save" | "file.saveAs" | "file.print"
            | "file.exportPdf" => {
                self.host_commands.push((id.to_owned(), params));
            }
            _ => return Err(format!("{}:{id}", self.tr("未知命令"))),
        }
        Ok(Value::Null)
    }

    fn find_step(&mut self, forward: bool) {
        self.refresh_matches();
        if self.find.matches.is_empty() {
            return;
        }
        let selection = self.selection().range();
        // A fresh caret (search as you type) may sit on a match: that match comes first.
        let index = if forward {
            self.find
                .matches
                .iter()
                .position(|m| {
                    m.start >= selection.end || (m.start >= selection.start && selection.is_empty())
                })
                .unwrap_or(0)
        } else {
            self.find
                .matches
                .iter()
                .rposition(|m| m.end <= selection.start)
                .unwrap_or(self.find.matches.len() - 1)
        };
        self.find.current = Some(index);
        let m = self.find.matches[index].clone();
        self.select(Selection::new(m.start, m.end));
    }

    fn refresh_matches(&mut self) {
        let key = (
            self.generation,
            self.find.query.clone(),
            self.find.match_case,
            self.find.whole_word,
        );
        if self.find.searched.as_ref() != Some(&key) {
            self.find.matches = edit::find_all(
                &self.text,
                &self.find.query,
                self.find.match_case,
                self.find.whole_word,
            );
            self.find.current = None;
            self.find.searched = Some(key);
        }
    }

    fn replace_current(&mut self) {
        self.refresh_matches();
        let selection = self.selection().range();
        if let Some(m) = self.find.matches.iter().find(|m| **m == selection).cloned() {
            let replacement = self.find.replacement.clone();
            let end = m.start + replacement.len();
            self.apply(Edit {
                range: m,
                text: replacement,
                selection: Selection::caret(end),
            });
            self.refresh_matches();
        }
        self.find_step(true);
    }

    fn replace_all(&mut self) -> usize {
        self.refresh_matches();
        let matches = self.find.matches.clone();
        if matches.is_empty() {
            return 0;
        }
        let mut out = String::with_capacity(self.text.len());
        let mut at = 0;
        for m in &matches {
            out.push_str(&self.text[at..m.start]);
            out.push_str(&self.find.replacement);
            at = m.end;
        }
        out.push_str(&self.text[at..]);
        let len = self.text.len();
        self.apply(Edit {
            range: 0..len,
            text: out,
            selection: Selection::caret(0),
        });
        matches.len()
    }

    // ---- The host's editor contract -------------------------------------------------------

    pub fn hosted_logic(&mut self, ctx: &egui::Context) {
        self.ctx = Some(ctx.clone());
        ui::ensure_fonts(ctx);
    }

    pub fn hosted_ui(&mut self, ui: &mut egui::Ui) {
        self.ctx = Some(ui.ctx().clone());
        ui::show(self, ui);
    }

    pub fn hosted_command(&mut self, id: &str, params: Value) -> Result<Value, String> {
        self.command(id, params)
    }

    pub fn hosted_flush(&mut self) -> Result<(), String> {
        Ok(())
    }

    pub fn hosted_dirty(&self) -> bool {
        self.dirty()
    }

    pub fn hosted_save(&mut self, path: &Path) -> Result<(), String> {
        self.save(path)
    }

    pub fn hosted_deactivate(&mut self, ctx: &egui::Context) -> Result<(), String> {
        ctx.memory_mut(|memory| {
            if memory.has_focus(self.text_id()) {
                memory.surrender_focus(self.text_id());
            }
        });
        self.focus_text = true;
        Ok(())
    }

    pub fn hosted_blocks_host_navigation(&self) -> bool {
        // The palette owns the keyboard while it is open.
        self.palette.is_some()
    }
}

impl weave_ribbon::HostedRibbon for TextApp {
    fn ribbon_tabs(&self, _ctx: &egui::Context) -> weave_ribbon::Tabs {
        let mut tabs = vec![weave_ribbon::Tab::new("home", self.tr("开始"))];
        if self.markdown() {
            tabs.push(weave_ribbon::Tab::new("insert", self.tr("插入")));
        }
        tabs.push(weave_ribbon::Tab::new("view", self.tr("视图")));
        weave_ribbon::Tabs {
            tabs,
            active: self.ribbon_tab.clone(),
        }
    }

    fn ribbon_event(&mut self, event: weave_ribbon::Event) {
        match event {
            weave_ribbon::Event::Clicked(id) => {
                self.ribbon_tab = Some(id);
            }
            weave_ribbon::Event::DoubleClicked(id) => {
                self.ribbon_tab = if self.ribbon_tab.as_deref() == Some(&id) {
                    None
                } else {
                    Some(id)
                };
            }
        }
    }

    fn undo_redo(&self) -> (bool, bool) {
        self.undo_redo_available()
    }
}

fn common_prefix(a: &str, b: &str) -> usize {
    let mut n = a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count();
    while !a.is_char_boundary(n) || !b.is_char_boundary(n) {
        n -= 1;
    }
    n
}

fn common_suffix(a: &str, b: &str) -> usize {
    let mut n = a
        .bytes()
        .rev()
        .zip(b.bytes().rev())
        .take_while(|(x, y)| x == y)
        .count();
    while !a.is_char_boundary(a.len() - n) || !b.is_char_boundary(b.len() - n) {
        n -= 1;
    }
    n
}

/// Byte offset of the `index`-th character (clamped to the end).
pub fn byte_of_char(text: &str, index: usize) -> usize {
    text.char_indices()
        .nth(index)
        .map_or(text.len(), |(i, _)| i)
}

/// Character index of byte offset `at`.
pub fn char_of_byte(text: &str, at: usize) -> usize {
    let mut at = at.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    text[..at].chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_and_saving_without_edits_keeps_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.json");
        let bytes = b"{\r\n  \"a\": 1\r\n}\r\n";
        std::fs::write(&path, bytes).unwrap();
        let mut app = TextApp::open(Kind::PlainText, &path).unwrap();
        assert_eq!(app.language().name, "JSON");
        assert_eq!(app.text(), "{\n  \"a\": 1\n}\n");
        assert!(!app.dirty());
        let out = dir.path().join("b.json");
        app.save(&out).unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), bytes);
        assert_eq!(app.path(), Some(out.as_path()));
    }

    #[test]
    fn commands_edit_and_mark_dirty() {
        let mut app = TextApp::from_bytes(Kind::Markdown, "a.md", b"word").unwrap();
        app.command("format.heading", json!({"level": 2})).unwrap();
        assert_eq!(app.text(), "## word");
        assert!(app.dirty());
        app.command("format.json", json!({})).unwrap_err();
        let mut app = TextApp::from_bytes(Kind::PlainText, "a.json", b"{\"a\":[1,2]}").unwrap();
        app.command("format.json", json!({})).unwrap();
        assert_eq!(
            app.text(),
            "{\n    \"a\": [\n        1,\n        2\n    ]\n}"
        );
        assert!(app.command("nope", json!({})).is_err());
        app.command("file.save", json!({})).unwrap();
        assert_eq!(app.take_host_commands().len(), 1);
    }

    #[test]
    fn undo_and_redo_without_a_window() {
        let mut app = TextApp::from_bytes(Kind::PlainText, "a.txt", b"one").unwrap();
        assert_eq!(app.undo_redo_available(), (false, false));
        app.apply(Edit {
            range: 3..3,
            text: " two".into(),
            selection: Selection::caret(7),
        });
        app.apply(Edit {
            range: 0..3,
            text: "1".into(),
            selection: Selection::caret(1),
        });
        assert_eq!(app.undo_redo_available(), (true, false));
        app.command("edit.undo", json!({})).unwrap();
        assert_eq!(app.text(), "one two");
        app.command("edit.undo", json!({})).unwrap();
        assert_eq!(app.text(), "one");
        assert!(!app.dirty());
        assert_eq!(app.undo_redo_available(), (false, true));
        app.command("edit.redo", json!({})).unwrap();
        assert_eq!(app.text(), "one two");
        // Typing groups into steps through the same history.
        app.text.push('!');
        app.typed(100.0);
        app.typed(200.0);
        app.command("edit.undo", json!({})).unwrap();
        assert_eq!(app.text(), "one two");
        assert_eq!(common_suffix("a中b", "x中b"), 4);
    }

    #[test]
    fn find_and_replace_all() {
        let mut app = TextApp::from_bytes(Kind::PlainText, "a.txt", b"cat Cat cat").unwrap();
        app.find.query = "cat".into();
        app.find.replacement = "dog".into();
        assert_eq!(app.replace_all(), 3);
        assert_eq!(app.text(), "dog dog dog");
        app.find.match_case = true;
        app.find.query = "Dog".into();
        assert_eq!(app.replace_all(), 0);
    }

    #[test]
    fn encoding_switch_refuses_lossy_targets() {
        let mut app = TextApp::from_bytes(Kind::PlainText, "a.txt", "中文".as_bytes()).unwrap();
        assert!(
            app.command("file.encoding", json!({"value": "windows-1252"}))
                .is_err()
        );
        assert_eq!(app.format().encoding, file::TextEncoding::Utf8);
        app.command("file.encoding", json!({"value": "gbk"}))
            .unwrap();
        assert!(app.dirty());
        assert_eq!(
            app.bytes().unwrap(),
            encoding_rs::GBK.encode("中文").0.as_ref()
        );
    }

    #[test]
    fn char_byte_conversions() {
        let text = "a中b";
        assert_eq!(byte_of_char(text, 2), 4);
        assert_eq!(byte_of_char(text, 9), 5);
        assert_eq!(char_of_byte(text, 4), 2);
        assert_eq!(char_of_byte(text, 2), 1);
    }
}
