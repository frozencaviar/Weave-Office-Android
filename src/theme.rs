//! Shared Weave chrome and control styling. Document fonts and layout remain
//! owned by the individual editor models.

use crate::language::tr;
use eframe::egui;
use egui::{Color32, CornerRadius, Frame, Pos2, Rect, Sense, Stroke, Vec2};

const ACCENT: Color32 = Color32::from_rgb(35, 103, 184);
const ACCENT_LIGHT: Color32 = Color32::from_rgb(112, 165, 224);
pub const TITLE_BAR_HEIGHT: f32 = 38.0;
pub const NATIVE_CONTROLS_WIDTH: f32 = 84.0;
/// The ribbon tab row the host draws above every editor's commands.
pub const RIBBON_TABS_HEIGHT: f32 = 32.0;

pub fn style(ctx: &egui::Context) -> egui::Style {
    let mut style = (*ctx.style_of(ctx.theme())).clone();
    let dark = style.visuals.dark_mode;
    let (surface, panel, field, border, text, muted, hover, pressed) = if dark {
        (
            Color32::from_rgb(42, 43, 46),
            Color32::from_rgb(35, 36, 39),
            Color32::from_rgb(31, 32, 35),
            Color32::from_rgb(68, 69, 73),
            Color32::from_rgb(233, 234, 236),
            Color32::from_rgb(163, 165, 170),
            Color32::from_rgb(57, 59, 63),
            Color32::from_rgb(69, 72, 78),
        )
    } else {
        (
            Color32::from_rgb(255, 255, 255),
            Color32::from_rgb(246, 246, 246),
            Color32::from_rgb(255, 255, 255),
            Color32::from_rgb(214, 215, 217),
            Color32::from_rgb(37, 38, 40),
            Color32::from_rgb(117, 119, 123),
            Color32::from_rgb(231, 232, 234),
            Color32::from_rgb(218, 221, 225),
        )
    };
    let accent = if dark { ACCENT_LIGHT } else { ACCENT };

    style.spacing.item_spacing = Vec2::new(6.0, 4.0);
    // A 24 pt control has room for a 13 pt label without inflating the fixed
    // two/three-row ribbons. Larger primary actions opt into their own height.
    style.spacing.button_padding = Vec2::new(6.0, 3.0);
    style.spacing.interact_size.y = 24.0;
    style.spacing.window_margin = egui::Margin::same(12);
    style.spacing.menu_margin = egui::Margin::same(5);
    style.spacing.extra_text_line_spacing = 0.0;
    style.spacing.menu_width = 220.0;
    // Keep editor-owned UI families and normalize only chrome text styles.
    for (kind, size) in [
        (egui::TextStyle::Body, 13.0),
        (egui::TextStyle::Button, 13.0),
        (egui::TextStyle::Small, 11.0),
        (egui::TextStyle::Heading, 18.0),
    ] {
        if let Some(font) = style.text_styles.get_mut(&kind) {
            font.size = size;
        }
    }

    let visuals = &mut style.visuals;
    // Keep per-widget selection colors effective (white text on the accent fill).
    visuals.override_text_color = None;
    visuals.weak_text_color = Some(muted);
    visuals.panel_fill = panel;
    visuals.window_fill = surface;
    visuals.window_stroke = Stroke::new(1.0, border);
    visuals.window_corner_radius = CornerRadius::same(6);
    visuals.menu_corner_radius = CornerRadius::same(4);
    visuals.faint_bg_color = panel;
    visuals.extreme_bg_color = field;
    visuals.text_edit_bg_color = Some(field);
    visuals.code_bg_color = panel;
    visuals.hyperlink_color = accent;
    visuals.selection.bg_fill = accent;
    visuals.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    visuals.button_frame = true;
    visuals.text_options.subpixel_binning = false;
    visuals.interact_cursor = Some(egui::CursorIcon::PointingHand);

    let widgets = &mut visuals.widgets;
    widgets.noninteractive.bg_fill = panel;
    widgets.noninteractive.weak_bg_fill = panel;
    widgets.noninteractive.bg_stroke = Stroke::new(1.0, border);
    widgets.noninteractive.fg_stroke = Stroke::new(1.0, text);
    widgets.noninteractive.corner_radius = CornerRadius::same(3);
    widgets.inactive.bg_fill = surface;
    widgets.inactive.weak_bg_fill = surface;
    widgets.inactive.bg_stroke = Stroke::new(1.0, border);
    widgets.inactive.fg_stroke = Stroke::new(1.0, text);
    widgets.inactive.corner_radius = CornerRadius::same(3);
    widgets.hovered.bg_fill = hover;
    widgets.hovered.weak_bg_fill = hover;
    widgets.hovered.bg_stroke = Stroke::new(1.0, border);
    widgets.hovered.fg_stroke = Stroke::new(1.0, text);
    widgets.hovered.corner_radius = CornerRadius::same(3);
    widgets.active.bg_fill = pressed;
    widgets.active.weak_bg_fill = pressed;
    widgets.active.bg_stroke = Stroke::new(1.0, border);
    widgets.active.fg_stroke = Stroke::new(1.0, text);
    widgets.active.corner_radius = CornerRadius::same(3);
    widgets.open = widgets.hovered;
    style
}

pub fn surface_frame(ctx: &egui::Context) -> Frame {
    let visuals = &style(ctx).visuals;
    Frame::new()
        .fill(visuals.window_fill)
        .stroke(visuals.window_stroke)
        .corner_radius(visuals.window_corner_radius)
        .inner_margin(egui::Margin::same(16))
}

pub fn panel_frame(ctx: &egui::Context) -> Frame {
    let visuals = &style(ctx).visuals;
    Frame::new()
        .fill(visuals.panel_fill)
        .stroke(Stroke::new(1.0, visuals.window_stroke.color))
        .corner_radius(CornerRadius::same(4))
        .inner_margin(egui::Margin::symmetric(12, 9))
}

/// One-line frameless window chrome. `add_contents` owns the left area for
/// File, document tabs, and New. Returns its result plus a close request; the
/// app must route that request through its unsaved-document flow.
pub fn window_bar<R>(
    ui: &mut egui::Ui,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> (R, bool, Rect) {
    const HEIGHT: f32 = TITLE_BAR_HEIGHT;
    const BUTTON_WIDTH: f32 = 46.0;
    const DRAG_STRIP: f32 = 5.0;

    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), HEIGHT), Sense::hover());
    let dark = ui.visuals().dark_mode;
    let background = if dark {
        Color32::from_rgb(35, 36, 39)
    } else {
        Color32::from_rgb(236, 237, 239)
    };
    let divider = if dark {
        Color32::from_rgb(62, 63, 67)
    } else {
        Color32::from_rgb(216, 218, 221)
    };
    let foreground = if dark {
        Color32::from_rgb(224, 226, 238)
    } else {
        Color32::from_rgb(59, 61, 78)
    };
    ui.painter().rect_filled(rect, 0, background);
    ui.painter().line_segment(
        [rect.left_bottom(), rect.right_bottom()],
        Stroke::new(1.0, divider),
    );

    // macOS draws its own traffic lights, and Android has no window to minimise, maximise or
    // close — the system's own navigation does that — so neither reserves the three buttons.
    let window_buttons = if cfg!(target_os = "macos") || cfg!(target_os = "android") { 0 } else { 3 };
    let button_start = rect.right() - window_buttons as f32 * BUTTON_WIDTH;
    let content_left = rect.left()
        + if cfg!(target_os = "macos") {
            NATIVE_CONTROLS_WIDTH
        } else {
            8.0
        };
    let content_rect = Rect::from_min_max(
        Pos2::new(content_left, rect.top() + DRAG_STRIP),
        Pos2::new(button_start - 4.0, rect.bottom() - 1.0),
    );
    // Register drag targets before the content widgets: egui gives later widgets
    // priority, so tabs and buttons keep their own click and drag behavior.
    let strip = Rect::from_min_max(
        Pos2::new(content_left, rect.top()),
        Pos2::new(button_start, rect.top() + DRAG_STRIP),
    );
    let blank = Rect::from_min_max(
        Pos2::new(content_left, rect.top() + DRAG_STRIP),
        Pos2::new(button_start, rect.bottom()),
    );
    let drag_responses = [("strip", strip), ("blank", blank)].map(|(part, drag_rect)| {
        ui.interact(
            drag_rect,
            ui.scope_id().with(("weave-window-drag", part)),
            Sense::click_and_drag(),
        )
    });
    let mut content_ui = ui.new_child(
        egui::UiBuilder::new()
            .id_salt("weave-window-content")
            .max_rect(content_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    // Tab frames include a close button and the font's CJK line metrics.
    // Keep their controls compact enough to stay inside the single title row.
    content_ui.spacing_mut().item_spacing = Vec2::new(4.0, 0.0);
    content_ui.spacing_mut().button_padding = Vec2::new(6.0, 2.0);
    content_ui.spacing_mut().interact_size.y = 28.0;
    let result = add_contents(&mut content_ui);
    let maximized = ui
        .ctx()
        .input(|input| input.viewport().maximized.unwrap_or(false));
    for response in drag_responses {
        if response.double_clicked() {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
        } else if response.drag_started() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
    }

    let mut close = false;
    for index in 0..window_buttons {
        let button_rect = Rect::from_min_max(
            Pos2::new(button_start + index as f32 * BUTTON_WIDTH, rect.top()),
            Pos2::new(
                button_start + (index + 1) as f32 * BUTTON_WIDTH,
                rect.bottom(),
            ),
        );
        let response = ui.interact(
            button_rect,
            ui.scope_id().with(("weave-window-button", index)),
            Sense::click(),
        );
        let label = match index {
            0 => tr("最小化窗口"),
            1 if maximized => tr("还原窗口"),
            1 => tr("最大化窗口"),
            _ => tr("关闭窗口"),
        };
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
        let ink = if index == 2 && response.hovered() {
            Color32::WHITE
        } else if response.hovered() {
            if dark { Color32::WHITE } else { ACCENT }
        } else {
            foreground
        };
        if response.hovered() || response.has_focus() {
            let fill = if index == 2 && response.hovered() {
                Color32::from_rgb(197, 58, 68)
            } else if dark {
                Color32::from_rgb(58, 58, 78)
            } else {
                Color32::from_rgb(236, 234, 249)
            };
            ui.painter().rect_filled(button_rect.shrink(3.0), 7, fill);
        }
        let center = button_rect.center();
        let stroke = Stroke::new(1.5, ink);
        match index {
            0 => {
                ui.painter().line_segment(
                    [center + Vec2::new(-6.0, 0.0), center + Vec2::new(6.0, 0.0)],
                    stroke,
                );
            }
            1 => {
                let side = if maximized { 9.0 } else { 11.0 };
                ui.painter().rect_stroke(
                    Rect::from_center_size(center, Vec2::splat(side)),
                    1,
                    stroke,
                    egui::StrokeKind::Inside,
                );
            }
            _ => {
                for sign in [-1.0, 1.0] {
                    ui.painter().line_segment(
                        [
                            center + Vec2::new(-5.0, 5.0 * sign),
                            center + Vec2::new(5.0, -5.0 * sign),
                        ],
                        stroke,
                    );
                }
            }
        }
        if response.clicked() {
            match index {
                0 => ui
                    .ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Minimized(true)),
                1 => ui
                    .ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized)),
                _ => close = true,
            }
        }
    }
    (result, close, rect)
}
