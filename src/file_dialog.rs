//! Native file selection with a Weave-styled error/retry surface.
//! This module only returns paths. Saving remains the host's responsibility.
use crate::dialog::{self, ButtonKind};
use eframe::egui;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileDialogMode {
    Open { multiple: bool },
    Save,
    Folder,
}

#[derive(Clone, Debug, Default)]
pub struct FileDialogOptions {
    pub title: String,
    pub directory: Option<PathBuf>,
    pub file_name: String,
    /// Display names and extensions without dots, e.g. ["docx"].
    /// An empty extension list or "*" accepts all files.
    pub filters: Vec<(String, Vec<String>)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileDialogResult {
    Selected(Vec<PathBuf>),
    Cancelled,
}

pub struct FileDialog {
    mode: FileDialogMode,
    options: FileDialogOptions,
    error: Option<String>,
    /// A native panel opens once, or after an explicit retry, never each frame.
    native_pending: bool,
    selected_path: Option<PathBuf>,
}

impl FileDialog {
    pub fn new(mode: FileDialogMode, options: FileDialogOptions) -> Self {
        Self {
            mode,
            options,
            error: None,
            native_pending: true,
            selected_path: None,
        }
    }

    /// Preserve the last destination and wait for an explicit retry or cancel.
    pub fn set_error(&mut self, error: String) {
        self.error = Some(error);
        self.native_pending = false;
    }

    /// Call on the UI/main thread. rfd's synchronous macOS implementation runs
    /// NSSavePanel/NSOpenPanel's modal loop here, so native editing shortcuts and
    /// the app's AppleEvent quit handler continue to use the main run loop.
    #[cfg(not(target_os = "android"))]
    pub fn show(&mut self, ctx: &egui::Context) -> Option<FileDialogResult> {
        if self.error.is_some() {
            return self.show_error(ctx);
        }
        if !self.native_pending {
            return None;
        }
        self.native_pending = false;
        let dialog = self.native_dialog();
        let paths = crate::native_file_panel::with_file_panel_menu(ctx, || match self.mode {
            FileDialogMode::Open { multiple: true } => dialog.pick_files(),
            FileDialogMode::Open { multiple: false } => dialog.pick_file().map(|path| vec![path]),
            FileDialogMode::Save => dialog.save_file().map(|path| vec![path]),
            FileDialogMode::Folder => dialog.pick_folder().map(|path| vec![path]),
        });
        ctx.request_repaint();
        Some(self.receive_native_result(paths))
    }

    /// Android: the system pickers are asynchronous and live in the Activity, which answers
    /// through the SAF bridge. Until that bridge is wired, a dialog is cancelled.
    #[cfg(target_os = "android")]
    pub fn show(&mut self, ctx: &egui::Context) -> Option<FileDialogResult> {
        if self.error.is_some() {
            return self.show_error(ctx);
        }
        if !self.native_pending {
            return None;
        }
        self.native_pending = false;
        ctx.request_repaint();
        Some(FileDialogResult::Cancelled)
    }

    #[cfg(not(target_os = "android"))]
    fn native_dialog(&self) -> rfd::FileDialog {
        let mut dialog = rfd::FileDialog::new();
        if !self.options.title.is_empty() {
            dialog = dialog.set_title(crate::language::tr(&self.options.title));
        }
        if let Some(directory) = &self.options.directory {
            dialog = dialog.set_directory(directory);
        }
        if !self.options.file_name.is_empty() && self.mode != FileDialogMode::Folder {
            dialog = dialog.set_file_name(&self.options.file_name);
        }
        // macOS merges all filter groups. Do not send wildcard strings as UTIs
        // or extensions: omitting filters is the native "all files" behavior.
        if self.mode != FileDialogMode::Folder && !accepts_all_files(&self.options.filters) {
            for (name, extensions) in &self.options.filters {
                let extensions: Vec<_> = extensions
                    .iter()
                    .map(|extension| extension.trim_start_matches("*.").trim_start_matches('.'))
                    .filter(|extension| !extension.is_empty())
                    .collect();
                if !extensions.is_empty() {
                    dialog = dialog.add_filter(crate::language::tr(name), &extensions);
                }
            }
        }
        dialog
    }

    fn receive_native_result(&mut self, paths: Option<Vec<PathBuf>>) -> FileDialogResult {
        self.native_pending = false;
        match paths.filter(|paths| !paths.is_empty()) {
            Some(paths) => {
                if let Some(path) = paths.first() {
                    self.selected_path = Some(path.clone());
                    if self.mode == FileDialogMode::Folder {
                        self.options.directory = Some(path.clone());
                    } else {
                        self.options.directory = path.parent().map(Path::to_path_buf);
                        self.options.file_name = path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned();
                    }
                }
                FileDialogResult::Selected(paths)
            }
            None => FileDialogResult::Cancelled,
        }
    }

    fn show_error(&mut self, ctx: &egui::Context) -> Option<FileDialogResult> {
        let mut cancel = false;
        let mut retry = false;
        let response = egui::Modal::new(egui::Id::unique("weave-file-error"))
            .frame(dialog::frame(ctx))
            .show(ctx, |ui| {
                dialog::prepare(ui);
                dialog::header(ui, "操作未完成");
                if let Some(path) = &self.selected_path {
                    ui.label(
                        egui::RichText::new(path.display().to_string())
                            .small()
                            .weak(),
                    );
                }
                if let Some(error) = &self.error {
                    dialog::body(ui, error);
                }
                dialog::footer(ui, |ui| {
                    retry = dialog::button(ui, "再次选择…", ButtonKind::Primary).clicked();
                    cancel = dialog::button(ui, "取消", ButtonKind::Secondary).clicked();
                });
            });
        if cancel || response.should_close() {
            return Some(FileDialogResult::Cancelled);
        }
        if retry {
            self.error = None;
            self.native_pending = true;
            // Open on the next call, after the error surface finishes this pass.
            ctx.request_repaint();
        }
        None
    }
}

fn accepts_all_files(filters: &[(String, Vec<String>)]) -> bool {
    filters.is_empty()
        || filters.iter().any(|(_, extensions)| {
            extensions.is_empty()
                || extensions
                    .iter()
                    .any(|extension| extension == "*" || extension == "*.*")
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_save_preserves_destination_without_creating_a_file_or_reopening() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("报告.docx");
        let mut dialog = FileDialog::new(FileDialogMode::Save, FileDialogOptions::default());
        assert_eq!(
            dialog.receive_native_result(Some(vec![path.clone()])),
            FileDialogResult::Selected(vec![path.clone()])
        );
        dialog.set_error("写入失败".into());
        assert_eq!(dialog.options.directory.as_deref(), Some(directory.path()));
        assert_eq!(dialog.options.file_name, "报告.docx");
        assert_eq!(dialog.selected_path.as_ref(), Some(&path));
        assert!(!dialog.native_pending);
        assert!(!path.exists());
    }

    #[test]
    fn native_cancellation_neither_changes_destination_nor_rearms_panel() {
        let directory = PathBuf::from("/original/location");
        let mut dialog = FileDialog::new(
            FileDialogMode::Save,
            FileDialogOptions {
                directory: Some(directory.clone()),
                file_name: "original.pdf".into(),
                ..Default::default()
            },
        );
        assert_eq!(
            dialog.receive_native_result(None),
            FileDialogResult::Cancelled
        );
        assert_eq!(dialog.options.directory, Some(directory));
        assert_eq!(dialog.options.file_name, "original.pdf");
        assert!(dialog.selected_path.is_none());
        assert!(!dialog.native_pending);
    }
}
