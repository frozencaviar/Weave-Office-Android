//! A forgiving, line-based Markdown scanner for live preview.
//!
//! It never changes the text. It reports, for every byte, what it is (heading text, bold,
//! a link, markup that the preview may hide, ...), plus the decorations the preview paints
//! (checkboxes, bullets, quote bars, code block backgrounds, rules) and the document outline.
//! It covers CommonMark's everyday constructs and Obsidian's additions (`[[links]]`, `#tags`,
//! `==highlights==`, callouts); it is not a full CommonMark implementation.

use crate::syntax::{self, Language, Token};
use std::ops::Range;

/// What a byte of the text is. Several flags combine (a bold link).
pub mod flag {
    pub const BOLD: u32 = 1;
    pub const ITALIC: u32 = 1 << 1;
    pub const STRIKE: u32 = 1 << 2;
    pub const HIGHLIGHT: u32 = 1 << 3;
    /// Inline code.
    pub const CODE: u32 = 1 << 4;
    pub const LINK: u32 = 1 << 5;
    /// Syntax the live preview hides away from the cursor (`**`, `](url)`, `# `).
    pub const MARKUP: u32 = 1 << 6;
    pub const TAG: u32 = 1 << 7;
    /// Dimmed: front matter, code fences, done tasks.
    pub const DIM: u32 = 1 << 8;
    /// Monospace: code blocks, front matter.
    pub const MONO: u32 = 1 << 9;
    /// Syntax the preview draws over (a list bullet, `>`, `[ ]`): invisible but keeps its width.
    pub const COVERED: u32 = 1 << 10;
    pub const QUOTE: u32 = 1 << 11;
    /// A finished task's text.
    pub const DONE: u32 = 1 << 12;
    /// The title line of a callout (`> [!note] Title`).
    pub const CALLOUT: u32 = 1 << 13;
    pub const IMAGE: u32 = 1 << 14;
}

#[derive(Clone, Debug, PartialEq)]
pub enum Decoration {
    /// A fenced code block, fences included (byte range of whole lines), and its language tag.
    CodeBlock { range: Range<usize>, label: String },
    /// One line of a block quote; `depth` nested levels.
    Quote { line: Range<usize>, depth: usize },
    /// A thematic break line.
    Rule(Range<usize>),
    /// A list bullet character.
    Bullet(Range<usize>),
    /// A task checkbox: the `[ ]` / `[x]` bytes.
    Task { marker: Range<usize>, checked: bool },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Link {
    /// The bytes a click activates.
    pub range: Range<usize>,
    pub target: String,
    /// `[[wiki]]` links name a note, not a URL.
    pub wiki: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Heading {
    pub level: u8,
    pub title: String,
    /// Byte offset of the heading line.
    pub start: usize,
}

/// One line's block-level role.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Line {
    pub range: Range<usize>,
    /// 1–6 for headings.
    pub heading: u8,
    /// Syntax-highlighted code lines and front matter lines are not inline-parsed.
    pub code: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TableRow {
    pub range: Range<usize>,
    pub cells: Vec<Range<usize>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Table {
    pub range: Range<usize>,
    pub separator: Range<usize>,
    pub alignments: Vec<egui::Align>,
    pub rows: Vec<TableRow>,
}

#[derive(Default)]
pub struct Parsed {
    pub lines: Vec<Line>,
    /// Per-byte flags.
    pub flags: Vec<u32>,
    /// Highlighted runs inside code blocks.
    pub tokens: Vec<(Range<usize>, Token)>,
    pub decorations: Vec<Decoration>,
    pub links: Vec<Link>,
    pub headings: Vec<Heading>,
    pub tables: Vec<Table>,
}

fn fence(line: &str) -> Option<(&str, usize)> {
    let trimmed = line.trim_start();
    if line.len() - trimmed.len() > 3 {
        return None;
    }
    let ch = trimmed.chars().next()?;
    if ch != '`' && ch != '~' {
        return None;
    }
    let count = trimmed.chars().take_while(|&c| c == ch).count();
    (count >= 3).then(|| (&trimmed[..count], line.len() - trimmed.len() + count))
}

fn is_rule(line: &str) -> bool {
    let trimmed = line.trim();
    let Some(ch) = trimmed.chars().next() else {
        return false;
    };
    matches!(ch, '-' | '*' | '_')
        && trimmed.chars().filter(|&c| c == ch).count() >= 3
        && trimmed.chars().all(|c| c == ch || c == ' ' || c == '\t')
}

/// `(marker_end, bullet range within line or None for ordered)` for a list item line.
fn list_marker(line: &str) -> Option<(usize, Option<Range<usize>>)> {
    let indent = line.len() - line.trim_start_matches([' ', '\t']).len();
    let rest = &line[indent..];
    let first = rest.chars().next()?;
    if matches!(first, '-' | '*' | '+') && (rest.len() == 1 || rest[1..].starts_with([' ', '\t'])) {
        let end = indent + 1 + usize::from(rest.len() > 1);
        return Some((end, Some(indent..indent + 1)));
    }
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    if (1..=9).contains(&digits)
        && rest[digits..].starts_with(['.', ')'])
        && (rest.len() == digits + 1 || rest[digits + 1..].starts_with([' ', '\t']))
    {
        let end = indent + digits + 1 + usize::from(rest.len() > digits + 1);
        return Some((end, None));
    }
    None
}

pub fn parse(text: &str) -> Parsed {
    let mut parsed = Parsed {
        flags: vec![0; text.len()],
        ..Default::default()
    };
    let mut start = 0;
    // (fence marker, block start, language, content start, label)
    let mut open_fence: Option<(String, usize, Option<&'static Language>, usize, String)> = None;
    let mut front_matter = text.starts_with("---\n") || text.starts_with("---\r\n");
    let mut first = true;
    let mut lines = text.split_inclusive('\n').peekable();
    while let Some(raw) = lines.next() {
        let line_start = start;
        start += raw.len();
        let body = raw.trim_end_matches(['\n', '\r']);
        let range = line_start..line_start + body.len();
        let mut line = Line {
            range: range.clone(),
            ..Default::default()
        };
        let set = |parsed: &mut Parsed, r: Range<usize>, f: u32| {
            for b in &mut parsed.flags[r] {
                *b |= f;
            }
        };

        // Front matter: `---` on the first line until the next `---`.
        if front_matter {
            line.code = true;
            set(&mut parsed, range.clone(), flag::MONO | flag::DIM);
            if !first && (body == "---" || body == "...") {
                front_matter = false;
            }
            first = false;
            parsed.lines.push(line);
            continue;
        }
        first = false;

        // Fenced code.
        if let Some((marker, block_start, language, content_start, label)) = &open_fence {
            line.code = true;
            let closes = fence(body).is_some_and(|(m, end)| {
                m.starts_with(&marker[..1])
                    && m.len() >= marker.len()
                    && body[end..].trim().is_empty()
            });
            if closes || lines.peek().is_none() {
                let content_end = if closes { line_start } else { start };
                if let Some(language) = language {
                    syntax::highlight(
                        language,
                        &text[*content_start..content_end],
                        *content_start,
                        &mut parsed.tokens,
                    );
                }
                set(&mut parsed, *content_start..content_end, flag::MONO);
                if closes {
                    set(
                        &mut parsed,
                        range.clone(),
                        flag::MONO | flag::DIM | flag::MARKUP,
                    );
                }
                parsed.decorations.push(Decoration::CodeBlock {
                    range: *block_start..range.end,
                    label: label.clone(),
                });
                open_fence = None;
            }
            parsed.lines.push(line);
            continue;
        }
        if let Some((marker, end)) = fence(body) {
            let info = body[end..].trim();
            if !(marker.starts_with('`') && info.contains('`')) {
                line.code = true;
                set(
                    &mut parsed,
                    range.clone(),
                    flag::MONO | flag::DIM | flag::MARKUP,
                );
                let tag = info.split_whitespace().next().unwrap_or("");
                let language = Language::by_name(tag);
                if lines.peek().is_none() {
                    parsed.decorations.push(Decoration::CodeBlock {
                        range: range.clone(),
                        label: tag.to_owned(),
                    });
                } else {
                    open_fence = Some((
                        marker.to_owned(),
                        line_start,
                        language,
                        start,
                        tag.to_owned(),
                    ));
                }
                parsed.lines.push(line);
                continue;
            }
        }

        if body.trim().is_empty() {
            parsed.lines.push(line);
            continue;
        }

        // Thematic break.
        if is_rule(body) && !body.trim_start().starts_with("- ") || body.trim() == "---" {
            set(&mut parsed, range.clone(), flag::COVERED | flag::DIM);
            parsed.decorations.push(Decoration::Rule(range.clone()));
            parsed.lines.push(line);
            continue;
        }

        let mut content = 0; // offset within body where inline content starts

        // Block quotes (possibly nested) and callouts.
        let mut depth = 0;
        loop {
            let rest = &body[content..];
            let trimmed = rest.trim_start_matches(' ');
            if trimmed.starts_with('>') && rest.len() - trimmed.len() <= 3 {
                let marker = content + (rest.len() - trimmed.len());
                content = marker + 1;
                if body[content..].starts_with(' ') {
                    content += 1;
                }
                set(
                    &mut parsed,
                    line_start + marker..line_start + content,
                    flag::COVERED,
                );
                depth += 1;
            } else {
                break;
            }
        }
        if depth > 0 {
            set(&mut parsed, line_start + content..range.end, flag::QUOTE);
            parsed.decorations.push(Decoration::Quote {
                line: range.clone(),
                depth,
            });
            let rest = &body[content..];
            if rest.starts_with("[!")
                && let Some(close) = rest.find(']')
            {
                set(
                    &mut parsed,
                    line_start + content..line_start + content + close + 1,
                    flag::MARKUP,
                );
                set(
                    &mut parsed,
                    line_start + content..range.end,
                    flag::CALLOUT | flag::BOLD,
                );
                content += close + 1;
            }
        }

        // ATX heading.
        let rest = &body[content..];
        let hashes = rest.chars().take_while(|&c| c == '#').count();
        if (1..=6).contains(&hashes) && (rest.len() == hashes || rest[hashes..].starts_with(' ')) {
            let marker_end = content + hashes + usize::from(rest.len() > hashes);
            set(
                &mut parsed,
                line_start + content..line_start + marker_end,
                flag::MARKUP,
            );
            // Optional closing hashes.
            let title = body[marker_end..].trim_end();
            let closing = title.trim_end_matches('#');
            let title_end =
                if closing.len() < title.len() && (closing.is_empty() || closing.ends_with(' ')) {
                    set(
                        &mut parsed,
                        line_start + marker_end + closing.trim_end().len()..range.end,
                        flag::MARKUP,
                    );
                    marker_end + closing.trim_end().len()
                } else {
                    marker_end + title.len()
                };
            line.heading = hashes as u8;
            parsed.headings.push(Heading {
                level: hashes as u8,
                title: plain_title(&body[marker_end..title_end]),
                start: line_start,
            });
            inline(
                text,
                &mut parsed,
                line_start + marker_end..line_start + title_end,
            );
            parsed.lines.push(line);
            continue;
        }

        // List items and tasks.
        if let Some((marker_end, bullet)) = list_marker(rest) {
            let marker_end = content + marker_end;
            let after = &body[marker_end..];
            let task = ["[ ] ", "[x] ", "[X] "]
                .iter()
                .find(|t| after.starts_with(**t) || after == t.trim_end());
            match (bullet, task) {
                (bullet, Some(task)) => {
                    let checked = task.as_bytes()[1] != b' ';
                    let box_start = line_start + marker_end;
                    if let Some(b) = &bullet {
                        set(
                            &mut parsed,
                            line_start + content + b.start..line_start + content + b.end,
                            flag::COVERED,
                        );
                    }
                    set(&mut parsed, box_start..box_start + 3, flag::COVERED);
                    parsed.decorations.push(Decoration::Task {
                        marker: box_start..box_start + 3,
                        checked,
                    });
                    content = (marker_end + task.len()).min(body.len());
                    if checked {
                        set(&mut parsed, line_start + content..range.end, flag::DONE);
                    }
                }
                (Some(b), None) => {
                    let at = line_start + content + b.start;
                    set(&mut parsed, at..at + 1, flag::COVERED);
                    parsed.decorations.push(Decoration::Bullet(at..at + 1));
                    content = marker_end;
                }
                (None, None) => {
                    content = marker_end;
                }
            }
        }

        inline(text, &mut parsed, line_start + content..range.end);
        parsed.lines.push(line);
    }
    parsed.tables = tables(text, &parsed.lines, &parsed.flags);
    parsed
}

fn table_cells(text: &str, range: Range<usize>) -> Option<Vec<Range<usize>>> {
    let body = &text[range.clone()];
    if body.len() - body.trim_start().len() > 3 || body.trim().is_empty() {
        return None;
    }
    let mut boundaries = Vec::new();
    let mut escaped = false;
    let mut code_ticks = 0;
    let mut wiki = false;
    let mut chars = body.char_indices().peekable();
    while let Some((offset, ch)) = chars.next() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '`' => {
                let mut count = 1;
                while chars.peek().is_some_and(|(_, next)| *next == '`') {
                    chars.next();
                    count += 1;
                }
                if code_ticks == 0 {
                    code_ticks = count;
                } else if code_ticks == count {
                    code_ticks = 0;
                }
            }
            '[' if code_ticks == 0 && body[offset..].starts_with("[[") => wiki = true,
            ']' if body[offset..].starts_with("]]") => wiki = false,
            '|' if code_ticks == 0 && !wiki => boundaries.push(offset),
            _ => {}
        }
    }
    if boundaries.is_empty() {
        return None;
    }
    let mut start = 0;
    let mut cells = Vec::new();
    for end in boundaries.iter().copied().chain(std::iter::once(body.len())) {
        let cell = &body[start..end];
        let left = start + cell.len() - cell.trim_start().len();
        let right = left + cell.trim().len();
        cells.push(range.start + left..range.start + right);
        start = end + 1;
    }
    if body[..boundaries[0]].trim().is_empty() {
        cells.remove(0);
    }
    if body[boundaries[boundaries.len() - 1] + 1..].trim().is_empty() {
        cells.pop();
    }
    (!cells.is_empty()).then_some(cells)
}

fn tables(text: &str, lines: &[Line], flags: &[u32]) -> Vec<Table> {
    let eligible = |line: &Line| {
        !line.code
            && line.heading == 0
            && !flags[line.range.clone()].iter().any(|f| f & flag::QUOTE != 0)
    };
    let mut tables = Vec::new();
    let mut index = 0;
    while index + 1 < lines.len() {
        let header = &lines[index];
        let separator = &lines[index + 1];
        let candidate = eligible(header)
            && eligible(separator)
            && table_cells(text, header.range.clone())
                .zip(table_cells(text, separator.range.clone()))
                .is_some_and(|(header, separator)| header.len() == separator.len());
        if !candidate {
            index += 1;
            continue;
        }
        let separator_cells = table_cells(text, separator.range.clone()).unwrap();
        let alignments: Option<Vec<_>> = separator_cells
            .iter()
            .map(|range| {
                let value = &text[range.clone()];
                let dashes = value.strip_prefix(':').unwrap_or(value);
                let dashes = dashes.strip_suffix(':').unwrap_or(dashes);
                (dashes.len() >= 3 && dashes.bytes().all(|byte| byte == b'-')).then(|| {
                    match (value.starts_with(':'), value.ends_with(':')) {
                        (true, true) => egui::Align::Center,
                        (_, true) => egui::Align::RIGHT,
                        _ => egui::Align::LEFT,
                    }
                })
            })
            .collect();
        let Some(alignments) = alignments else {
            index += 1;
            continue;
        };
        let mut table = Table {
            range: header.range.start..separator.range.end,
            separator: separator.range.clone(),
            alignments,
            rows: vec![TableRow {
                range: header.range.clone(),
                cells: table_cells(text, header.range.clone()).unwrap(),
            }],
        };
        index += 2;
        while let Some(line) = lines.get(index).filter(|line| eligible(line)) {
            let Some(mut cells) = table_cells(text, line.range.clone()) else {
                break;
            };
            cells.truncate(table.alignments.len());
            cells.resize(table.alignments.len(), line.range.end..line.range.end);
            table.range.end = line.range.end;
            table.rows.push(TableRow {
                range: line.range.clone(),
                cells,
            });
            index += 1;
        }
        tables.push(table);
    }
    tables
}

/// A heading's text without inline markup, for the outline.
fn plain_title(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    let mut chars = title.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '*' | '_' | '`' | '=' | '~' => {}
            '[' => {
                if chars.peek() == Some(&'[') {
                    chars.next();
                }
            }
            ']' => {
                if chars.peek() == Some(&']') {
                    chars.next();
                } else if chars.peek() == Some(&'(') {
                    for c in chars.by_ref() {
                        if c == ')' {
                            break;
                        }
                    }
                }
            }
            '\\' => {
                if let Some(next) = chars.next() {
                    out.push(next);
                }
            }
            _ => out.push(c),
        }
    }
    out.trim().to_owned()
}

fn set_flags(parsed: &mut Parsed, range: Range<usize>, f: u32) {
    for b in &mut parsed.flags[range] {
        *b |= f;
    }
}

fn find_from(text: &str, from: usize, end: usize, pattern: &str) -> Option<usize> {
    text.get(from..end)?.find(pattern).map(|i| from + i)
}

fn prev_char(text: &str, at: usize) -> Option<char> {
    text[..at].chars().next_back()
}

fn is_url_char(c: char) -> bool {
    !c.is_whitespace() && !matches!(c, '<' | '>' | '"' | '）' | '，' | '。')
}

/// Mark inline syntax in `range` (one line's content).
fn inline(text: &str, parsed: &mut Parsed, range: Range<usize>) {
    let end = range.end;
    let mut i = range.start;
    while i < end {
        let rest = &text[i..end];
        let c = rest.chars().next().unwrap_or(' ');
        // Escapes.
        if c == '\\'
            && rest[1..]
                .chars()
                .next()
                .is_some_and(|n| n.is_ascii_punctuation())
        {
            set_flags(parsed, i..i + 1, flag::MARKUP);
            i += 1 + rest[1..].chars().next().map_or(0, char::len_utf8);
            continue;
        }
        // Inline code.
        if c == '`' {
            let ticks = rest.chars().take_while(|&c| c == '`').count();
            let marker = &rest[..ticks];
            if let Some(close) = find_from(text, i + ticks, end, marker) {
                set_flags(parsed, i..i + ticks, flag::MARKUP);
                set_flags(parsed, i + ticks..close, flag::CODE);
                set_flags(parsed, close..close + ticks, flag::MARKUP);
                i = close + ticks;
                continue;
            }
            i += ticks;
            continue;
        }
        // Wiki links and embeds: [[target|alias]], ![[target]].
        let embed = rest.starts_with("![[");
        if rest.starts_with("[[") || embed {
            let open = if embed { 3 } else { 2 };
            if let Some(close) = find_from(text, i + open, end, "]]") {
                let inner = &text[i + open..close];
                set_flags(parsed, i..i + open, flag::MARKUP);
                set_flags(parsed, close..close + 2, flag::MARKUP);
                let (target, shown) = match inner.find('|') {
                    Some(bar) => {
                        set_flags(parsed, i + open..i + open + bar + 1, flag::MARKUP);
                        (&inner[..bar], i + open + bar + 1..close)
                    }
                    None => (inner, i + open..close),
                };
                set_flags(
                    parsed,
                    shown.clone(),
                    flag::LINK | if embed { flag::IMAGE } else { 0 },
                );
                parsed.links.push(Link {
                    range: shown,
                    target: target.trim().to_owned(),
                    wiki: true,
                });
                i = close + 2;
                continue;
            }
        }
        // Links and images: [text](url), ![alt](url).
        let image = rest.starts_with("![");
        if (c == '[' || image) && !rest.starts_with("[^") {
            let open = if image { 2 } else { 1 };
            if let Some(mid) = find_from(text, i + open, end, "](")
                && let Some(close) = find_from(text, mid + 2, end, ")")
                && !text[i + open..mid].contains('[')
            {
                let target = text[mid + 2..close]
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .trim_matches(['<', '>']);
                set_flags(parsed, i..i + open, flag::MARKUP);
                set_flags(parsed, mid..close + 1, flag::MARKUP);
                set_flags(
                    parsed,
                    i + open..mid,
                    flag::LINK | if image { flag::IMAGE } else { 0 },
                );
                parsed.links.push(Link {
                    range: i + open..mid,
                    target: target.to_owned(),
                    wiki: false,
                });
                inline(text, parsed, i + open..mid);
                i = close + 1;
                continue;
            }
        }
        // Autolinks: <https://...> and bare URLs.
        if c == '<'
            && (rest.starts_with("<http://")
                || rest.starts_with("<https://")
                || rest.starts_with("<mailto:"))
            && let Some(close) = find_from(text, i, end, ">")
        {
            set_flags(parsed, i..i + 1, flag::MARKUP);
            set_flags(parsed, close..close + 1, flag::MARKUP);
            set_flags(parsed, i + 1..close, flag::LINK);
            parsed.links.push(Link {
                range: i + 1..close,
                target: text[i + 1..close].to_owned(),
                wiki: false,
            });
            i = close + 1;
            continue;
        }
        if (rest.starts_with("https://") || rest.starts_with("http://"))
            && prev_char(text, i).is_none_or(|p| !p.is_alphanumeric())
        {
            let mut len = rest.find(|c: char| !is_url_char(c)).unwrap_or(rest.len());
            while len > 0 && rest[..len].ends_with(['.', ',', ';', ':', '!', '?', ')', '\'']) {
                len -= 1;
            }
            set_flags(parsed, i..i + len, flag::LINK);
            parsed.links.push(Link {
                range: i..i + len,
                target: rest[..len].to_owned(),
                wiki: false,
            });
            i += len.max(1);
            continue;
        }
        // Tags: #tag (not a heading, not inside a word).
        if c == '#'
            && prev_char(text, i).is_none_or(char::is_whitespace)
            && rest[1..]
                .chars()
                .next()
                .is_some_and(|n| n.is_alphanumeric() || n == '_')
        {
            let len = 1 + rest[1..]
                .find(|c: char| !(c.is_alphanumeric() || matches!(c, '_' | '-' | '/')))
                .unwrap_or(rest.len() - 1);
            if !rest[1..len].chars().all(|c| c.is_ascii_digit()) {
                set_flags(parsed, i..i + len, flag::TAG);
                i += len;
                continue;
            }
        }
        // Emphasis, strikethrough and highlights.
        if let Some((marker, f)) = [
            ("***", flag::BOLD | flag::ITALIC),
            ("___", flag::BOLD | flag::ITALIC),
            ("**", flag::BOLD),
            ("__", flag::BOLD),
            ("~~", flag::STRIKE),
            ("==", flag::HIGHLIGHT),
            ("*", flag::ITALIC),
            ("_", flag::ITALIC),
        ]
        .into_iter()
        .find(|(m, _)| rest.starts_with(m))
        {
            let n = marker.len();
            let opens = rest[n..]
                .chars()
                .next()
                .is_some_and(|c| !c.is_whitespace() && c.to_string() != marker[..1]);
            let intraword =
                marker.starts_with('_') && prev_char(text, i).is_some_and(char::is_alphanumeric);
            if opens && !intraword {
                let mut search = i + n;
                let mut found = None;
                while let Some(close) = find_from(text, search, end, marker) {
                    let before = prev_char(text, close);
                    let after = text[close + n..end].chars().next();
                    let valid = before.is_some_and(|b| !b.is_whitespace())
                        && close > i + n
                        && !(marker.starts_with('_') && after.is_some_and(char::is_alphanumeric))
                        // `*` must not close on the first half of a `**`.
                        && !(n == 1 && after == marker.chars().next());
                    if valid {
                        found = Some(close);
                        break;
                    }
                    search = close + n;
                }
                if let Some(close) = found {
                    set_flags(parsed, i..i + n, flag::MARKUP);
                    set_flags(parsed, close..close + n, flag::MARKUP);
                    set_flags(parsed, i + n..close, f);
                    inline(text, parsed, i + n..close);
                    i = close + n;
                    continue;
                }
            }
            i += n;
            continue;
        }
        i += c.len_utf8();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flagged(text: &str, f: u32) -> String {
        let parsed = parse(text);
        text.char_indices()
            .filter(|(i, _)| parsed.flags[*i] & f != 0)
            .map(|(_, c)| c)
            .collect()
    }

    #[test]
    fn headings_and_outline() {
        let text = "# Title\n\nbody\n## Sub **bold** ##\n####### not\n#notheading";
        let parsed = parse(text);
        assert_eq!(parsed.headings.len(), 2);
        assert_eq!(parsed.headings[0].title, "Title");
        assert_eq!(parsed.headings[1].title, "Sub bold");
        assert_eq!(parsed.headings[1].level, 2);
        assert_eq!(parsed.lines[3].heading, 2);
        assert_eq!(parsed.lines[4].heading, 0);
        assert_eq!(flagged(text, flag::MARKUP), "# ## **** ##");
        assert_eq!(flagged(text, flag::TAG), "#notheading");
    }

    #[test]
    fn inline_styles() {
        let text = "a **bold** *it* ~~del~~ ==hi== `co*de*` \\*lit\\* snake_case_name ***both***";
        assert_eq!(flagged(text, flag::BOLD), "boldboth");
        assert_eq!(flagged(text, flag::ITALIC), "itboth");
        assert_eq!(flagged(text, flag::STRIKE), "del");
        assert_eq!(flagged(text, flag::HIGHLIGHT), "hi");
        assert_eq!(flagged(text, flag::CODE), "co*de*");
        assert!(!flagged(text, flag::ITALIC).contains("case"));
    }

    #[test]
    fn links() {
        let text =
            "see [docs](https://x.y/a) and [[Note#h|alias]] and https://example.com. ![img](p.png)";
        let parsed = parse(text);
        let targets: Vec<_> = parsed
            .links
            .iter()
            .map(|l| (l.target.as_str(), l.wiki))
            .collect();
        assert_eq!(
            targets,
            [
                ("https://x.y/a", false),
                ("Note#h", true),
                ("https://example.com", false),
                ("p.png", false)
            ]
        );
        assert_eq!(flagged(text, flag::LINK), "docsaliashttps://example.comimg");
        assert_eq!(&text[parsed.links[0].range.clone()], "docs");
        assert_eq!(
            flagged(text, flag::MARKUP),
            "[](https://x.y/a)[[Note#h|]]![](p.png)"
        );
    }

    #[test]
    fn blocks() {
        let text = "- item\n- [ ] todo\n- [x] done\n1. one\n> quote\n> [!tip] Title\n---\n```rust\nfn x() {}\n```\nafter **b**";
        let parsed = parse(text);
        let tasks: Vec<_> = parsed
            .decorations
            .iter()
            .filter_map(|d| match d {
                Decoration::Task { marker, checked } => Some((&text[marker.clone()], *checked)),
                _ => None,
            })
            .collect();
        assert_eq!(tasks, [("[ ]", false), ("[x]", true)]);
        assert!(
            parsed
                .decorations
                .iter()
                .any(|d| matches!(d, Decoration::Rule(_)))
        );
        assert!(parsed.decorations.iter().any(|d| matches!(d, Decoration::CodeBlock { range: r, label } if text[r.clone()].starts_with("```rust") && text[r.clone()].ends_with("```") && label == "rust")));
        assert!(
            parsed
                .tokens
                .iter()
                .any(|(r, t)| &text[r.clone()] == "fn" && *t == Token::Keyword)
        );
        assert_eq!(flagged(text, flag::DONE), "done");
        assert_eq!(flagged(text, flag::CALLOUT), "[!tip] Title");
        assert_eq!(flagged(text, flag::BOLD), "[!tip] Titleb");
        // Code is not inline-parsed.
        assert_eq!(flagged("```\n**x**\n```", flag::BOLD), "");
    }

    #[test]
    fn front_matter_and_unclosed_fence() {
        let text = "---\ntitle: x\n---\n# H";
        let parsed = parse(text);
        assert!(parsed.lines[1].code);
        assert_eq!(parsed.headings.len(), 1);
        let parsed = parse("```\ncode\nmore");
        assert!(parsed.lines.iter().all(|l| l.code));
    }

    #[test]
    fn parses_tables_with_alignment_and_inline_pipes() {
        let text = "| 项目 | 内容 | 数值 |\r\n| :--- | :---: | ---: |\r\n| 中文 | `a|b` 与 [[note|别名]] 和 a\\|b | **4.46×** |\r\n| 短行 |\r\n\r\nafter";
        let parsed = parse(text);
        assert_eq!(parsed.tables.len(), 1);
        let table = &parsed.tables[0];
        assert_eq!(
            table.alignments,
            [egui::Align::LEFT, egui::Align::Center, egui::Align::RIGHT]
        );
        assert_eq!(table.rows.len(), 3);
        assert_eq!(
            &text[table.rows[1].cells[1].clone()],
            "`a|b` 与 [[note|别名]] 和 a\\|b"
        );
        assert_eq!(&text[table.rows[2].cells[1].clone()], "");
        assert_eq!(table.rows[2].cells.len(), 3);
        assert!(text[table.range.clone()].ends_with("| 短行 |"));
        assert_eq!(flagged(text, flag::BOLD), "4.46×");
    }

    #[test]
    fn tables_require_matching_header_and_separator_outside_code() {
        for text in [
            "| just | text |",
            "| a | b |\n| --- |\n| c | d |",
            "| a | b |\n| -- | --- |",
            "| a | b |\n| ::--- | --- |",
            "```\n| a | b |\n| --- | --- |\n```",
            "---\n| a | b |\n| --- | --- |\n---\n",
            "> | a | b |\n> | --- | --- |",
        ] {
            assert!(parse(text).tables.is_empty(), "{text}");
        }
        let parsed = parse("a | b\n--- | ---\nc | d\n\nx | y\n--- | ---");
        assert_eq!(parsed.tables.len(), 2);
        assert_eq!(parsed.tables[0].rows.len(), 2);
        assert_eq!(parsed.tables[1].rows.len(), 1);
    }

    #[test]
    fn never_panics() {
        for text in [
            "*", "**", "[", "[a](", "![", "[[", "`", "<http", "#", "> ", ">", "- [", "1.", "~~",
            "==a", "\\", "中*文*", "_a_b", "* * *", "|", "```", "- [x]",
        ] {
            let parsed = parse(text);
            assert_eq!(parsed.flags.len(), text.len());
            for link in &parsed.links {
                assert!(
                    text.is_char_boundary(link.range.start)
                        && text.is_char_boundary(link.range.end)
                );
            }
        }
    }
}
