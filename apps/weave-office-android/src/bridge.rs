//! The file bridge between the Rust shell and the Activity: small JSON files in the app's
//! private storage, so the Rust side needs no JNI calls and no `unsafe`.
//!
//! ```text
//! <app files>/bridge/
//!  ├─ request.json   Rust → Activity   {"id", "kind": "open"|"save", "multiple", "title",
//!  │                                   "extensions", "file_name"}
//!  ├─ answer.json    Activity → Rust   {"id", "paths": [...], "cancelled", "error"}
//!  ├─ inbox/<id>/    Activity → Rust   copies of the documents an Open picker returned
//!  └─ out/<id>/      Rust → Activity   where a Save picker's document is written
//! ```
//!
//! Each side writes under a temporary name and renames, so the other side never reads half of
//! one. The Activity polls the bridge every 150 ms while the app is in the foreground; the Rust
//! side polls for an answer every 100 ms on the frame that asked.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use weave_office::file_dialog::{FileDialogMode, FileDialogOptions, FileDialogResult};

pub struct Bridge {
    root: PathBuf,
    next: AtomicU64,
}

impl Bridge {
    pub fn new(files_dir: &Path) -> std::io::Result<Self> {
        let root = files_dir.join("bridge");
        for dir in ["inbox", "out"] {
            fs::create_dir_all(root.join(dir))?;
        }
        Ok(Self { root, next: AtomicU64::new(1) })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Ask the Activity for a document; returns the request id to poll.
    pub fn request(&self, mode: FileDialogMode, options: &FileDialogOptions) -> u64 {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let kind = match mode {
            FileDialogMode::Open { .. } => "open",
            FileDialogMode::Save => "save",
            FileDialogMode::Folder => "folder",
        };
        let multiple = matches!(mode, FileDialogMode::Open { multiple: true });
        let extensions: Vec<String> = options
            .filters
            .iter()
            .flat_map(|(_, extensions)| extensions.iter().cloned())
            .collect();
        let request = serde_json::json!({
            "id": id,
            "kind": kind,
            "multiple": multiple,
            "title": options.title,
            "extensions": extensions,
            "file_name": options.file_name,
        });
        if let Err(error) = write_then_rename(&self.root.join("request.json"), request.to_string().as_bytes()) {
            log::warn!("bridge: couldn't write the request: {error}");
        }
        id
    }

    /// Poll the Activity's answer for `id`.
    pub fn poll(&self, id: u64) -> Option<FileDialogResult> {
        let path = self.root.join("answer.json");
        let text = fs::read_to_string(&path).ok()?;
        let answer: serde_json::Value = serde_json::from_str(&text).ok()?;
        if answer.get("id").and_then(serde_json::Value::as_u64) != Some(id) {
            return None;
        }
        let _ = fs::remove_file(&path);
        if let Some(error) = answer.get("error").and_then(serde_json::Value::as_str) {
            log::warn!("bridge: the picker failed: {error}");
        }
        if answer.get("cancelled").and_then(serde_json::Value::as_bool).unwrap_or(false) {
            return Some(FileDialogResult::Cancelled);
        }
        let paths: Vec<PathBuf> = answer
            .get("paths")
            .and_then(serde_json::Value::as_array)
            .map(|paths| {
                paths
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .map(PathBuf::from)
                    .collect()
            })
            .unwrap_or_default();
        if paths.is_empty() {
            return Some(FileDialogResult::Cancelled);
        }
        Some(FileDialogResult::Selected(paths))
    }
}

fn write_then_rename(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path)
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn bridge() -> (tempfile::TempDir, Bridge) {
        let dir = tempfile::tempdir().unwrap();
        let bridge = Bridge::new(dir.path()).unwrap();
        (dir, bridge)
    }

    #[test]
    fn request_and_answer_round_trip() {
        let (_dir, bridge) = bridge();
        let id = bridge.request(FileDialogMode::Open { multiple: false }, &FileDialogOptions::default());
        let request: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(bridge.root().join("request.json")).unwrap()).unwrap();
        assert_eq!(request["id"], id);
        assert_eq!(request["kind"], "open");
        assert_eq!(request["multiple"], false);
        assert_eq!(bridge.poll(id), None, "no answer yet");
        fs::write(
            bridge.root().join("answer.json"),
            format!(r#"{{"id": {id}, "paths": ["/x/inbox/1/a.docx"]}}"#),
        )
        .unwrap();
        assert_eq!(
            bridge.poll(id),
            Some(FileDialogResult::Selected(vec![PathBuf::from("/x/inbox/1/a.docx")]))
        );
        assert!(!bridge.root().join("answer.json").exists(), "the answer is consumed");
    }

    #[test]
    fn a_stale_answer_is_ignored() {
        let (_dir, bridge) = bridge();
        let id = bridge.request(FileDialogMode::Save, &FileDialogOptions::default());
        fs::write(bridge.root().join("answer.json"), r#"{"id": 999, "paths": ["/x"]}"#).unwrap();
        assert_eq!(bridge.poll(id), None);
        assert!(bridge.root().join("answer.json").exists(), "another request's answer stays");
    }

    #[test]
    fn a_cancelled_answer_is_cancelled() {
        let (_dir, bridge) = bridge();
        let id = bridge.request(FileDialogMode::Open { multiple: true }, &FileDialogOptions::default());
        fs::write(bridge.root().join("answer.json"), format!(r#"{{"id": {id}, "cancelled": true}}"#)).unwrap();
        assert_eq!(bridge.poll(id), Some(FileDialogResult::Cancelled));
    }

    #[test]
    fn the_request_carries_the_filters_and_name() {
        let (_dir, bridge) = bridge();
        let options = FileDialogOptions {
            title: "Open".into(),
            file_name: "Report.docx".into(),
            filters: vec![("Documents".into(), vec!["docx".into(), "doc".into()])],
            ..Default::default()
        };
        let id = bridge.request(FileDialogMode::Save, &options);
        let request: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(bridge.root().join("request.json")).unwrap()).unwrap();
        assert_eq!(request["id"], id);
        assert_eq!(request["kind"], "save");
        assert_eq!(request["file_name"], "Report.docx");
        assert_eq!(request["extensions"], serde_json::json!(["docx", "doc"]));
    }
}
