//! Translate the editors' application commands into the shared shell's vocabulary.
//!
//! Classification never performs I/O or grants permission to discard changes. In particular,
//! engine document/window IDs are not host tab IDs, and close requests still need the host's
//! save transaction. Unsupported requests retain their original parameters for diagnostics.

use crate::language::UiLanguage;
use serde_json::Value;
use std::path::PathBuf;
use weave_workspace::DocumentKind;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SettingsPane {
    #[default]
    General,
    Language,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeRequest {
    Light,
    Dark,
    System,
    Toggle,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ScaleRequest {
    Auto,
    /// A percentage, not a zoom multiplier. The shell applies its own supported limits.
    Percent(f32),
}

#[derive(Clone, Debug, PartialEq)]
pub enum HostRequest {
    /// None asks the shell to offer its document kinds; Some requests a blank of that kind.
    New {
        kind: Option<DocumentKind>,
    },
    /// None opens the host picker. Explicit paths must not be replaced by a picker.
    Open {
        path: Option<PathBuf>,
    },
    Save {
        path: Option<PathBuf>,
        save_as: bool,
    },
    ExportPdf {
        path: Option<PathBuf>,
    },
    Print,
    Undo,
    Redo,
    CloseCurrent,
    CloseAll,
    Quit,
    Settings {
        pane: SettingsPane,
    },
    InterfaceLanguage(UiLanguage),
    About,
    NextTab,
    PreviousTab,
    Theme(ThemeRequest),
    InterfaceScale(ScaleRequest),
    /// None toggles the host window; Some sets its fullscreen state.
    FullScreen(Option<bool>),
    /// A request to leave an application surface, not to close a document.
    Dismiss,
    /// Open a web or email address in the system's default browser or mail app.
    OpenUrl(String),
    Unsupported {
        id: String,
        params: Value,
    },
}

/// Resolve exact known command IDs and parameter shapes from the four hosted editors.
/// Malformed targets and unsupported workflows do not degrade to a different action.
pub fn classify(id: &str, params: &Value) -> HostRequest {
    classify_known(id, params).unwrap_or_else(|| HostRequest::Unsupported {
        id: id.to_owned(),
        params: params.clone(),
    })
}

fn classify_known(id: &str, params: &Value) -> Option<HostRequest> {
    use HostRequest as R;
    if !params.is_null() && !params.is_object() {
        return None;
    }
    Some(match id {
        "file.new" => {
            if params.get("sample").is_some() || params.get("theme").is_some() {
                return None;
            }
            // A nonblank Word template needs a dedicated host operation; do not silently
            // create an empty document in its place.
            if params
                .get("template")
                .is_some_and(|v| v.as_str() != Some("blank"))
            {
                return None;
            }
            R::New { kind: None }
        }
        "app.home" | "view.home" => R::New { kind: None },
        "create.blank" => R::New {
            kind: Some(DocumentKind::Pdf),
        },
        "file.open" | "app.openDialog" | "ui.openFileDialog" => {
            // In-memory files/password handling cannot be fulfilled by the path-only shell.
            if params.get("data").is_some() || params.get("password").is_some() {
                return None;
            }
            R::Open {
                path: optional_path(params)?,
            }
        }
        "file.open_recent" => {
            // A PDF editor's recent index belongs to its own history, not the host's history.
            let path = optional_path(params)??;
            R::Open { path: Some(path) }
        }
        "file.save" | "file.saveAs" | "file.save_as" | "app.saveAsDialog" => {
            if ["password", "format", "data"]
                .iter()
                .any(|key| params.get(*key).is_some())
            {
                return None;
            }
            R::Save {
                path: optional_path(params)?,
                save_as: id != "file.save",
            }
        }
        "file.exportPdf" | "file.export_pdf" | "file.printPdf" | "file.share" => {
            if params
                .as_object()
                .is_some_and(|p| p.keys().any(|key| key != "path" && key != "format"))
                || params
                    .get("format")
                    .is_some_and(|format| format.as_str() != Some("pdf"))
            {
                return None;
            }
            R::ExportPdf {
                path: optional_path(params)?,
            }
        }
        "file.export" | "app.exportDialog" => {
            // Deck's export command and dialog can produce several formats. Only an explicit
            // PDF request has the same meaning as the host's PDF-copy action.
            if params.get("format").and_then(Value::as_str) != Some("pdf")
                || params
                    .as_object()
                    .is_some_and(|p| p.keys().any(|key| key != "path" && key != "format"))
            {
                return None;
            }
            R::ExportPdf {
                path: optional_path(params)?,
            }
        }
        "file.print" | "app.print" | "ui.print" | "print.dialog" | "ui.printSettings" => {
            if params.as_object().is_some_and(|p| !p.is_empty()) {
                return None;
            }
            R::Print
        }
        "edit.undo" | "edit.redo" => {
            if params.as_object().is_some_and(|p| !p.is_empty()) {
                return None;
            }
            if id == "edit.undo" { R::Undo } else { R::Redo }
        }
        "file.close" | "file.closeDocument" | "app.closePresentation" | "window.close" => {
            if ["id", "document", "doc", "window", "index"]
                .iter()
                .any(|key| params.get(*key).is_some_and(|v| !v.is_null()))
            {
                return None;
            }
            // Word uses {quit:true} while walking an app-wide close transaction. `force`
            // from an editor never bypasses the host's save/discard decision.
            match params.get("quit") {
                None | Some(Value::Bool(false)) => R::CloseCurrent,
                Some(Value::Bool(true)) => R::Quit,
                _ => return None,
            }
        }
        "file.close_all" => R::CloseAll,
        "app.quit" | "app.exit" | "file.quit" | "file.exit" | "ui.quit" | "ui.exit" => R::Quit,
        "file.options" | "app.preferences" => R::Settings {
            pane: settings_pane(params)?,
        },
        "ui.language" => R::InterfaceLanguage(UiLanguage::parse(params.get("value")?.as_str()?)?),
        "app.language.set" => R::InterfaceLanguage(UiLanguage::parse(
            params
                .get("language")
                .or_else(|| params.get("code"))?
                .as_str()?,
        )?),
        "app.language.english" => R::InterfaceLanguage(UiLanguage::En),
        "app.about" | "help.about" => R::About,
        "system.open_url" => R::OpenUrl(params.get("url")?.as_str()?.to_owned()),
        "view.next_tab" => R::NextTab,
        "view.previous_tab" => R::PreviousTab,
        "view.theme.light" => R::Theme(ThemeRequest::Light),
        "view.theme.dark" => R::Theme(ThemeRequest::Dark),
        "view.theme.system" => R::Theme(ThemeRequest::System),
        "view.theme" => {
            // Grid passes `mode`; PDF's parameterless command toggles light/dark.
            let theme = match params.get("mode").or_else(|| params.get("value")) {
                Some(value) => parse_theme(value)?,
                None => ThemeRequest::Toggle,
            };
            R::Theme(theme)
        }
        "view.interfaceTheme" => {
            // Word also forwards engine requests in their original {interfaceTheme} form.
            let theme = match params.get("value").or_else(|| params.get("interfaceTheme")) {
                Some(value) => parse_theme(value)?,
                None if params
                    .get("toggle")
                    .is_none_or(|v| v.as_str() == Some("interfaceTheme")) =>
                {
                    ThemeRequest::Toggle
                }
                None => return None,
            };
            R::Theme(theme)
        }
        "ui.theme" => R::Theme(parse_theme(params.get("value")?)?),
        // Word's document-page darkMode is not forwarded. The forwarded view.darkMode is
        // Grid's application appearance; Deck uses view.dark, and Word uses ui.dark.
        "view.darkMode" | "view.dark" | "ui.dark" => R::Theme(match optional_bool(params)? {
            Some(true) => ThemeRequest::Dark,
            Some(false) => ThemeRequest::Light,
            None => ThemeRequest::Toggle,
        }),
        "ui.interfaceSize" | "view.interfaceScale" => R::InterfaceScale(parse_scale(
            params.get("percent").or_else(|| params.get("value"))?,
        )?),
        "view.full_screen" | "view.fullScreen" | "view.fullScreenView" | "view.focus" => {
            R::FullScreen(optional_bool(params)?)
        }
        "ui.tab" if params.get("tab").and_then(Value::as_str) == Some("File") => {
            R::New { kind: None }
        }
        "ui.dialog" => match params.get("name").and_then(Value::as_str)? {
            "options" | "preferences" => R::Settings {
                pane: settings_pane(params)?,
            },
            "about" => R::About,
            "backstage" => backstage(params)?,
            // Ribbon/keyboard customization is not the host's current settings workflow.
            _ => return None,
        },
        "ui.backstage" | "app.backstage" => backstage(params)?,
        "ui.fileBrowser" => {
            let purpose = match params.get("purpose") {
                None => "document",
                Some(value) => value.as_str()?,
            };
            match purpose {
                "document" => R::Open {
                    path: optional_path(params)?,
                },
                "save" => R::Save {
                    path: optional_path(params)?,
                    save_as: true,
                },
                _ => return None,
            }
        }
        // Window creation/arrangement, updates, printing, export, recovery, file conversions
        // and templates require their own implementation. Never guess from an ID substring.
        _ => return None,
    })
}

fn settings_pane(params: &Value) -> Option<SettingsPane> {
    match params.get("pane") {
        None => Some(SettingsPane::General),
        Some(Value::String(pane)) if pane == "general" => Some(SettingsPane::General),
        Some(Value::String(pane)) if pane == "language" => Some(SettingsPane::Language),
        _ => None,
    }
}

fn backstage(params: &Value) -> Option<HostRequest> {
    use HostRequest as R;
    if optional_bool(params)? == Some(false) {
        return Some(R::Dismiss);
    }
    let page = match params.get("page") {
        None => "home",
        Some(value) => value.as_str()?.trim(),
    };
    Some(match page {
        "home" | "new" => R::New { kind: None },
        "open" => R::Open {
            path: optional_path(params)?,
        },
        "save" => R::Save {
            path: optional_path(params)?,
            save_as: false,
        },
        "saveAs" | "save_as" => R::Save {
            path: optional_path(params)?,
            save_as: true,
        },
        "export" | "share" => R::ExportPdf {
            path: optional_path(params)?,
        },
        "print" => R::Print,
        "options" | "preferences" | "account" => R::Settings {
            pane: SettingsPane::General,
        },
        "about" => R::About,
        "close" | "none" | "" => R::Dismiss,
        _ => return None,
    })
}

/// Outer None means invalid; inner None means the caller must ask for a path.
fn optional_path(params: &Value) -> Option<Option<PathBuf>> {
    match params.get("path") {
        None | Some(Value::Null) => Some(None),
        Some(Value::String(path)) if !path.trim().is_empty() && !path.contains('\0') => {
            Some(Some(PathBuf::from(path)))
        }
        _ => None,
    }
}

fn optional_bool(params: &Value) -> Option<Option<bool>> {
    let mut result = None;
    for key in ["value", "on", "enabled"] {
        if let Some(value) = params.get(key) {
            let value = value.as_bool()?;
            if result.is_some_and(|previous| previous != value) {
                return None;
            }
            result = Some(value);
        }
    }
    Some(result)
}

fn parse_theme(value: &Value) -> Option<ThemeRequest> {
    match value.as_str()? {
        "light" => Some(ThemeRequest::Light),
        "dark" => Some(ThemeRequest::Dark),
        "system" => Some(ThemeRequest::System),
        _ => None,
    }
}

fn parse_scale(value: &Value) -> Option<ScaleRequest> {
    let percent = match value {
        Value::String(text) if text.trim().eq_ignore_ascii_case("auto") => {
            return Some(ScaleRequest::Auto);
        }
        Value::String(text) => text
            .trim()
            .trim_end_matches('%')
            .trim()
            .parse::<f64>()
            .ok()?,
        _ => value.as_f64()?,
    };
    if !percent.is_finite() || percent <= 0.0 || percent > f32::MAX as f64 {
        return None;
    }
    Some(ScaleRequest::Percent(percent as f32))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn open_url_requests_carry_their_address() {
        assert_eq!(
            classify("system.open_url", &json!({"url":"https://example.com"})),
            HostRequest::OpenUrl("https://example.com".into())
        );
        assert!(matches!(
            classify("system.open_url", &json!({})),
            HostRequest::Unsupported { .. }
        ));
    }

    #[test]
    fn shared_language_requests_preserve_panes_and_reject_unsupported_languages() {
        for id in ["file.options", "app.preferences", "ui.dialog"] {
            assert_eq!(
                classify(id, &json!({"name":"options", "pane":"language"})),
                HostRequest::Settings {
                    pane: SettingsPane::Language
                }
            );
            for pane in [json!("proofing"), json!(false), json!(null)] {
                assert!(matches!(
                    classify(id, &json!({"name":"options", "pane":pane})),
                    HostRequest::Unsupported { .. }
                ));
            }
        }
        assert_eq!(
            classify("ui.language", &json!({"value":"en"})),
            HostRequest::InterfaceLanguage(UiLanguage::En)
        );
        assert_eq!(
            classify("app.language.set", &json!({"language":"zh"})),
            HostRequest::InterfaceLanguage(UiLanguage::ZhHans)
        );
        assert_eq!(
            classify("app.language.set", &json!({"code":"en"})),
            HostRequest::InterfaceLanguage(UiLanguage::En)
        );
        for value in [
            json!("fr"),
            json!("zh-hant"),
            json!("auto"),
            json!(12),
            json!(null),
        ] {
            assert!(matches!(
                classify("ui.language", &json!({"value":value})),
                HostRequest::Unsupported { .. }
            ));
        }
        assert!(matches!(
            classify("review.language", &json!({"lang":"fr-FR"})),
            HostRequest::Unsupported { .. }
        ));
    }

    #[test]
    fn close_all_does_not_quit_and_engine_targets_are_not_host_tabs() {
        assert_eq!(
            classify("file.close_all", &json!({})),
            HostRequest::CloseAll
        );
        assert_eq!(classify("app.quit", &json!({})), HostRequest::Quit);
        assert_eq!(
            classify("file.close", &json!({"force":true})),
            HostRequest::CloseCurrent
        );
        assert_eq!(
            classify("file.close", &json!({"quit":true})),
            HostRequest::Quit
        );
        for id in ["view.newWindow", "window.activate", "view.arrangeAll"] {
            assert!(matches!(
                classify(id, &json!({})),
                HostRequest::Unsupported { .. }
            ));
        }
        assert!(matches!(
            classify("file.closeDocument", &json!({"id":7})),
            HostRequest::Unsupported { .. }
        ));
    }

    #[test]
    fn explicit_paths_and_save_modes_survive_translation() {
        let path = PathBuf::from("/tmp/报告.docx");
        assert_eq!(
            classify("file.open", &json!({"path":path})),
            HostRequest::Open {
                path: Some(path.clone())
            }
        );
        assert_eq!(
            classify("file.save", &json!({"path":path})),
            HostRequest::Save {
                path: Some(path.clone()),
                save_as: false
            }
        );
        assert_eq!(
            classify("file.save_as", &json!({"path":path})),
            HostRequest::Save {
                path: Some(path),
                save_as: true
            }
        );
        assert_eq!(
            classify("app.openDialog", &Value::Null),
            HostRequest::Open { path: None }
        );
        assert!(matches!(
            classify("file.open_recent", &json!({"index":0})),
            HostRequest::Unsupported { .. }
        ));
        assert!(matches!(
            classify("file.open", &json!({"path":42})),
            HostRequest::Unsupported { .. }
        ));
    }

    #[test]
    fn editor_theme_and_scale_parameter_conventions_are_resolved() {
        for (id, params, expected) in [
            ("view.theme", json!({"mode":"system"}), ThemeRequest::System),
            (
                "view.interfaceTheme",
                json!({"interfaceTheme":"dark"}),
                ThemeRequest::Dark,
            ),
            (
                "view.interfaceTheme",
                json!({"toggle":"interfaceTheme"}),
                ThemeRequest::Toggle,
            ),
            ("view.dark", json!({"on":false}), ThemeRequest::Light),
            ("ui.dark", json!({"value":true}), ThemeRequest::Dark),
            ("view.theme", json!({}), ThemeRequest::Toggle),
            ("view.theme.light", json!({}), ThemeRequest::Light),
        ] {
            assert_eq!(classify(id, &params), HostRequest::Theme(expected));
        }
        assert_eq!(
            classify("view.interfaceScale", &json!({"percent":125})),
            HostRequest::InterfaceScale(ScaleRequest::Percent(125.0))
        );
        assert_eq!(
            classify("ui.interfaceSize", &json!({"value":"150%"})),
            HostRequest::InterfaceScale(ScaleRequest::Percent(150.0))
        );
        assert_eq!(
            classify("ui.interfaceSize", &json!({"percent":"auto"})),
            HostRequest::InterfaceScale(ScaleRequest::Auto)
        );
        for value in [json!(0), json!(-1), json!("NaN"), json!("inf"), json!(true)] {
            assert!(matches!(
                classify("ui.interfaceSize", &json!({"value":value})),
                HostRequest::Unsupported { .. }
            ));
        }
    }

    #[test]
    fn backstage_and_dialog_parameters_do_not_create_unrelated_documents() {
        assert_eq!(
            classify("ui.tab", &json!({"tab":"File"})),
            HostRequest::New { kind: None }
        );
        assert_eq!(
            classify("ui.dialog", &json!({"name":"options"})),
            HostRequest::Settings {
                pane: SettingsPane::General
            }
        );
        assert_eq!(
            classify("app.backstage", &json!({"on":false})),
            HostRequest::Dismiss
        );
        assert_eq!(
            classify("ui.backstage", &json!({"page":"close"})),
            HostRequest::Dismiss
        );
        assert_eq!(
            classify("create.blank", &json!({})),
            HostRequest::New {
                kind: Some(DocumentKind::Pdf)
            }
        );
        assert_eq!(
            classify("ui.backstage", &json!({"page":"export"})),
            HostRequest::ExportPdf { path: None }
        );
        assert_eq!(
            classify("file.exportPdf", &json!({"path":"/tmp/copy.pdf"})),
            HostRequest::ExportPdf {
                path: Some(PathBuf::from("/tmp/copy.pdf"))
            }
        );
        assert_eq!(
            classify(
                "file.export",
                &json!({"path":"/tmp/deck.pdf","format":"pdf"})
            ),
            HostRequest::ExportPdf {
                path: Some(PathBuf::from("/tmp/deck.pdf"))
            }
        );
        assert_eq!(
            classify("app.exportDialog", &json!({"format":"pdf"})),
            HostRequest::ExportPdf { path: None }
        );
        assert_eq!(classify("print.dialog", &json!({})), HostRequest::Print);
        for (id, params) in [
            ("ui.tab", json!({"tab":"Home"})),
            ("file.exportPdf", json!({"format":"docx"})),
            ("file.export", json!({"path":"/tmp/deck.pdf"})),
            (
                "file.export",
                json!({"format":"png","path":"/tmp/deck.png"}),
            ),
            ("file.export", json!({"format":"pdf","slides":[0]})),
            ("app.exportDialog", json!({})),
            ("file.exportCsv", json!({"path":"/tmp/data.csv"})),
            ("file.exportHtml", json!({"path":"/tmp/data.html"})),
            ("file.saveTemplate", json!({"path":"/tmp/base.potx"})),
            ("file.print", json!({"copies":3})),
            ("ui.dialog", json!({"name":"customizeRibbon"})),
            ("create.images", json!({})),
            ("file.new", json!({"template":"resume"})),
            ("file.new", json!({"sample":"budget"})),
            ("file.new", json!({"theme":"office"})),
            ("view.theme.unknown", json!({})),
        ] {
            assert_eq!(
                classify(id, &params),
                HostRequest::Unsupported {
                    id: id.into(),
                    params
                }
            );
        }
    }

    #[test]
    fn tab_navigation_and_fullscreen_have_explicit_semantics() {
        assert_eq!(classify("view.next_tab", &json!({})), HostRequest::NextTab);
        assert_eq!(
            classify("view.previous_tab", &json!({})),
            HostRequest::PreviousTab
        );
        assert_eq!(
            classify("view.full_screen", &json!({})),
            HostRequest::FullScreen(None)
        );
        assert_eq!(
            classify("view.fullScreen", &json!({"value":false})),
            HostRequest::FullScreen(Some(false))
        );
        assert_eq!(
            classify("view.full_screen", &json!({"enabled":true})),
            HostRequest::FullScreen(Some(true))
        );
        assert!(matches!(
            classify("view.fullScreen", &json!({"value":true,"on":false})),
            HostRequest::Unsupported { .. }
        ));
    }
}
