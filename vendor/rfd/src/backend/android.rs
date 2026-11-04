//! Android: no native file dialogs. The shell answers the app's pickers through the SAF bridge
//! (a later layer); until then every dialog is cancelled.

use super::*;
use crate::file_dialog::FileDialog;
use crate::message_dialog::{MessageDialog, MessageDialogResult};
use crate::FileHandle;
use std::path::PathBuf;

impl FilePickerDialogImpl for FileDialog {
    fn pick_file(self) -> Option<PathBuf> {
        None
    }
    fn pick_files(self) -> Option<Vec<PathBuf>> {
        None
    }
}

impl FileSaveDialogImpl for FileDialog {
    fn save_file(self) -> Option<PathBuf> {
        None
    }
}

impl FolderPickerDialogImpl for FileDialog {
    fn pick_folder(self) -> Option<PathBuf> {
        None
    }
    fn pick_folders(self) -> Option<Vec<PathBuf>> {
        None
    }
}

impl MessageDialogImpl for MessageDialog {
    fn show(self) -> MessageDialogResult {
        MessageDialogResult::Cancel
    }
}

impl AsyncFilePickerDialogImpl for FileDialog {
    fn pick_file_async(self) -> DialogFutureType<Option<FileHandle>> {
        Box::pin(async { None })
    }
    fn pick_files_async(self) -> DialogFutureType<Option<Vec<FileHandle>>> {
        Box::pin(async { None })
    }
}

impl AsyncFolderPickerDialogImpl for FileDialog {
    fn pick_folder_async(self) -> DialogFutureType<Option<FileHandle>> {
        Box::pin(async { None })
    }
    fn pick_folders_async(self) -> DialogFutureType<Option<Vec<FileHandle>>> {
        Box::pin(async { None })
    }
}

impl AsyncFileSaveDialogImpl for FileDialog {
    fn save_file_async(self) -> DialogFutureType<Option<FileHandle>> {
        Box::pin(async { None })
    }
}

impl AsyncMessageDialogImpl for MessageDialog {
    fn show_async(self) -> DialogFutureType<MessageDialogResult> {
        Box::pin(async { MessageDialogResult::Cancel })
    }
}
