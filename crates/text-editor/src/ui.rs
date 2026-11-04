//! Drawing the editor: the command bar under the host's ribbon tabs, the find bar, the text
//! with its gutter and live-preview decorations, the outline and the status bar.

use crate::edit::{self, Selection};
use crate::layout::{self, BOLD_FAMILY, MONO_FAMILY, Palette, Reveal, Style};
use crate::markdown::{self, Decoration, Parsed};
use crate::syntax::{self, Language, Token};
use crate::{Kind, TextApp, View, byte_of_char};
use egui::text::{CCursor, Galley};
use egui::{
    Color32, FontFamily, FontId, Key, Modifiers, Rect, RichText, Sense, Shape, Stroke, Ui, Vec2,
    pos2, vec2,
};
use serde_json::{Value, json};
use std::ops::Range;
use std::sync::Arc;

/// Above this size, code is shown without syntax colors so typing stays fluid.
pub const HIGHLIGHT_LIMIT: usize = 2 << 20;

/// What the layouter derived from the text, kept until the text changes.
#[derive(Default)]
pub struct Cache {
    source: String,
    language: &'static str,
    valid: bool,
    tokens: Vec<(Range<usize>, Token)>,
    parsed: Option<Parsed>,
    /// `(byte, char)` index of every line start, for byte → char conversions.
    line_starts: Vec<(usize, usize)>,
    composing: bool,
}

impl Cache {
    fn update(&mut self, text: &str, kind: Kind, language: &'static Language) {
        if self.valid && self.language == language.name && self.source == text {
            return;
        }
        self.valid = true;
        self.language = language.name;
        self.source.clear();
        self.source.push_str(text);
        self.tokens.clear();
        self.parsed = None;
        match kind {
            Kind::Markdown => self.parsed = Some(markdown::parse(text)),
            Kind::PlainText => {
                if text.len() <= HIGHLIGHT_LIMIT {
                    syntax::highlight(language, text, 0, &mut self.tokens);
                    self.tokens.sort_by_key(|(range, _)| range.start);
                }
            }
        }
        self.line_starts.clear();
        self.line_starts.push((0, 0));
        let mut chars = 0;
        let mut last = 0;
        for (i, _) in text.match_indices('\n') {
            chars += text[last..=i].chars().count();
            last = i + 1;
            self.line_starts.push((i + 1, chars));
        }
    }

    /// Character index of byte `at` in the cached text.
    fn char_at(&self, at: usize) -> usize {
        let at = at.min(self.source.len());
        let line = self
            .line_starts
            .partition_point(|(byte, _)| *byte <= at)
            .saturating_sub(1);
        let (byte, char) = self.line_starts.get(line).copied().unwrap_or((0, 0));
        let mut end = at;
        while !self.source.is_char_boundary(end) {
            end -= 1;
        }
        char + self.source[byte..end].chars().count()
    }

    fn cursor(&self, at: usize) -> CCursor {
        CCursor::new(self.char_at(at))
    }
}

/// Install the editor's bold and monospace families once: bold from the interface's
/// semibold faces, monospace with the interface's Chinese faces behind it.
pub fn ensure_fonts(ctx: &egui::Context) {
    if ctx.cumulative_pass_nr() == 0 {
        return;
    }
    let bold = FontFamily::Name(BOLD_FAMILY.into());
    if ctx.fonts(|f| f.definitions().families.contains_key(&bold)) {
        return;
    }
    let mut definitions = ctx.fonts(|f| f.definitions().clone());
    let proportional = definitions
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let monospace = definitions
        .families
        .get(&FontFamily::Monospace)
        .cloned()
        .unwrap_or_default();
    let existing = |names: &[&str]| -> Vec<String> {
        names
            .iter()
            .filter(|n| definitions.font_data.contains_key(**n))
            .map(|n| (*n).to_owned())
            .collect()
    };
    let join = |mut chain: Vec<String>, rest: &[String]| {
        for name in rest {
            if !chain.contains(name) {
                chain.push(name.clone());
            }
        }
        chain
    };
    let bold_chain = join(
        existing(&["weave-ui-semibold", "weave-ui-cjk-medium"]),
        &proportional,
    );
    let mono_chain = join(
        join(
            monospace,
            &existing(&["weave-ui-cjk", "weave-ui-cjk-fallback"]),
        ),
        &proportional,
    );
    if bold_chain.is_empty() || mono_chain.is_empty() {
        return;
    }
    definitions.families.insert(bold, bold_chain);
    definitions
        .families
        .insert(FontFamily::Name(MONO_FAMILY.into()), mono_chain);
    ctx.set_fonts(definitions);
}

fn families(ctx: &egui::Context) -> (FontFamily, FontFamily) {
    let bold = FontFamily::Name(BOLD_FAMILY.into());
    let mono = FontFamily::Name(MONO_FAMILY.into());
    ctx.fonts(|f| {
        let families = &f.definitions().families;
        (
            if families.contains_key(&bold) {
                bold
            } else {
                FontFamily::Proportional
            },
            if families.contains_key(&mono) {
                mono
            } else {
                FontFamily::Monospace
            },
        )
    })
}

pub fn show(app: &mut TextApp, ui: &mut Ui) {
    ensure_fonts(ui.ctx());
    crate::sidebar::begin_frame(app);
    shortcuts(app, ui);
    if app.ribbon_tab.is_some() {
        egui::Panel::top("weave-text-commands")
            .exact_size(36.0)
            .frame(
                egui::Frame::NONE
                    .fill(ui.visuals().panel_fill)
                    .inner_margin(egui::Margin::symmetric(10, 4)),
            )
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| command_bar(app, ui));
            });
    }
    egui::Panel::bottom("weave-text-status")
        .exact_size(24.0)
        .frame(
            egui::Frame::NONE
                .fill(ui.visuals().panel_fill)
                .inner_margin(egui::Margin::symmetric(10, 2)),
        )
        .show(ui, |ui| status_bar(app, ui));
    if !app.focus_mode {
        side_panels(app, ui);
    }
    if app.find.open {
        egui::Panel::top("weave-text-find")
            .frame(
                egui::Frame::NONE
                    .fill(ui.visuals().faint_bg_color)
                    .inner_margin(egui::Margin::symmetric(10, 6)),
            )
            .show(ui, |ui| find_bar(app, ui));
    }
    let background = if ui.visuals().dark_mode {
        ui.visuals().extreme_bg_color
    } else {
        Color32::WHITE
    };
    egui::CentralPanel::no_frame()
        .frame(egui::Frame::NONE.fill(background))
        .show(ui, |ui| editor(app, ui));
    crate::palette::show(app, &ui.ctx().clone());
}

/// The activity strip and the sidebar pane behind it.
fn side_panels(app: &mut TextApp, ui: &mut Ui) {
    egui::Panel::left("weave-text-activity")
        .exact_size(38.0)
        .resizable(false)
        .frame(
            egui::Frame::NONE
                .fill(ui.visuals().panel_fill)
                .inner_margin(egui::Margin::symmetric(4, 0)),
        )
        .show(ui, |ui| crate::sidebar::activity_bar(app, ui));
    if app.sidebar.pane.is_some_and(|p| p.available(app.kind)) {
        // One id for every text tab, so the width the user drags to is kept across tabs.
        egui::Panel::left("weave-text-sidebar")
            .resizable(true)
            .default_size(250.0)
            .size_range(160.0..=480.0)
            .frame(
                egui::Frame::NONE
                    .fill(ui.visuals().panel_fill)
                    .inner_margin(egui::Margin::same(10)),
            )
            .show(ui, |ui| crate::sidebar::pane(app, ui));
    }
}

fn tr<'a>(app: &TextApp, text: &'a str) -> &'a str {
    crate::i18n::tr(app.zh, text)
}

fn run(app: &mut TextApp, id: &str, params: Value) {
    if let Err(error) = app.command(id, params) {
        app.notice = Some(error);
    }
}

fn shortcuts(app: &mut TextApp, ui: &mut Ui) {
    let text_id = app.text_id();
    let focused = ui.memory(|m| m.has_focus(text_id));
    let markdown = app.kind == Kind::Markdown;
    let editable = !(markdown && app.view == View::Reading);
    let code = !markdown && !app.language.is_plain();
    let mut commands: Vec<(&'static str, Value)> = Vec::new();
    let mut pairs: Vec<char> = Vec::new();
    ui.input_mut(|i| {
        // IME composition: Enter and Tab belong to the input method.
        for event in &i.events {
            if let egui::Event::Ime(ime) = event {
                app.cache.composing =
                    matches!(ime, egui::ImeEvent::Preedit { text, .. } if !text.is_empty());
            }
        }
        let plain_enter = i.modifiers.is_none() || i.modifiers == Modifiers::SHIFT;
        let cmd = Modifiers::COMMAND;
        let shift = Modifiers::SHIFT;
        let alt = Modifiers::ALT;
        let mut key = |m: Modifiers,
                       k: Key,
                       id: &'static str,
                       params: Value,
                       commands: &mut Vec<(&'static str, Value)>| {
            if i.consume_key(m, k) {
                commands.push((id, params));
                true
            } else {
                false
            }
        };
        key(
            cmd | shift,
            Key::P,
            "palette.commands",
            json!({}),
            &mut commands,
        );
        key(cmd | alt, Key::O, "palette.files", json!({}), &mut commands);
        key(
            cmd | shift,
            Key::F,
            "edit.searchFolder",
            json!({}),
            &mut commands,
        );
        key(cmd | alt, Key::F, "edit.replace", json!({}), &mut commands);
        key(cmd, Key::F, "edit.find", json!({}), &mut commands);
        key(
            cmd | shift,
            Key::G,
            "edit.findPrevious",
            json!({}),
            &mut commands,
        );
        key(cmd, Key::G, "edit.findNext", json!({}), &mut commands);
        key(
            shift,
            Key::F3,
            "edit.findPrevious",
            json!({}),
            &mut commands,
        );
        key(
            Modifiers::NONE,
            Key::F3,
            "edit.findNext",
            json!({}),
            &mut commands,
        );
        key(cmd, Key::L, "edit.goToLine", json!({}), &mut commands);
        if !focused || !editable {
            return;
        }
        if !app.cache.composing {
            key(shift, Key::Tab, "edit.outdent", json!({}), &mut commands);
            key(
                Modifiers::NONE,
                Key::Tab,
                "edit.indent",
                json!({}),
                &mut commands,
            );
            if plain_enter {
                key(
                    Modifiers::NONE,
                    Key::Enter,
                    "edit.newline",
                    json!({}),
                    &mut commands,
                );
            }
        }
        // Undo goes through the editor's history, not the text widget's.
        key(cmd | shift, Key::Z, "edit.redo", json!({}), &mut commands);
        key(cmd, Key::Y, "edit.redo", json!({}), &mut commands);
        key(cmd, Key::Z, "edit.undo", json!({}), &mut commands);
        key(
            cmd,
            Key::Slash,
            "edit.toggleComment",
            json!({}),
            &mut commands,
        );
        key(
            shift | alt,
            Key::ArrowDown,
            "edit.duplicateLine",
            json!({}),
            &mut commands,
        );
        key(
            shift | alt,
            Key::ArrowUp,
            "edit.duplicateLine",
            json!({}),
            &mut commands,
        );
        key(
            alt,
            Key::ArrowUp,
            "edit.moveLineUp",
            json!({}),
            &mut commands,
        );
        key(
            alt,
            Key::ArrowDown,
            "edit.moveLineDown",
            json!({}),
            &mut commands,
        );
        key(
            cmd | shift,
            Key::K,
            "edit.deleteLine",
            json!({}),
            &mut commands,
        );
        if markdown {
            key(
                cmd | shift,
                Key::X,
                "format.strike",
                json!({}),
                &mut commands,
            );
            key(
                cmd | shift,
                Key::H,
                "format.highlight",
                json!({}),
                &mut commands,
            );
            key(cmd, Key::B, "format.bold", json!({}), &mut commands);
            key(cmd, Key::I, "format.italic", json!({}), &mut commands);
            key(cmd, Key::K, "format.link", json!({}), &mut commands);
            key(cmd, Key::E, "format.code", json!({}), &mut commands);
            key(
                cmd,
                Key::Enter,
                "format.toggleTask",
                json!({}),
                &mut commands,
            );
            for (k, level) in [
                (Key::Num0, 0),
                (Key::Num1, 1),
                (Key::Num2, 2),
                (Key::Num3, 3),
                (Key::Num4, 4),
                (Key::Num5, 5),
                (Key::Num6, 6),
            ] {
                key(
                    cmd,
                    k,
                    "format.heading",
                    json!({"level": level}),
                    &mut commands,
                );
            }
        }
        if code && !app.cache.composing {
            // Bracket and quote pairs.
            i.events.retain(|event| {
                if let egui::Event::Text(text) = event {
                    let mut chars = text.chars();
                    if let (Some(c), None) = (chars.next(), chars.next())
                        && matches!(c, '(' | '[' | '{' | '"' | '\'' | '`' | ')' | ']' | '}')
                    {
                        pairs.push(c);
                        return false;
                    }
                }
                true
            });
        }
    });
    for c in pairs {
        auto_pair(app, c);
    }
    for (id, params) in commands {
        match id {
            "edit.newline" => {
                let selection = app.selection();
                let text = app.text.clone();
                let indent = app.indent.clone();
                app.apply(edit::newline(&text, selection, markdown, &indent));
            }
            "format.toggleTask" => {
                let selection = app.selection();
                let start = edit::line_start(&app.text, selection.head);
                let line = &app.text[start..edit::line_end(&app.text, selection.head)];
                let indent = line.len() - line.trim_start().len();
                let rest = &line[indent..];
                let marker = ["- [", "* [", "+ ["]
                    .iter()
                    .find(|p| rest.starts_with(**p))
                    .map(|_| start + indent + 2);
                match marker.and_then(|m| edit::toggle_task(&app.text, m)) {
                    Some(mut toggle) => {
                        toggle.selection = selection;
                        app.apply(toggle);
                    }
                    None => run(app, "format.task", json!({})),
                }
            }
            _ => run(app, id, params),
        }
    }
}

fn auto_pair(app: &mut TextApp, c: char) {
    let selection = app.selection();
    let range = selection.range();
    let next = app.text[range.end..].chars().next();
    let close = match c {
        '(' => ')',
        '[' => ']',
        '{' => '}',
        other => other,
    };
    // Typing a closer that is already there steps over it.
    if matches!(c, ')' | ']' | '}' | '"' | '\'' | '`') && range.is_empty() && next == Some(c) {
        app.select(Selection::caret(range.end + c.len_utf8()));
        return;
    }
    let opener = matches!(c, '(' | '[' | '{' | '"' | '\'' | '`');
    let before = app.text[..range.start].chars().next_back();
    let quote_inside_word =
        matches!(c, '"' | '\'' | '`') && before.is_some_and(|b| b.is_alphanumeric());
    let next_is_word = next.is_some_and(|n| n.is_alphanumeric() || n == '_');
    let edit = if opener && !range.is_empty() {
        let inner = app.text[range.clone()].to_owned();
        let start = range.start + 1;
        edit::Edit {
            range: range.clone(),
            text: format!("{c}{inner}{close}"),
            selection: Selection::new(start, start + inner.len()),
        }
    } else if opener && !quote_inside_word && !next_is_word {
        edit::Edit {
            range: range.clone(),
            text: format!("{c}{close}"),
            selection: Selection::caret(range.start + 1),
        }
    } else {
        edit::Edit {
            range: range.clone(),
            text: c.to_string(),
            selection: Selection::caret(range.start + c.len_utf8()),
        }
    };
    app.apply(edit);
}

fn tool(ui: &mut Ui, label: impl Into<egui::WidgetText>, tip: &str) -> bool {
    ui.add(egui::Button::new(label).frame_when_inactive(false))
        .on_hover_text(tip)
        .clicked()
}

fn separator(ui: &mut Ui) {
    ui.add_space(4.0);
    ui.separator();
    ui.add_space(4.0);
}

fn command_bar(app: &mut TextApp, ui: &mut Ui) {
    ui.spacing_mut().item_spacing.x = 2.0;
    let markdown = app.kind == Kind::Markdown;
    let tab = app.ribbon_tab.clone().unwrap_or_default();
    let mut clicked: Option<(&'static str, Value)> = None;
    fn button(
        ui: &mut Ui,
        clicked: &mut Option<(&'static str, Value)>,
        label: &str,
        tip: &str,
        id: &'static str,
        params: Value,
    ) {
        if tool(ui, label, tip) {
            *clicked = Some((id, params));
        }
    }
    /// A drop-down of commands: `(label, command, params)`, labels translated.
    fn menu(
        ui: &mut Ui,
        clicked: &mut Option<(&'static str, Value)>,
        zh: bool,
        label: &str,
        items: &[(&str, &'static str, Value)],
    ) {
        ui.scope(|ui| {
            // Frameless like the other tools until hovered.
            let idle = &mut ui.style_mut().visuals.widgets.inactive;
            idle.weak_bg_fill = Color32::TRANSPARENT;
            idle.bg_stroke = Stroke::NONE;
            // Trailing room for a painted chevron; the UI font has no arrow glyph.
            let response = ui
                .menu_button(format!("{}    ", crate::i18n::tr(zh, label)), |ui| {
                    for (name, id, params) in items {
                        if ui.button(crate::i18n::tr(zh, name)).clicked() {
                            *clicked = Some((id, params.clone()));
                        }
                    }
                })
                .response;
            let c = pos2(response.rect.right() - 9.0, response.rect.center().y + 1.0);
            ui.painter().add(Shape::convex_polygon(
                vec![c + vec2(-3.5, -2.0), c + vec2(3.5, -2.0), c + vec2(0.0, 2.0)],
                ui.visuals().weak_text_color(),
                Stroke::NONE,
            ));
        });
    }
    let zh = app.zh;
    let line_tools = || {
        vec![
            ("升序排列", "edit.sortLines", json!({})),
            ("降序排列", "edit.sortLines", json!({"descending": true})),
            ("删除重复行", "edit.uniqueLines", json!({})),
            ("反转行序", "edit.reverseLines", json!({})),
            ("合并行", "edit.joinLines", json!({})),
        ]
    };
    let case_tools = || {
        vec![
            ("大写", "edit.upperCase", json!({})),
            ("小写", "edit.lowerCase", json!({})),
            ("首字母大写", "edit.titleCase", json!({})),
        ]
    };
    let space_tools = || {
        vec![
            ("删除行尾空格", "edit.trimTrailing", json!({})),
            ("缩进转为空格", "edit.indentToSpaces", json!({})),
            ("缩进转为 Tab", "edit.indentToTabs", json!({})),
        ]
    };
    match (tab.as_str(), markdown) {
        ("home", true) => {
            let level = current_heading(app);
            let names = [
                "正文", "标题 1", "标题 2", "标题 3", "标题 4", "标题 5", "标题 6",
            ];
            let zh = app.zh;
            egui::ComboBox::from_id_salt("weave-text-heading")
                .width(86.0)
                .selected_text(crate::i18n::tr(zh, names[level as usize]))
                .show_ui(ui, |ui| {
                    for (index, name) in names.iter().enumerate() {
                        if ui
                            .selectable_label(index == level as usize, crate::i18n::tr(zh, name))
                            .clicked()
                        {
                            clicked = Some(("format.heading", json!({"level": index})));
                        }
                    }
                });
            separator(ui);
            fn styled(
                ui: &mut Ui,
                clicked: &mut Option<(&'static str, Value)>,
                label: RichText,
                tip: &str,
                id: &'static str,
            ) {
                if tool(ui, label, tip) {
                    *clicked = Some((id, json!({})));
                }
            }
            styled(
                ui,
                &mut clicked,
                RichText::new("B").strong(),
                &format!("{} (⌘B)", tr(app, "粗体")),
                "format.bold",
            );
            styled(
                ui,
                &mut clicked,
                RichText::new("I").italics(),
                &format!("{} (⌘I)", tr(app, "斜体")),
                "format.italic",
            );
            styled(
                ui,
                &mut clicked,
                RichText::new("S").strikethrough(),
                &format!("{} (⇧⌘X)", tr(app, "删除线")),
                "format.strike",
            );
            styled(
                ui,
                &mut clicked,
                RichText::new("H")
                    .background_color(Color32::from_rgba_unmultiplied(255, 210, 60, 120)),
                &format!("{} (⇧⌘H)", tr(app, "高亮")),
                "format.highlight",
            );
            styled(
                ui,
                &mut clicked,
                RichText::new("</>").monospace(),
                &format!("{} (⌘E)", tr(app, "行内代码")),
                "format.code",
            );
            styled(
                ui,
                &mut clicked,
                RichText::new(tr(app, "链接")),
                &format!("{} (⌘K)", tr(app, "链接")),
                "format.link",
            );
            styled(
                ui,
                &mut clicked,
                RichText::new("[[ ]]"),
                tr(app, "双链"),
                "format.wikilink",
            );
            separator(ui);
            styled(
                ui,
                &mut clicked,
                RichText::new("• ≡"),
                tr(app, "无序列表"),
                "format.bullet",
            );
            styled(
                ui,
                &mut clicked,
                RichText::new("1. ≡"),
                tr(app, "有序列表"),
                "format.numbered",
            );
            styled(
                ui,
                &mut clicked,
                RichText::new("☐ ≡"),
                &format!("{} (⌘↩)", tr(app, "任务")),
                "format.task",
            );
            styled(
                ui,
                &mut clicked,
                RichText::new("❝"),
                tr(app, "引用"),
                "format.quote",
            );
            separator(ui);
            styled(
                ui,
                &mut clicked,
                RichText::new(tr(app, "查找")),
                &format!("{} (⌘F)", tr(app, "查找")),
                "edit.find",
            );
            styled(
                ui,
                &mut clicked,
                RichText::new(tr(app, "替换")),
                &format!("{} (⌥⌘F)", tr(app, "替换")),
                "edit.replace",
            );
        }
        ("home", false) => {
            button(
                ui,
                &mut clicked,
                tr(app, "缩进"),
                "Tab",
                "edit.indent",
                json!({}),
            );
            button(
                ui,
                &mut clicked,
                tr(app, "减少缩进"),
                "⇧Tab",
                "edit.outdent",
                json!({}),
            );
            button(
                ui,
                &mut clicked,
                tr(app, "注释"),
                "⌘/",
                "edit.toggleComment",
                json!({}),
            );
            separator(ui);
            button(
                ui,
                &mut clicked,
                tr(app, "上移行"),
                "⌥↑",
                "edit.moveLineUp",
                json!({}),
            );
            button(
                ui,
                &mut clicked,
                tr(app, "下移行"),
                "⌥↓",
                "edit.moveLineDown",
                json!({}),
            );
            button(
                ui,
                &mut clicked,
                tr(app, "复制行"),
                "⇧⌥↓",
                "edit.duplicateLine",
                json!({}),
            );
            button(
                ui,
                &mut clicked,
                tr(app, "删除行"),
                "⇧⌘K",
                "edit.deleteLine",
                json!({}),
            );
            separator(ui);
            button(
                ui,
                &mut clicked,
                tr(app, "查找"),
                "⌘F",
                "edit.find",
                json!({}),
            );
            button(
                ui,
                &mut clicked,
                tr(app, "替换"),
                "⌥⌘F",
                "edit.replace",
                json!({}),
            );
            button(
                ui,
                &mut clicked,
                tr(app, "转到行"),
                "⌘L",
                "edit.goToLine",
                json!({}),
            );
            separator(ui);
            menu(ui, &mut clicked, zh, "行", &line_tools());
            menu(ui, &mut clicked, zh, "大小写", &case_tools());
            menu(ui, &mut clicked, zh, "空白", &space_tools());
            if app.language.name == "JSON" {
                separator(ui);
                button(
                    ui,
                    &mut clicked,
                    tr(app, "格式化 JSON"),
                    "",
                    "format.json",
                    json!({}),
                );
                button(
                    ui,
                    &mut clicked,
                    tr(app, "压缩 JSON"),
                    "",
                    "format.jsonMinify",
                    json!({}),
                );
            }
        }
        ("insert", _) => {
            button(
                ui,
                &mut clicked,
                tr(app, "代码块"),
                "```",
                "insert.codeBlock",
                json!({}),
            );
            button(
                ui,
                &mut clicked,
                tr(app, "表格"),
                "| |",
                "insert.table",
                json!({}),
            );
            button(
                ui,
                &mut clicked,
                tr(app, "分隔线"),
                "---",
                "insert.rule",
                json!({}),
            );
            separator(ui);
            button(
                ui,
                &mut clicked,
                tr(app, "链接"),
                "⌘K",
                "format.link",
                json!({}),
            );
            button(
                ui,
                &mut clicked,
                tr(app, "双链"),
                "[[ ]]",
                "format.wikilink",
                json!({}),
            );
            button(
                ui,
                &mut clicked,
                tr(app, "任务"),
                "- [ ]",
                "format.task",
                json!({}),
            );
            separator(ui);
            button(
                ui,
                &mut clicked,
                tr(app, "图片"),
                "![]()",
                "insert.image",
                json!({}),
            );
            button(
                ui,
                &mut clicked,
                tr(app, "脚注"),
                "[^1]",
                "insert.footnote",
                json!({}),
            );
            menu(
                ui,
                &mut clicked,
                zh,
                "标注块",
                &[
                    ("备注", "insert.callout", json!({"kind": "note"})),
                    ("提示", "insert.callout", json!({"kind": "tip"})),
                    ("重要", "insert.callout", json!({"kind": "important"})),
                    ("警告", "insert.callout", json!({"kind": "warning"})),
                    ("危险", "insert.callout", json!({"kind": "danger"})),
                    ("引文", "insert.callout", json!({"kind": "quote"})),
                ],
            );
            menu(
                ui,
                &mut clicked,
                zh,
                "公式",
                &[
                    ("行内公式", "insert.math", json!({})),
                    ("公式块", "insert.mathBlock", json!({})),
                ],
            );
            menu(
                ui,
                &mut clicked,
                zh,
                "日期时间",
                &[
                    ("日期", "insert.date", json!({})),
                    ("时间", "insert.time", json!({})),
                    ("日期和时间", "insert.dateTime", json!({})),
                ],
            );
            button(
                ui,
                &mut clicked,
                tr(app, "属性"),
                "---",
                "insert.properties",
                json!({}),
            );
        }
        _ => {
            if markdown {
                for (value, label, view) in [
                    ("live", "实时预览", View::Live),
                    ("source", "源码", View::Source),
                    ("reading", "阅读", View::Reading),
                ] {
                    if ui
                        .selectable_label(app.view == view, tr(app, label))
                        .clicked()
                    {
                        clicked = Some(("view.mode", json!({"value": value})));
                    }
                }
            } else if ui
                .selectable_label(app.line_numbers, tr(app, "行号"))
                .clicked()
            {
                clicked = Some(("view.lineNumbers", json!({})));
            }
            if ui.selectable_label(app.wrap, tr(app, "自动换行")).clicked() {
                clicked = Some(("view.wrap", json!({})));
            }
            if ui
                .selectable_label(app.focus_mode, tr(app, "专注模式"))
                .clicked()
            {
                clicked = Some(("view.focus", json!({})));
            }
            if ui
                .selectable_label(app.typewriter, tr(app, "打字机滚动"))
                .clicked()
            {
                clicked = Some(("view.typewriter", json!({})));
            }
            separator(ui);
            for pane in crate::sidebar::Pane::ALL {
                if pane.available(app.kind)
                    && ui
                        .selectable_label(app.sidebar.pane == Some(pane), tr(app, pane.label()))
                        .clicked()
                {
                    clicked = Some(("view.sidebar", json!({"pane": pane.name(), "toggle": true})));
                }
            }
            separator(ui);
            button(
                ui,
                &mut clicked,
                tr(app, "放大"),
                "",
                "view.zoomIn",
                json!({}),
            );
            button(
                ui,
                &mut clicked,
                tr(app, "缩小"),
                "",
                "view.zoomOut",
                json!({}),
            );
            button(
                ui,
                &mut clicked,
                tr(app, "实际大小"),
                "",
                "view.zoomReset",
                json!({}),
            );
            separator(ui);
            button(
                ui,
                &mut clicked,
                tr(app, "命令面板"),
                "⇧⌘P",
                "palette.commands",
                json!({}),
            );
            button(
                ui,
                &mut clicked,
                tr(app, "快速打开"),
                "⌥⌘O",
                "palette.files",
                json!({}),
            );
        }
    }
    if let Some((id, params)) = clicked {
        run(app, id, params);
    }
}

fn current_heading(app: &TextApp) -> u8 {
    let at = app.selection().head.min(app.text.len());
    let start = edit::line_start(&app.text, at);
    let line = &app.text[start..edit::line_end(&app.text, at)];
    let hashes = line.chars().take_while(|&c| c == '#').count();
    if (1..=6).contains(&hashes) && line[hashes..].starts_with(' ') {
        hashes as u8
    } else {
        0
    }
}

fn find_bar(app: &mut TextApp, ui: &mut Ui) {
    let focus = std::mem::take(&mut app.find.focus);
    let mut close = false;
    if let Some(mut line) = app.find.goto.take() {
        ui.horizontal(|ui| {
            ui.label(tr(app, "行号:"));
            let response = ui.add(egui::TextEdit::singleline(&mut line).desired_width(80.0));
            if focus {
                response.request_focus();
            }
            let go = (response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)))
                || ui.button(tr(app, "跳转")).clicked();
            let count = app.text.matches('\n').count() + 1;
            ui.weak(tr(app, "{count} 行").replace("{count}", &count.to_string()));
            if go {
                if let Ok(number) = line.trim().parse::<usize>() {
                    let at = edit::offset_of_line(&app.text, number.max(1));
                    app.select(Selection::caret(at));
                    app.focus_text = true;
                }
                close = true;
            } else if ui.input(|i| i.key_pressed(Key::Escape)) || tool(ui, "✕", tr(app, "关闭"))
            {
                close = true;
            } else {
                app.find.goto = Some(line);
            }
        });
        if close {
            app.find.open = false;
            app.focus_text = true;
        }
        return;
    }
    let (find_hint, replace_hint) = (tr(app, "查找"), tr(app, "替换"));
    let mut step: Option<bool> = None;
    let mut replace_one = false;
    let mut replace_all = false;
    ui.horizontal(|ui| {
        let response = ui.add(
            egui::TextEdit::singleline(&mut app.find.query)
                .desired_width(220.0)
                .hint_text(find_hint),
        );
        if focus {
            response.request_focus();
        }
        if response.changed() {
            // Search as you type, from where the selection starts.
            let start = app.selection().range().start;
            app.select(Selection::caret(start));
            step = Some(true);
        }
        if response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
            step = Some(!ui.input(|i| i.modifiers.shift));
            response.request_focus();
        }
        ui.toggle_value(&mut app.find.match_case, "Aa")
            .on_hover_text(tr(app, "区分大小写"));
        ui.toggle_value(&mut app.find.whole_word, "ab|")
            .on_hover_text(tr(app, "全词匹配"));
        app.refresh_matches();
        let count = app.find.matches.len();
        let label = if app.find.query.is_empty() {
            String::new()
        } else if count == 0 {
            tr(app, "无结果").to_owned()
        } else {
            format!("{}/{count}", app.find.current.map_or(0, |c| c + 1))
        };
        ui.weak(label);
        if tool(ui, "↑", tr(app, "上一个")) {
            step = Some(false);
        }
        if tool(ui, "↓", tr(app, "下一个")) {
            step = Some(true);
        }
        if !app.find.replace && tool(ui, tr(app, "替换"), tr(app, "替换")) {
            app.find.replace = true;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if tool(ui, "✕", tr(app, "关闭")) {
                close = true;
            }
        });
    });
    if app.find.replace && !(app.kind == Kind::Markdown && app.view == View::Reading) {
        ui.horizontal(|ui| {
            let response = ui.add(
                egui::TextEdit::singleline(&mut app.find.replacement)
                    .desired_width(220.0)
                    .hint_text(replace_hint),
            );
            if response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                replace_one = true;
                response.request_focus();
            }
            if ui.button(tr(app, "替换")).clicked() {
                replace_one = true;
            }
            if ui.button(tr(app, "全部替换")).clicked() {
                replace_all = true;
            }
        });
    }
    if ui.input(|i| i.key_pressed(Key::Escape)) {
        close = true;
    }
    if let Some(forward) = step {
        app.find_step(forward);
    }
    if replace_one {
        app.replace_current();
    }
    if replace_all {
        let count = app.replace_all();
        app.notice = Some(tr(app, "已替换 {count} 处").replace("{count}", &count.to_string()));
    }
    if close {
        app.find.open = false;
        app.find.matches.clear();
        app.find.searched = None;
        app.focus_text = true;
    }
}

fn status_bar(app: &mut TextApp, ui: &mut Ui) {
    ui.horizontal_centered(|ui| {
        ui.spacing_mut().item_spacing.x = 14.0;
        let small = |text: String| RichText::new(text).size(11.5);
        let selection = app.selection();
        let head = selection.head.min(app.text.len());
        let (line, column) = edit::line_column(&app.text, head);
        if let Some(notice) = &app.notice {
            ui.label(small(notice.clone()).color(ui.visuals().warn_fg_color));
        }
        ui.label(small(
            tr(app, "行 {line},列 {column}")
                .replace("{line}", &line.to_string())
                .replace("{column}", &column.to_string()),
        ));
        if !selection.is_empty() {
            let count = app.text[selection.range()].chars().count();
            ui.label(small(
                tr(app, "已选 {count} 字符").replace("{count}", &count.to_string()),
            ));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 14.0;
            let format = app.format;
            let zh = app.zh;
            let mut chosen: Option<(&'static str, Value)> = None;
            ui.menu_button(small(format.line_ending.label().into()), |ui| {
                ui.label(crate::i18n::tr(zh, "换行符"));
                if ui.button("LF").clicked() {
                    chosen = Some(("file.lineEnding", json!({"value": "lf"})));
                }
                if ui.button("CRLF").clicked() {
                    chosen = Some(("file.lineEnding", json!({"value": "crlf"})));
                }
            });
            let encoding = format!(
                "{}{}",
                format.encoding.label(),
                if format.bom { " BOM" } else { "" }
            );
            ui.menu_button(small(encoding), |ui| {
                ui.label(crate::i18n::tr(zh, "编码"));
                for (label, value, bom) in [
                    ("UTF-8", "utf-8", false),
                    ("UTF-8 BOM", "utf-8", true),
                    ("UTF-16 LE", "utf-16le", true),
                    ("GBK", "gbk", false),
                    ("GB18030", "gb18030", false),
                    ("Big5", "big5", false),
                    ("Shift_JIS", "shift_jis", false),
                    ("Windows-1252", "windows-1252", false),
                ] {
                    if ui.button(label).clicked() {
                        chosen = Some(("file.encoding", json!({"value": value, "bom": bom})));
                    }
                }
            });
            if app.kind == Kind::PlainText {
                ui.menu_button(
                    small(crate::i18n::tr(zh, app.language.name).to_owned()),
                    |ui| {
                        egui::ScrollArea::vertical()
                            .max_height(320.0)
                            .show(ui, |ui| {
                                for language in
                                    syntax::LANGUAGES.iter().filter(|l| l.name != "Markdown")
                                {
                                    if ui
                                        .selectable_label(
                                            language.name == app.language.name,
                                            crate::i18n::tr(zh, language.name),
                                        )
                                        .clicked()
                                    {
                                        chosen =
                                            Some(("language.set", json!({"value": language.name})));
                                    }
                                }
                            });
                    },
                );
            } else {
                let words = word_count(&app.text);
                ui.label(small(
                    tr(app, "{count} 字").replace("{count}", &words.to_string()),
                ));
            }
            if let Some((id, params)) = chosen {
                run(app, id, params);
            }
        });
    });
}

/// Words the way writing apps count them: each CJK character is a word, other words are
/// runs of letters and digits.
pub fn word_count(text: &str) -> usize {
    let mut count = 0;
    let mut in_word = false;
    for c in text.chars() {
        let cjk = matches!(c as u32, 0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF);
        if cjk {
            count += 1;
            in_word = false;
        } else if c.is_alphanumeric() {
            if !in_word {
                count += 1;
            }
            in_word = true;
        } else if c != '\'' && c != '’' {
            in_word = false;
        }
    }
    count
}

pub(crate) fn outline(app: &mut TextApp, ui: &mut Ui) {
    let headings: Vec<_> = app
        .cache
        .parsed
        .as_ref()
        .map(|p| {
            p.headings
                .iter()
                .map(|h| (h.level, h.title.clone(), h.start))
                .collect()
        })
        .unwrap_or_default();
    if headings.is_empty() {
        ui.weak(tr(app, "没有标题"));
        return;
    }
    let head = app.selection().head;
    let current = headings.iter().rposition(|(_, _, start)| *start <= head);
    let min_level = headings
        .iter()
        .map(|(level, _, _)| *level)
        .min()
        .unwrap_or(1);
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            for (index, (level, title, start)) in headings.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.add_space(f32::from(level - min_level) * 14.0);
                    let text = if title.is_empty() {
                        "—".to_owned()
                    } else {
                        title.clone()
                    };
                    if ui.selectable_label(current == Some(index), text).clicked() {
                        app.select(Selection::caret(*start));
                        app.focus_text = true;
                    }
                });
            }
        });
}

/// The byte ranges of the lines the cursor or selection is on, while the text has focus.
fn revealed_lines(app: &TextApp, focused: bool) -> Vec<Range<usize>> {
    if !focused {
        return Vec::new();
    }
    let selection = app.selection();
    let text = &app.text;
    let range = selection.range();
    let range = range.start.min(text.len())..range.end.min(text.len());
    let mut lines = vec![edit::line_start(text, range.start)..edit::line_end(text, range.end)];
    // Inside a code block its fences show too, as in Obsidian.
    if let Some(parsed) = &app.cache.parsed
        && app.cache.source.len() == text.len()
    {
        for decoration in &parsed.decorations {
            if let Decoration::CodeBlock { range: block, .. } = decoration
                && block.start <= range.end
                && range.start <= block.end
            {
                lines.push(block.clone());
            }
        }
        for table in &parsed.tables {
            if table.range.start <= range.end && range.start <= table.range.end {
                lines.push(table.range.clone());
            }
        }
    }
    lines
}

fn editor(app: &mut TextApp, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    let text_id = app.text_id();
    let markdown = app.kind == Kind::Markdown;
    let reading = markdown && app.view == View::Reading;
    let focused = ctx.memory(|m| m.has_focus(text_id));
    let (bold, mono) = families(&ctx);
    let palette = Palette::new(ui.visuals().dark_mode, ui.visuals().text_color());
    let size = app.font_size;
    let mono_size = if markdown { size * 0.92 } else { size };
    let char_width = ctx
        .fonts_mut(|f| f.glyph_width(&FontId::new(mono_size, mono.clone()), '0'))
        .max(4.0);
    let line_count = app.text.matches('\n').count() + 1;
    let gutter = if !markdown && app.line_numbers {
        (line_count.to_string().len().max(3) as f32) * char_width + 24.0
    } else {
        0.0
    };
    let available = ui.available_width();
    let (side, width) = if markdown {
        let readable = 46.0 * size;
        let side = ((available - readable) / 2.0).max(28.0);
        (side, (available - 2.0 * side).max(120.0))
    } else {
        (10.0, (available - gutter - 20.0).max(120.0))
    };
    let wrap = app.wrap || markdown;
    let reveal_lines = revealed_lines(app, focused);
    let style = Style {
        palette,
        size,
        mono: mono.clone(),
        bold,
        proportional: FontFamily::Proportional,
        wrap_width: if wrap { width } else { f32::INFINITY },
    };
    let view = app.view;
    let kind = app.kind;
    let language = app.language;
    let scroll = egui::ScrollArea::new([!wrap, true])
        .id_salt(app.id.with("scroll"))
        .auto_shrink(false);
    // A newly opened document starts with the caret at the top.
    if egui::TextEdit::load_state(&ctx, text_id).is_none() {
        let mut state = egui::text_edit::TextEditState::default();
        state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::one(CCursor::new(0))));
        state.store(&ctx, text_id);
    }
    let scroll_to = app.scroll_to.take();
    let focus_text = std::mem::take(&mut app.focus_text);
    let mut clicked_task: Option<usize> = None;
    let mut clicked_link: Option<markdown::Link> = None;
    let mut rendered_tables = Vec::new();
    scroll.show(ui, |ui| {
        let background = ui.painter().add(Shape::Noop);
        let height = ui.available_height();
        let output = ui
            .horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.add_space(gutter + side);
                let cache = &mut app.cache;
                let reveal_lines = &reveal_lines;
                let mut layouter =
                    |ui: &Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| -> Arc<Galley> {
                        let text = buf.as_str();
                        cache.update(text, kind, language);
                        let mut style = style.clone();
                        style.wrap_width = if wrap { wrap_width } else { f32::INFINITY };
                        rendered_tables.clear();
                        let job = match (kind, &cache.parsed) {
                            (Kind::Markdown, Some(parsed)) => {
                                let reveal = match view {
                                    View::Source => Reveal::All,
                                    View::Reading => Reveal::None,
                                    View::Live => Reveal::Lines(reveal_lines),
                                };
                                let mut job = layout::markdown_job(text, parsed, reveal, &style);
                                rendered_tables = crate::tables::prepare(
                                    ui, &mut job, parsed, reveal, &style,
                                );
                                job
                            }
                            _ => layout::code_job(text, &cache.tokens, &style),
                        };
                        ui.fonts_mut(|f| f.layout_job(job))
                    };
                let edit = egui::TextEdit::multiline(&mut app.text)
                    .id(text_id)
                    .frame(egui::Frame::NONE)
                    .margin(egui::Margin::symmetric(0, if markdown { 28 } else { 6 }))
                    .desired_width(if wrap {
                        width
                    } else {
                        width.max(available - gutter - side)
                    })
                    .min_size(vec2(0.0, height))
                    .lock_focus(true)
                    .interactive(!reading)
                    .layouter(&mut layouter);
                edit.show(ui)
            })
            .inner;
        if app.typewriter {
            // Room below the last line so it, too, can sit in the middle.
            ui.add_space(height * 0.5);
        }
        if output.response.changed() {
            app.changed();
        }
        if output.response.changed() || app.undoer.is_in_flux() {
            app.typed(ui.input(|i| i.time));
        }
        if focus_text && !reading {
            output.response.request_focus();
        }
        let galley = output.galley.clone();
        let origin = output.galley_pos;
        let clip = ui.clip_rect();
        let cache = &app.cache;
        let row_top = |at: usize| galley.pos_from_cursor(cache.cursor(at)).top();
        let rect_of = |at: usize| {
            rendered_tables
                .iter()
                .find_map(|table| table.rect_at(at, row_top))
                .unwrap_or_else(|| galley.pos_from_cursor(cache.cursor(at)))
                .translate(origin.to_vec2())
        };
        let mut shapes: Vec<Shape> = Vec::new();
        let mut over: Vec<Shape> = Vec::new();
        for table in &rendered_tables {
            for mut shape in table.shapes(row_top, &style) {
                shape.translate(origin.to_vec2());
                if matches!(shape, Shape::Text(_)) {
                    over.push(shape);
                } else {
                    shapes.push(shape);
                }
            }
        }
        // The current line (code) and find matches go behind the text.
        if !markdown && output.response.has_focus() {
            let at = app.selection().head;
            let row = rect_of(at);
            shapes.push(Shape::rect_filled(
                Rect::from_x_y_ranges(clip.left()..=clip.right(), row.top()..=row.bottom()),
                0.0,
                palette.line_bg,
            ));
        }
        if app.find.open && !app.find.matches.is_empty() {
            let current = app
                .find
                .current
                .and_then(|c| app.find.matches.get(c))
                .cloned();
            for m in app.find.matches.iter().take(5000) {
                let start = rect_of(m.start);
                if start.bottom() < clip.top() || start.top() > clip.bottom() {
                    continue;
                }
                let end = rect_of(m.end);
                let right = if (end.top() - start.top()).abs() < 1.0 {
                    end.left()
                } else {
                    start.left() + 8.0
                };
                let color = if Some(m) == current.as_ref() {
                    palette.current_match_bg
                } else {
                    palette.match_bg
                };
                shapes.push(Shape::rect_filled(
                    Rect::from_min_max(
                        pos2(start.left(), start.top()),
                        pos2(right.max(start.left() + 2.0), start.bottom()),
                    ),
                    2.0,
                    color,
                ));
            }
        }
        // Live-preview decorations.
        if markdown
            && view != View::Source
            && let Some(parsed) = &cache.parsed
        {
            let left = origin.x;
            let right = origin.x + width;
            let revealed = |range: &Range<usize>| {
                reveal_lines
                    .iter()
                    .any(|r| r.start <= range.end && range.start <= r.end)
            };
            for decoration in &parsed.decorations {
                match decoration {
                    Decoration::CodeBlock { range, label } => {
                        let top = rect_of(range.start).top();
                        let bottom = rect_of(range.end).bottom();
                        if bottom < clip.top() || top > clip.bottom() {
                            continue;
                        }
                        let block = Rect::from_min_max(
                            pos2(left - 10.0, top - 6.0),
                            pos2(right + 10.0, bottom + 6.0),
                        );
                        shapes.push(Shape::rect_filled(block, 6.0, palette.code_bg));
                        if !label.is_empty() && !revealed(range) {
                            over.push(ui.fonts_mut(|f| {
                                Shape::text(
                                    f,
                                    pos2(block.right() - 10.0, block.top() + 6.0),
                                    egui::Align2::RIGHT_TOP,
                                    label,
                                    FontId::new(size * 0.75, FontFamily::Proportional),
                                    palette.dim,
                                )
                            }));
                        }
                    }
                    Decoration::Quote { line, depth } => {
                        let top = rect_of(line.start).top();
                        let bottom = rect_of(line.end).bottom();
                        if bottom < clip.top() || top > clip.bottom() {
                            continue;
                        }
                        for level in 0..*depth {
                            let x = left + 2.0 + level as f32 * 14.0;
                            over.push(Shape::rect_filled(
                                Rect::from_min_max(pos2(x, top), pos2(x + 3.0, bottom)),
                                1.5,
                                palette.quote,
                            ));
                        }
                    }
                    Decoration::Rule(range) if !revealed(range) => {
                        let row = rect_of(range.start);
                        let y = row.center().y;
                        over.push(Shape::line_segment(
                            [pos2(left, y), pos2(right, y)],
                            Stroke::new(1.0, palette.dim.gamma_multiply(0.6)),
                        ));
                    }
                    Decoration::Bullet(range) if !revealed(range) => {
                        let rect = rect_of(range.start).union(rect_of(range.end));
                        let center = pos2(rect.center().x, rect.center().y + 0.5);
                        over.push(Shape::circle_filled(
                            center,
                            (size * 0.17).max(2.0),
                            palette.text.gamma_multiply(0.85),
                        ));
                    }
                    Decoration::Task { marker, checked } if !revealed(marker) => {
                        let start = rect_of(marker.start);
                        let end = rect_of(marker.end);
                        let side_len = (start.height() * 0.62).clamp(10.0, 20.0);
                        let center =
                            pos2((start.left() + end.left()) / 2.0, start.center().y + 0.5);
                        let check = Rect::from_center_size(center, Vec2::splat(side_len));
                        let response = ui.interact(
                            check.expand(3.0),
                            app.id.with(("task", marker.start)),
                            Sense::click(),
                        );
                        if response.clicked() && !reading {
                            clicked_task = Some(marker.start);
                        }
                        let hovered = response.hovered();
                        if *checked {
                            over.push(Shape::rect_filled(check, 3.0, palette.accent));
                            let a = pos2(check.left() + side_len * 0.22, check.center().y);
                            let b = pos2(
                                check.left() + side_len * 0.42,
                                check.bottom() - side_len * 0.25,
                            );
                            let c = pos2(
                                check.right() - side_len * 0.2,
                                check.top() + side_len * 0.25,
                            );
                            over.push(Shape::line(vec![a, b, c], Stroke::new(1.8, Color32::WHITE)));
                        } else {
                            over.push(Shape::rect_stroke(
                                check,
                                3.0,
                                Stroke::new(
                                    1.3,
                                    if hovered { palette.accent } else { palette.dim },
                                ),
                                egui::StrokeKind::Inside,
                            ));
                        }
                    }
                    _ => {}
                }
            }
            // ⌘-click opens links.
            let command = ui.input(|i| i.modifiers.command);
            if let Some(pointer) = ui.input(|i| i.pointer.hover_pos())
                && output.response.rect.contains(pointer)
            {
                let table_byte = rendered_tables
                    .iter()
                    .find_map(|table| table.byte_at(pointer - origin.to_vec2(), row_top));
                let byte = table_byte.unwrap_or_else(|| {
                    let at = galley.cursor_from_pos(pointer - origin);
                    byte_of_char(&app.text, at.index.0)
                });
                if let Some(byte) = table_byte
                    && !reading
                    && ui.input(|i| i.pointer.primary_pressed())
                    && !command
                {
                    let mut state = output.state.clone();
                    state.cursor.set_char_range(Some(egui::text::CCursorRange::one(
                        cache.cursor(byte),
                    )));
                    state.store(&ctx, text_id);
                    output.response.request_focus();
                    ctx.request_repaint();
                }
                if let Some(link) = parsed
                    .links
                    .iter()
                    .find(|l| l.range.start <= byte && byte < l.range.end)
                {
                    if command {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        if output.response.clicked()
                            || (reading && ui.input(|i| i.pointer.primary_clicked()))
                        {
                            clicked_link = Some(link.clone());
                        }
                    } else {
                        output
                            .response
                            .response
                            .clone()
                            .on_hover_text_at_pointer(tr(app, "按住 ⌘ 点击打开链接"));
                    }
                    if reading && !command && ui.input(|i| i.pointer.primary_clicked()) {
                        clicked_link = Some(link.clone());
                    }
                }
            }
        }
        ui.painter().set(background, Shape::Vec(shapes));
        // Gutter line numbers.
        if gutter > 0.0 {
            let current_line =
                edit::line_column(&app.text, app.selection().head.min(app.text.len())).0;
            let painter = ui.painter();
            let font = FontId::new(mono_size * 0.92, mono.clone());
            let x = origin.x - side - 12.0;
            let mut number = 1;
            let mut new_line = true;
            for row in &galley.rows {
                if new_line {
                    let y = origin.y + row.pos.y;
                    if y + row.size.y >= clip.top() && y <= clip.bottom() {
                        let color = if number == current_line {
                            palette.text
                        } else {
                            palette.dim
                        };
                        painter.text(
                            pos2(x, y + row.size.y / 2.0),
                            egui::Align2::RIGHT_CENTER,
                            number.to_string(),
                            font.clone(),
                            color,
                        );
                    }
                    number += 1;
                }
                new_line = row.ends_with_newline;
            }
        }
        if !over.is_empty() {
            ui.painter().extend(over);
        }
        if let Some(at) = scroll_to {
            let target = rect_of(at.min(app.text.len()));
            ui.scroll_to_rect(target.expand2(vec2(40.0, 60.0)), None);
        }
        // Typewriter scrolling: when the caret moves or the text changes, bring its line to the
        // middle; scrolling with the wheel in between is left alone.
        if app.typewriter && output.response.has_focus() {
            let head = app.selection().head.min(app.text.len());
            let key = (head, app.generation);
            if app.typewriter_at != Some(key) {
                app.typewriter_at = Some(key);
                ui.scroll_to_rect(rect_of(head), Some(egui::Align::Center));
            }
        }
    });
    if let Some(marker) = clicked_task {
        let selection = app.selection();
        if let Some(mut toggle) = edit::toggle_task(&app.text, marker) {
            toggle.selection = selection;
            app.apply(toggle);
            app.focus_text = false;
        }
    }
    if let Some(link) = clicked_link {
        open_link(app, &ctx, &link);
    }
}

fn open_link(app: &mut TextApp, ctx: &egui::Context, link: &markdown::Link) {
    let target = link.target.trim();
    if !link.wiki
        && (target.starts_with("http://")
            || target.starts_with("https://")
            || target.starts_with("mailto:"))
    {
        ctx.open_url(egui::OpenUrl::new_tab(target));
        return;
    }
    if !link.wiki
        && let Some(anchor) = target.strip_prefix('#')
    {
        jump_to_heading(app, anchor);
        return;
    }
    let (file, anchor) = match target.split_once('#') {
        Some((file, anchor)) => (file, Some(anchor)),
        None => (target, None),
    };
    if file.is_empty() {
        if let Some(anchor) = anchor {
            jump_to_heading(app, anchor);
        }
        return;
    }
    let decoded = percent_decode(file);
    let base = app
        .path
        .as_deref()
        .and_then(std::path::Path::parent)
        .map(std::path::Path::to_path_buf);
    let mut candidates = Vec::new();
    let relative = std::path::PathBuf::from(&decoded);
    let with_extension = if relative.extension().is_none() {
        relative.with_extension("md")
    } else {
        relative.clone()
    };
    if let Some(base) = &base {
        candidates.push(base.join(&relative));
        candidates.push(base.join(&with_extension));
    }
    candidates.push(relative.clone());
    candidates.push(with_extension.clone());
    if let Some(found) = candidates.into_iter().find(|p| p.is_file()) {
        app.host_commands
            .push(("file.open".into(), json!({"path": found})));
    } else {
        app.notice = Some(tr(app, "找不到 {target}").replace("{target}", &decoded));
    }
}

fn jump_to_heading(app: &mut TextApp, anchor: &str) {
    let wanted = slug(anchor);
    let start = app
        .cache
        .parsed
        .as_ref()
        .and_then(|p| {
            p.headings
                .iter()
                .find(|h| slug(&h.title) == wanted || h.title == anchor)
        })
        .map(|h| h.start);
    if let Some(start) = start {
        app.select(Selection::caret(start));
        app.focus_text = true;
    }
}

fn slug(text: &str) -> String {
    text.trim()
        .to_lowercase()
        .chars()
        .filter_map(|c| {
            if c.is_whitespace() {
                Some('-')
            } else if c.is_alphanumeric() || c == '-' || c == '_' {
                Some(c)
            } else {
                None
            }
        })
        .collect()
}

pub(crate) fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Some(hex) = text.get(i + 1..i + 3)
            && let Ok(byte) = u8::from_str_radix(hex, 16)
        {
            out.push(byte);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| text.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_words_in_mixed_text() {
        assert_eq!(word_count("Hello, world! 你好世界 don't"), 7);
        assert_eq!(word_count(""), 0);
    }

    #[test]
    fn decodes_links() {
        assert_eq!(percent_decode("My%20Note.md"), "My Note.md");
        assert_eq!(percent_decode("%E4%B8%AD"), "中");
        assert_eq!(percent_decode("bad%zz"), "bad%zz");
        assert_eq!(slug("Hello World"), "hello-world");
    }

    #[test]
    fn cache_maps_bytes_to_chars() {
        let mut cache = Cache::default();
        cache.update("a中\nb文c", Kind::PlainText, &syntax::PLAIN);
        assert_eq!(cache.char_at(0), 0);
        assert_eq!(cache.char_at(4), 2);
        assert_eq!(cache.char_at(5), 3);
        assert_eq!(cache.char_at(9), 5);
        assert_eq!(cache.char_at(100), 6);
    }

    #[test]
    fn clicking_table_content_reveals_its_source_without_changing_text() {
        let text = "# Report\n\n| Item | Value |\n| --- | --- |\n| Date | 2026-09-20 |\n\nAfter";
        let mut app = TextApp::from_bytes(Kind::Markdown, "report.md", text.as_bytes()).unwrap();
        let context = egui::Context::default();
        let mut pointer = None;
        for frame in 0..4 {
            let mut input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(900.0, 700.0))),
                focused: true,
                ..Default::default()
            };
            if frame == 2 {
                let pos = pointer.expect("table value is painted as a separate cell");
                input.events = vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: Modifiers::NONE,
                    },
                ];
            }
            let output = context.run_ui(input, |ui| app.hosted_ui(ui));
            for shape in &output.shapes {
                if let Shape::Text(shape) = &shape.shape
                    && shape.galley.job.text == "2026-09-20"
                {
                    pointer = Some(shape.pos + vec2(1.0, shape.galley.size().y * 0.5));
                }
            }
            output.drop_without_applying_deltas();
            if frame >= 2 {
                assert_eq!(app.selection().head, text.find("2026").unwrap());
                let table = &app.cache.parsed.as_ref().unwrap().tables[0];
                assert!(revealed_lines(&app, true).contains(&table.range));
            }
            assert_eq!(app.text(), text);
            assert!(!app.dirty());
        }
    }
}
