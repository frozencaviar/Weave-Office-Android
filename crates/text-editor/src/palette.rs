//! The command palette (⇧⌘P) and quick open (⌥⌘O), as in VS Code and Obsidian: a box at the
//! top of the window that filters every command, or every file of the note's folder, by a
//! forgiving fuzzy match as you type.

use crate::sidebar;
use crate::syntax::LANGUAGES;
use crate::{Kind, TextApp};
use egui::{Color32, FontId, Key, Sense, Ui, vec2};
use serde_json::{Value, json};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Commands,
    Files,
}

#[derive(Debug)]
pub struct Palette {
    pub mode: Mode,
    pub query: String,
    selected: usize,
    focus: bool,
}

impl Palette {
    pub fn new(mode: Mode) -> Self {
        Self {
            mode,
            query: String::new(),
            selected: 0,
            focus: true,
        }
    }
}

/// Which documents a command applies to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Scope {
    All,
    Markdown,
    /// Plain text and code files.
    Text,
    Json,
}

/// `(Chinese label, English label, command, params, scope, shortcut)`.
type Entry = (
    &'static str,
    &'static str,
    &'static str,
    fn() -> Value,
    Scope,
    &'static str,
);

fn none() -> Value {
    json!({})
}

#[rustfmt::skip]
const COMMANDS: &[Entry] = &[
    ("撤销", "Undo", "edit.undo", none, Scope::All, "⌘Z"),
    ("重做", "Redo", "edit.redo", none, Scope::All, "⇧⌘Z"),
    ("全选", "Select all", "edit.selectAll", none, Scope::All, "⌘A"),
    ("查找", "Find", "edit.find", none, Scope::All, "⌘F"),
    ("替换", "Replace", "edit.replace", none, Scope::All, "⌥⌘F"),
    ("在文件夹中搜索", "Search in folder", "edit.searchFolder", none, Scope::All, "⇧⌘F"),
    ("转到行", "Go to line", "edit.goToLine", none, Scope::All, "⌘L"),
    ("快速打开文件", "Quick open file", "palette.files", none, Scope::All, "⌥⌘O"),
    ("增加缩进", "Indent", "edit.indent", none, Scope::All, "Tab"),
    ("减少缩进", "Outdent", "edit.outdent", none, Scope::All, "⇧Tab"),
    ("上移行", "Move line up", "edit.moveLineUp", none, Scope::All, "⌥↑"),
    ("下移行", "Move line down", "edit.moveLineDown", none, Scope::All, "⌥↓"),
    ("复制行", "Duplicate line", "edit.duplicateLine", none, Scope::All, "⇧⌥↓"),
    ("删除行", "Delete line", "edit.deleteLine", none, Scope::All, "⇧⌘K"),
    ("切换注释", "Toggle comment", "edit.toggleComment", none, Scope::All, "⌘/"),
    ("粗体", "Bold", "format.bold", none, Scope::Markdown, "⌘B"),
    ("斜体", "Italic", "format.italic", none, Scope::Markdown, "⌘I"),
    ("删除线", "Strikethrough", "format.strike", none, Scope::Markdown, "⇧⌘X"),
    ("高亮", "Highlight", "format.highlight", none, Scope::Markdown, "⇧⌘H"),
    ("行内代码", "Inline code", "format.code", none, Scope::Markdown, "⌘E"),
    ("插入链接", "Insert link", "format.link", none, Scope::Markdown, "⌘K"),
    ("插入双链", "Insert internal link", "format.wikilink", none, Scope::Markdown, ""),
    ("正文", "Body text", "format.paragraph", none, Scope::Markdown, "⌘0"),
    ("标题 1", "Heading 1", "format.heading", || json!({"level": 1}), Scope::Markdown, "⌘1"),
    ("标题 2", "Heading 2", "format.heading", || json!({"level": 2}), Scope::Markdown, "⌘2"),
    ("标题 3", "Heading 3", "format.heading", || json!({"level": 3}), Scope::Markdown, "⌘3"),
    ("标题 4", "Heading 4", "format.heading", || json!({"level": 4}), Scope::Markdown, "⌘4"),
    ("标题 5", "Heading 5", "format.heading", || json!({"level": 5}), Scope::Markdown, "⌘5"),
    ("标题 6", "Heading 6", "format.heading", || json!({"level": 6}), Scope::Markdown, "⌘6"),
    ("无序列表", "Bulleted list", "format.bullet", none, Scope::Markdown, ""),
    ("有序列表", "Numbered list", "format.numbered", none, Scope::Markdown, ""),
    ("任务列表", "Task list", "format.task", none, Scope::Markdown, "⌘↩"),
    ("引用", "Quote", "format.quote", none, Scope::Markdown, ""),
    ("插入代码块", "Insert code block", "insert.codeBlock", none, Scope::Markdown, ""),
    ("插入表格", "Insert table", "insert.table", none, Scope::Markdown, ""),
    ("插入分隔线", "Insert horizontal rule", "insert.rule", none, Scope::Markdown, ""),
    ("实时预览", "Live preview", "view.mode", || json!({"value": "live"}), Scope::Markdown, ""),
    ("源码模式", "Source mode", "view.mode", || json!({"value": "source"}), Scope::Markdown, ""),
    ("阅读模式", "Reading mode", "view.mode", || json!({"value": "reading"}), Scope::Markdown, ""),
    ("格式化 JSON", "Format JSON", "format.json", none, Scope::Json, ""),
    ("压缩 JSON", "Minify JSON", "format.jsonMinify", none, Scope::Json, ""),
    ("升序排列行", "Sort lines ascending", "edit.sortLines", none, Scope::All, ""),
    ("降序排列行", "Sort lines descending", "edit.sortLines", || json!({"descending": true}), Scope::All, ""),
    ("删除重复行", "Delete duplicate lines", "edit.uniqueLines", none, Scope::All, ""),
    ("反转行序", "Reverse lines", "edit.reverseLines", none, Scope::All, ""),
    ("合并行", "Join lines", "edit.joinLines", none, Scope::All, ""),
    ("转为大写", "Transform to uppercase", "edit.upperCase", none, Scope::All, ""),
    ("转为小写", "Transform to lowercase", "edit.lowerCase", none, Scope::All, ""),
    ("首字母大写", "Transform to title case", "edit.titleCase", none, Scope::All, ""),
    ("删除行尾空格", "Trim trailing whitespace", "edit.trimTrailing", none, Scope::All, ""),
    ("缩进转为空格", "Convert indentation to spaces", "edit.indentToSpaces", none, Scope::All, ""),
    ("缩进转为 Tab", "Convert indentation to tabs", "edit.indentToTabs", none, Scope::All, ""),
    ("插入图片", "Insert image", "insert.image", none, Scope::Markdown, ""),
    ("插入脚注", "Insert footnote", "insert.footnote", none, Scope::Markdown, ""),
    ("插入标注块", "Insert callout", "insert.callout", none, Scope::Markdown, ""),
    ("插入行内公式", "Insert inline math", "insert.math", none, Scope::Markdown, ""),
    ("插入公式块", "Insert math block", "insert.mathBlock", none, Scope::Markdown, ""),
    ("插入日期", "Insert date", "insert.date", none, Scope::All, ""),
    ("插入时间", "Insert time", "insert.time", none, Scope::All, ""),
    ("插入日期和时间", "Insert date and time", "insert.dateTime", none, Scope::All, ""),
    ("编辑属性", "Edit properties", "insert.properties", none, Scope::Markdown, ""),
    ("切换专注模式", "Toggle focus mode", "view.focus", none, Scope::All, ""),
    ("切换打字机滚动", "Toggle typewriter scrolling", "view.typewriter", none, Scope::All, ""),
    ("显示文件", "Show files", "view.sidebar", || json!({"pane": "files"}), Scope::All, ""),
    ("显示搜索", "Show search", "view.sidebar", || json!({"pane": "search"}), Scope::All, "⇧⌘F"),
    ("显示大纲", "Show outline", "view.sidebar", || json!({"pane": "outline"}), Scope::Markdown, ""),
    ("显示标签", "Show tags", "view.sidebar", || json!({"pane": "tags"}), Scope::Markdown, ""),
    ("显示反向链接", "Show backlinks", "view.sidebar", || json!({"pane": "backlinks"}), Scope::Markdown, ""),
    ("隐藏侧栏", "Hide sidebar", "view.sidebar", || json!({"pane": null}), Scope::All, ""),
    ("切换自动换行", "Toggle word wrap", "view.wrap", none, Scope::All, ""),
    ("切换行号", "Toggle line numbers", "view.lineNumbers", none, Scope::Text, ""),
    ("放大", "Zoom in", "view.zoomIn", none, Scope::All, ""),
    ("缩小", "Zoom out", "view.zoomOut", none, Scope::All, ""),
    ("实际大小", "Actual size", "view.zoomReset", none, Scope::All, ""),
    ("换行符改为 LF", "Line endings: LF", "file.lineEnding", || json!({"value": "lf"}), Scope::All, ""),
    ("换行符改为 CRLF", "Line endings: CRLF", "file.lineEnding", || json!({"value": "crlf"}), Scope::All, ""),
    ("保存", "Save", "file.save", none, Scope::All, "⌘S / ⇧F12"),
    ("另存为", "Save as", "file.saveAs", none, Scope::All, "⇧⌘S / F12"),
    ("打印", "Print", "file.print", none, Scope::All, "⌘P"),
    ("导出 PDF", "Export PDF", "file.exportPdf", none, Scope::All, ""),
];

/// One row of the palette.
#[derive(Clone, Debug)]
pub struct Item {
    pub label: String,
    /// Grey text after the label: a shortcut, or a file's folder.
    pub detail: String,
    /// Also matched, but not shown: the English name of a command.
    pub alias: String,
    pub action: Action,
}

#[derive(Clone, Debug)]
pub enum Action {
    Command(&'static str, Value),
    Open(PathBuf),
}

/// The commands that apply to the open document.
pub fn commands(app: &TextApp) -> Vec<Item> {
    let markdown = app.kind == Kind::Markdown;
    let json = app.language.name == "JSON";
    let mut items: Vec<Item> = COMMANDS
        .iter()
        .filter(|(.., scope, _)| match scope {
            Scope::All => true,
            Scope::Markdown => markdown,
            Scope::Text => !markdown,
            Scope::Json => json,
        })
        .map(|&(zh, en, id, params, _, shortcut)| Item {
            label: if app.zh { zh } else { en }.to_owned(),
            detail: shortcut.to_owned(),
            alias: format!("{en} {id}"),
            action: Action::Command(id, params()),
        })
        .collect();
    if !markdown {
        for language in LANGUAGES {
            if language.name == "Markdown" {
                continue;
            }
            let name = crate::i18n::tr(app.zh, language.name);
            items.push(Item {
                label: format!("{}{name}", crate::i18n::tr(app.zh, "语言：")),
                detail: String::new(),
                alias: format!("language {}", language.name),
                action: Action::Command("language.set", json!({"value": language.name})),
            });
        }
    }
    items
}

/// The files of the note's folder, as paths relative to its root.
pub fn files(app: &mut TextApp) -> Vec<Item> {
    if !sidebar::ensure_folder(app) {
        return Vec::new();
    }
    let Some(folder) = &app.sidebar.folder else {
        return Vec::new();
    };
    folder
        .files
        .iter()
        .map(|path| {
            let relative = path.strip_prefix(&folder.root).unwrap_or(path);
            let name = relative
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let dir = relative
                .parent()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            Item {
                label: name,
                alias: relative.to_string_lossy().into_owned(),
                detail: dir,
                action: Action::Open(path.clone()),
            }
        })
        .collect()
}

/// A fuzzy match of `query` in `text`: every query character in order, ignoring case.
/// Returns a score (higher is better) and the matched character positions of `text`.
pub fn fuzzy(query: &str, text: &str) -> Option<(i32, Vec<usize>)> {
    let query: Vec<char> = query
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    if query.is_empty() {
        return Some((0, Vec::new()));
    }
    let chars: Vec<char> = text.chars().collect();
    let lower: Vec<char> = chars
        .iter()
        .map(|c| c.to_lowercase().next().unwrap_or(*c))
        .collect();
    let mut positions: Vec<usize> = Vec::with_capacity(query.len());
    let mut score = 0;
    let mut at = 0;
    for &wanted in &query {
        let found = (at..lower.len()).find(|&i| lower[i] == wanted)?;
        let boundary = found == 0
            || matches!(
                chars[found - 1],
                ' ' | '/' | '\\' | '_' | '-' | '.' | '：' | ':'
            )
            || (chars[found].is_uppercase() && chars[found - 1].is_lowercase());
        score += 10;
        if boundary {
            score += 15;
        }
        if positions.last().is_some_and(|&last| last + 1 == found) {
            score += 12;
        } else if let Some(&last) = positions.last() {
            score -= (found - last - 1).min(10) as i32;
        } else {
            score -= found.min(10) as i32;
        }
        positions.push(found);
        at = found + 1;
    }
    score -= (chars.len() / 8) as i32;
    Some((score, positions))
}

/// `items` that match `query`, best first, with the matched characters of each label.
pub fn filter(items: &[Item], query: &str) -> Vec<(usize, Vec<usize>)> {
    let mut ranked: Vec<(i32, usize, Vec<usize>)> = items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            if let Some((score, positions)) = fuzzy(query, &item.label) {
                return Some((score + 5, index, positions));
            }
            fuzzy(query, &item.alias).map(|(score, _)| (score, index, Vec::new()))
        })
        .collect();
    // A stable sort keeps the table's order among equal scores.
    ranked.sort_by(|a, b| b.0.cmp(&a.0));
    ranked
        .into_iter()
        .map(|(_, index, positions)| (index, positions))
        .collect()
}

pub fn open(app: &mut TextApp, mode: Mode) {
    app.palette = Some(Palette::new(mode));
}

/// Draw the open palette and run what the user picks.
pub fn show(app: &mut TextApp, ctx: &egui::Context) {
    let Some(mode) = app.palette.as_ref().map(|p| p.mode) else {
        return;
    };
    let items = match mode {
        Mode::Commands => commands(app),
        Mode::Files => files(app),
    };
    let palette = app.palette.as_mut().expect("open palette");
    let matches = filter(&items, &palette.query);
    // Keys the list owns, taken before the text box sees them.
    let (up, down, enter, escape) = ctx.input_mut(|i| {
        (
            i.consume_key(egui::Modifiers::NONE, Key::ArrowUp),
            i.consume_key(egui::Modifiers::NONE, Key::ArrowDown),
            i.consume_key(egui::Modifiers::NONE, Key::Enter),
            i.consume_key(egui::Modifiers::NONE, Key::Escape),
        )
    });
    if down && !matches.is_empty() {
        palette.selected = (palette.selected + 1) % matches.len();
    }
    if up && !matches.is_empty() {
        palette.selected = (palette.selected + matches.len() - 1) % matches.len();
    }
    palette.selected = palette.selected.min(matches.len().saturating_sub(1));
    let hint = crate::i18n::tr(
        app.zh,
        match mode {
            Mode::Commands => "输入命令…",
            Mode::Files => "输入文件名…",
        },
    );
    let empty = crate::i18n::tr(
        app.zh,
        match mode {
            Mode::Commands => "没有匹配的命令",
            Mode::Files if items.is_empty() => "保存后可浏览所在文件夹",
            Mode::Files => "没有匹配的文件",
        },
    );
    let mut chosen = enter
        .then(|| matches.get(palette.selected).map(|(i, _)| *i))
        .flatten();
    let id = egui::Id::unique(("weave-text-palette", app.id));
    let width = 560.0_f32.min(ctx.content_rect().width() - 32.0);
    let area = egui::Modal::default_area(id)
        .anchor(egui::Align2::CENTER_TOP, vec2(0.0, 72.0))
        .default_width(width);
    let visuals = ctx.global_style().visuals.clone();
    let response = egui::Modal::new(id)
        .area(area)
        .backdrop_color(Color32::from_black_alpha(40))
        .frame(
            egui::Frame::popup(&ctx.global_style())
                .inner_margin(egui::Margin::same(8))
                .corner_radius(10),
        )
        .show(ctx, |ui| {
            ui.set_width(width);
            let edit = ui.add(
                egui::TextEdit::singleline(&mut palette.query)
                    .hint_text(hint)
                    .font(FontId::proportional(15.0))
                    .desired_width(f32::INFINITY)
                    .margin(vec2(8.0, 6.0)),
            );
            if std::mem::take(&mut palette.focus) || !edit.has_focus() {
                edit.request_focus();
            }
            if edit.changed() {
                palette.selected = 0;
            }
            ui.add_space(6.0);
            if matches.is_empty() {
                ui.add_space(4.0);
                ui.weak(empty);
                ui.add_space(4.0);
                return;
            }
            egui::ScrollArea::vertical()
                .max_height(360.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for (row, (index, positions)) in matches.iter().enumerate().take(200) {
                        let item = &items[*index];
                        let selected = row == palette.selected;
                        if item_row(ui, item, positions, selected, &visuals).clicked() {
                            chosen = Some(*index);
                        }
                        if selected && (up || down) {
                            ui.scroll_to_rect(
                                ui.min_rect().with_min_y(ui.cursor().top() - 30.0),
                                None,
                            );
                        }
                    }
                });
        });
    if escape || response.should_close() {
        app.palette = None;
        app.focus_text = true;
        return;
    }
    if let Some(index) = chosen {
        app.palette = None;
        app.focus_text = true;
        match items[index].action.clone() {
            Action::Command(id, params) => {
                if let Err(error) = app.command(id, params) {
                    app.notice = Some(error);
                }
            }
            Action::Open(path) => {
                if app.path.as_deref() != Some(path.as_path()) {
                    app.host_commands
                        .push(("file.open".into(), json!({"path": path})));
                }
            }
        }
    }
}

fn item_row(
    ui: &mut Ui,
    item: &Item,
    positions: &[usize],
    selected: bool,
    visuals: &egui::Visuals,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
    if selected {
        ui.painter()
            .rect_filled(rect, 6, visuals.selection.bg_fill.gamma_multiply(0.3));
    } else if response.hovered() {
        ui.painter()
            .rect_filled(rect, 6, visuals.widgets.hovered.weak_bg_fill);
    }
    let font = FontId::proportional(14.0);
    let mut job = egui::text::LayoutJob::default();
    let normal = egui::TextFormat::simple(font.clone(), visuals.text_color());
    let mut strong = normal.clone();
    strong.color = visuals.hyperlink_color;
    for (index, ch) in item.label.chars().enumerate() {
        let format = if positions.contains(&index) {
            strong.clone()
        } else {
            normal.clone()
        };
        job.append(ch.encode_utf8(&mut [0; 4]), 0.0, format);
    }
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let text_pos = rect.left_center() + vec2(10.0, -galley.size().y / 2.0);
    let label_right = text_pos.x + galley.size().x;
    ui.painter().galley(text_pos, galley, visuals.text_color());
    if !item.detail.is_empty() {
        let detail_rect = egui::Rect::from_min_max(
            egui::pos2(label_right + 12.0, rect.top()),
            egui::pos2(rect.right() - 10.0, rect.bottom()),
        );
        ui.painter().with_clip_rect(detail_rect).text(
            egui::pos2(rect.right() - 10.0, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            &item.detail,
            FontId::proportional(12.5),
            visuals.weak_text_color(),
        );
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_prefers_word_starts_and_runs() {
        let (word, _) = fuzzy("gl", "Go to line").unwrap();
        let (inner, _) = fuzzy("gl", "Toggle").unwrap();
        assert!(word > inner);
        let (_, positions) = fuzzy("同步", "同步文件").unwrap();
        assert_eq!(positions, [0, 1]);
        assert!(fuzzy("xyz", "Go to line").is_none());
        assert_eq!(fuzzy("", "anything").unwrap().0, 0);
        assert_eq!(fuzzy("GTL", "go to line").unwrap().1, [0, 3, 6]);
    }

    #[test]
    fn commands_follow_the_document_kind() {
        let markdown = TextApp::new(Kind::Markdown);
        let names: Vec<_> = commands(&markdown).into_iter().map(|i| i.label).collect();
        assert!(names.iter().any(|n| n == "粗体"));
        assert!(!names.iter().any(|n| n.starts_with("语言：")));
        let json = TextApp::from_bytes(Kind::PlainText, "a.json", b"{}").unwrap();
        let names: Vec<_> = commands(&json).into_iter().map(|i| i.label).collect();
        assert!(names.iter().any(|n| n == "格式化 JSON"));
        assert!(!names.iter().any(|n| n == "粗体"));
        assert!(names.iter().any(|n| n == "语言：Python"));
    }

    #[test]
    fn every_listed_command_is_known_to_the_editor() {
        for kind in [Kind::Markdown, Kind::PlainText] {
            let mut app = TextApp::from_bytes(kind, "a.json", b"{\"a\": 1}").unwrap();
            for item in commands(&app) {
                if let Action::Command(id, params) = item.action {
                    if id == "palette.files" {
                        continue;
                    }
                    let result = app.command(id, params);
                    assert!(
                        result
                            .as_ref()
                            .err()
                            .is_none_or(|e| !e.contains("未知命令")),
                        "{id}: {result:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn english_names_find_chinese_commands() {
        let app = TextApp::new(Kind::Markdown);
        let items = commands(&app);
        let found = filter(&items, "bold");
        assert_eq!(items[found[0].0].label, "粗体");
        assert!(found[0].1.is_empty(), "alias matches highlight nothing");
    }
}
