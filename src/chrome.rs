//! Shared, UI-only file menu and quick access controls for hosted editors.

use crate::icons::{self, Icon};
use crate::language::tr;
use eframe::egui;
use serde_json::Value;

pub type Command = (String, Value);

/// The host refreshes this before painting the active editor's header prefix. The editor's
/// modal keeps running; these controls must not start a competing document operation.
pub(crate) fn set_host_navigation_blocked(ctx: &egui::Context, blocked: bool) {
    ctx.data_mut(|data| {
        data.insert_temp(egui::Id::unique("weave-host-navigation-blocked"), blocked)
    });
}

/// Draw the compact controls before an editor's ribbon tabs.
pub fn toolbar(ui: &mut egui::Ui, can_undo: bool, can_redo: bool) -> Vec<Command> {
    draw_toolbar(ui, true, can_undo, can_redo)
}

/// Draw the same controls on the start page, with document actions unavailable.
pub fn toolbar_for_home(ui: &mut egui::Ui) -> Vec<Command> {
    draw_toolbar(ui, false, false, false)
}

fn draw_toolbar(
    ui: &mut egui::Ui,
    has_document: bool,
    can_undo: bool,
    can_redo: bool,
) -> Vec<Command> {
    let navigation_enabled = !ui.ctx().data(|data| {
        data.get_temp::<bool>(egui::Id::unique("weave-host-navigation-blocked"))
            .unwrap_or(false)
    });
    ui.visuals_mut().button_frame = false;
    let mut commands = Vec::new();
    ui.menu_button(tr("文件"), |ui| {
        menu_item(ui, &mut commands, "新建", "⌘N", "file.new", true);
        menu_item(ui, &mut commands, "打开", "⌘O", "file.open", true);
        ui.separator();
        menu_item(ui, &mut commands, "保存", "⌘S", "file.save", has_document);
        menu_item(
            ui,
            &mut commands,
            "另存为",
            "⇧⌘S / F12",
            "file.save_as",
            has_document,
        );
        menu_item(
            ui,
            &mut commands,
            "导出 PDF",
            "",
            "file.exportPdf",
            has_document,
        );
        menu_item(ui, &mut commands, "打印", "⌘P", "file.print", has_document);
        ui.separator();
        menu_item(
            ui,
            &mut commands,
            "关闭",
            "⌘W / ⌘F4",
            "file.close",
            has_document,
        );
        menu_item(ui, &mut commands, "设置", "⌘,", "file.options", true);
        menu_item(ui, &mut commands, "关于", "", "app.about", true);
        ui.separator();
        menu_item(ui, &mut commands, "退出", "⌘Q", "app.quit", true);
    });

    for (icon, label, command, enabled) in [
        (Icon::OpenFolder, "打开 (⌘O)", "file.open", true),
        (Icon::Save, "保存 (⌘S)", "file.save", has_document),
        (Icon::Print, "打印 (⌘P)", "file.print", has_document),
        (
            Icon::Undo,
            "撤销 (⌘Z)",
            "edit.undo",
            has_document && can_undo,
        ),
        (
            Icon::Redo,
            "重做 (⇧⌘Z)",
            "edit.redo",
            has_document && can_redo,
        ),
    ] {
        if ui
            .add_enabled_ui(enabled && navigation_enabled, |ui| {
                icons::icon_button(ui, icon, tr(label), 28.0)
            })
            .inner
            .clicked()
        {
            commands.push((command.into(), Value::Null));
        }
    }
    commands
}

fn menu_item(
    ui: &mut egui::Ui,
    commands: &mut Vec<Command>,
    label: &str,
    shortcut: &str,
    command: &str,
    enabled: bool,
) {
    let navigation_enabled = command == "app.quit"
        || !ui.ctx().data(|data| {
            data.get_temp::<bool>(egui::Id::unique("weave-host-navigation-blocked"))
                .unwrap_or(false)
        });
    let label = tr(label);
    let icon = match command {
        "file.new" => Icon::New,
        "file.open" => Icon::OpenFolder,
        "file.save" => Icon::Save,
        "file.save_as" => Icon::SaveAs,
        "file.exportPdf" => Icon::ExportPdf,
        "file.print" => Icon::Print,
        "file.close" | "app.quit" => Icon::Close,
        "file.options" => Icon::Settings,
        _ => Icon::More,
    };
    let clicked = ui
        .add_enabled_ui(enabled && navigation_enabled, |ui| {
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(220.0, 28.0), egui::Sense::click());
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), label)
            });
            if response.hovered() {
                ui.painter()
                    .rect_filled(rect, 3, ui.visuals().widgets.hovered.weak_bg_fill);
            }
            let color = ui.style().interact(&response).fg_stroke.color;
            icons::paint(
                ui.painter(),
                egui::Rect::from_center_size(
                    egui::pos2(rect.left() + 16.0, rect.center().y),
                    egui::vec2(18.0, 18.0),
                ),
                icon,
                color,
            );
            ui.painter().text(
                egui::pos2(rect.left() + 34.0, rect.center().y),
                egui::Align2::LEFT_CENTER,
                label,
                egui::FontId::proportional(13.0),
                color,
            );
            ui.painter().text(
                egui::pos2(rect.right() - 8.0, rect.center().y),
                egui::Align2::RIGHT_CENTER,
                shortcut,
                egui::FontId::proportional(11.0),
                ui.visuals().weak_text_color(),
            );
            response.clicked()
        })
        .inner;
    if clicked {
        commands.push((command.into(), Value::Null));
        ui.close();
    }
}
