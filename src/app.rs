use crate::language::{self, UiLanguage, tr};
use crate::{
    commands::{self, HostRequest, ScaleRequest, SettingsPane, ThemeRequest},
    dialog::{self, ButtonKind},
    file_dialog::{FileDialog, FileDialogMode, FileDialogOptions, FileDialogResult},
    host::{Host, extension},
};
use eframe::egui::{self, Color32, Key, RichText};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
};
use weave_workspace::{DocumentKind, TabId};

#[derive(Clone)]
enum Action {
    New(DocumentKind),
    /// A new code or data file whose language follows the extension.
    NewText(&'static str),
    OpenDialog,
    Open(PathBuf),
    /// Finder/file-drop input waits for the active editor's modal, without blocking its
    /// confirmed save/export requests behind the deferred open.
    OpenExternal(PathBuf),
    Activate(TabId),
    Save(TabId, bool),
    SaveTo(TabId, PathBuf),
    ExportPdf(TabId, Option<PathBuf>),
    Close(Vec<TabId>, bool),
    Command(&'static str),
    Reorder(TabId, TabId),
}
struct ClosePlan {
    ids: Vec<TabId>,
    index: usize,
    quit: bool,
}
#[derive(Clone, Copy)]
enum FilePurpose {
    Open,
    Save(TabId),
    ExportPdf(TabId),
    Pdf { tab: TabId, request: u64 },
    Office { tab: TabId, request: u64 },
}
struct PendingFileDialog {
    dialog: FileDialog,
    purpose: FilePurpose,
}
#[derive(Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    recent: Vec<PathBuf>,
    scale: f32,
    theme: String,
    language: UiLanguage,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            recent: Vec::new(),
            scale: 1.0,
            theme: "light".into(),
            language: UiLanguage::default(),
        }
    }
}

pub struct WeaveApp {
    _native_menu: crate::native_file_panel::AppMenu,
    pub host: Host,
    mounted: Option<TabId>,
    actions: VecDeque<Action>,
    close_plan: Option<ClosePlan>,
    close_prompt: Option<TabId>,
    /// Automatic recovery, when this run keeps copies of unsaved documents.
    recovery: Option<crate::recovery::Recovery>,
    /// Asking whether to reopen what a crashed run left.
    restore_prompt: bool,
    /// The last keyboard or pointer press: copies wait until the person pauses.
    last_input: std::time::Instant,
    close_focus: Option<TabId>,
    error: Option<String>,
    new_picker: bool,
    new_anchor: Option<crate::new_document::Anchor>,
    preferences: bool,
    preferences_pane: SettingsPane,
    about: bool,
    print_dialog: Option<crate::printing::PrintDialog>,
    file_dialogs: VecDeque<PendingFileDialog>,
    allow_exit: bool,
    settings: Settings,
    ui_font: Option<crate::ui_fonts::UiFonts>,
    /// One font set for all open editors; see [`crate::font_set`].
    fonts: crate::font_set::FontSet,
    tab_scroll: f32,
    reveal_tab: Option<TabId>,
    control_click: bool,
    #[cfg(target_os = "macos")]
    pub os_events: Option<fmv_macos_events::Inbox>,
    /// Development smoke mode: render the real window, then exit through normal close handling.
    pub startup_frames: Option<u8>,
}
impl WeaveApp {
    fn host_dialog_open(&self) -> bool {
        self.new_picker || self.blocking_dialog_open()
    }
    fn blocking_dialog_open(&self) -> bool {
        self.restore_prompt
            || self.close_plan.is_some()
            || self.preferences
            || self.error.is_some()
            || self.about
            || self.print_dialog.is_some()
            || !self.file_dialogs.is_empty()
    }
    fn editor_blocks_host_navigation(&self) -> bool {
        self.host
            .active()
            .is_some_and(|document| document.editor.blocks_host_navigation())
    }
    pub fn new(cc: &eframe::CreationContext<'_>, files: Vec<PathBuf>) -> Self {
        let settings = std::fs::read(settings_path())
            .ok()
            .and_then(|b| serde_json::from_slice::<Settings>(&b).ok())
            .unwrap_or_default();
        language::set_current(settings.language);
        // One scan of the installed fonts serves every editor's font lists and lookups.
        weave_system_fonts::start_scan();
        cc.egui_ctx.set_theme(theme_preference(&settings.theme));
        cc.egui_ctx.set_zoom_factor(settings.scale.clamp(0.75, 2.0));
        let mut shell_fonts = pdfcraft_ui_egui::theme::font_definitions_for(true);
        let ui_font = Some(crate::ui_fonts::UiFonts::load());
        if let Some(font) = &ui_font {
            font.apply(&mut shell_fonts);
        }
        let fonts = crate::font_set::FontSet::new(shell_fonts.clone());
        cc.egui_ctx.set_fonts(shell_fonts);
        Self {
            _native_menu: crate::native_file_panel::install_app_menu(&cc.egui_ctx),
            host: Host::default(),
            mounted: None,
            actions: files.into_iter().map(Action::Open).collect(),
            close_plan: None,
            close_prompt: None,
            recovery: None,
            restore_prompt: false,
            last_input: std::time::Instant::now(),
            close_focus: None,
            error: None,
            new_picker: false,
            new_anchor: None,
            preferences: false,
            preferences_pane: SettingsPane::General,
            about: false,
            print_dialog: None,
            file_dialogs: VecDeque::new(),
            allow_exit: false,
            settings,
            ui_font,
            fonts,
            tab_scroll: 0.0,
            reveal_tab: None,
            control_click: false,
            #[cfg(target_os = "macos")]
            os_events: None,
            startup_frames: None,
        }
    }
    /// Keep copies of unsaved documents for after a crash, and offer the copies a crashed run
    /// left. Off unless the application turns it on (tests construct the app without it).
    pub fn enable_recovery(&mut self) {
        self.enable_recovery_in(&crate::recovery::default_root(&settings_path()));
    }
    pub fn enable_recovery_in(&mut self, root: &Path) {
        match crate::recovery::Recovery::open(root) {
            Ok(recovery) => {
                self.restore_prompt = !recovery.found().is_empty();
                self.recovery = Some(recovery);
            }
            Err(error) => eprintln!("automatic recovery is off: {error}"),
        }
    }
    /// Run one command from a platform shell (the Android Shortcut Pad): a host command
    /// (`edit.undo`, `file.save`, `file.print`) or an editor command (`view.zoomIn`,
    /// `view.zoom` with a percentage).
    pub fn run_shell_command(&mut self, ctx: &egui::Context, id: &str, params: serde_json::Value) {
        // The PDF editor's zoom commands take no parameters and have their own ids.
        if let Some(document) = self.host.active_mut() {
            if let crate::editor::Editor::Pdf(pdf) = &mut document.editor {
                let view = pdf.active.and_then(|index| pdf.views.get_mut(index));
                match id {
                    "view.zoom" => {
                        if let Some(percent) = params.get("value").or_else(|| params.get("percent")).and_then(serde_json::Value::as_f64) {
                            if let Some(view) = view {
                                view.set_zoom(percent as f32 / 100.0);
                            }
                            return;
                        }
                    }
                    "view.zoomIn" => {
                        if let Some(view) = view {
                            view.zoom_step(true);
                        }
                        return;
                    }
                    "view.zoomOut" => {
                        if let Some(view) = view {
                            view.zoom_step(false);
                        }
                        return;
                    }
                    _ => {}
                }
            }
        }
        match commands::classify(id, &params) {
            HostRequest::Unsupported { .. } => {
                if let Some(document) = self.host.active_mut() {
                    if let Err(error) = document.editor.command(id, params) {
                        eprintln!("shell command {id}: {error}");
                    }
                }
            }
            _ => self.dispatch_requests(ctx, vec![(id.to_owned(), params)]),
        }
    }
    /// The active document's zoom as a percentage (100 = 100%), for the Android Shortcut
    /// Pad's dial. `None` when the document has no zoom (text and rich-text files).
    pub fn zoom_percent(&self) -> Option<f32> {
        use crate::editor::Editor;
        let document = self.host.active()?;
        match &document.editor {
            Editor::Word(word) => Some(word.session.view.zoom * 100.0),
            Editor::Sheet(sheet) => {
                let doc = sheet.session.active()?;
                Some(doc.wb.active()?.zoom as f32)
            }
            Editor::Slides(slides) => slides.ui.zoom.map(|zoom| zoom * 100.0),
            Editor::Pdf(pdf) => {
                let view = pdf.views.get(pdf.active?)?;
                Some(view.zoom * 100.0)
            }
            Editor::TextFile(_) | Editor::Rich(_) => None,
        }
    }
    /// The active document's zoom range in percent, for the Android Shortcut Pad's dial.
    pub fn zoom_range(&self) -> Option<(f32, f32)> {
        use crate::editor::Editor;
        let document = self.host.active()?;
        match &document.editor {
            Editor::Word(_) => Some((10.0, 500.0)),
            Editor::Sheet(_) => Some((10.0, 400.0)),
            Editor::Slides(_) => Some((10.0, 400.0)),
            Editor::Pdf(_) => Some((8.0, 6400.0)),
            Editor::TextFile(_) | Editor::Rich(_) => None,
        }
    }
    /// Open a document another app sent (Android's "Open with" / "Share").
    pub fn open_external(&mut self, path: PathBuf) {
        self.actions.push_back(Action::OpenExternal(path));
    }
    /// Export the active document to a PDF at `path` (the Android shell prints it).
    pub fn export_pdf_to(&mut self, path: PathBuf) {
        if let Some(tab) = self.host.workspace.active() {
            self.actions.push_back(Action::ExportPdf(tab, Some(path)));
        }
    }
    /// Reopen every copy a crashed run left as an unsaved document, then delete that run's
    /// folder once this run holds its own copies of them.
    pub fn restore_recovered(&mut self) {
        let Some(recovery) = self.recovery.as_mut() else {
            return;
        };
        let mut errors = Vec::new();
        for found in recovery.found().to_vec() {
            let Some(kind) = found.manifest.document_kind() else {
                continue;
            };
            match self.host.restore(
                kind,
                &found.manifest.title,
                &found.file,
                found.manifest.origin.as_deref(),
            ) {
                Ok(id) => {
                    if let Ok(snapshot) = self.host.documents[&id].editor.snapshot() {
                        recovery.copy(
                            id,
                            kind,
                            &found.manifest.title,
                            found.manifest.origin.as_deref(),
                            snapshot,
                            std::time::Instant::now(),
                        );
                    }
                }
                Err(error) => errors.push(
                    tr("无法恢复 {title}：{error}")
                        .replace("{title}", &found.manifest.title)
                        .replace("{error}", &error),
                ),
            }
        }
        recovery.flush();
        if errors.is_empty() {
            recovery.clear_found();
        } else {
            self.error = Some(errors.join("\n"));
        }
        self.restore_prompt = false;
    }
    /// Copy each unsaved document the person has changed since its last copy, once they pause.
    fn autosave(&mut self, ctx: &egui::Context) {
        let Some(recovery) = self.recovery.as_mut() else {
            return;
        };
        let now = std::time::Instant::now();
        let ids: Vec<TabId> = self
            .host
            .workspace
            .tabs()
            .iter()
            .map(|tab| tab.id)
            .collect();
        recovery.retain(&ids);
        let mut wake: Option<std::time::Duration> = None;
        for id in ids {
            match recovery.due(id, self.host.dirty(id), now, self.last_input) {
                Ok(()) => {
                    let (Some(tab), Some(document)) =
                        (self.host.workspace.tab(id), self.host.documents.get(&id))
                    else {
                        continue;
                    };
                    if let Ok(snapshot) = document.editor.snapshot() {
                        let origin = tab.path.as_deref().or(document.origin.as_deref());
                        recovery.copy(id, tab.kind, &tab.title, origin, snapshot, now);
                    }
                }
                Err(Some(wait)) => wake = Some(wake.map_or(wait, |w| w.min(wait))),
                Err(None) => {}
            }
        }
        for error in recovery.take_errors() {
            eprintln!("automatic recovery: {error}");
        }
        if let Some(wait) = wake {
            ctx.request_repaint_after(wait);
        }
    }
    fn capture_fonts(&mut self, ctx: &egui::Context) {
        self.fonts.capture_tab(ctx, &mut self.host, self.mounted);
    }
    /// Keep the union of every open editor's fonts installed, before any text is laid out.
    fn sync_fonts(&mut self, ctx: &egui::Context) {
        self.fonts
            .sync(ctx, &mut self.host, self.mounted, self.ui_font.as_ref());
    }
    fn install_shell_font(&self, ctx: &egui::Context) {
        if let Some(font) = &self.ui_font {
            font.install(ctx);
        }
    }
    fn apply_weave_style(&self, ctx: &egui::Context) {
        crate::chrome::set_host_navigation_blocked(ctx, self.editor_blocks_host_navigation());
        let style = crate::theme::style(ctx);
        if ctx.global_style().as_ref() != &style {
            ctx.set_global_style(style);
        }
    }
    /// Each editor's global widget IDs and dynamic font families live in its tab's saved context state.
    fn mount_active(&mut self, ctx: &egui::Context) -> Result<(), String> {
        let next = self.host.workspace.active();
        if next == self.mounted {
            return Ok(());
        }
        // A change the leaving editor applied this pass is still its own.
        self.capture_fonts(ctx);
        if let Some(old) = self.mounted {
            if let Some(doc) = self.host.documents.get_mut(&old) {
                if let Err(error) = doc.editor.deactivate(ctx) {
                    self.host.workspace.activate(old);
                    self.close_plan = None;
                    self.close_prompt = None;
                    return Err(error);
                }
                // A font change it queued stays in its Memory and lands when it is back.
                doc.memory = ctx.memory(|m| m.clone());
            }
        }
        let options = ctx.options(|options| options.clone());
        if let Some(id) = next {
            if let Some(doc) = self.host.documents.get(&id) {
                ctx.memory_mut(|m| {
                    *m = doc.memory.clone();
                    m.options = options;
                });
            }
        } else {
            ctx.memory_mut(|m| {
                *m = egui::Memory::default();
                m.options = options;
            });
        }
        // Fonts stay: the installed set already has every open editor's families.
        self.mounted = next;
        self.reveal_tab = next;
        ctx.request_repaint();
        Ok(())
    }
    fn remember(&mut self, path: PathBuf) {
        self.settings.recent.retain(|p| p != &path);
        self.settings.recent.insert(0, path);
        self.settings.recent.truncate(16);
    }
    fn file_directory(&self) -> Option<PathBuf> {
        self.settings
            .recent
            .first()
            .and_then(|path| path.parent().map(Path::to_path_buf))
    }
    fn choose_file(
        &mut self,
        mode: FileDialogMode,
        mut options: FileDialogOptions,
        purpose: FilePurpose,
    ) {
        if options.directory.is_none() {
            options.directory = self.file_directory();
        }
        self.file_dialogs.push_back(PendingFileDialog {
            dialog: FileDialog::new(mode, options),
            purpose,
        });
    }
    fn export_pdf(&mut self, id: TabId, path: Option<PathBuf>) -> Result<(), String> {
        if !self.receive_sheet_transfers() {
            return Err(self.error.clone().unwrap_or_default());
        }
        if let Some(path) = path {
            return self.host.export_pdf_as(id, &path);
        }
        let tab = self.host.workspace.tab(id).ok_or("标签不存在")?;
        let stem = Path::new(&tab.title)
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or(&tab.title);
        let options = FileDialogOptions {
            title: tr("导出 PDF 副本").into(),
            file_name: tr("{stem}-导出.pdf").replace("{stem}", stem),
            directory: tab
                .path
                .as_ref()
                .and_then(|path| path.parent().map(Path::to_path_buf)),
            filters: vec![("PDF".into(), vec!["pdf".into()])],
        };
        self.choose_file(FileDialogMode::Save, options, FilePurpose::ExportPdf(id));
        Ok(())
    }
    fn receive_file_requests(&mut self) {
        let origins: Vec<_> = self
            .host
            .workspace
            .tabs()
            .iter()
            .map(|tab| tab.id)
            .collect();
        for tab in origins {
            let (office_requests, errors) = self
                .host
                .documents
                .get(&tab)
                .map(|document| {
                    (
                        document.file_services.take_requests(),
                        document.file_services.take_errors(),
                    )
                })
                .unwrap_or_default();
            if let Some(error) = errors.into_iter().next_back() {
                self.error = Some(error);
                self.close_plan = None;
                self.close_prompt = None;
            }
            for request in office_requests {
                self.choose_file(
                    request.mode,
                    request.options,
                    FilePurpose::Office {
                        tab,
                        request: request.id,
                    },
                );
                self.close_plan = None;
                self.close_prompt = None;
            }
            let requests = match self.host.documents.get_mut(&tab).map(|doc| &mut doc.editor) {
                Some(crate::editor::Editor::Pdf(app)) => app.take_host_file_requests(),
                _ => Vec::new(),
            };
            for request in requests {
                use pdfcraft_ui_egui::HostFileKind;
                let mode = match request.kind {
                    HostFileKind::File => FileDialogMode::Open { multiple: false },
                    HostFileKind::Files => FileDialogMode::Open { multiple: true },
                    HostFileKind::Save => FileDialogMode::Save,
                    HostFileKind::Folder => FileDialogMode::Folder,
                };
                let title = request.title.unwrap_or_else(|| {
                    match mode {
                        FileDialogMode::Open { .. } => tr("选择文件"),
                        FileDialogMode::Save => tr("保存文件"),
                        FileDialogMode::Folder => tr("选择文件夹"),
                    }
                    .into()
                });
                self.choose_file(
                    mode,
                    FileDialogOptions {
                        title,
                        directory: request.directory,
                        file_name: request.file_name.unwrap_or_default(),
                        filters: request
                            .filters
                            .into_iter()
                            .map(|filter| (filter.name, filter.extensions))
                            .collect(),
                    },
                    FilePurpose::Pdf {
                        tab,
                        request: request.id,
                    },
                );
                self.close_plan = None;
                self.close_prompt = None;
            }
        }
    }
    fn cancel_file_dialogs(&mut self) {
        loop {
            self.receive_file_requests();
            let Some(pending) = self.file_dialogs.pop_front() else {
                break;
            };
            match pending.purpose {
                FilePurpose::Pdf { tab, request } => {
                    if let Some(crate::editor::Editor::Pdf(app)) =
                        self.host.documents.get_mut(&tab).map(|doc| &mut doc.editor)
                    {
                        let _ = app.hosted_file_result(request, None);
                    }
                }
                FilePurpose::Office { tab, request } => {
                    if let Some(document) = self.host.documents.get(&tab) {
                        let _ = document.file_services.complete(request, None);
                    }
                }
                _ => {}
            }
        }
    }
    fn file_dialog(&mut self, ctx: &egui::Context) {
        let result = self
            .file_dialogs
            .front_mut()
            .and_then(|pending| pending.dialog.show(ctx));
        let Some(result) = result else {
            return;
        };
        self.complete_file_dialog(result, ctx);
    }
    fn complete_file_dialog(&mut self, result: FileDialogResult, ctx: &egui::Context) {
        let Some(mut pending) = self.file_dialogs.pop_front() else {
            return;
        };
        let paths = match result {
            FileDialogResult::Selected(paths) => Some(paths),
            FileDialogResult::Cancelled => None,
        };
        let selected = paths.is_some();
        if let Err(error) = self.finish_file_selection(pending.purpose, paths, ctx) {
            let retryable = selected
                && match pending.purpose {
                    FilePurpose::Save(_) | FilePurpose::ExportPdf(_) => true,
                    FilePurpose::Office { tab, request } => self
                        .host
                        .documents
                        .get(&tab)
                        .is_some_and(|document| document.file_services.can_retry(request)),
                    _ => false,
                };
            if retryable {
                pending.dialog.set_error(error);
                self.file_dialogs.push_front(pending);
            } else {
                self.error = Some(error);
            }
        }
    }
    fn finish_file_selection(
        &mut self,
        purpose: FilePurpose,
        paths: Option<Vec<PathBuf>>,
        ctx: &egui::Context,
    ) -> Result<(), String> {
        if matches!(purpose, FilePurpose::Save(_) | FilePurpose::ExportPdf(_))
            && !self.receive_sheet_transfers()
        {
            return Err(self.error.clone().unwrap_or_default());
        }
        let result = match purpose {
            FilePurpose::Office { tab, request } => self
                .host
                .documents
                .get(&tab)
                .ok_or_else(|| "文件已关闭，选择结果未应用".to_string())
                .and_then(|document| document.file_services.complete(request, paths)),
            FilePurpose::Pdf { tab, request } => {
                match self.host.documents.get_mut(&tab).map(|doc| &mut doc.editor) {
                    Some(crate::editor::Editor::Pdf(app)) => app.hosted_file_result(request, paths),
                    _ => Err("文件已关闭，选择结果未应用".into()),
                }
            }
            FilePurpose::Open => {
                if let Some(paths) = paths {
                    self.actions.extend(paths.into_iter().map(Action::Open));
                }
                Ok(())
            }
            FilePurpose::Save(id) => {
                if let Some(path) = paths.and_then(|paths| paths.into_iter().next()) {
                    self.host.save_as(id, &path).map(|_| {
                        self.reveal_tab = Some(id);
                        if let Some(path) = self.host.path(id).map(Path::to_path_buf) {
                            self.remember(path);
                        }
                        if self.close_prompt == Some(id) {
                            self.close_prompt = None;
                            if let Some(plan) = &mut self.close_plan {
                                plan.index += 1;
                            }
                        }
                    })
                } else {
                    Ok(())
                }
            }
            FilePurpose::ExportPdf(id) => {
                if let Some(path) = paths.and_then(|paths| paths.into_iter().next()) {
                    self.host.export_pdf_as(id, &path)
                } else {
                    Ok(())
                }
            }
        };
        self.receive_file_requests();
        self.receive_generated();
        ctx.request_repaint();
        result
    }
    fn begin_print(&mut self, ctx: &egui::Context) {
        if let Some(tab) = self
            .host
            .workspace
            .active()
            .and_then(|id| self.host.workspace.tab(id))
        {
            self.print_dialog = Some(crate::printing::PrintDialog::new(
                tab.id,
                tab.title.clone(),
                ctx,
            ));
        }
    }
    fn save(&mut self, id: TabId, save_as: bool) -> Result<bool, String> {
        if !self.receive_sheet_transfers() {
            return Err(self.error.clone().unwrap_or_default());
        }
        let tab = self.host.workspace.tab(id).ok_or("标签不存在")?;
        // A text file keeps its own extension; other kinds save in their own format.
        let own_extension = Path::new(&tab.title)
            .extension()
            .and_then(|e| e.to_str())
            .filter(|_| tab.kind == DocumentKind::PlainText)
            .map(str::to_owned);
        let ext = own_extension
            .as_deref()
            .unwrap_or(extension(tab.kind))
            .to_owned();
        let ext = ext.as_str();
        let path = if !save_as {
            tab.path.clone().filter(|p| tab.kind.can_save_to(p))
        } else {
            None
        };
        let Some(path) = path else {
            let options = FileDialogOptions {
                title: tr("保存文件").into(),
                file_name: format!(
                    "{}.{}",
                    Path::new(&tab.title)
                        .file_stem()
                        .and_then(|name| name.to_str())
                        .unwrap_or(&tab.title),
                    ext
                ),
                directory: tab
                    .path
                    .as_deref()
                    .or(self.host.origin(id))
                    .and_then(|path| path.parent().map(Path::to_path_buf)),
                filters: if tab.kind == DocumentKind::PlainText {
                    Vec::new()
                } else {
                    vec![(ext.to_uppercase(), vec![ext.into()])]
                },
            };
            self.choose_file(FileDialogMode::Save, options, FilePurpose::Save(id));
            return Ok(false);
        };
        self.host.save_as(id, &path)?;
        if let Some(path) = self.host.path(id).map(Path::to_path_buf) {
            self.remember(path);
        }
        Ok(true)
    }
    fn receive_sheet_transfers(&mut self) -> bool {
        let pending = self.host.has_sheet_transfers();
        let errors = self.host.receive_sheet_transfers();
        if pending {
            // A move between two workbooks changes a document that is not the active one.
            if let Some(recovery) = self.recovery.as_mut() {
                for tab in self.host.workspace.tabs() {
                    recovery.touch(tab.id);
                }
            }
        }
        if pending || !errors.is_empty() {
            // Any old save/discard choices predate this two-document transaction.
            self.close_plan = None;
            self.close_prompt = None;
            self.close_focus = None;
        }
        if !errors.is_empty() {
            self.error = Some(errors.join("\n"));
            self.actions.clear();
            return false;
        }
        true
    }
    fn process_actions(&mut self, ctx: &egui::Context) {
        self.sync_interface_language();
        if !self.receive_sheet_transfers() {
            return;
        }
        let mut deferred_opens = VecDeque::new();
        let mut processed_action = false;
        while self.close_plan.is_none() && self.file_dialogs.is_empty() && self.error.is_none() {
            let Some(action) = self.actions.pop_front() else {
                break;
            };
            if matches!(action, Action::OpenExternal(_))
                && (self.blocking_dialog_open() || self.editor_blocks_host_navigation())
            {
                deferred_opens.push_back(action);
                continue;
            }
            processed_action = true;
            let result: Result<(), String> = match action {
                Action::New(kind) => self.host.new_document(kind).map(|_| ()),
                Action::NewText(extension) => self.host.new_text_file(extension).map(|_| ()),
                Action::OpenDialog => {
                    self.choose_file(
                        FileDialogMode::Open { multiple: true },
                        FileDialogOptions {
                            title: tr("打开文件").into(),
                            // Every file: text and code files of any extension open in the
                            // text editor (macOS merges filter groups, so none are sent).
                            filters: Vec::new(),
                            ..Default::default()
                        },
                        FilePurpose::Open,
                    );
                    Ok(())
                }
                Action::Open(path) | Action::OpenExternal(path) => self
                    .host
                    .open(&path)
                    .map_err(|error| {
                        format!("{} “{}”\n{error}", tr("无法打开文件"), path.display())
                    })
                    .map(|id| {
                        self.reveal_tab = Some(id);
                        if let Some(path) = self.host.path(id).map(Path::to_path_buf) {
                            self.remember(path);
                        }
                    }),
                Action::Activate(id) => {
                    self.host.workspace.activate(id);
                    self.reveal_tab = Some(id);
                    Ok(())
                }
                Action::Save(id, save_as) => self.save(id, save_as).map(|_| ()),
                Action::SaveTo(id, path) => self.host.save_as(id, &path).map(|_| {
                    self.reveal_tab = Some(id);
                    if let Some(path) = self.host.path(id).map(Path::to_path_buf) {
                        self.remember(path);
                    }
                }),
                Action::ExportPdf(id, path) => self.export_pdf(id, path),
                Action::Close(ids, quit) => {
                    self.host.prepare_close(&ids);
                    if !self.stop_close_for_background(&ids) {
                        self.close_plan = Some(ClosePlan {
                            ids,
                            index: 0,
                            quit,
                        });
                    }
                    Ok(())
                }
                Action::Command(command) => self
                    .host
                    .active_mut()
                    .ok_or_else(|| "请先打开文件".to_string())
                    .and_then(|d| d.editor.command(command, json!({})).map(|_| ())),
                Action::Reorder(from, to) => {
                    self.host.workspace.reorder(from, to);
                    Ok(())
                }
            };
            if let Err(error) = result {
                self.error = Some(error);
            }
            if !self.receive_sheet_transfers() {
                deferred_opens.clear();
                break;
            }
            // Switching is completed before input reaches a different editor on the next pass.
            if self.host.workspace.active() != self.mounted {
                break;
            }
        }
        deferred_opens.append(&mut self.actions);
        self.actions = deferred_opens;
        self.host.sync_dirty();
        if processed_action {
            ctx.request_repaint();
        }
    }
    fn begin_close(&mut self, id: Option<TabId>, quit: bool) {
        if !self.receive_sheet_transfers() {
            return;
        }
        if self
            .print_dialog
            .as_ref()
            .is_some_and(|dialog| !dialog.can_close())
        {
            return;
        }
        self.print_dialog = None;
        self.cancel_file_dialogs();
        self.receive_generated();
        let ids: Vec<_> = id.map(|id| vec![id]).unwrap_or_else(|| {
            self.host
                .workspace
                .tabs()
                .iter()
                .map(|tab| tab.id)
                .collect()
        });
        self.host.prepare_close(&ids);
        if self.stop_close_for_background(&ids) {
            return;
        }
        if let Some(plan) = &mut self.close_plan {
            if quit && !plan.quit {
                plan.ids = self
                    .host
                    .workspace
                    .tabs()
                    .iter()
                    .map(|tab| tab.id)
                    .collect();
                plan.index = 0;
                plan.quit = true;
                self.close_prompt = None;
                self.close_focus = None;
            }
            return;
        }
        self.new_picker = false;
        self.preferences = false;
        self.about = false;
        self.close_plan = Some(ClosePlan {
            ids,
            index: 0,
            quit,
        });
    }
    fn stop_close_for_background(&mut self, ids: &[TabId]) -> bool {
        let Some(id) = self.host.pending_background(ids) else {
            return false;
        };
        let title = self
            .host
            .workspace
            .tab(id)
            .map(|tab| tab.title.as_str())
            .unwrap_or(tr("文档"));
        self.error = Some(format!(
            "“{title}”仍有后台任务或待处理的结果。请等待完成，或回到该标签完成处理后再关闭。"
        ));
        self.close_plan = None;
        self.close_prompt = None;
        self.close_focus = None;
        true
    }
    fn request_quit(&mut self, ctx: &egui::Context) {
        crate::native_file_panel::reveal_app();
        self.begin_close(None, true);
        // Native menu and Dock quit also arrive while eframe suspends UI for hidden windows.
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        ctx.request_repaint();
    }
    fn receive_generated(&mut self) {
        let before = self.host.workspace.tabs().len();
        let errors = self.host.receive_generated_pdfs();
        if self.host.workspace.tabs().len() != before || !errors.is_empty() {
            // Previous save/discard choices covered an older set of files and source content.
            // Keep the entire workspace when a derived document arrives during a close scan.
            self.close_plan = None;
            self.close_prompt = None;
            self.close_focus = None;
        }
        if let Some(error) = errors.into_iter().next_back() {
            self.error = Some(error);
        }
    }
    fn advance_close(&mut self, ctx: &egui::Context) {
        if !self.receive_sheet_transfers() {
            return;
        }
        if !self.file_dialogs.is_empty() {
            return;
        }
        self.receive_generated();
        if let Some(ids) = self.close_plan.as_ref().map(|plan| plan.ids.clone())
            && self.stop_close_for_background(&ids)
        {
            return;
        }
        if self.close_prompt.is_some() {
            return;
        }
        let Some(plan) = &self.close_plan else {
            return;
        };
        if plan.index >= plan.ids.len() {
            if let Some(plan) = self.close_plan.take() {
                for id in plan.ids {
                    self.host.close(id, true);
                }
                if plan.quit {
                    self.allow_exit = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            return;
        }
        let Some(id) = plan.ids.get(plan.index).copied() else {
            return;
        };
        if !self.host.documents.contains_key(&id) {
            if let Some(p) = self.close_plan.as_mut() {
                p.index += 1;
            }
            ctx.request_repaint();
            return;
        }
        if self.host.workspace.active() != Some(id) {
            self.host.workspace.activate(id);
            ctx.request_repaint();
            return;
        }
        if self.mounted != Some(id) {
            ctx.request_repaint();
            return;
        }
        // Ask before committing drafts. Discard must remain available even when a draft
        // cannot be saved until the editor's own Post/Cancel action has been chosen.
        if self.host.dirty(id) {
            self.close_prompt = Some(id);
            return;
        }
        let result = self
            .host
            .documents
            .get_mut(&id)
            .map(|d| d.editor.flush())
            .unwrap_or(Ok(()));
        if let Err(error) = result {
            self.error = Some(error);
            self.close_plan = None;
            return;
        }
        if self.host.dirty(id) {
            self.close_prompt = Some(id);
        } else if let Some(p) = self.close_plan.as_mut() {
            p.index += 1;
            ctx.request_repaint();
        }
    }
    fn next_tab(&mut self, backwards: bool) {
        let tabs = self.host.workspace.tabs();
        if tabs.is_empty() {
            return;
        }
        let current = tabs
            .iter()
            .position(|tab| Some(tab.id) == self.host.workspace.active())
            .unwrap_or(0);
        let next = if backwards {
            (current + tabs.len() - 1) % tabs.len()
        } else {
            (current + 1) % tabs.len()
        };
        if let Some(tab) = tabs.get(next) {
            self.actions.push_back(Action::Activate(tab.id));
        }
    }
    fn sync_interface_language(&mut self) {
        language::set_current(self.settings.language);
        for document in self.host.documents.values_mut() {
            document
                .editor
                .set_interface_language(self.settings.language);
        }
    }
    fn apply_interface_language(&mut self, ctx: &egui::Context, language: UiLanguage) {
        self.settings.language = language;
        self.sync_interface_language();
        self._native_menu.refresh_language();
        ctx.request_repaint();
    }
    fn apply_theme(&mut self, ctx: &egui::Context, request: ThemeRequest) {
        self.settings.theme = match request {
            ThemeRequest::Light => "light",
            ThemeRequest::Dark => "dark",
            ThemeRequest::System => "system",
            ThemeRequest::Toggle if ctx.global_style().visuals.dark_mode => "light",
            ThemeRequest::Toggle => "dark",
        }
        .into();
        ctx.set_theme(theme_preference(&self.settings.theme));
    }
    fn handle_editor_requests(&mut self, ctx: &egui::Context) {
        let requests = self
            .host
            .active_mut()
            .map(|d| d.editor.host_commands())
            .unwrap_or_default();
        self.dispatch_requests(ctx, requests);
    }
    fn dispatch_requests(
        &mut self,
        ctx: &egui::Context,
        requests: Vec<(String, serde_json::Value)>,
    ) {
        for (id, params) in requests {
            if self.host_dialog_open() {
                break;
            }
            let current = self.host.workspace.active();
            match commands::classify(&id, &params) {
                HostRequest::New { kind: Some(kind) } => self.actions.push_back(Action::New(kind)),
                HostRequest::New { kind: None } => self.new_picker = true,
                HostRequest::Open { path: Some(path) } => {
                    self.actions.push_back(Action::Open(path))
                }
                HostRequest::Open { path: None } => self.actions.push_back(Action::OpenDialog),
                HostRequest::Save { path, save_as } => {
                    if let Some(tab) = current {
                        self.actions.push_back(match path {
                            Some(path) => Action::SaveTo(tab, path),
                            None => Action::Save(tab, save_as),
                        });
                    }
                }
                HostRequest::ExportPdf { path } => {
                    if let Some(tab) = current {
                        self.actions.push_back(Action::ExportPdf(tab, path));
                    }
                }
                HostRequest::Print => self.begin_print(ctx),
                HostRequest::Undo => self.actions.push_back(Action::Command("edit.undo")),
                HostRequest::Redo => self.actions.push_back(Action::Command("edit.redo")),
                HostRequest::CloseCurrent => self.begin_close(current, false),
                HostRequest::CloseAll => self.begin_close(None, false),
                HostRequest::Quit => self.request_quit(ctx),
                HostRequest::Settings { pane } => {
                    self.preferences = true;
                    self.preferences_pane = pane;
                }
                HostRequest::InterfaceLanguage(language) => {
                    self.apply_interface_language(ctx, language)
                }
                HostRequest::About => self.about = true,
                HostRequest::NextTab => self.next_tab(false),
                HostRequest::PreviousTab => self.next_tab(true),
                HostRequest::Theme(theme) => self.apply_theme(ctx, theme),
                HostRequest::InterfaceScale(scale) => {
                    self.settings.scale = match scale {
                        ScaleRequest::Auto => 1.0,
                        ScaleRequest::Percent(percent) => (percent / 100.0).clamp(0.75, 2.0),
                    };
                    ctx.set_zoom_factor(self.settings.scale);
                }
                HostRequest::FullScreen(enabled) => {
                    let enabled = enabled.unwrap_or_else(|| {
                        !ctx.input(|i| i.viewport().fullscreen.unwrap_or(false))
                    });
                    ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(enabled));
                }
                HostRequest::Dismiss => {
                    self.new_picker = false;
                    self.preferences = false;
                    self.about = false;
                }
                HostRequest::Unsupported { id, .. } => {
                    self.error = Some(format!("此操作尚未接入统一工作区：{id}"))
                }
            }
        }
    }
    fn shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.input_mut(|input| consume_exact_shortcut(input, egui::Modifiers::COMMAND, Key::Q)) {
            self.request_quit(ctx);
            return;
        }
        let blocked = self.host_dialog_open()
            || self.editor_blocks_host_navigation()
            || ctx.memory(|memory| memory.top_modal_layer().is_some());
        let mut pressed = Vec::new();
        ctx.input_mut(|i| {
            for (key, shift, action) in [
                (Key::O, false, 0),
                (Key::N, false, 1),
                (Key::S, true, 3),
                (Key::S, false, 2),
                (Key::W, false, 4),
                (Key::Comma, false, 6),
                (Key::P, false, 9),
            ] {
                let mods = if shift {
                    egui::Modifiers::COMMAND | egui::Modifiers::SHIFT
                } else {
                    egui::Modifiers::COMMAND
                };
                if consume_exact_shortcut(i, mods, key) {
                    pressed.push(action);
                }
            }
            if consume_exact_shortcut(i, egui::Modifiers::CTRL | egui::Modifiers::SHIFT, Key::Tab) {
                pressed.push(8);
            }
            if consume_exact_shortcut(i, egui::Modifiers::CTRL, Key::Tab) {
                pressed.push(7);
            }
        });
        // Consume owned chords even while blocked: leaving them in the stream would let an
        // editor forward the same file command back through its hosted request channel.
        if blocked {
            return;
        }
        for action in pressed {
            match action {
                0 => self.actions.push_back(Action::OpenDialog),
                1 => {
                    let kind = self
                        .host
                        .workspace
                        .active()
                        .and_then(|id| self.host.workspace.tab(id))
                        .map(|t| t.kind)
                        .unwrap_or(DocumentKind::Text);
                    self.actions.push_back(Action::New(kind));
                }
                2 | 3 => {
                    if let Some(id) = self.host.workspace.active() {
                        self.actions.push_back(Action::Save(id, action == 3));
                    }
                }
                4 => self.begin_close(self.host.workspace.active(), false),
                6 => self.preferences = true,
                7 | 8 => self.next_tab(action == 8),
                9 => self.begin_print(ctx),
                _ => {}
            }
        }
    }
    fn shell(&mut self, ui: &mut egui::Ui) -> egui::Rect {
        egui::Panel::top("weave-files")
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                let ctx = ui.ctx().clone();
                ui.set_style(crate::theme::style(&ctx));
                let blocked = self.blocking_dialog_open() || self.editor_blocks_host_navigation();
                let (_, close, bar) = crate::theme::window_bar(ui, |ui| {
                    ui.add_enabled_ui(!blocked, |ui| {
                        let tabs: Vec<_> = self
                            .host
                            .workspace
                            .tabs()
                            .iter()
                            .map(|t| (t.id, t.kind, t.title.clone(), t.is_dirty()))
                            .collect();
                        if tabs.is_empty() {
                            ui.label(RichText::new("Weave Office").size(13.0));
                            ui.add_space(12.0);
                        } else {
                            let width = (ui.available_width() - 50.0).max(60.0);
                            let area = egui::ScrollArea::horizontal()
                                .id_salt("file-tabs")
                                .max_width(width)
                                .scroll_bar_visibility(
                                    egui::scroll_area::ScrollBarVisibility::AlwaysHidden,
                                )
                                .animated(false)
                                .horizontal_scroll_offset(self.tab_scroll)
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 2.0;
                                        for (id, kind, title, dirty) in tabs {
                                            let selected = self.host.workspace.active() == Some(id);
                                            let (rect, _) = ui.allocate_exact_size(
                                                egui::vec2(198.0, 28.0),
                                                egui::Sense::hover(),
                                            );
                                            let body = egui::Rect::from_min_max(
                                                rect.min,
                                                rect.max - egui::vec2(28.0, 0.0),
                                            );
                                            let caption = format!(
                                                "{}{}",
                                                short_title(&title),
                                                if dirty { " •" } else { "" }
                                            );
                                            let response = ui.interact(
                                                body,
                                                ui.scope_id().with(("file-tab", id)),
                                                egui::Sense::click_and_drag(),
                                            );
                                            response.widget_info(|| {
                                                egui::WidgetInfo::labeled(
                                                    egui::Role::Button,
                                                    ui.is_enabled(),
                                                    &caption,
                                                )
                                            });
                                            let fill = if selected {
                                                ui.visuals().window_fill()
                                            } else if response.hovered() {
                                                ui.visuals().widgets.hovered.weak_bg_fill
                                            } else {
                                                Color32::TRANSPARENT
                                            };
                                            ui.painter().rect_filled(
                                                rect,
                                                egui::CornerRadius {
                                                    nw: 5,
                                                    ne: 5,
                                                    sw: 0,
                                                    se: 0,
                                                },
                                                fill,
                                            );
                                            if !selected {
                                                ui.painter().vline(
                                                    rect.right(),
                                                    rect.y_range().shrink(6.0),
                                                    ui.visuals().widgets.noninteractive.bg_stroke,
                                                );
                                            }
                                            let icon_rect = egui::Rect::from_center_size(
                                                egui::pos2(rect.left() + 17.0, rect.center().y),
                                                egui::vec2(20.0, 20.0),
                                            );
                                            crate::icons::document(ui.painter(), icon_rect, kind);
                                            let icon_response = ui.interact(
                                                icon_rect,
                                                ui.scope_id().with(("file-icon", id)),
                                                egui::Sense::hover(),
                                            );
                                            icon_response.widget_info(|| {
                                                egui::WidgetInfo::labeled(
                                                    egui::Role::Image,
                                                    true,
                                                    tr("{kind}图标")
                                                        .replace("{kind}", kind_name(kind)),
                                                )
                                            });
                                            let mut job = egui::text::LayoutJob::simple(
                                                caption,
                                                egui::FontId::proportional(13.0),
                                                ui.visuals().text_color(),
                                                body.width() - 38.0,
                                            );
                                            job.wrap.max_rows = 1;
                                            job.wrap.break_anywhere = true;
                                            let galley = ui.painter().layout_job(job);
                                            ui.painter().galley(
                                                egui::pos2(
                                                    rect.left() + 32.0,
                                                    rect.center().y - galley.size().y / 2.0,
                                                ),
                                                galley,
                                                ui.visuals().text_color(),
                                            );
                                            if response.clicked() {
                                                self.actions.push_back(Action::Activate(id));
                                            }
                                            response.dnd_set_drag_payload(id);
                                            if let Some(from) =
                                                response.dnd_release_payload::<TabId>()
                                            {
                                                self.actions.push_back(Action::Reorder(*from, id));
                                            }
                                            response.context_menu(|ui| {
                                                if ui.button(tr("关闭")).clicked() {
                                                    self.actions
                                                        .push_back(Action::Close(vec![id], false));
                                                    ui.close();
                                                }
                                                if ui.button(tr("关闭其他标签")).clicked() {
                                                    let ids = self
                                                        .host
                                                        .workspace
                                                        .tabs()
                                                        .iter()
                                                        .filter(|t| t.id != id)
                                                        .map(|t| t.id)
                                                        .collect();
                                                    self.actions
                                                        .push_back(Action::Close(ids, false));
                                                    ui.close();
                                                }
                                            });
                                            response.on_hover_text(&title);
                                            let close_rect = egui::Rect::from_min_max(
                                                egui::pos2(rect.right() - 28.0, rect.top()),
                                                rect.max,
                                            );
                                            let close = ui.interact(
                                                close_rect,
                                                ui.scope_id().with(("file-close", id)),
                                                egui::Sense::click(),
                                            );
                                            close.widget_info(|| {
                                                egui::WidgetInfo::labeled(
                                                    egui::Role::Button,
                                                    ui.is_enabled(),
                                                    tr("关闭 {title}").replace("{title}", &title),
                                                )
                                            });
                                            if close.hovered() {
                                                ui.painter().rect_filled(
                                                    close_rect.shrink(4.0),
                                                    3,
                                                    ui.visuals().widgets.hovered.weak_bg_fill,
                                                );
                                            }
                                            crate::icons::paint(
                                                ui.painter(),
                                                egui::Rect::from_center_size(
                                                    close_rect.center(),
                                                    egui::vec2(13.0, 13.0),
                                                ),
                                                crate::icons::Icon::Close,
                                                ui.visuals().weak_text_color(),
                                            );
                                            if close.on_hover_text(tr("关闭标签")).clicked() {
                                                self.actions
                                                    .push_back(Action::Close(vec![id], false));
                                            }
                                            if self.reveal_tab == Some(id) {
                                                ui.scroll_to_rect(rect, Some(egui::Align::Center));
                                            }
                                        }
                                    });
                                });
                            self.tab_scroll = area.state.clamped_offset().x;
                            self.reveal_tab = None;
                        }
                        let new_file = crate::new_document::button(ui, self.new_picker);
                        self.new_anchor = Some((new_file.rect, ui.layer_id(), new_file.id));
                        if new_file.clicked() {
                            self.new_picker = !self.new_picker;
                        }
                    });
                });
                if close {
                    self.request_quit(&ctx);
                }
                bar
            })
            .inner
    }
    fn home(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("weave-home-tools")
            .frame(
                egui::Frame::NONE
                    .fill(ui.visuals().window_fill())
                    .inner_margin(egui::Margin::symmetric(10, 2)),
            )
            .show(ui, |ui| {
                let commands = ui.horizontal(crate::chrome::toolbar_for_home).inner;
                self.dispatch_requests(ui.ctx(), commands);
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(ui.visuals().panel_fill))
            .show(ui, |ui| {
                ui.set_style(crate::theme::style(ui.ctx()));
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let width = ui.available_width().min(900.0);
                    let left = ((ui.available_width() - width) / 2.0).max(24.0);
                    ui.add_space(40.0);
                    ui.horizontal(|ui| {
                        ui.add_space(left);
                        ui.vertical(|ui| {
                            ui.set_width((width - 48.0).max(240.0));
                            ui.label(RichText::new(tr("新建")).size(20.0).strong());
                            ui.add_space(16.0);
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing.x = 12.0;
                                for kind in kinds() {
                                    let (rect, response) = ui.allocate_exact_size(
                                        egui::vec2(180.0, 78.0),
                                        egui::Sense::click(),
                                    );
                                    response.widget_info(|| {
                                        egui::WidgetInfo::labeled(
                                            egui::Role::Button,
                                            true,
                                            kind_name(kind),
                                        )
                                    });
                                    let fill = if response.hovered() {
                                        ui.visuals().widgets.hovered.weak_bg_fill
                                    } else {
                                        ui.visuals().window_fill()
                                    };
                                    ui.painter().rect_filled(rect, 5, fill);
                                    ui.painter().rect_stroke(
                                        rect,
                                        5,
                                        ui.visuals().widgets.noninteractive.bg_stroke,
                                        egui::StrokeKind::Inside,
                                    );
                                    crate::icons::document(
                                        ui.painter(),
                                        egui::Rect::from_center_size(
                                            egui::pos2(rect.left() + 36.0, rect.center().y),
                                            egui::vec2(40.0, 40.0),
                                        ),
                                        kind,
                                    );
                                    ui.painter().text(
                                        egui::pos2(rect.left() + 67.0, rect.center().y),
                                        egui::Align2::LEFT_CENTER,
                                        kind_name(kind),
                                        egui::FontId::proportional(14.0),
                                        ui.visuals().text_color(),
                                    );
                                    if response.clicked() {
                                        self.actions.push_back(Action::New(kind));
                                    }
                                }
                            });
                            ui.add_space(36.0);
                            ui.label(RichText::new(tr("最近文件")).size(16.0).strong());
                            ui.add_space(12.0);
                            if self.settings.recent.is_empty() {
                                ui.label(
                                    RichText::new(tr("打开或新建文件，开始办公"))
                                        .color(ui.visuals().weak_text_color()),
                                );
                            }
                            for path in self.settings.recent.clone() {
                                let name = path
                                    .file_name()
                                    .map(|s| s.to_string_lossy())
                                    .unwrap_or_default();
                                let (rect, response) = ui.allocate_exact_size(
                                    egui::vec2(ui.available_width(), 52.0),
                                    egui::Sense::click(),
                                );
                                response.widget_info(|| {
                                    egui::WidgetInfo::labeled(
                                        egui::Role::Button,
                                        true,
                                        name.as_ref(),
                                    )
                                });
                                if response.hovered() {
                                    ui.painter().rect_filled(
                                        rect,
                                        3,
                                        ui.visuals().widgets.hovered.weak_bg_fill,
                                    );
                                }
                                if let Some(kind) = DocumentKind::from_path(&path) {
                                    crate::icons::document(
                                        ui.painter(),
                                        egui::Rect::from_center_size(
                                            egui::pos2(rect.left() + 18.0, rect.center().y),
                                            egui::vec2(30.0, 30.0),
                                        ),
                                        kind,
                                    );
                                }
                                ui.painter().text(
                                    egui::pos2(rect.left() + 44.0, rect.top() + 15.0),
                                    egui::Align2::LEFT_CENTER,
                                    name,
                                    egui::FontId::proportional(13.0),
                                    ui.visuals().text_color(),
                                );
                                ui.painter().text(
                                    egui::pos2(rect.left() + 44.0, rect.top() + 35.0),
                                    egui::Align2::LEFT_CENTER,
                                    path.parent()
                                        .map(|p| p.display().to_string())
                                        .unwrap_or_default(),
                                    egui::FontId::proportional(11.0),
                                    ui.visuals().weak_text_color(),
                                );
                                if response.on_hover_text(path.display().to_string()).clicked() {
                                    self.actions.push_back(Action::Open(path));
                                }
                            }
                        });
                    });
                });
            });
    }
    fn dialogs(&mut self, ctx: &egui::Context) {
        if self.error.is_none() && !self.file_dialogs.is_empty() {
            self.file_dialog(ctx);
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape)) {
            if self.error.is_some() {
                self.error = None;
            } else {
                self.new_picker = false;
                self.preferences = false;
                self.about = false;
                self.close_prompt = None;
                self.close_plan = None;
                if self
                    .print_dialog
                    .as_ref()
                    .is_some_and(|dialog| dialog.can_close())
                {
                    self.print_dialog = None;
                }
            }
        }
        if let Some(dialog) = &mut self.print_dialog {
            if dialog.show(ctx, &mut self.host) {
                self.print_dialog = None;
            }
        }
        if self.new_picker {
            if let Some(anchor) = self.new_anchor {
                if let Some(choice) = crate::new_document::show(ctx, anchor, &mut self.new_picker) {
                    self.actions.push_back(match choice.extension {
                        Some(extension) => Action::NewText(extension),
                        None => Action::New(choice.kind),
                    });
                }
            }
        }
        if self.preferences {
            let mut open = self.preferences;
            let mut selected_language = self.settings.language;
            egui::Window::new(tr("设置"))
                .frame(crate::theme::surface_frame(ctx))
                .id(egui::Id::unique("weave-settings"))
                .open(&mut open)
                .collapsible(false)
                .default_width(380.0)
                .show(ctx, |ui| {
                    ui.set_style(crate::theme::style(ctx));
                    ui.horizontal(|ui| {
                        ui.selectable_value(
                            &mut self.preferences_pane,
                            SettingsPane::General,
                            tr("常规"),
                        );
                        ui.selectable_value(
                            &mut self.preferences_pane,
                            SettingsPane::Language,
                            tr("语言"),
                        );
                    });
                    ui.separator();
                    match self.preferences_pane {
                        SettingsPane::General => {
                            ui.horizontal(|ui| {
                                ui.label(tr("外观"));
                                for (value, label) in
                                    [("light", "浅色"), ("dark", "深色"), ("system", "跟随系统")]
                                {
                                    if ui
                                        .selectable_label(self.settings.theme == value, tr(label))
                                        .clicked()
                                    {
                                        self.settings.theme = value.into();
                                        ctx.set_theme(theme_preference(value));
                                    }
                                }
                            });
                            ui.label(tr("界面缩放"));
                            if ui
                                .add(
                                    egui::Slider::new(&mut self.settings.scale, 0.75..=2.0)
                                        .suffix("×"),
                                )
                                .changed()
                            {
                                ctx.set_zoom_factor(self.settings.scale);
                            }
                        }
                        SettingsPane::Language => {
                            ui.horizontal(|ui| {
                                ui.label(tr("界面语言"));
                                egui::ComboBox::from_id_salt("weave-interface-language")
                                    .selected_text(selected_language.name())
                                    .show_ui(ui, |ui| {
                                        for language in UiLanguage::ALL {
                                            ui.selectable_value(
                                                &mut selected_language,
                                                language,
                                                language.name(),
                                            );
                                        }
                                    });
                            });
                            ui.label(
                                RichText::new(tr("应用于所有文档窗口，不改变文档校对语言。"))
                                    .small()
                                    .weak(),
                            );
                            if let Some(status) = self
                                .host
                                .active()
                                .and_then(|doc| doc.editor.proofing_status())
                            {
                                ui.add_space(8.0);
                                ui.label(tr("校对语言"));
                                ui.label(RichText::new(status).small().weak());
                            }
                        }
                    }
                });
            self.preferences = open;
            if selected_language != self.settings.language {
                self.apply_interface_language(ctx, selected_language);
            }
        }
        if self.about {
            egui::Window::new(tr("关于 Weave Office"))
                .frame(crate::theme::surface_frame(ctx))
                .open(&mut self.about)
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.set_style(crate::theme::style(ctx));
                    ui.heading("Weave Office");
                    ui.label(env!("CARGO_PKG_VERSION"));
                    ui.label(tr("文字 · 表格 · 演示 · PDF"));
                    ui.label(tr(
                        "编辑引擎基于 WordCraft、GridCraft、DeckCraft 和 PdfCraft。",
                    ));
                });
        }
        if self.restore_prompt && self.error.is_none() {
            let titles: Vec<String> = self
                .recovery
                .as_ref()
                .map(|recovery| {
                    recovery
                        .found()
                        .iter()
                        .map(|found| found.manifest.title.clone())
                        .collect()
                })
                .unwrap_or_default();
            let mut choice = None;
            egui::Modal::new(egui::Id::unique("weave-restore"))
                .frame(dialog::frame(ctx))
                .show(ctx, |ui| {
                    dialog::prepare(ui);
                    dialog::header(ui, tr("恢复未保存的文档？"));
                    dialog::body(
                        ui,
                        tr("Weave Office 上次没有正常退出。以下文档有自动保存的副本："),
                    );
                    for title in &titles {
                        ui.label(format!("• {title}"));
                    }
                    let (discard, restore) = dialog::split_footer(
                        ui,
                        |ui| dialog::button(ui, tr("不恢复"), ButtonKind::Destructive),
                        |ui| dialog::button(ui, tr("恢复"), ButtonKind::Primary),
                    );
                    if ui.memory(|memory| memory.focused().is_none()) {
                        restore.request_focus();
                    }
                    choice = if restore.clicked() {
                        Some(true)
                    } else if discard.clicked() {
                        Some(false)
                    } else {
                        None
                    };
                });
            match choice {
                Some(true) => self.restore_recovered(),
                Some(false) => {
                    if let Some(recovery) = self.recovery.as_mut() {
                        recovery.clear_found();
                    }
                    self.restore_prompt = false;
                }
                None => {}
            }
        }
        if let Some(error) = self.error.clone() {
            egui::Modal::new(egui::Id::unique("weave-error"))
                .frame(dialog::frame(ctx))
                .show(ctx, |ui| {
                    dialog::prepare(ui);
                    dialog::header(ui, tr("操作未完成"));
                    dialog::body(ui, &error);
                    dialog::footer(ui, |ui| {
                        if dialog::button(ui, tr("确定"), ButtonKind::Primary).clicked() {
                            self.error = None;
                        }
                    });
                });
        }
        if let Some(id) = self.close_prompt.filter(|_| self.error.is_none()) {
            let title = self
                .host
                .workspace
                .tab(id)
                .map(|t| t.title.clone())
                .unwrap_or_default();
            let mut choice = None;
            let focus_save = self.close_focus != Some(id);
            self.close_focus = Some(id);
            egui::Modal::new(egui::Id::unique("weave-close"))
                .frame(dialog::frame(ctx))
                .show(ctx, |ui| {
                    dialog::prepare(ui);
                    dialog::header(ui, tr("保存更改？"));
                    dialog::body(
                        ui,
                        &tr("“{title}”有未保存的修改。").replace("{title}", &title),
                    );
                    let (discard, (save, cancel)) = dialog::split_footer(
                        ui,
                        |ui| dialog::button(ui, tr("不保存"), ButtonKind::Destructive),
                        |ui| {
                            let save = dialog::button(ui, tr("保存"), ButtonKind::Primary);
                            let cancel = dialog::button(ui, tr("取消"), ButtonKind::Secondary);
                            (save, cancel)
                        },
                    );
                    if focus_save {
                        save.request_focus();
                    }
                    choice = if cancel.clicked() {
                        Some(0)
                    } else if discard.clicked() {
                        Some(1)
                    } else if save.clicked() {
                        Some(2)
                    } else {
                        None
                    };
                });
            match choice {
                Some(0) => {
                    self.close_prompt = None;
                    self.close_plan = None;
                }
                Some(1) => {
                    self.close_prompt = None;
                    if let Some(p) = self.close_plan.as_mut() {
                        p.index += 1;
                    }
                }
                Some(2) => match self.save(id, false) {
                    Ok(true) => {
                        self.close_prompt = None;
                        if let Some(p) = self.close_plan.as_mut() {
                            p.index += 1;
                        }
                    }
                    Ok(false) => {}
                    Err(error) => self.error = Some(error),
                },
                _ => {}
            }
        } else {
            self.close_focus = None;
        }
    }
}

impl eframe::App for WeaveApp {
    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        visuals.panel_fill.to_normalized_gamma_f32()
    }
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.sync_interface_language();
        // Restore the next document's focus and IME memory before egui consumes its events.
        if self.mounted != self.host.workspace.active() {
            if let Err(error) = self.mount_active(ctx) {
                self.error = Some(error);
            }
        }
        if cfg!(target_os = "macos") {
            normalize_control_click(&mut raw.events, &mut self.control_click);
            if !raw.focused {
                self.control_click = false;
            }
        }
        for file in std::mem::take(&mut raw.dropped_files) {
            self.actions
                .push_back(Action::OpenExternal(file.path().to_path_buf()));
        }
        if raw.events.iter().any(edits_input) {
            self.last_input = std::time::Instant::now();
            if let (Some(recovery), Some(id)) =
                (self.recovery.as_mut(), self.host.workspace.active())
            {
                recovery.touch(id);
            }
        }
        if !self.host_dialog_open() && self.mounted == self.host.workspace.active() {
            if let Some(doc) = self.host.active_mut() {
                doc.editor.raw_input(ctx, raw);
            }
        }
    }
    fn logic(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.sync_interface_language();
        // A replayed egui pass may follow a tab action from the previous pass.
        // Restore its fonts before any editor or shell text is laid out.
        if let Err(error) = self.mount_active(ctx) {
            self.error = Some(error);
        }
        self.sync_fonts(ctx);
        self.host.poll_background(ctx);
        self.receive_generated();
        #[cfg(target_os = "macos")]
        if let Some(inbox) = &self.os_events {
            let events = inbox.drain();
            for event in events {
                match event {
                    fmv_macos_events::Event::Open(paths) => self
                        .actions
                        .extend(paths.into_iter().map(Action::OpenExternal)),
                    fmv_macos_events::Event::Quit => self.request_quit(ctx),
                }
            }
        }
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_exit {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.request_quit(ctx);
        }
        self.shortcuts(ctx);
        if !self.host_dialog_open() && self.mounted == self.host.workspace.active() {
            if let Some(doc) = self.host.active_mut() {
                doc.editor.logic(ctx);
            }
        }
        self.receive_sheet_transfers();
        self.apply_weave_style(ctx);
        if self.allow_exit {
            // The shared save transaction already approved the whole workspace. Use
            // the platform's deferred quit directly, without depending on another
            // viewport Close event after removing the last document.
            if craft_quit::quit() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        } else {
            craft_quit::on_close_request(ctx);
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.sync_interface_language();
        let ctx = ui.ctx().clone();
        if let Err(error) = self.mount_active(&ctx) {
            self.error = Some(error);
        }
        self.sync_fonts(&ctx);
        self.install_shell_font(&ctx);
        self.apply_weave_style(&ctx);
        // The host owns a complete opaque surface, including initialization/error
        // paths where an editor has no document canvas yet.
        ui.painter().rect_filled(
            ctx.content_rect(),
            0,
            crate::theme::style(&ctx).visuals.panel_fill,
        );
        self.advance_close(&ctx);
        let bar = self.shell(ui);
        #[cfg(target_os = "macos")]
        frame.set_traffic_lights_position(&ctx, bar.y_range(), bar.left() + 12.0);
        #[cfg(not(target_os = "macos"))]
        let _ = (frame, bar);
        let blocked = self.host_dialog_open();
        let mut row_commands = Vec::new();
        ui.add_enabled_ui(!self.blocking_dialog_open(), |ui| {
            if self.mounted != self.host.workspace.active() {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE.fill(ui.visuals().extreme_bg_color))
                    .show(ui, |_| {});
                ctx.request_repaint();
            } else if let Some(doc) = self.host.active_mut() {
                if blocked {
                    frozen_editor_input(&ctx, || {
                        row_commands = doc.editor.ui(ui);
                    });
                } else {
                    row_commands = doc.editor.ui(ui);
                }
            } else {
                if blocked {
                    frozen_editor_input(&ctx, || self.home(ui));
                } else {
                    self.home(ui);
                }
            }
        });
        self.receive_sheet_transfers();
        self.dispatch_requests(&ctx, row_commands);
        self.handle_editor_requests(&ctx);
        self.receive_file_requests();
        self.receive_generated();
        self.host.sync_dirty();
        self.autosave(&ctx);
        self.dialogs(&ctx);
        let title = self
            .host
            .workspace
            .active()
            .and_then(|id| self.host.workspace.tab(id))
            .map(|t| {
                format!(
                    "{}{} — Weave Office",
                    t.title,
                    if t.is_dirty() { " •" } else { "" }
                )
            })
            .unwrap_or_else(|| "Weave Office".into());
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
        if !self.allow_exit && !self.actions.is_empty() {
            self.process_actions(&ctx);
        }
        if self.close_plan.is_some() {
            ctx.request_repaint();
        }
        if let Some(frames) = &mut self.startup_frames {
            *frames = frames.saturating_sub(1);
            if *frames == 0 {
                self.startup_frames = None;
                self.request_quit(&ctx);
            }
            ctx.request_repaint();
        }
    }
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Some(parent) = settings_path().parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(data) = serde_json::to_vec_pretty(&self.settings) {
            let path = settings_path();
            if let Some(parent) = path.parent() {
                if let Ok(mut file) = tempfile::NamedTempFile::new_in(parent) {
                    use std::io::Write;
                    if file.write_all(&data).is_ok() {
                        let _ = file.persist(path);
                    }
                }
            }
        }
    }
}
fn theme_preference(value: &str) -> egui::ThemePreference {
    match value {
        "dark" => egui::ThemePreference::Dark,
        "system" => egui::ThemePreference::System,
        _ => egui::ThemePreference::Light,
    }
}

/// Whether an input event can change a document: a key, typed or pasted text, a press or an
/// input method. Pointer movement and focus changes cannot.
fn edits_input(event: &egui::Event) -> bool {
    matches!(
        event,
        egui::Event::Key { .. }
            | egui::Event::Text(_)
            | egui::Event::Paste(_)
            | egui::Event::Cut
            | egui::Event::Ime(_)
            | egui::Event::PointerButton { .. }
    )
}

/// Normalize macOS Control-click once for the shell and all embedded editing canvases.
fn normalize_control_click(events: &mut [egui::Event], held: &mut bool) {
    for event in events {
        if let egui::Event::PointerButton {
            button: button @ egui::PointerButton::Primary,
            pressed,
            modifiers,
            ..
        } = event
        {
            if *pressed {
                *held = modifiers.ctrl && !modifiers.mac_cmd;
            }
            if *held {
                *button = egui::PointerButton::Secondary;
            }
            if !*pressed {
                *held = false;
            }
        }
    }
}

/// Keep the document visible behind host dialogs. Some upstream canvases read Context input
/// directly, so disabling widgets alone does not stop typing, dragging or viewport gestures.
fn frozen_editor_input(ctx: &egui::Context, draw: impl FnOnce()) {
    let options = ctx.options(|o| o.input_options);
    let original = ctx.input_mut(|input| {
        let original = input.clone();
        let mut raw = original.raw.clone();
        raw.events.clear();
        raw.dropped_files.clear();
        raw.hovered_files.clear();
        raw.focused = false;
        raw.screen_rect = Some(original.viewport_rect());
        raw.time = Some(original.time);
        *input =
            egui::InputState::default().begin_pass(raw, false, original.pixels_per_point, options);
        original
    });
    draw();
    ctx.input_mut(|input| *input = original);
}
fn settings_path() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Library/Application Support/Weave Office/settings.json")
}
fn kinds() -> [DocumentKind; 7] {
    [
        DocumentKind::Text,
        DocumentKind::Spreadsheet,
        DocumentKind::Presentation,
        DocumentKind::Pdf,
        DocumentKind::Markdown,
        DocumentKind::PlainText,
        DocumentKind::RichText,
    ]
}
fn kind_name(k: DocumentKind) -> &'static str {
    match k {
        DocumentKind::Text => tr("文字文档"),
        DocumentKind::Spreadsheet => tr("电子表格"),
        DocumentKind::Presentation => tr("演示文稿"),
        DocumentKind::Pdf => "PDF",
        DocumentKind::Markdown => "Markdown",
        DocumentKind::PlainText => tr("纯文本"),
        DocumentKind::RichText => tr("富文本"),
    }
}

fn short_title(title: &str) -> String {
    let text: String = title.chars().take(22).collect();
    if title.chars().count() > 22 {
        text + "…"
    } else {
        text
    }
}

/// Host shortcuts must leave modified editor commands (e.g. ⇧⌘P) in the event stream.
/// egui's consume_key deliberately ignores additional Shift/Alt modifiers.
fn consume_exact_shortcut(
    input: &mut egui::InputState,
    modifiers: egui::Modifiers,
    key: Key,
) -> bool {
    let mut consumed = false;
    input.events.retain(|event| {
        let matches = matches!(event,
            egui::Event::Key { key: actual, modifiers: actual_modifiers, pressed: true, .. }
                if *actual == key && actual_modifiers.matches_exact(modifiers)
        );
        consumed |= matches;
        !matches
    });
    consumed
}

#[cfg(test)]
mod editing_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_shortcuts_preserve_modified_editor_commands_and_use_each_events_modifiers() {
        let mut host = Host::default();
        let tab = host.new_document(DocumentKind::Presentation).unwrap();
        let mut app = test_app(host);
        let ctx = egui::Context::default();
        let key = |key, modifiers| egui::Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat: false,
            modifiers,
        };
        let editor_commands = vec![
            key(Key::P, egui::Modifiers::COMMAND | egui::Modifiers::SHIFT),
            key(Key::N, egui::Modifiers::COMMAND | egui::Modifiers::SHIFT),
            key(Key::O, egui::Modifiers::COMMAND | egui::Modifiers::ALT),
            key(Key::W, egui::Modifiers::COMMAND | egui::Modifiers::SHIFT),
        ];
        ctx.begin_pass(egui::RawInput {
            events: editor_commands.clone(),
            ..Default::default()
        });
        app.shortcuts(&ctx);
        assert!(
            app.print_dialog.is_none(),
            "command palette must not open print"
        );
        assert!(
            app.actions.is_empty(),
            "modified editor keys must not create/open files"
        );
        assert!(app.close_plan.is_none());
        assert_eq!(ctx.input(|input| input.events.clone()), editor_commands);
        ctx.end_pass().textures_delta.clear();

        let palette = key(Key::P, egui::Modifiers::COMMAND | egui::Modifiers::SHIFT);
        let ctx = egui::Context::default();
        ctx.begin_pass(egui::RawInput {
            events: vec![
                key(Key::S, egui::Modifiers::COMMAND | egui::Modifiers::SHIFT),
                palette.clone(),
            ],
            ..Default::default()
        });
        app.shortcuts(&ctx);
        assert!(matches!(app.actions.pop_front(), Some(Action::Save(id, true)) if id == tab));
        assert!(app.actions.is_empty());
        assert_eq!(ctx.input(|input| input.events.clone()), vec![palette]);
        ctx.end_pass().textures_delta.clear();
    }

    fn open_editor_modal(editor: &mut crate::editor::Editor) {
        let (command, params) = match editor {
            crate::editor::Editor::Word(_) | crate::editor::Editor::Sheet(_) => {
                ("ui.dialog", json!({"name":"zoom"}))
            }
            crate::editor::Editor::Slides(_) => ("view.zoomDialog", json!({})),
            crate::editor::Editor::Pdf(_) => ("file.properties", json!({})),
            crate::editor::Editor::TextFile(_) | crate::editor::Editor::Rich(_) => {
                unreachable!("the text editors have no modal")
            }
        };
        editor.command(command, params).unwrap();
        assert!(editor.blocks_host_navigation());
    }

    fn shortcut_event(key: Key, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    #[test]
    fn editor_modals_consume_host_shortcuts_without_closing_or_switching_documents() {
        for kind in [
            DocumentKind::Text,
            DocumentKind::Spreadsheet,
            DocumentKind::Presentation,
            DocumentKind::Pdf,
        ] {
            let mut host = Host::default();
            host.new_document(DocumentKind::Text).unwrap();
            let id = host.new_document(kind).unwrap();
            open_editor_modal(&mut host.active_mut().unwrap().editor);
            let mut app = test_app(host);
            let ctx = egui::Context::default();
            let copy = shortcut_event(Key::C, egui::Modifiers::COMMAND);
            let palette = shortcut_event(Key::P, egui::Modifiers::COMMAND | egui::Modifiers::SHIFT);
            let mut events = [Key::N, Key::O, Key::S, Key::W, Key::Comma, Key::P]
                .into_iter()
                .map(|key| shortcut_event(key, egui::Modifiers::COMMAND))
                .collect::<Vec<_>>();
            events.extend([
                shortcut_event(Key::S, egui::Modifiers::COMMAND | egui::Modifiers::SHIFT),
                shortcut_event(Key::Tab, egui::Modifiers::CTRL | egui::Modifiers::SHIFT),
                shortcut_event(Key::Tab, egui::Modifiers::CTRL),
                copy.clone(),
                palette.clone(),
            ]);
            ctx.begin_pass(egui::RawInput {
                events,
                ..Default::default()
            });
            let editor_events = ctx.input(|input| input.events[input.events.len() - 2..].to_vec());
            app.shortcuts(&ctx);
            assert!(app.actions.is_empty(), "{kind:?}");
            assert!(app.close_plan.is_none());
            assert!(app.file_dialogs.is_empty());
            assert!(app.print_dialog.is_none());
            assert!(!app.preferences);
            assert_eq!(app.host.workspace.active(), Some(id));
            assert_eq!(app.host.documents.len(), 2);
            assert!(app.editor_blocks_host_navigation());
            assert_eq!(ctx.input(|input| input.events.clone()), editor_events);
            ctx.end_pass().textures_delta.clear();
        }
    }

    #[test]
    fn navigation_work_panes_keep_host_shortcuts_available() {
        for (kind, command) in [
            (DocumentKind::Text, "edit.find"),
            (DocumentKind::Pdf, "page.combine"),
        ] {
            let mut host = Host::default();
            let id = host.new_document(kind).unwrap();
            host.active_mut()
                .unwrap()
                .editor
                .command(command, json!({}))
                .unwrap();
            match &host.active().unwrap().editor {
                crate::editor::Editor::Word(word) => assert!(word.session.view.nav_pane),
                crate::editor::Editor::Pdf(pdf) => assert!(pdf.combine_showing()),
                _ => unreachable!(),
            }
            let mut app = test_app(host);
            assert!(!app.editor_blocks_host_navigation());
            let ctx = egui::Context::default();
            ctx.begin_pass(egui::RawInput {
                events: vec![shortcut_event(Key::N, egui::Modifiers::COMMAND)],
                ..Default::default()
            });
            app.shortcuts(&ctx);
            assert!(
                matches!(app.actions.front(), Some(Action::New(new_kind)) if *new_kind == kind)
            );
            assert_eq!(app.host.workspace.active(), Some(id));
            ctx.end_pass().textures_delta.clear();
        }
    }

    #[test]
    fn editor_modals_disable_tab_pointer_actions_until_cancelled() {
        for kind in [
            DocumentKind::Text,
            DocumentKind::Spreadsheet,
            DocumentKind::Presentation,
            DocumentKind::Pdf,
        ] {
            let mut host = Host::default();
            let other = host.new_document(DocumentKind::Text).unwrap();
            let id = host.new_document(kind).unwrap();
            open_editor_modal(&mut host.active_mut().unwrap().editor);
            let other_title = host.workspace.tab(other).unwrap().title.clone();
            let title = host.workspace.tab(id).unwrap().title.clone();
            let mut replay = ShellPointerReplay::new(host);
            for label in [other_title, format!("关闭 {title}"), "新建文件".into()] {
                let rect = replay.widget_rect(&label);
                replay.click(rect.center());
                assert_eq!(replay.app.host.workspace.active(), Some(id), "{kind:?}");
                assert_eq!(replay.app.host.documents.len(), 2);
                assert!(replay.app.close_plan.is_none());
                assert!(!replay.app.new_picker);
            }
            // Cancel through each editor's existing command and retry the same pointer.
            if matches!(kind, DocumentKind::Text | DocumentKind::Spreadsheet) {
                let (command, params) = if kind == DocumentKind::Text {
                    ("ui.closeDialog", json!({}))
                } else {
                    ("ui.dialog", json!({"name":""}))
                };
                replay
                    .app
                    .host
                    .active_mut()
                    .unwrap()
                    .editor
                    .command(command, params)
                    .unwrap();
                replay.frame(Vec::new());
                replay.frame(Vec::new());
                let plus = replay.widget_rect("新建文件");
                replay.click(plus.center());
                assert!(replay.app.new_picker);
            }
        }
    }

    #[test]
    fn editor_modal_defers_external_opens_but_allows_confirmed_save_then_resumes() {
        let directory = tempfile::tempdir().unwrap();
        let external = directory.path().join("incoming.docx");
        let saved = directory.path().join("confirmed.docx");
        let mut source = Host::default();
        let source_id = source.new_document(DocumentKind::Text).unwrap();
        source.save_as(source_id, &external).unwrap();
        let external = std::fs::canonicalize(external).unwrap();
        let mut host = Host::default();
        let id = host.new_document(DocumentKind::Text).unwrap();
        host.active_mut()
            .unwrap()
            .editor
            .command("text.insert", json!({"text":"尚未保存的对话框草稿"}))
            .unwrap();
        open_editor_modal(&mut host.active_mut().unwrap().editor);
        let mut app = test_app(host);
        let ctx = egui::Context::default();
        app.actions
            .push_back(Action::OpenExternal(external.clone()));
        // Simulate an already-confirmed editor request through the real host dispatcher.
        app.dispatch_requests(&ctx, vec![("file.save_as".into(), json!({"path":saved}))]);
        app.process_actions(&ctx);
        assert!(app.error.is_none(), "{:?}", app.error);
        assert_eq!(
            app.host.path(id),
            Some(std::fs::canonicalize(&saved).unwrap().as_path())
        );
        assert!(!app.host.dirty(id));
        assert_eq!(app.host.documents.len(), 1);
        assert!(
            matches!(app.actions.front(), Some(Action::OpenExternal(path)) if path == &external)
        );
        let mut reader = Host::default();
        reader.open(&saved).unwrap();
        assert_eq!(
            reader
                .active_mut()
                .unwrap()
                .editor
                .command("document.text", json!({}))
                .unwrap()["text"]
                .as_str()
                .unwrap()
                .trim(),
            "尚未保存的对话框草稿"
        );

        app.host
            .active_mut()
            .unwrap()
            .editor
            .command("ui.closeDialog", json!({}))
            .unwrap();
        app.process_actions(&ctx);
        assert!(app.actions.is_empty());
        assert_eq!(app.host.documents.len(), 2);
        let active = app.host.workspace.active().unwrap();
        assert_ne!(active, id);
        assert_eq!(app.host.path(active), Some(external.as_path()));
    }

    #[test]
    fn host_dialogs_keep_external_opens_in_order_until_dismissed() {
        let directory = tempfile::tempdir().unwrap();
        let mut source = Host::default();
        let source_id = source.new_document(DocumentKind::Text).unwrap();
        let incoming = ["first.docx", "second.docx"].map(|name| directory.path().join(name));
        for path in &incoming {
            source.save_as(source_id, path).unwrap();
        }
        let incoming = incoming.map(|path| std::fs::canonicalize(path).unwrap());
        for dialog in ["preferences", "about", "error", "print"] {
            let mut host = Host::default();
            let original = host.new_document(DocumentKind::Text).unwrap();
            host.active_mut()
                .unwrap()
                .editor
                .command("text.insert", json!({"text":"保留当前文档"}))
                .unwrap();
            let mut app = test_app(host);
            let ctx = egui::Context::default();
            match dialog {
                "preferences" => app.preferences = true,
                "about" => app.about = true,
                "error" => app.error = Some("尚未关闭的错误".into()),
                "print" => {
                    app.print_dialog = Some(crate::printing::PrintDialog::new(
                        original,
                        "当前文档".into(),
                        &ctx,
                    ))
                }
                _ => unreachable!(),
            }
            app.actions
                .extend(incoming.iter().cloned().map(Action::OpenExternal));
            for _ in 0..3 {
                app.process_actions(&ctx);
                assert_eq!(app.host.workspace.active(), Some(original), "{dialog}");
                assert_eq!(app.host.documents.len(), 1, "{dialog}");
                let queued = app
                    .actions
                    .iter()
                    .map(|action| match action {
                        Action::OpenExternal(path) => path.clone(),
                        _ => panic!("unexpected queued action"),
                    })
                    .collect::<Vec<_>>();
                assert_eq!(queued, incoming, "{dialog}");
            }
            assert!(app.host.dirty(original));
            app.preferences = false;
            app.about = false;
            app.error = None;
            app.print_dialog = None;
            for path in &incoming {
                app.process_actions(&ctx);
                let active = app.host.workspace.active().unwrap();
                assert_eq!(app.host.path(active), Some(path.as_path()), "{dialog}");
                mount_test_frame(&mut app, &ctx, false);
            }
            assert!(app.actions.is_empty());
            assert_eq!(app.host.documents.len(), 3);
            assert!(app.host.dirty(original));
            assert_eq!(
                app.host
                    .documents
                    .get_mut(&original)
                    .unwrap()
                    .editor
                    .command("document.text", json!({}))
                    .unwrap()["text"]
                    .as_str()
                    .unwrap()
                    .trim(),
                "保留当前文档"
            );
        }
    }

    #[test]
    fn host_preferences_defer_external_opens_without_starving_confirmed_save() {
        let directory = tempfile::tempdir().unwrap();
        let incoming = directory.path().join("incoming.docx");
        let saved = directory.path().join("confirmed.docx");
        let mut source = Host::default();
        let source_id = source.new_document(DocumentKind::Text).unwrap();
        source.save_as(source_id, &incoming).unwrap();
        let mut host = Host::default();
        let original = host.new_document(DocumentKind::Text).unwrap();
        host.active_mut()
            .unwrap()
            .editor
            .command("text.insert", json!({"text":"已确认的保存"}))
            .unwrap();
        let mut app = test_app(host);
        app.preferences = true;
        app.actions.push_back(Action::OpenExternal(incoming));
        app.actions
            .push_back(Action::SaveTo(original, saved.clone()));
        app.process_actions(&egui::Context::default());
        assert_eq!(app.host.workspace.active(), Some(original));
        assert_eq!(app.host.documents.len(), 1);
        assert!(app.preferences);
        assert!(!app.host.dirty(original));
        assert_eq!(app.actions.len(), 1);
        assert!(matches!(app.actions.front(), Some(Action::OpenExternal(_))));
        let mut reader = Host::default();
        reader.open(&saved).unwrap();
        assert_eq!(
            reader
                .active_mut()
                .unwrap()
                .editor
                .command("document.text", json!({}))
                .unwrap()["text"]
                .as_str()
                .unwrap()
                .trim(),
            "已确认的保存"
        );
    }

    #[test]
    fn batch_open_errors_wait_for_acknowledgement_and_resume_without_losing_files() {
        let directory = tempfile::tempdir().unwrap();
        let mut source = Host::default();
        let source_id = source.new_document(DocumentKind::Text).unwrap();
        let valid = ["first.docx", "last.docx"].map(|name| directory.path().join(name));
        for path in &valid {
            source.save_as(source_id, path).unwrap();
        }
        let bad =
            ["broken-first.docx", "broken-second.docx"].map(|name| directory.path().join(name));
        for path in &bad {
            std::fs::write(path, b"This is not an Office ZIP archive").unwrap();
        }
        let mut host = Host::default();
        let original = host.new_document(DocumentKind::Text).unwrap();
        host.active_mut()
            .unwrap()
            .editor
            .command("text.insert", json!({"text":"原文件未保存的文字"}))
            .unwrap();
        let mut app = test_app(host);
        let ctx = egui::Context::default();
        app.actions.extend(
            [
                valid[0].clone(),
                bad[0].clone(),
                bad[1].clone(),
                valid[1].clone(),
            ]
            .map(Action::Open),
        );
        app.process_actions(&ctx);
        let first = app.host.workspace.active().unwrap();
        assert_eq!(
            app.host.path(first),
            Some(std::fs::canonicalize(&valid[0]).unwrap().as_path())
        );
        mount_test_frame(&mut app, &ctx, false);
        app.process_actions(&ctx);
        assert!(app.error.is_some());
        assert!(app.error.as_ref().unwrap().contains("broken-first.docx"));
        assert_eq!(app.host.workspace.active(), Some(first));
        assert_eq!(app.actions.len(), 2);
        let first_error = app.error.clone();
        for _ in 0..3 {
            app.process_actions(&ctx);
            assert_eq!(app.error, first_error);
            assert_eq!(app.actions.len(), 2);
            assert_eq!(app.host.workspace.active(), Some(first));
        }
        app.error = None;
        app.process_actions(&ctx);
        assert!(app.error.is_some());
        assert!(app.error.as_ref().unwrap().contains("broken-second.docx"));
        assert_eq!(app.actions.len(), 1);
        assert_eq!(app.host.workspace.active(), Some(first));
        app.error = None;
        app.process_actions(&ctx);
        let last = app.host.workspace.active().unwrap();
        assert_ne!(last, first);
        assert_eq!(
            app.host.path(last),
            Some(std::fs::canonicalize(&valid[1]).unwrap().as_path())
        );
        assert!(app.actions.is_empty());
        assert!(app.error.is_none());
        assert_eq!(app.host.documents.len(), 3);
        assert!(app.host.dirty(original));
        assert_eq!(
            app.host
                .documents
                .get_mut(&original)
                .unwrap()
                .editor
                .command("document.text", json!({}))
                .unwrap()["text"]
                .as_str()
                .unwrap()
                .trim(),
            "原文件未保存的文字"
        );
    }

    #[test]
    fn editor_modal_disables_shared_file_controls_but_keeps_quit_available() {
        let mut host = Host::default();
        let id = host.new_document(DocumentKind::Text).unwrap();
        open_editor_modal(&mut host.active_mut().unwrap().editor);
        let mut replay = ShellPointerReplay::new(host);
        replay.draw_chrome = true;
        replay.frame(Vec::new());
        replay.frame(Vec::new());
        let open = replay.widget_rect("打开 (⌘O)");
        replay.click(open.center());
        assert!(replay.app.file_dialogs.is_empty());
        assert_eq!(replay.app.host.workspace.active(), Some(id));
        let file = replay.widget_rect("文件");
        replay.click(file.center());
        replay.frame(Vec::new());
        let nodes = &replay
            .output
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes;
        for label in [
            "新建",
            "打开",
            "保存",
            "另存为",
            "打印",
            "关闭",
            "设置",
            "关于",
        ] {
            let node = nodes
                .iter()
                .find(|(_, node)| node.label() == Some(label))
                .unwrap_or_else(|| panic!("missing menu item {label}"));
            assert!(
                node.1.is_disabled(),
                "{label} must wait for the editor modal"
            );
        }
        let quit = nodes
            .iter()
            .find(|(_, node)| node.label() == Some("退出"))
            .unwrap();
        assert!(!quit.1.is_disabled());
        let quit = replay.widget_rect("退出");
        replay.click(quit.center());
        assert!(replay.app.close_plan.as_ref().unwrap().quit);
        assert!(replay.app.host.documents.contains_key(&id));
    }

    #[test]
    fn editor_modal_allows_quit_and_cancel_preserves_the_document_and_dialog() {
        let mut host = Host::default();
        let id = host.new_document(DocumentKind::Text).unwrap();
        host.active_mut()
            .unwrap()
            .editor
            .command("text.insert", json!({"text":"保留文字和专业弹窗"}))
            .unwrap();
        open_editor_modal(&mut host.active_mut().unwrap().editor);
        let mut replay = ShellPointerReplay::new(host);
        replay.ctx.begin_pass(egui::RawInput {
            events: vec![shortcut_event(Key::Q, egui::Modifiers::COMMAND)],
            ..Default::default()
        });
        replay.app.shortcuts(&replay.ctx);
        replay.ctx.end_pass().textures_delta.clear();
        assert!(replay.app.close_plan.as_ref().unwrap().quit);
        replay.app.advance_close(&replay.ctx);
        replay.frame(Vec::new());
        replay.frame(Vec::new());
        assert_eq!(replay.app.close_prompt, Some(id));
        let cancel = replay.widget_rect("取消");
        replay.click(cancel.center());
        assert!(replay.app.close_plan.is_none());
        assert!(replay.app.close_prompt.is_none());
        assert!(!replay.app.allow_exit);
        assert!(replay.app.host.dirty(id));
        assert!(replay.app.editor_blocks_host_navigation());
        assert_eq!(
            replay
                .app
                .host
                .active_mut()
                .unwrap()
                .editor
                .command("document.text", json!({}))
                .unwrap()["text"]
                .as_str()
                .unwrap()
                .trim(),
            "保留文字和专业弹窗"
        );
    }

    #[test]
    fn shared_language_settings_migrate_and_round_trip() {
        let legacy: Settings =
            serde_json::from_value(json!({"theme":"dark", "scale":1.25})).unwrap();
        assert_eq!(legacy.language, UiLanguage::ZhHans);
        let invalid: Settings =
            serde_json::from_value(json!({"theme":"dark", "language":"fr"})).unwrap();
        assert_eq!(invalid.language, UiLanguage::ZhHans);
        assert_eq!(invalid.theme, "dark");
        let saved = Settings {
            language: UiLanguage::En,
            ..Settings::default()
        };
        let restored: Settings =
            serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
        assert_eq!(restored.language, UiLanguage::En);
    }

    #[test]
    fn shared_language_switch_reaches_all_editors_without_changing_word_proofing() {
        use crate::editor::Editor;
        language::set_current(UiLanguage::ZhHans);
        let mut host = Host::default();
        let blank = host.new_document(DocumentKind::Text).unwrap();
        let edited = host.new_document(DocumentKind::Text).unwrap();
        host.documents
            .get_mut(&edited)
            .unwrap()
            .editor
            .command("text.insert", json!({"text":"Bonjour"}))
            .unwrap();
        host.documents
            .get_mut(&edited)
            .unwrap()
            .editor
            .command("review.language", json!({"lang":"fr-FR", "default":true}))
            .unwrap();
        let sheet = host.new_document(DocumentKind::Spreadsheet).unwrap();
        let slides = host.new_document(DocumentKind::Presentation).unwrap();
        let pdf = host.new_document(DocumentKind::Pdf).unwrap();
        let word_before = [blank, edited].map(|id| {
            let Editor::Word(word) = &host.documents[&id].editor else {
                panic!("Word")
            };
            (
                word.session.doc.clone(),
                word.session.rev(),
                word.hosted_dirty(),
                word.session.can_undo(),
                word.session.prefs.default_language.clone(),
            )
        });
        let mut app = test_app(host);
        app.mounted = None;
        let ctx = egui::Context::default();
        for language in [UiLanguage::En, UiLanguage::ZhHans, UiLanguage::En] {
            // Use the editor's actual command boundary; the source document cannot mutate its
            // own UI preference before the host applies it to the workspace.
            app.host.workspace.activate(blank);
            app.host
                .active_mut()
                .unwrap()
                .editor
                .command("ui.language", json!({"value":language.code()}))
                .unwrap();
            app.handle_editor_requests(&ctx);
            assert_eq!(app.settings.language, language);
            for id in [blank, edited, sheet, slides, pdf] {
                app.host.workspace.activate(id);
                for _ in 0..3 {
                    let mut input = egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1200.0, 800.0),
                        )),
                        ..Default::default()
                    };
                    eframe::App::raw_input_hook(&mut app, &ctx, &mut input);
                    ctx.run_ui(input, |ui| {
                        app.host.active_mut().unwrap().editor.logic(ui.ctx());
                        app.apply_weave_style(ui.ctx());
                        app.shell(ui);
                        app.host.active_mut().unwrap().editor.ui(ui);
                    })
                    .drop_without_applying_deltas();
                }
                match &app.host.active().unwrap().editor {
                    Editor::Word(word) => assert_eq!(word.ui.language, language.code()),
                    Editor::Sheet(sheet) => assert_eq!(
                        sheet.ui.language.code(),
                        if language == UiLanguage::En {
                            "en"
                        } else {
                            "zh"
                        }
                    ),
                    Editor::Pdf(pdf) => {
                        assert_eq!(pdf.language, language.code());
                        let expected = pdfcraft_ui_egui::theme::installed_font_definitions(
                            pdfcraft_ui_egui::theme::CjkPreference::for_language(language.code()),
                        );
                        let medium = egui::FontFamily::Name("medium".into());
                        ctx.fonts(|fonts| {
                            assert_eq!(
                                fonts.definitions().families.get(&medium),
                                expected.families.get(&medium)
                            )
                        });
                    }
                    Editor::TextFile(text) => {
                        assert_eq!(text.interface_zh(), language == UiLanguage::ZhHans)
                    }
                    Editor::Rich(rich) => {
                        assert_eq!(rich.interface_zh(), language == UiLanguage::ZhHans)
                    }
                    Editor::Slides(_) => {
                        // Deck stores its hosted projection in the active egui context.
                        assert_eq!(
                            ctx.data(|data| data
                                .get_temp::<bool>(egui::Id::unique("deckcraft_hosted_zh_hans"))),
                            Some(language == UiLanguage::ZhHans)
                        );
                    }
                }
            }
            for (index, id) in [blank, edited].into_iter().enumerate() {
                let Editor::Word(word) = &app.host.documents[&id].editor else {
                    panic!("Word")
                };
                assert_eq!(
                    (
                        word.session.doc.clone(),
                        word.session.rev(),
                        word.hosted_dirty(),
                        word.session.can_undo(),
                        word.session.prefs.default_language.clone()
                    ),
                    word_before[index]
                );
            }
        }
        for kind in kinds() {
            let id = app.host.new_document(kind).unwrap();
            assert!(
                app.host
                    .workspace
                    .tab(id)
                    .unwrap()
                    .title
                    .starts_with("Untitled ")
            );
            match &app.host.documents[&id].editor {
                Editor::Word(word) => assert_eq!(word.ui.language, "en"),
                Editor::Sheet(sheet) => {
                    assert_eq!(sheet.ui.language, gridcraft_ui_egui::i18n::Language::En)
                }
                Editor::Pdf(pdf) => assert_eq!(pdf.language, "en"),
                Editor::TextFile(text) => assert!(!text.interface_zh()),
                Editor::Rich(rich) => assert!(!rich.interface_zh()),
                Editor::Slides(_) => {}
            }
        }
        let Editor::Pdf(source) = &mut app.host.documents.get_mut(&pdf).unwrap().editor else {
            panic!("PDF")
        };
        let bytes = source.session.create_blank(100.0, 100.0, 1).unwrap();
        let generated = app
            .host
            .open_generated_pdf("Generated.pdf", (*bytes).clone(), None)
            .unwrap();
        let Editor::Pdf(generated) = &app.host.documents[&generated].editor else {
            panic!("PDF")
        };
        assert_eq!(generated.language, "en");
        // Grid's in-ribbon selector also goes through the same global authority.
        app.host.workspace.activate(sheet);
        app.host
            .active_mut()
            .unwrap()
            .editor
            .command("app.language.set", json!({"language":"zh"}))
            .unwrap();
        app.handle_editor_requests(&ctx);
        assert_eq!(app.settings.language, UiLanguage::ZhHans);
        let Editor::Word(word) = &app.host.documents[&blank].editor else {
            panic!("Word")
        };
        assert_eq!(word.ui.language, "zh-hans");
        language::set_current(UiLanguage::ZhHans);
    }

    #[test]
    fn shared_language_preferences_preserve_the_word_entry_and_translate_shell() {
        language::set_current(UiLanguage::ZhHans);
        let mut host = Host::default();
        host.new_document(DocumentKind::Text).unwrap();
        let mut app = test_app(host);
        let ctx = egui::Context::default();
        app.host
            .active_mut()
            .unwrap()
            .editor
            .command("file.options", json!({"pane":"language"}))
            .unwrap();
        app.handle_editor_requests(&ctx);
        assert!(app.preferences);
        assert_eq!(app.preferences_pane, SettingsPane::Language);
        app.apply_interface_language(&ctx, UiLanguage::En);
        let mut labels = Vec::new();
        fn collect(shape: &egui::Shape, labels: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(text) => labels.push(text.galley.text().to_string()),
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect(shape, labels);
                    }
                }
                _ => {}
            }
        }
        for _ in 0..3 {
            let output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1200.0, 800.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    app.dialogs(ui.ctx());
                },
            );
            labels.clear();
            for clipped in &output.shapes {
                collect(&clipped.shape, &mut labels);
            }
            output.drop_without_applying_deltas();
        }
        for label in [
            "Settings",
            "General",
            "Language",
            "Interface language",
            "English",
            "Proofing language",
        ] {
            assert!(
                labels.iter().any(|text| text == label),
                "missing {label}: {labels:?}"
            );
        }
        for label in [
            "文件",
            "新建文件",
            "保存更改？",
            "打印",
            "操作未完成",
            "再次选择…",
            "恢复未保存的文档？",
        ] {
            assert_ne!(tr(label), label);
        }
        language::set_current(UiLanguage::ZhHans);
    }

    #[test]
    fn host_dialogs_hide_editor_input_and_restore_it_for_the_dialog() {
        let ctx = egui::Context::default();
        ctx.begin_pass(egui::RawInput {
            focused: true,
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 800.0),
            )),
            events: vec![
                egui::Event::Text("批注草稿".into()),
                egui::Event::Zoom(1.5),
                egui::Event::PointerMoved(egui::pos2(10.0, 20.0)),
                egui::Event::PointerButton {
                    pos: egui::pos2(10.0, 20.0),
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            ..Default::default()
        });
        let before = ctx.input(|i| (i.events.len(), i.viewport_rect()));
        frozen_editor_input(&ctx, || {
            ctx.input(|i| {
                assert!(i.events.is_empty());
                assert!(i.raw.events.is_empty());
                assert!(!i.focused);
                assert!(!i.pointer.any_down());
                assert_eq!(i.zoom_delta(), 1.0);
                assert_eq!(i.viewport_rect(), before.1);
            });
        });
        ctx.input(|i| {
            assert_eq!(i.events.len(), before.0);
            assert!(i.focused);
            assert!(i.pointer.primary_down());
            assert_eq!(i.zoom_delta(), 1.5);
        });
        let mut output = ctx.end_pass();
        output.textures_delta.clear();
    }

    #[test]
    fn unsaved_documents_survive_a_crash_and_reopen_as_unsaved() {
        let root = tempfile::tempdir().unwrap();
        let mut host = Host::default();
        let word = host.new_document(DocumentKind::Text).unwrap();
        host.documents
            .get_mut(&word)
            .unwrap()
            .editor
            .command("text.insert", json!({"text":"崩溃前的文字"}))
            .unwrap();
        let sheet = host.new_document(DocumentKind::Spreadsheet).unwrap();
        host.documents
            .get_mut(&sheet)
            .unwrap()
            .editor
            .command("cell.set", json!({"cell":"B2", "input":"42"}))
            .unwrap();
        let titles: Vec<String> = [word, sheet]
            .iter()
            .map(|id| host.workspace.tab(*id).unwrap().title.clone())
            .collect();
        let mut app = test_app(host);
        app.enable_recovery_in(root.path());
        assert!(!app.restore_prompt);
        // The person paused: both unsaved documents are copied.
        app.last_input = std::time::Instant::now() - crate::recovery::IDLE;
        app.autosave(&egui::Context::default());
        let mut recovery = app.recovery.take().unwrap();
        recovery.flush();
        assert!(recovery.take_errors().is_empty());
        recovery.crash();

        let mut next = test_app(Host::default());
        next.enable_recovery_in(root.path());
        assert!(next.restore_prompt);
        next.restore_recovered();
        assert!(next.error.is_none(), "{:?}", next.error);
        assert!(!next.restore_prompt);
        let mut restored: Vec<(String, bool)> = next
            .host
            .workspace
            .tabs()
            .iter()
            .map(|tab| (tab.title.clone(), next.host.dirty(tab.id)))
            .collect();
        restored.sort();
        let mut expected: Vec<(String, bool)> = titles.into_iter().map(|t| (t, true)).collect();
        expected.sort();
        assert_eq!(restored, expected);
        let find = |next: &WeaveApp, kind| {
            next.host
                .workspace
                .tabs()
                .iter()
                .find(|tab| tab.kind == kind)
                .unwrap()
                .id
        };
        let word = find(&next, DocumentKind::Text);
        let sheet = find(&next, DocumentKind::Spreadsheet);
        assert!(next.host.path(word).is_none());
        let text = next
            .host
            .documents
            .get_mut(&word)
            .unwrap()
            .editor
            .command("document.text", json!({}))
            .unwrap();
        assert_eq!(text["text"].as_str().unwrap().trim(), "崩溃前的文字");
        let cell = next
            .host
            .documents
            .get_mut(&sheet)
            .unwrap()
            .editor
            .command("cell.get", json!({"cell":"B2"}))
            .unwrap();
        assert!(cell.to_string().contains("42"), "{cell}");
        // The crashed run's folder is gone; this run holds its own copies now.
        let runs = std::fs::read_dir(root.path()).unwrap().count();
        assert_eq!(runs, 1);
        // Saving ends the recovered state.
        let saved = root.path().join("saved.docx");
        next.host.save_as(word, &saved).unwrap();
        assert!(!next.host.dirty(word));
        next.autosave(&egui::Context::default());
        let dir = next.recovery.as_ref().unwrap().dir().to_path_buf();
        next.recovery.as_mut().unwrap().flush();
        let left: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(!left.iter().any(|name| name.ends_with(".docx")), "{left:?}");
        drop(next);
        assert!(!dir.exists());
    }

    #[test]
    fn every_editor_s_recovery_copy_reopens() {
        let directory = tempfile::tempdir().unwrap();
        for kind in kinds() {
            let mut host = Host::default();
            let id = host.new_document(kind).unwrap();
            let bytes = host.documents[&id].editor.snapshot().unwrap()().unwrap();
            let copy = directory.path().join(format!("1.{}", extension(kind)));
            std::fs::write(&copy, bytes).unwrap();
            let origin = directory.path().join(format!("原文件.{}", extension(kind)));
            let restored = host.restore(kind, "原文件", &copy, Some(&origin)).unwrap();
            assert!(host.dirty(restored), "{kind:?}");
            assert_eq!(host.origin(restored), Some(origin.as_path()));
            assert!(host.path(restored).is_none());
            std::fs::remove_file(&copy).unwrap();
        }
    }

    pub(super) fn test_app(host: Host) -> WeaveApp {
        let mounted = host.workspace.active();
        WeaveApp {
            _native_menu: crate::native_file_panel::AppMenu::default(),
            host,
            mounted,
            actions: VecDeque::new(),
            close_plan: None,
            close_prompt: None,
            recovery: None,
            restore_prompt: false,
            last_input: std::time::Instant::now(),
            close_focus: None,
            error: None,
            new_picker: false,
            new_anchor: None,
            preferences: false,
            preferences_pane: SettingsPane::General,
            about: false,
            print_dialog: None,
            file_dialogs: VecDeque::new(),
            allow_exit: false,
            settings: Settings::default(),
            ui_font: None,
            fonts: crate::font_set::FontSet::new(egui::FontDefinitions::default()),
            tab_scroll: 0.0,
            reveal_tab: None,
            control_click: false,
            #[cfg(target_os = "macos")]
            os_events: None,
            startup_frames: None,
        }
    }

    fn pending_sheet_move() -> (WeaveApp, TabId, TabId) {
        let mut host = Host::with_clipboard(crate::clipboard::Clipboard::memory());
        let source = host.new_document(DocumentKind::Spreadsheet).unwrap();
        let target = host.new_document(DocumentKind::Spreadsheet).unwrap();
        host.documents
            .get_mut(&source)
            .unwrap()
            .editor
            .command("cell.set", json!({"cell":"A1", "input":"7"}))
            .unwrap();
        host.documents
            .get_mut(&source)
            .unwrap()
            .editor
            .command("edit.cut", json!({"range":"A1"}))
            .unwrap();
        host.documents
            .get_mut(&target)
            .unwrap()
            .editor
            .command("edit.paste", json!({"at":"B1"}))
            .unwrap();
        (test_app(host), source, target)
    }

    #[test]
    fn sheet_transfer_replaces_old_close_decisions_before_close_or_quit() {
        let (mut app, source, target) = pending_sheet_move();
        app.close_plan = Some(ClosePlan {
            ids: vec![source, target],
            index: 2,
            quit: true,
        });
        app.advance_close(&egui::Context::default());
        assert!(app.close_plan.is_none());
        assert!(!app.allow_exit);
        assert_eq!(app.host.documents.len(), 2);
        assert!(app.host.dirty(source));
        assert!(app.host.dirty(target));
        app.begin_close(None, true);
        assert_eq!(app.close_plan.as_ref().unwrap().index, 0);
    }

    #[test]
    fn sheet_transfer_failure_cancels_close_and_queued_commands() {
        let (mut app, source, target) = pending_sheet_move();
        app.host
            .documents
            .get_mut(&source)
            .unwrap()
            .editor
            .command("cell.set", json!({"cell":"A1", "input":"changed"}))
            .unwrap();
        app.close_plan = Some(ClosePlan {
            ids: vec![source, target],
            index: 2,
            quit: true,
        });
        app.actions.push_back(Action::Command("edit.undo"));
        app.advance_close(&egui::Context::default());
        assert!(app.close_plan.is_none());
        assert!(app.actions.is_empty());
        assert!(app.error.is_some());
        assert!(!app.allow_exit);
        assert_eq!(app.host.documents.len(), 2);
    }

    #[test]
    fn sheet_transfer_drains_before_the_next_editor_command() {
        let (mut app, source, target) = pending_sheet_move();
        app.actions.push_back(Action::Command("edit.undo"));
        app.process_actions(&egui::Context::default());
        let source_cell = app
            .host
            .documents
            .get_mut(&source)
            .unwrap()
            .editor
            .command("cell.get", json!({"cell":"A1"}))
            .unwrap();
        let target_cell = app
            .host
            .documents
            .get_mut(&target)
            .unwrap()
            .editor
            .command("cell.get", json!({"cell":"B1"}))
            .unwrap();
        assert_eq!(source_cell["text"], "");
        assert_eq!(target_cell["text"], "");
        assert!(app.error.is_none());
    }

    #[test]
    fn inactive_pdf_worker_is_polled_and_cannot_be_dropped_by_close_or_quit() {
        use std::sync::{Arc, Mutex, mpsc};
        use std::time::{Duration, Instant};

        let mut host = Host::default();
        let pdf = host.new_document(DocumentKind::Pdf).unwrap();
        let word = host.new_document(DocumentKind::Text).unwrap();
        let directory = tempfile::tempdir().unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let release_rx = Mutex::new(release_rx);
        let crate::editor::Editor::Pdf(editor) = &mut host.documents.get_mut(&pdf).unwrap().editor
        else {
            unreachable!()
        };
        editor.export_dir_override = Some(directory.path().to_string_lossy().into_owned());
        editor.set_host_export_writer(Arc::new(move |_, bytes| {
            assert!(bytes.starts_with(b"%PDF-"));
            started_tx.send(()).map_err(|error| error.to_string())?;
            release_rx
                .lock()
                .map_err(|error| error.to_string())?
                .recv_timeout(Duration::from_secs(5))
                .map_err(|error| error.to_string())?;
            Ok(())
        }));
        let bytes = editor.session.create_blank(200.0, 300.0, 1).unwrap();
        editor.run_action_on(
            pdfcraft_engine::actions::Action {
                name: "Background close fixture".into(),
                description: String::new(),
                steps: Vec::new(),
                builtin: false,
            },
            vec![("Worker.pdf".into(), (*bytes).clone())],
        );
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(
            host.close(pdf, true),
            weave_workspace::CloseResult::NeedsSave
        );
        assert_eq!(host.workspace.active(), Some(word));
        let mut app = test_app(host);
        app.begin_close(None, true);
        assert!(app.close_plan.is_none() && app.close_prompt.is_none());
        assert!(app.error.as_deref().unwrap().contains("后台任务"));
        assert!(!app.allow_exit);
        assert_eq!(app.host.documents.len(), 2);
        release_tx.send(()).unwrap();
        let ctx = egui::Context::default();
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.host.pending_background(&[pdf]).is_some() && Instant::now() < deadline {
            app.host.poll_background(&ctx);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(app.host.pending_background(&[pdf]).is_none());
        assert_eq!(
            app.host.workspace.active(),
            Some(word),
            "polling must not activate the PDF"
        );
        let crate::editor::Editor::Pdf(editor) = &app.host.documents[&pdf].editor else {
            unreachable!()
        };
        assert!(
            editor
                .toast
                .as_ref()
                .unwrap()
                .0
                .contains("Background close fixture")
        );
        app.error = None;
        app.begin_close(Some(pdf), false);
        assert_eq!(app.close_plan.as_ref().unwrap().ids, [pdf]);
        assert_eq!(
            app.host.documents.len(),
            2,
            "no tab was discarded during the rejected quit"
        );
    }

    fn grid_stop_validation_draft(host: &mut Host, id: TabId) {
        let crate::editor::Editor::Sheet(sheet) = &mut host.documents.get_mut(&id).unwrap().editor
        else {
            panic!("spreadsheet fixture");
        };
        sheet
            .session
            .execute(
                "data.validation",
                json!({
                    "range": "A1", "type": "list", "formula1": "\"Red,Green\"", "errorStyle": "stop"
                }),
            )
            .unwrap();
        sheet.begin_edit(Some("Blue".into()), false);
        let draft = sheet.editor.as_mut().unwrap();
        draft.anchor = 1;
        draft.caret = 3;
        draft.caret_to_end = false;
        draft.sync_caret = false;
        draft.keep_selection = true;
        host.sync_dirty();
    }

    fn assert_grid_stop_draft(host: &Host, id: TabId) {
        let crate::editor::Editor::Sheet(sheet) = &host.documents[&id].editor else {
            panic!("spreadsheet fixture");
        };
        let draft = sheet
            .editor
            .as_ref()
            .expect("invalid cell draft remains editable");
        assert_eq!((&*draft.text, draft.anchor, draft.caret), ("Blue", 1, 3));
        assert!(host.dirty(id));
    }

    fn mount_test_frame(app: &mut WeaveApp, ctx: &egui::Context, draw_active: bool) {
        ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1200.0, 800.0),
                )),
                focused: true,
                ..Default::default()
            },
            |ui| {
                app.mount_active(ui.ctx())
                    .expect("host tab switch preserves drafts");
                if draw_active {
                    app.host.active_mut().unwrap().editor.ui(ui);
                }
            },
        )
        .drop_without_applying_deltas();
        assert_eq!(app.mounted, app.host.workspace.active());
    }

    // Unlike the draft-only helper, run the editors' font/layout logic as well as their UI.
    // A restored field is insufficient evidence: the next real paint must retain the view.
    fn settle_hosted_view(app: &mut WeaveApp, ctx: &egui::Context, frame: &mut u64) {
        // Font installation can delay the first page/thumbnail layout by several passes.
        // Allow its subsequent navigation animation to finish before taking the baseline.
        for _ in 0..32 {
            *frame += 1;
            let mut input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1200.0, 800.0),
                )),
                focused: true,
                time: Some(*frame as f64 / 30.0),
                ..Default::default()
            };
            // Match the real host's admission, styling, shell geometry and child UI scope.
            eframe::App::raw_input_hook(app, ctx, &mut input);
            ctx.run_ui(input, |ui| {
                app.host.active_mut().unwrap().editor.logic(ui.ctx());
                app.apply_weave_style(ui.ctx());
                app.mount_active(ui.ctx()).unwrap();
                app.install_shell_font(ui.ctx());
                app.apply_weave_style(ui.ctx());
                app.shell(ui);
                ui.add_enabled_ui(true, |ui| {
                    app.host.active_mut().unwrap().editor.ui(ui);
                });
            })
            .drop_without_applying_deltas();
        }
        assert_eq!(app.mounted, app.host.workspace.active());
    }

    /// Selection (including the active page/sheet), zoom, and the actual drawn scroll position.
    /// Word/PDF deliberately use page geometry, since their scroll fields are private.
    fn hosted_view_snapshot(
        editor: &crate::editor::Editor,
    ) -> (serde_json::Value, f32, egui::Vec2) {
        use crate::editor::Editor;
        match editor {
            Editor::Word(word) => {
                let page = word.canvas.page_rects.first().expect("Word page was drawn");
                let viewport = word.canvas.canvas_rect.expect("Word canvas was drawn");
                (
                    json!({"selection": word.session.sel, "page": word.canvas.viewed_page}),
                    word.session.view.zoom,
                    page.min - viewport.min,
                )
            }
            Editor::Sheet(sheet) => {
                let doc = sheet.session.active().unwrap();
                let geo = sheet.grid.geo.as_ref().expect("spreadsheet grid was drawn");
                assert_eq!(geo.scroll, sheet.view().scroll);
                (
                    json!({"selection": doc.selection, "sheet": doc.wb.active_sheet()}),
                    doc.wb.active().unwrap().zoom as f32 / 100.0,
                    geo.scroll,
                )
            }
            Editor::Slides(slides) => (
                json!(slides.session.active().unwrap().selection),
                slides.ui.zoom.expect("explicit presentation zoom"),
                egui::vec2(0.0, slides.ui.thumbs_scroll),
            ),
            Editor::Pdf(pdf) => {
                let view = &pdf.views[pdf.active.unwrap()];
                let page = view
                    .page_screen_rect(view.current)
                    .expect("active PDF page was drawn");
                (
                    json!({"selection": view.selected, "page": view.current}),
                    view.zoom,
                    page.min - view.viewport_rect().min,
                )
            }
            Editor::TextFile(_) | Editor::Rich(_) => unreachable!("not part of the view fixture"),
        }
    }

    #[test]
    fn mixed_tab_round_trip_retains_real_selection_scroll_zoom_and_active_page() {
        use crate::editor::Editor;
        let mut host = Host::default();
        let word = host.new_document(DocumentKind::Text).unwrap();
        let sheet = host.new_document(DocumentKind::Spreadsheet).unwrap();
        let slides = host.new_document(DocumentKind::Presentation).unwrap();
        let pdf = host.new_document(DocumentKind::Pdf).unwrap();

        let Editor::Word(editor) = &mut host.documents.get_mut(&word).unwrap().editor else {
            panic!()
        };
        editor
            .hosted_command(
                "text.insert",
                json!({"text": "View retention paragraph\r".repeat(64)}),
            )
            .unwrap();
        editor
            .hosted_command("view.zoom", json!({"value": 125}))
            .unwrap();
        editor.session.sel.anchor = wordcraft_doc::Pos::body(60, 1);
        editor.session.sel.focus = wordcraft_doc::Pos::body(60, 8);
        editor.canvas.scroll_to_caret = true;

        let Editor::Sheet(editor) = &mut host.documents.get_mut(&sheet).unwrap().editor else {
            panic!()
        };
        editor.use_prepared_fonts();
        editor
            .hosted_command("home.insertSheet", json!({"name": "Retained view"}))
            .unwrap();
        editor
            .hosted_command("sheet.activate", json!({"sheet": "Retained view"}))
            .unwrap();
        editor
            .hosted_command(
                "selection.set",
                json!({"range": "H24:J27", "active": "I25"}),
            )
            .unwrap();
        editor
            .hosted_command("view.zoom", json!({"percent": 150}))
            .unwrap();
        editor.view_mut().unwrap().scroll = egui::vec2(240.0, 360.0);
        host.documents.get_mut(&sheet).unwrap().fonts =
            Some(gridcraft_ui_egui::theme::pinned_font_definitions());

        let Editor::Slides(editor) = &mut host.documents.get_mut(&slides).unwrap().editor else {
            panic!()
        };
        for _ in 0..12 {
            editor.hosted_command("slide.new", json!({})).unwrap();
        }
        editor
            .hosted_command("slide.go", json!({"index": 0}))
            .unwrap();
        editor.ui.zoom = Some(1.1);

        let Editor::Pdf(editor) = &mut host.documents.get_mut(&pdf).unwrap().editor else {
            panic!()
        };
        for at in 1..4 {
            assert!(editor.apply_edit(pdfcraft_engine::Edit::InsertBlankPage {
                at,
                width: 612.0,
                height: 792.0,
            }));
        }
        let view = &mut editor.views[editor.active.unwrap()];
        view.set_zoom(1.3);
        view.select_pages(&[1, 2]);
        view.go_to_page(2);
        view.goto = Some((2, 0.3));
        assert_eq!(view.selected.iter().copied().collect::<Vec<_>>(), [1, 2]);
        assert_eq!(view.current, 2);

        let mut app = test_app(host);
        app.mounted = None;
        app.ui_font = Some(crate::ui_fonts::UiFonts::load());
        let ctx = egui::Context::default();
        let mut frame = 0;
        let ids = [word, sheet, slides, pdf];
        let mut expected = Vec::new();
        for id in ids {
            app.host.workspace.activate(id);
            settle_hosted_view(&mut app, &ctx, &mut frame);
            if id == slides {
                // Navigate after the thumbnail pane has been drawn: its actual ScrollArea
                // follows the active slide, then records the resulting offset every frame.
                let Editor::Slides(editor) = &mut app.host.documents.get_mut(&id).unwrap().editor
                else {
                    panic!()
                };
                editor
                    .hosted_command("slide.go", json!({"index": 11}))
                    .unwrap();
                let doc = editor.session.active_mut().unwrap();
                doc.selection.slides = vec![doc.doc.slides[10].id, doc.doc.slides[11].id];
                settle_hosted_view(&mut app, &ctx, &mut frame);
            }
            let snapshot = hosted_view_snapshot(&app.host.documents[&id].editor);
            if id == word {
                // Elapsed frames alone must not keep moving the baseline view.
                settle_hosted_view(&mut app, &ctx, &mut frame);
                let idle = hosted_view_snapshot(&app.host.documents[&id].editor);
                assert!(
                    (idle.2 - snapshot.2).length() < 1.0,
                    "initial navigation had not settled without any tab switch: {:?} -> {:?}",
                    snapshot.2,
                    idle.2
                );
            }
            assert!(snapshot.1 > 1.0, "fixture has a non-default zoom: {id:?}");
            assert!(
                snapshot.2.y.abs() > 100.0,
                "fixture must actually scroll: {id:?}, {snapshot:?}"
            );
            match &app.host.documents[&id].editor {
                Editor::Word(editor) => {
                    assert_eq!(editor.session.sel.anchor, wordcraft_doc::Pos::body(60, 1));
                    assert_eq!(editor.session.sel.focus, wordcraft_doc::Pos::body(60, 8));
                    assert!(editor.canvas.viewed_page.is_some_and(|page| page > 0));
                }
                Editor::Sheet(editor) => {
                    let doc = editor.session.active().unwrap();
                    assert_eq!(doc.selection.a1(), "H24:J27");
                    assert_eq!(doc.selection.active.a1(), "I25");
                    assert_eq!(doc.wb.active_sheet(), 1);
                }
                Editor::Slides(editor) => {
                    let selection = &editor.session.active().unwrap().selection;
                    assert_eq!(selection.slide, 11);
                    assert_eq!(selection.slides.len(), 2);
                }
                Editor::Pdf(editor) => {
                    let view = &editor.views[editor.active.unwrap()];
                    assert_eq!(view.current, 2);
                    assert_eq!(view.selected.iter().copied().collect::<Vec<_>>(), [1, 2]);
                }
                Editor::TextFile(_) | Editor::Rich(_) => {
                    unreachable!("not part of the view fixture")
                }
            }
            expected.push((id, snapshot));
        }

        // Two complete cycles remount every type after another editor has used the same
        // egui Context. No fixture state or Memory value is re-applied on return.
        for _ in 0..2 {
            for (id, before) in &expected {
                app.host.workspace.activate(*id);
                settle_hosted_view(&mut app, &ctx, &mut frame);
                let after = hosted_view_snapshot(&app.host.documents[id].editor);
                assert_eq!(after.0, before.0, "selection and active page/sheet: {id:?}");
                assert_eq!(after.1, before.1, "zoom: {id:?}");
                assert!(
                    (after.2 - before.2).length() < 1.0,
                    "drawn scroll changed for {id:?}: {:?} -> {:?}",
                    before.2,
                    after.2
                );
            }
        }
    }

    #[test]
    fn grid_invalid_draft_first_in_quit_queue_can_be_discarded_before_mounting_next_tab() {
        let mut host = Host::default();
        let grid = host.new_document(DocumentKind::Spreadsheet).unwrap();
        let word = host.new_document(DocumentKind::Text).unwrap();
        grid_stop_validation_draft(&mut host, grid);
        host.workspace.activate(grid);
        let mut app = test_app(host);
        app.mounted = None;
        let ctx = egui::Context::default();
        mount_test_frame(&mut app, &ctx, false);
        app.begin_close(None, true);
        app.advance_close(&ctx);
        assert_eq!(app.close_prompt, Some(grid));
        assert_eq!(app.close_plan.as_ref().unwrap().ids, [grid, word]);

        // The shared prompt's Don't Save choice approves the draft without flushing it.
        app.close_prompt = None;
        app.close_plan.as_mut().unwrap().index += 1;
        app.advance_close(&ctx);
        assert_eq!(app.host.workspace.active(), Some(word));
        assert_eq!(app.mounted, Some(grid));
        mount_test_frame(&mut app, &ctx, false);
        assert!(
            app.close_plan.is_some(),
            "deactivation must not cancel the close transaction"
        );
        assert_grid_stop_draft(&app.host, grid);
        app.advance_close(&ctx);
        assert_eq!(app.close_plan.as_ref().unwrap().index, 2);
        app.advance_close(&ctx);
        assert!(app.allow_exit);
        assert!(app.host.workspace.tabs().is_empty());
        assert!(app.host.documents.is_empty());
        assert!(app.close_plan.is_none());
        assert!(app.error.is_none());
    }

    #[test]
    fn grid_invalid_draft_later_in_quit_queue_survives_scan_and_cancel() {
        let mut host = Host::default();
        let word = host.new_document(DocumentKind::Text).unwrap();
        let grid = host.new_document(DocumentKind::Spreadsheet).unwrap();
        grid_stop_validation_draft(&mut host, grid);
        let mut app = test_app(host);
        app.mounted = None;
        let ctx = egui::Context::default();
        mount_test_frame(&mut app, &ctx, false);
        app.begin_close(None, true);
        app.advance_close(&ctx);
        assert_eq!(app.host.workspace.active(), Some(word));
        assert_eq!(app.mounted, Some(grid));
        mount_test_frame(&mut app, &ctx, false);
        app.advance_close(&ctx);
        assert_eq!(app.close_plan.as_ref().unwrap().index, 1);
        app.advance_close(&ctx);
        assert_eq!(app.host.workspace.active(), Some(grid));
        mount_test_frame(&mut app, &ctx, false);
        app.advance_close(&ctx);
        assert_eq!(app.close_prompt, Some(grid));
        assert_grid_stop_draft(&app.host, grid);

        // Mirror Cancel in the shared close prompt: no previously scanned tab is removed.
        app.close_prompt = None;
        app.close_plan = None;
        assert_eq!(
            app.host
                .workspace
                .tabs()
                .iter()
                .map(|tab| tab.id)
                .collect::<Vec<_>>(),
            [word, grid]
        );
        assert_eq!(app.host.documents.len(), 2);
        assert_grid_stop_draft(&app.host, grid);
        assert!(!app.allow_exit);
        assert!(app.error.is_none());
    }

    #[test]
    fn grid_invalid_draft_survives_remount_and_render_but_still_cannot_be_saved() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("invalid.xlsx");
        let mut host = Host::default();
        let grid = host.new_document(DocumentKind::Spreadsheet).unwrap();
        let word = host.new_document(DocumentKind::Text).unwrap();
        grid_stop_validation_draft(&mut host, grid);
        host.workspace.activate(grid);
        let mut app = test_app(host);
        app.mounted = None;
        let ctx = egui::Context::default();
        mount_test_frame(&mut app, &ctx, true);
        assert_grid_stop_draft(&app.host, grid);

        app.host.workspace.activate(word);
        mount_test_frame(&mut app, &ctx, false);
        assert_grid_stop_draft(&app.host, grid);
        let crate::editor::Editor::Sheet(sheet) = &app.host.documents[&grid].editor else {
            panic!("spreadsheet fixture");
        };
        let draft = sheet.editor.as_ref().unwrap();
        assert!(draft.request_focus && draft.keep_selection);
        app.host.workspace.activate(grid);
        mount_test_frame(&mut app, &ctx, true);
        // Also let the restored TextEdit mirror its cursor back on the following frame.
        mount_test_frame(&mut app, &ctx, true);
        assert_grid_stop_draft(&app.host, grid);

        assert!(
            app.host.save_as(grid, &path).is_err(),
            "Stop validation still blocks saving"
        );
        assert!(!path.exists());
        assert!(app.host.path(grid).is_none());
        assert_grid_stop_draft(&app.host, grid);
        assert_eq!(app.host.workspace.tabs().len(), 2);
        assert!(app.error.is_none());
    }

    /// Exercise the real shell hit-testing across frames, without a native window.
    struct ShellPointerReplay {
        app: WeaveApp,
        ctx: egui::Context,
        output: egui::FullOutput,
        commands: Vec<egui::ViewportCommand>,
        shell_payloads: Vec<(Option<TabId>, Option<TabId>)>,
        time: f64,
        maximized: bool,
        draw_chrome: bool,
    }

    impl ShellPointerReplay {
        fn new(host: Host) -> Self {
            let ctx = egui::Context::default();
            ctx.enable_accesskit();
            let mut replay = Self {
                app: test_app(host),
                ctx,
                output: egui::FullOutput::default(),
                commands: Vec::new(),
                shell_payloads: Vec::new(),
                time: 0.0,
                maximized: false,
                draw_chrome: false,
            };
            // Hit-testing uses the previous frame's widgets; settle initial layout first.
            replay.frame(Vec::new());
            replay.frame(Vec::new());
            replay
        }

        fn frame(&mut self, events: Vec<egui::Event>) {
            self.time += 0.05;
            let mut input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1200.0, 800.0),
                )),
                time: Some(self.time),
                focused: true,
                events,
                ..Default::default()
            };
            input
                .viewports
                .get_mut(&egui::ViewportId::ROOT)
                .unwrap()
                .maximized = Some(self.maximized);
            self.shell_payloads.clear();
            let mut output = self.ctx.run_ui(input, |ui| {
                if self.draw_chrome {
                    self.app.apply_weave_style(ui.ctx());
                }
                let before = egui::DragAndDrop::payload::<TabId>(ui.ctx())
                    .as_deref()
                    .copied();
                self.app.shell(ui);
                if self.draw_chrome {
                    let commands = ui
                        .horizontal(|ui| crate::chrome::toolbar(ui, true, true))
                        .inner;
                    self.app.dispatch_requests(ui.ctx(), commands);
                }
                let after = egui::DragAndDrop::payload::<TabId>(ui.ctx())
                    .as_deref()
                    .copied();
                self.shell_payloads.push((before, after));
                self.app.dialogs(ui.ctx());
                self.app.process_actions(ui.ctx());
            });
            self.commands.extend(
                output
                    .viewport_output
                    .values()
                    .flat_map(|viewport| viewport.commands.iter().cloned()),
            );
            output.textures_delta.clear();
            self.output = output;
        }

        fn widget_rect(&self, label: &str) -> egui::Rect {
            let update = self
                .output
                .platform_output
                .accesskit_update
                .as_ref()
                .unwrap();
            let bounds = update
                .nodes
                .iter()
                .find(|(_, node)| node.label() == Some(label))
                .unwrap_or_else(|| panic!("missing accessible widget: {label}"))
                .1
                .bounds()
                .expect("widget bounds");
            egui::Rect::from_min_max(
                egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
                egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
            )
        }

        fn blank_title_bar(&self) -> egui::Pos2 {
            let new_file = self.widget_rect("新建文件");
            let point = egui::pos2(600.0, new_file.center().y);
            assert!(point.x > new_file.right() + 20.0);
            point
        }

        fn move_to(&mut self, pos: egui::Pos2) {
            self.frame(vec![egui::Event::PointerMoved(pos)]);
        }

        fn button(&mut self, pos: egui::Pos2, pressed: bool) {
            self.frame(vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
        }

        fn click(&mut self, pos: egui::Pos2) {
            self.button(pos, true);
            self.button(pos, false);
        }

        fn assert_no_window_drag(&self) {
            assert!(
                !self.commands.contains(&egui::ViewportCommand::StartDrag),
                "content interaction must not drag the native window: {:?}",
                self.commands,
            );
        }
    }

    #[test]
    fn title_bar_new_picker_is_anchored_dismissible_and_creates_the_chosen_format() {
        language::set_current(UiLanguage::ZhHans);
        let mut replay = ShellPointerReplay::new(Host::default());
        let plus = replay.widget_rect("新建文件");
        replay.click(plus.center());
        replay.frame(Vec::new());
        assert!(replay.app.new_picker);
        let choices = ["文字", "演示", "表格", "PDF"].map(|label| replay.widget_rect(label));
        assert!(choices.iter().all(|rect| rect.top() > plus.bottom()));
        assert!(
            choices
                .windows(2)
                .all(|pair| pair[0].right() < pair[1].left())
        );
        assert!(choices[0].left() >= plus.left());
        replay.click(egui::pos2(850.0, 500.0));
        assert!(!replay.app.new_picker);
        assert!(replay.app.host.workspace.tabs().is_empty());
        replay.click(plus.center());
        replay.frame(vec![egui::Event::Key {
            key: Key::Escape,
            physical_key: Some(Key::Escape),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        assert!(!replay.app.new_picker);
        replay.click(plus.center());
        replay.frame(Vec::new());
        let sheet = replay.widget_rect("表格").center();
        replay.click(sheet);
        assert!(!replay.app.new_picker);
        assert_eq!(replay.app.host.workspace.tabs().len(), 1);
        assert_eq!(
            replay.app.host.workspace.tabs()[0].kind,
            DocumentKind::Spreadsheet
        );
        replay.assert_no_window_drag();
    }

    #[test]
    fn shell_pointer_drag_reorders_tabs_without_dragging_the_window() {
        let mut host = Host::default();
        let first = host.new_document(DocumentKind::Text).unwrap();
        let second = host.new_document(DocumentKind::Spreadsheet).unwrap();
        // Only update the display identity; these tests do not read or write files.
        host.workspace.set_path(first, PathBuf::from("Alpha.docx"));
        host.workspace.set_path(second, PathBuf::from("Beta.xlsx"));
        let mut replay = ShellPointerReplay::new(host);
        let source = replay.widget_rect("Beta.xlsx").center();
        let target = replay.widget_rect("Alpha.docx").center();
        replay.move_to(source);
        replay.button(source, true);
        assert!(egui::DragAndDrop::payload::<TabId>(&replay.ctx).is_none());
        replay.move_to(source + egui::vec2(-18.0, 0.0));
        assert_eq!(
            egui::DragAndDrop::payload::<TabId>(&replay.ctx).as_deref(),
            Some(&second),
            "crossing the drag threshold must pick up the tab, not its parent",
        );
        replay.move_to(target);
        assert_eq!(
            replay.app.host.workspace.tabs()[0].id,
            first,
            "reorder waits for release"
        );
        replay.button(target, false);
        assert_eq!(
            replay
                .app
                .host
                .workspace
                .tabs()
                .iter()
                .map(|tab| tab.id)
                .collect::<Vec<_>>(),
            [second, first],
        );
        assert!(egui::DragAndDrop::payload::<TabId>(&replay.ctx).is_none());
        replay.assert_no_window_drag();
        assert!(replay.app.error.is_none());
    }

    #[test]
    fn shell_pointer_coalesced_drag_requires_a_held_frame_after_crossing_threshold() {
        // egui resolves drag_started after processing the frame's complete event list.
        // Releasing in that same list clears the potential drag before it can start.
        // Characterize this boundary separately from the normal multi-frame gesture.
        for batch in [
            "single_frame",
            "release_with_first_move",
            "release_after_held_move",
        ] {
            let mut host = Host::default();
            let first = host.new_document(DocumentKind::Text).unwrap();
            let second = host.new_document(DocumentKind::Spreadsheet).unwrap();
            host.workspace.set_path(first, PathBuf::from("Alpha.docx"));
            host.workspace.set_path(second, PathBuf::from("Beta.xlsx"));
            let mut replay = ShellPointerReplay::new(host);
            let source_rect = replay.widget_rect("Beta.xlsx");
            let source = source_rect.center();
            let threshold = source + egui::vec2(-18.0, 0.0);
            assert!(source_rect.contains(threshold));
            let target = replay.widget_rect("Alpha.docx").center();
            let button = |pos, pressed| egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            replay.move_to(source);
            match batch {
                "single_frame" => replay.frame(vec![
                    button(source, true),
                    egui::Event::PointerMoved(threshold),
                    egui::Event::PointerMoved(target),
                    button(target, false),
                ]),
                "release_with_first_move" => {
                    replay.button(source, true);
                    assert!(
                        replay
                            .shell_payloads
                            .iter()
                            .all(|pair| *pair == (None, None))
                    );
                    replay.frame(vec![
                        egui::Event::PointerMoved(threshold),
                        egui::Event::PointerMoved(target),
                        button(target, false),
                    ]);
                }
                "release_after_held_move" => {
                    // Establish the press before crossing the threshold. The release
                    // frame may then coalesce its final move without losing the payload.
                    replay.button(source, true);
                    replay.move_to(threshold);
                    assert!(
                        replay.shell_payloads.contains(&(None, Some(second))),
                        "a held frame must establish the source payload: {:?}",
                        replay.shell_payloads,
                    );
                    assert_eq!(
                        egui::DragAndDrop::payload::<TabId>(&replay.ctx).as_deref(),
                        Some(&second),
                    );
                    replay.frame(vec![
                        egui::Event::PointerMoved(target),
                        button(target, false),
                    ]);
                }
                _ => unreachable!(),
            }
            let expected = if batch == "release_after_held_move" {
                assert!(
                    replay.shell_payloads.contains(&(Some(second), None)),
                    "the shell must consume the payload before end-of-frame cleanup: {:?}",
                    replay.shell_payloads,
                );
                [second, first]
            } else {
                assert!(
                    replay
                        .shell_payloads
                        .iter()
                        .all(|pair| *pair == (None, None)),
                    "{batch}: no held drag frame means no payload: {:?}",
                    replay.shell_payloads,
                );
                [first, second]
            };
            assert_eq!(
                replay
                    .app
                    .host
                    .workspace
                    .tabs()
                    .iter()
                    .map(|tab| tab.id)
                    .collect::<Vec<_>>(),
                expected,
                "event batching: {batch}",
            );
            assert!(egui::DragAndDrop::payload::<TabId>(&replay.ctx).is_none());
            replay.assert_no_window_drag();
            assert!(replay.app.error.is_none());
        }
    }

    #[test]
    fn shell_pointer_drag_on_title_bar_whitespace_drags_the_window() {
        let mut replay = ShellPointerReplay::new(Host::default());
        let start = replay.blank_title_bar();
        replay.move_to(start);
        replay.button(start, true);
        assert!(!replay.commands.contains(&egui::ViewportCommand::StartDrag));
        let end = start + egui::vec2(24.0, 0.0);
        replay.move_to(end);
        replay.button(end, false);
        assert_eq!(
            replay
                .commands
                .iter()
                .filter(|command| **command == egui::ViewportCommand::StartDrag)
                .count(),
            1
        );
        assert!(egui::DragAndDrop::payload::<TabId>(&replay.ctx).is_none());
    }

    #[test]
    fn shell_pointer_double_click_on_title_bar_toggles_maximize() {
        for maximized in [false, true] {
            let mut replay = ShellPointerReplay::new(Host::default());
            replay.maximized = maximized;
            let point = replay.blank_title_bar();
            replay.move_to(point);
            replay.click(point);
            assert!(
                !replay
                    .commands
                    .iter()
                    .any(|command| matches!(command, egui::ViewportCommand::Maximized(_)))
            );
            replay.click(point);
            assert!(
                replay
                    .commands
                    .contains(&egui::ViewportCommand::Maximized(!maximized))
            );
            replay.assert_no_window_drag();
        }
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn shell_pointer_window_controls_emit_commands_and_route_close_through_host() {
        let mut replay = ShellPointerReplay::new(Host::default());
        let minimize = replay.widget_rect("最小化窗口").center();
        replay.click(minimize);
        assert!(
            replay
                .commands
                .contains(&egui::ViewportCommand::Minimized(true))
        );
        let maximize = replay.widget_rect("最大化窗口").center();
        replay.click(maximize);
        assert!(
            replay
                .commands
                .contains(&egui::ViewportCommand::Maximized(true))
        );
        replay.maximized = true;
        replay.frame(Vec::new());
        let restore = replay.widget_rect("还原窗口").center();
        replay.click(restore);
        assert!(
            replay
                .commands
                .contains(&egui::ViewportCommand::Maximized(false))
        );
        let close = replay.widget_rect("关闭窗口").center();
        replay.click(close);
        assert!(replay.app.close_plan.as_ref().is_some_and(|plan| plan.quit));
        assert!(
            !replay.commands.contains(&egui::ViewportCommand::Close),
            "the host must finish the close transaction first"
        );
        replay.assert_no_window_drag();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn shell_uses_native_mac_window_controls_and_preserves_their_space() {
        let mut host = Host::default();
        let first = host.new_document(DocumentKind::Text).unwrap();
        host.workspace.set_path(first, PathBuf::from("Alpha.docx"));
        let replay = ShellPointerReplay::new(host);
        let update = replay
            .output
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap();
        for label in ["最小化窗口", "最大化窗口", "还原窗口", "关闭窗口"] {
            assert!(
                update
                    .nodes
                    .iter()
                    .all(|(_, node)| node.label() != Some(label)),
                "macOS native control must not have a duplicate egui widget: {label}"
            );
        }
        let first_tab = replay.widget_rect("Alpha.docx");
        assert!(
            first_tab.left() >= crate::theme::NATIVE_CONTROLS_WIDTH,
            "first tab intrudes into the native traffic-light area: {first_tab:?}"
        );
    }

    #[test]
    fn an_unposted_pdf_comment_can_be_discarded_from_the_shared_close_prompt() {
        let mut host = Host::default();
        let id = host.new_document(DocumentKind::Pdf).expect("new PDF");
        let crate::editor::Editor::Pdf(pdf) =
            &mut host.documents.get_mut(&id).expect("document").editor
        else {
            panic!("PDF editor");
        };
        let (index, _) = pdf.active_ids().expect("PDF view");
        pdf.views[index].comments.add_box = "Unposted review draft".into();
        assert!(
            pdf.hosted_flush().is_err(),
            "draft needs an explicit Post or Cancel"
        );
        let mut app = test_app(host);
        let ctx = egui::Context::default();
        app.begin_close(Some(id), false);
        app.advance_close(&ctx);
        assert_eq!(app.close_prompt, Some(id));
        assert!(app.error.is_none());
        assert!(app.host.documents.contains_key(&id));
        // The user chooses Don't Save; no draft flush can prevent this explicit decision.
        app.close_prompt = None;
        app.close_plan.as_mut().expect("close transaction").index += 1;
        app.advance_close(&ctx);
        assert!(!app.host.documents.contains_key(&id));
        assert!(!app.allow_exit);
    }
    #[test]
    fn generated_pdf_arriving_during_quit_keeps_the_whole_workspace() {
        let mut host = Host::default();
        let source = host.new_document(DocumentKind::Pdf).unwrap();
        let mut app = test_app(host);
        let ctx = egui::Context::default();
        app.begin_close(None, true);
        let plan = app.close_plan.as_mut().unwrap();
        plan.index = plan.ids.len(); // all earlier choices were approved
        let crate::editor::Editor::Pdf(pdf) =
            &mut app.host.documents.get_mut(&source).unwrap().editor
        else {
            panic!("PDF")
        };
        let bytes = pdf.session.create_blank(100.0, 100.0, 1).unwrap();
        pdf.open_bytes("Arrived during quit.pdf", None, (*bytes).clone())
            .unwrap();
        app.advance_close(&ctx);
        assert_eq!(app.host.workspace.tabs().len(), 2);
        assert!(app.host.documents.contains_key(&source));
        assert!(app.close_plan.is_none());
        assert!(!app.allow_exit);
        assert!(app.host.workspace.tabs().iter().all(|tab| tab.is_dirty()));
        app.begin_close(None, true);
        assert_eq!(app.close_plan.as_ref().unwrap().ids.len(), 2);
    }

    #[test]
    fn opening_host_settings_drops_later_commands_in_the_same_batch() {
        let mut host = Host::default();
        host.new_document(DocumentKind::Text).unwrap();
        let mut app = test_app(host);
        app.dispatch_requests(
            &egui::Context::default(),
            vec![
                ("app.preferences".into(), json!({})),
                ("edit.undo".into(), json!({})),
                ("file.save".into(), json!({})),
            ],
        );
        assert!(app.preferences);
        assert!(app.actions.is_empty());
    }

    #[test]
    fn closing_waits_for_the_shared_save_picker_and_cancellation_keeps_the_draft() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("saved.docx");
        let mut host = Host::default();
        let id = host.new_document(DocumentKind::Text).unwrap();
        host.active_mut()
            .unwrap()
            .editor
            .command("text.insert", json!({"text":"Asynchronous save draft"}))
            .unwrap();
        let mut app = test_app(host);
        let ctx = egui::Context::default();
        app.begin_close(Some(id), false);
        app.advance_close(&ctx);
        assert_eq!(app.close_prompt, Some(id));
        assert!(!app.save(id, false).unwrap());
        assert_eq!(app.file_dialogs.len(), 1);
        app.advance_close(&ctx);
        assert!(app.host.dirty(id));
        assert!(app.host.path(id).is_none());
        let pending = app.file_dialogs.pop_front().unwrap();
        app.finish_file_selection(pending.purpose, None, &ctx)
            .unwrap();
        assert_eq!(app.close_prompt, Some(id));
        assert!(app.host.dirty(id));
        assert!(!path.exists());
        assert!(!app.save(id, false).unwrap());
        let pending = app.file_dialogs.pop_front().unwrap();
        app.finish_file_selection(pending.purpose, Some(vec![path.clone()]), &ctx)
            .unwrap();
        assert!(app.error.is_none());
        assert!(app.close_prompt.is_none());
        assert_eq!(app.close_plan.as_ref().unwrap().index, 1);
        assert!(!app.host.dirty(id));
        app.advance_close(&ctx);
        assert!(!app.host.documents.contains_key(&id));
        let mut reader = Host::default();
        reader.open(&path).unwrap();
        let text = reader
            .active_mut()
            .unwrap()
            .editor
            .command("document.text", json!({}))
            .unwrap();
        assert_eq!(
            text["text"].as_str().unwrap().trim(),
            "Asynchronous save draft"
        );
    }

    #[test]
    fn quit_shortcut_cancels_pending_file_requests_before_scanning_documents() {
        let mut host = Host::default();
        let id = host.new_document(DocumentKind::Pdf).unwrap();
        let mut app = test_app(host);
        app.choose_file(
            FileDialogMode::Open { multiple: true },
            FileDialogOptions::default(),
            FilePurpose::Open,
        );
        let ctx = egui::Context::default();
        ctx.begin_pass(egui::RawInput {
            events: vec![egui::Event::Key {
                key: Key::Q,
                physical_key: Some(Key::Q),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::COMMAND,
            }],
            ..Default::default()
        });
        app.shortcuts(&ctx);
        assert!(app.file_dialogs.is_empty());
        assert!(app.close_plan.as_ref().unwrap().quit);
        assert!(app.host.documents.contains_key(&id));
        assert!(!app.allow_exit);
        ctx.end_pass().textures_delta.clear();
    }
    #[test]
    fn quit_upgrades_a_single_document_close_transaction() {
        let mut host = Host::default();
        let first = host.new_document(DocumentKind::Pdf).unwrap();
        let second = host.new_document(DocumentKind::Text).unwrap();
        let mut app = test_app(host);
        app.begin_close(Some(second), false);
        assert!(!app.close_plan.as_ref().unwrap().quit);
        app.request_quit(&egui::Context::default());
        let plan = app.close_plan.as_ref().unwrap();
        assert!(plan.quit);
        assert_eq!(plan.ids, [first, second]);
        assert_eq!(plan.index, 0);
        assert_eq!(app.host.documents.len(), 2);
    }

    #[test]
    fn a_rejected_save_keeps_the_same_file_picker_available_for_retry() {
        let directory = tempfile::tempdir().unwrap();
        let occupied = directory.path().join("occupied.docx");
        let mut host = Host::default();
        let existing = host.new_document(DocumentKind::Text).unwrap();
        host.save_as(existing, &occupied).unwrap();
        let draft = host.new_document(DocumentKind::Text).unwrap();
        host.active_mut()
            .unwrap()
            .editor
            .command("text.insert", json!({"text":"Keep this draft"}))
            .unwrap();
        let mut app = test_app(host);
        app.choose_file(
            FileDialogMode::Save,
            FileDialogOptions {
                directory: Some(directory.path().to_path_buf()),
                file_name: "occupied.docx".into(),
                ..Default::default()
            },
            FilePurpose::Save(draft),
        );
        let before = std::fs::read(&occupied).unwrap();
        app.complete_file_dialog(
            FileDialogResult::Selected(vec![occupied.clone()]),
            &egui::Context::default(),
        );
        assert_eq!(app.file_dialogs.len(), 1);
        assert!(
            matches!(app.file_dialogs.front().unwrap().purpose, FilePurpose::Save(id) if id == draft)
        );
        assert!(
            app.error.is_none(),
            "error belongs inside the retained chooser"
        );
        assert!(app.host.dirty(draft));
        assert!(app.host.path(draft).is_none());
        assert_eq!(std::fs::read(&occupied).unwrap(), before);
    }

    #[test]
    fn office_picker_answers_stay_with_the_origin_even_when_request_ids_match() {
        let mut host = Host::default();
        let first = host.new_document(DocumentKind::Text).unwrap();
        host.documents
            .get_mut(&first)
            .unwrap()
            .editor
            .command("insert.picture", json!({}))
            .unwrap();
        let second = host.new_document(DocumentKind::Text).unwrap();
        host.documents
            .get_mut(&second)
            .unwrap()
            .editor
            .command("insert.picture", json!({}))
            .unwrap();
        let mut app = test_app(host);
        let ctx = egui::Context::default();
        app.receive_file_requests();
        assert_eq!(app.file_dialogs.len(), 2);
        assert!(
            matches!(app.file_dialogs[0].purpose, FilePurpose::Office { tab, request: 1 } if tab == first)
        );
        assert!(
            matches!(app.file_dialogs[1].purpose, FilePurpose::Office { tab, request: 1 } if tab == second)
        );
        app.complete_file_dialog(FileDialogResult::Cancelled, &ctx);
        for id in [first, second] {
            app.host
                .documents
                .get_mut(&id)
                .unwrap()
                .editor
                .flush()
                .unwrap();
        }
        let crate::editor::Editor::Word(first_app) = &app.host.documents[&first].editor else {
            panic!()
        };
        let crate::editor::Editor::Word(second_app) = &app.host.documents[&second].editor else {
            panic!()
        };
        assert!(!first_app.file_dialog_open());
        assert!(second_app.file_dialog_open());
        app.begin_close(None, true);
        assert!(app.file_dialogs.is_empty());
        app.host
            .documents
            .get_mut(&second)
            .unwrap()
            .editor
            .flush()
            .unwrap();
        let crate::editor::Editor::Word(second_app) = &app.host.documents[&second].editor else {
            panic!()
        };
        assert!(!second_app.file_dialog_open());
        assert!(!app.host.dirty(first));
        assert!(!app.host.dirty(second));
    }

    #[test]
    fn office_picker_result_for_a_closed_source_never_retargets_the_active_tab() {
        let mut host = Host::default();
        let source = host.new_document(DocumentKind::Text).unwrap();
        host.documents
            .get_mut(&source)
            .unwrap()
            .editor
            .command("insert.picture", json!({}))
            .unwrap();
        let other = host.new_document(DocumentKind::Presentation).unwrap();
        let mut app = test_app(host);
        let ctx = egui::Context::default();
        app.receive_file_requests();
        assert_eq!(app.file_dialogs.len(), 1);
        assert_eq!(
            app.host.close(source, true),
            weave_workspace::CloseResult::Closed
        );
        app.complete_file_dialog(
            FileDialogResult::Selected(vec![PathBuf::from("not-read-after-source-closes.png")]),
            &ctx,
        );
        assert!(
            app.error
                .as_deref()
                .is_some_and(|error| error.contains("文件已关闭"))
        );
        assert_eq!(app.host.workspace.active(), Some(other));
        assert_eq!(app.host.workspace.tabs().len(), 1);
        assert!(!app.host.dirty(other));
        let crate::editor::Editor::Slides(slides) = &app.host.documents[&other].editor else {
            panic!()
        };
        assert!(slides.session.active().unwrap().doc.media.is_empty());
    }

    fn crowded_shell() -> (ShellPointerReplay, Vec<TabId>) {
        let mut host = Host::default();
        let mut ids = Vec::new();
        for index in 0..24 {
            let kind = [
                DocumentKind::Text,
                DocumentKind::Spreadsheet,
                DocumentKind::Presentation,
                DocumentKind::Pdf,
            ][index % 4];
            let id = host.new_document(kind).expect("synthetic editor");
            // Distinct captions retain a common CJK prefix and exercise real glyph widths.
            let extension = ["docx", "xlsx", "pptx", "pdf"][index % 4];
            let title = format!("年度经营分析与客户回访计划{index:02}华东事业部.{extension}");
            host.workspace.set_path(id, PathBuf::from(title));
            ids.push(id);
        }
        let mut replay = ShellPointerReplay::new(host);
        replay.app.ui_font = Some(crate::ui_fonts::UiFonts::load());
        shell_contract_frame(&mut replay, Vec::new());
        shell_contract_frame(&mut replay, Vec::new());
        (replay, ids)
    }

    fn shell_contract_frame(replay: &mut ShellPointerReplay, events: Vec<egui::Event>) {
        replay.time += 0.05;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200.0, 800.0),
            )),
            time: Some(replay.time),
            focused: true,
            events,
            ..Default::default()
        };
        let mut output = replay.ctx.run_ui(input, |ui| {
            replay.app.shortcuts(ui.ctx());
            replay
                .app
                .mount_active(ui.ctx())
                .expect("switch mounted editor");
            replay.app.install_shell_font(ui.ctx());
            replay.app.apply_weave_style(ui.ctx());
            replay.app.shell(ui);
            replay.app.process_actions(ui.ctx());
        });
        output.textures_delta.clear();
        replay.output = output;
    }

    fn assert_active_tab_fully_visible(replay: &ShellPointerReplay, id: TabId) {
        assert_eq!(replay.app.host.workspace.active(), Some(id));
        assert_eq!(replay.app.mounted, Some(id));
        assert!(replay.ctx.fonts_mut(|fonts| fonts.has_glyphs(
            &egui::FontId::proportional(13.0),
            "年度经营分析与客户回访计划 English"
        )));
        let tab = replay.app.host.workspace.tab(id).unwrap();
        let caption = format!(
            "{}{}",
            short_title(&tab.title),
            if tab.is_dirty() { " •" } else { "" }
        );
        let body = replay.widget_rect(&caption);
        let new_file = replay.widget_rect("新建文件");
        let update = replay
            .output
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap();
        let rect = |bounds: egui::accesskit::Rect| {
            egui::Rect::from_min_max(
                egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
                egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
            )
        };
        let icon_label = format!("{}图标", kind_name(tab.kind));
        let icon = update
            .nodes
            .iter()
            .filter_map(|(_, node)| {
                (node.role() == egui::accesskit::Role::Image
                    && node.label() == Some(icon_label.as_str()))
                .then(|| node.bounds())
                .flatten()
            })
            .map(rect)
            .filter(|candidate| {
                candidate.center().y >= body.top()
                    && candidate.center().y <= body.bottom()
                    && candidate.left() >= body.left()
                    && candidate.right() <= body.right()
            })
            .next()
            .expect("active tab icon accessible inside its body");
        let close_label = format!("关闭 {}", tab.title);
        let close = update
            .nodes
            .iter()
            .filter_map(|(_, node)| {
                (node.role() == egui::accesskit::Role::Button
                    && node.label() == Some(close_label.as_str()))
                .then(|| node.bounds())
                .flatten()
            })
            .map(rect)
            .next()
            .expect("active tab close button accessible");
        let complete_tab = egui::Rect::from_min_max(body.min, close.max);
        assert!(
            (complete_tab.width() - 198.0).abs() <= 1.0
                && (complete_tab.height() - 28.0).abs() <= 1.0,
            "active tab must retain its full 198×28 bounds: {complete_tab:?}"
        );
        assert!(
            (body.right() - close.left()).abs() <= 1.0 && (close.width() - 28.0).abs() <= 1.0,
            "close button must occupy the tab's final 28 points: body {body:?}, close {close:?}"
        );
        assert!(
            complete_tab.left()
                >= if cfg!(target_os = "macos") {
                    crate::theme::NATIVE_CONTROLS_WIDTH - 1.0
                } else {
                    0.0
                },
            "active tab overlaps native window controls: {complete_tab:?}"
        );
        assert!(
            complete_tab.right() <= new_file.left() + 1.0,
            "active tab is clipped at right: {complete_tab:?}, new {new_file:?}"
        );
        assert!(
            icon.top() >= complete_tab.top() && icon.bottom() <= complete_tab.bottom(),
            "active icon leaves the title-bar row: {icon:?}, tab {complete_tab:?}"
        );
    }

    #[test]
    fn crowded_mixed_shell_reveals_first_middle_and_last_complete_tabs() {
        let (mut replay, ids) = crowded_shell();
        for id in [ids[23], ids[11], ids[0]] {
            replay.app.actions.push_back(Action::Activate(id));
            shell_contract_frame(&mut replay, Vec::new());
            shell_contract_frame(&mut replay, Vec::new());
            shell_contract_frame(&mut replay, Vec::new());
            assert_active_tab_fully_visible(&replay, id);
        }
    }

    #[test]
    fn crowded_mixed_shell_ctrl_tab_cycles_ids_and_reveals_each_target() {
        let (mut replay, ids) = crowded_shell();
        for (start, backwards, expected) in [
            (ids[23], false, ids[0]),
            (ids[0], true, ids[23]),
            (ids[10], false, ids[11]),
            (ids[11], true, ids[10]),
        ] {
            replay.app.actions.push_back(Action::Activate(start));
            shell_contract_frame(&mut replay, Vec::new());
            shell_contract_frame(&mut replay, Vec::new());
            let modifiers = if backwards {
                egui::Modifiers::CTRL | egui::Modifiers::SHIFT
            } else {
                egui::Modifiers::CTRL
            };
            shell_contract_frame(
                &mut replay,
                vec![egui::Event::Key {
                    key: Key::Tab,
                    physical_key: Some(Key::Tab),
                    pressed: true,
                    repeat: false,
                    modifiers,
                }],
            );
            shell_contract_frame(&mut replay, Vec::new());
            shell_contract_frame(&mut replay, Vec::new());
            assert_active_tab_fully_visible(&replay, expected);
        }
    }
}
