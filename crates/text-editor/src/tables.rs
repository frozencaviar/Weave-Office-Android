use crate::layout::{Reveal, Style};
use crate::markdown::Parsed;
use egui::text::{CCursor, Galley, LayoutJob, LayoutSection, TextFormat};
use egui::{Color32, FontId, Pos2, Rect, Shape, Stroke, Ui, pos2, vec2};
use std::ops::Range;
use std::sync::Arc;

const PADDING: f32 = 8.0;

pub(crate) struct Cell {
    pub range: Range<usize>,
    pub rect: Rect,
    pub origin: Pos2,
    pub galley: Arc<Galley>,
}

pub(crate) struct Row {
    pub range: Range<usize>,
    pub height: f32,
    pub cells: Vec<Cell>,
}

pub(crate) struct Table {
    range: Range<usize>,
    separator: Range<usize>,
    width: f32,
    pub rows: Vec<Row>,
}

pub(crate) fn prepare(
    ui: &Ui,
    job: &mut LayoutJob,
    parsed: &Parsed,
    reveal: Reveal<'_>,
    style: &Style,
) -> Vec<Table> {
    if reveal == Reveal::All {
        return Vec::new();
    }
    let mut tables = Vec::new();
    for source in &parsed.tables {
        if let Reveal::Lines(lines) = reveal
            && lines
                .iter()
                .any(|line| line.start <= source.range.end && source.range.start <= line.end)
        {
            continue;
        }
        let columns = source.alignments.len();
        let width = style.wrap_width;
        let padding = PADDING.min(width / columns as f32 / 4.0);
        let minimum = (style.size * 4.0).min(width / columns as f32);
        let mut preferred = vec![minimum; columns];
        let mut jobs = Vec::new();
        for (row_index, row) in source.rows.iter().enumerate() {
            let mut cells = Vec::new();
            for (column, range) in row.cells.iter().enumerate() {
                let mut cell = LayoutJob {
                    text: job.text[range.clone()].to_owned(),
                    ..Default::default()
                };
                for section in &job.sections {
                    let start = section.byte_range.start.0.max(range.start);
                    let end = section.byte_range.end.0.min(range.end);
                    if start >= end {
                        continue;
                    }
                    let mut section = section.clone();
                    section.byte_range = (start - range.start).into()..(end - range.start).into();
                    if row_index == 0 && section.format.font_id.size > 1.0 {
                        section.format.font_id.family = style.bold.clone();
                    }
                    cell.sections.push(section);
                }
                if cell.sections.is_empty() {
                    cell.append(
                        "",
                        0.0,
                        TextFormat {
                            font_id: FontId::new(style.size, style.proportional.clone()),
                            color: style.palette.text,
                            ..Default::default()
                        },
                    );
                }
                let natural = ui
                    .fonts_mut(|fonts| fonts.layout_job(cell.clone()))
                    .size()
                    .x;
                preferred[column] =
                    preferred[column].max((natural + 2.0 * padding).min(style.size * 40.0));
                cells.push(cell);
            }
            jobs.push(cells);
        }
        let total: f32 = preferred.iter().sum();
        let extra = (width - minimum * columns as f32).max(0.0);
        let weight = total - minimum * columns as f32;
        let widths: Vec<_> = preferred
            .iter()
            .map(|preferred| {
                minimum
                    + if weight > 0.0 {
                        extra * (preferred - minimum) / weight
                    } else {
                        extra / columns as f32
                    }
            })
            .collect();
        let mut rows = Vec::new();
        for (source_row, jobs) in source.rows.iter().zip(jobs) {
            let mut cells = Vec::new();
            let mut left = 0.0;
            let mut height = style.size + 2.0 * padding;
            for (column, (range, mut cell)) in source_row.cells.iter().zip(jobs).enumerate() {
                let inner = (widths[column] - 2.0 * padding).max(1.0);
                cell.wrap.max_width = inner;
                cell.halign = source.alignments[column];
                let galley = ui.fonts_mut(|fonts| fonts.layout_job(cell));
                height = height.max(galley.size().y + 2.0 * padding);
                let x = left + padding + source.alignments[column].to_factor() * inner;
                cells.push(Cell {
                    range: range.clone(),
                    rect: Rect::from_min_size(pos2(left, 0.0), vec2(widths[column], 0.0)),
                    origin: pos2(x, padding),
                    galley,
                });
                left += widths[column];
            }
            rows.push(Row {
                range: source_row.range.clone(),
                height,
                cells,
            });
        }
        tables.push(Table {
            range: source.range.clone(),
            separator: source.separator.clone(),
            width,
            rows,
        });
    }
    job.sections.retain(|section| {
        !tables.iter().any(|table| {
            table.range.start <= section.byte_range.start.0
                && section.byte_range.start.0 <= table.range.end
        })
    });
    for table in &tables {
        for (range, height) in table
            .rows
            .iter()
            .map(|row| (&row.range, row.height))
            .chain(std::iter::once((&table.separator, 0.01)))
        {
            let end = range.end + usize::from(job.text[range.end..].starts_with('\r'));
            let end = end + usize::from(job.text[end..].starts_with('\n'));
            job.sections.push(LayoutSection {
                leading_space: 0.0,
                byte_range: range.start.into()..end.into(),
                format: TextFormat {
                    font_id: FontId::new(0.01, style.proportional.clone()),
                    color: Color32::TRANSPARENT,
                    line_height: Some(height),
                    ..Default::default()
                },
            });
        }
    }
    job.sections.sort_by_key(|section| section.byte_range.start);
    tables
}

impl Table {
    pub fn shapes(&self, top: impl Fn(usize) -> f32, style: &Style) -> Vec<Shape> {
        let mut shapes = Vec::new();
        let stroke = Stroke::new(1.0, style.palette.quote);
        for (index, row) in self.rows.iter().enumerate() {
            let y = top(row.range.start);
            let rect = Rect::from_min_size(pos2(0.0, y), vec2(self.width, row.height));
            if index == 0 || index % 2 == 0 {
                shapes.push(Shape::rect_filled(
                    rect,
                    0.0,
                    if index == 0 {
                        style.palette.code_bg
                    } else {
                        style.palette.line_bg
                    },
                ));
            }
            shapes.push(Shape::rect_stroke(
                rect,
                0.0,
                stroke,
                egui::StrokeKind::Inside,
            ));
            for cell in &row.cells {
                let origin = cell.origin + vec2(0.0, y);
                shapes.push(Shape::galley(
                    origin,
                    cell.galley.clone(),
                    style.palette.text,
                ));
                if cell.rect.min.x > 0.0 {
                    shapes.push(Shape::line_segment(
                        [
                            pos2(cell.rect.min.x, y),
                            pos2(cell.rect.min.x, y + row.height),
                        ],
                        stroke,
                    ));
                }
            }
        }
        shapes
    }

    pub fn rect_at(&self, at: usize, top: impl Fn(usize) -> f32) -> Option<Rect> {
        for row in &self.rows {
            for cell in &row.cells {
                if cell.range.start <= at && at <= cell.range.end {
                    let offset = at - cell.range.start;
                    let index = cell.galley.job.text[..offset].chars().count();
                    return Some(
                        cell.galley
                            .pos_from_cursor(CCursor::new(index))
                            .translate(cell.origin.to_vec2() + vec2(0.0, top(row.range.start))),
                    );
                }
            }
        }
        None
    }

    pub fn byte_at(&self, pos: Pos2, top: impl Fn(usize) -> f32) -> Option<usize> {
        for row in &self.rows {
            let y = top(row.range.start);
            if pos.y < y || pos.y >= y + row.height || pos.x < 0.0 || pos.x > self.width {
                continue;
            }
            for cell in &row.cells {
                if cell.rect.min.x <= pos.x && pos.x <= cell.rect.max.x {
                    let cursor = cell
                        .galley
                        .cursor_from_pos(pos - (cell.origin + vec2(0.0, y)));
                    return Some(
                        cell.range.start
                            + crate::byte_of_char(&cell.galley.job.text, cursor.index.0),
                    );
                }
            }
            return Some(row.range.start);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{self, Palette};
    use crate::markdown;
    use egui::FontFamily;

    fn check(
        text: &str,
        width: f32,
        reveal: Reveal<'_>,
        check: impl FnOnce(Vec<Table>, Arc<Galley>, &Style),
    ) {
        let context = egui::Context::default();
        let mut check = Some(check);
        let output = context.run_ui(egui::RawInput::default(), |ui| {
            let parsed = markdown::parse(text);
            let style = Style {
                palette: Palette::new(false, Color32::BLACK),
                size: 15.0,
                mono: FontFamily::Monospace,
                bold: FontFamily::Monospace,
                proportional: FontFamily::Proportional,
                wrap_width: width,
            };
            let mut job = layout::markdown_job(text, &parsed, reveal, &style);
            let tables = prepare(ui, &mut job, &parsed, reveal, &style);
            assert_eq!(job.text, text);
            let mut offset = 0;
            for section in &job.sections {
                assert_eq!(section.byte_range.start.0, offset);
                offset = section.byte_range.end.0;
            }
            assert_eq!(offset, text.len());
            let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
            if let Some(check) = check.take() {
                check(tables, galley, &style);
            }
        });
        output.drop_without_applying_deltas();
    }

    #[test]
    fn wrapped_cells_reserve_space_and_preserve_source_positions() {
        let text = "# Report\r\n\r\n| 项目 | 内容 |\r\n| --- | ---: |\r\n| 摘要 | A long report summary with **bold** and `inline_code` that must wrap within its own column. |\r\n| 日期 | 2026-09-20 |\r\n\r\nafter";
        check(text, 320.0, Reveal::None, |tables, galley, style| {
            assert_eq!(tables.len(), 1);
            let table = &tables[0];
            assert_eq!(table.rows.len(), 3);
            let top = |at| {
                galley
                    .pos_from_cursor(CCursor::new(text[..at].chars().count()))
                    .top()
            };
            let long = &table.rows[1].cells[1];
            assert!(long.galley.rows.len() > 1);
            assert!(table.rows[1].height > table.rows[2].height);
            for row in &table.rows {
                for cell in &row.cells {
                    assert!(cell.galley.size().x <= cell.rect.width() + 1.0);
                    assert!(cell.galley.size().y + cell.origin.y <= row.height);
                }
            }
            for rows in table.rows.windows(2) {
                assert!(
                    top(rows[1].range.start) + 1.0 >= top(rows[0].range.start) + rows[0].height
                );
            }
            let after = text.find("after").unwrap();
            let last = table.rows.last().unwrap();
            assert!(top(after) >= top(last.range.start) + last.height - 1.0);
            let target = text.find("2026").unwrap();
            let rect = table.rect_at(target, top).unwrap();
            assert_eq!(table.byte_at(rect.center(), top), Some(target));
            assert_eq!(
                table.rows[0].cells[0].galley.job.sections[0]
                    .format
                    .font_id
                    .family,
                style.bold
            );
            assert!(
                long.galley
                    .job
                    .sections
                    .iter()
                    .any(|section| section.format.color == style.palette.tag)
            );
            assert!(!table.shapes(top, style).is_empty());
        });
    }

    #[test]
    fn source_and_active_tables_remain_editable_text() {
        let text = "| a | b |\n| --- | --- |\n| c | d |";
        for reveal in [Reveal::All, Reveal::Lines(&[25..26])] {
            check(text, 320.0, reveal, |tables, _, _| {
                assert!(tables.is_empty())
            });
        }
        check(text, 320.0, Reveal::Lines(&[]), |tables, _, _| {
            assert_eq!(tables.len(), 1)
        });
    }

    #[test]
    fn multiple_tables_and_empty_cells_fit_narrow_views() {
        let text = "a | b | c\n:--- | :---: | ---:\n| | | |\n\nx | y\n--- | ---\ntext | value";
        check(text, 120.0, Reveal::None, |tables, _, _| {
            assert_eq!(tables.len(), 2);
            for table in tables {
                assert_eq!(table.width, 120.0);
                for row in table.rows {
                    assert!(row.height > 0.0);
                    assert!(row.cells.last().unwrap().rect.right() <= 120.01);
                }
            }
        });
    }
}
