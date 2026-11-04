<p align="center">
  <b>English</b> · <a href="README.zh-CN.md">简体中文</a>
</p>

<p align="center">
  <img src="assets/social/social-preview.en.png" alt="Weave Office: one app for every document, built for AI agents" width="100%">
</p>

<h1 align="center">Weave Office</h1>

<p align="center">
  <b>One app for every document. Built for AI agents.</b><br>
  An open-source desktop office suite written in Rust: documents, spreadsheets, slides, PDF, Markdown, code and rich text,<br>
  opened, edited and saved in one row of file tabs. The same binary is a command line that AI agents call directly.
</p>

<p align="center">
  <img alt="Rust 1.95" src="https://img.shields.io/badge/Rust-1.95-orange?logo=rust">
  <img alt="Platform: macOS" src="https://img.shields.io/badge/platform-macOS-lightgrey?logo=apple">
  <img alt="UI: egui" src="https://img.shields.io/badge/UI-egui-4b8bbe">
  <img alt="AI-friendly CLI" src="https://img.shields.io/badge/AI--friendly-CLI%20%2B%20JSON-7c5cff">
  <a href="LICENSE"><img alt="License: MIT" src="https://img.shields.io/badge/license-MIT-blue"></a>
  <img alt="Status: in development" src="https://img.shields.io/badge/status-in%20development-yellow">
</p>

<p align="center">
  <a href="#features">Features</a> ·
  <a href="#command-line-for-ai-agents-and-scripts">Command line</a> ·
  <a href="#build-from-source">Build</a> ·
  <a href="#project-layout">Layout</a> ·
  <a href="#documentation">Docs</a>
</p>

> [!NOTE]
> Weave Office is under active development and is not yet a day-to-day replacement for a mature office suite. Fidelity on complex documents, input methods and some advanced workflows are still being polished. See the [acceptance log](docs/acceptance.md) (Chinese) for current status and known issues.

## Android

An **unofficial community port** runs Weave Office on Android tablets and phones: the same
workspace (documents, spreadsheets, presentations, PDF, Markdown, code) in an arm64-v8a APK for
Android 8.0 (API 26) and newer. It is a prototype, not made or supported by the Weave Office
developers.

It adds what a tablet needs: Android's document pickers for opening and saving files, "Open with"
and "Share", the soft keyboard, the system clipboard, a Shortcut Pad of touch keys and a wheel,
two-finger scrolling, a phone layout, printing through Android's print service, and a self-updater
from GitHub releases.

Build it with `apps/weave-office-android/build.sh`; [docs/android/](docs/android/) has the
details.

## Why Weave Office

Two things set it apart: it is **all-in-one**, and it is **AI-friendly**.

Most of us bounce between a word processor, a spreadsheet, a slide tool, a PDF reader, a Markdown notes app and a code editor every day, each with its own window, menus, settings and save logic. Weave Office puts them into **one real application**:

- **One row of file tabs.** A contract, a quote, a pitch deck, a PDF invoice, your notes and a config file sit side by side, and you can open several of the same kind. Switch away and back, and the caret, selection, scroll position, zoom, undo history and unsaved changes are all still there.
- **One set of shared systems.** New, open, save, save as, close, quit, recent files, autosave, printing and dialogs are handled by one host, not by four apps each doing it their own way.
- **AI can call it directly.** The app binary is also a command line. An AI agent can read, change and export documents by path without opening a window, and gets JSON back (see [Command line](#command-line-for-ai-agents-and-scripts)).
- **One shared foundation.** Fonts, text shaping, rasterization, PDF output and metafile parsing exist once, so the download is smaller and the editors behave consistently.

## Features

### Formats at a glance

| Kind | Opens | New files saved as | Editor |
|---|---|---|---|
| Documents | `.docx` `.docm` `.doc` `.odt` `.rtf` | `.docx` | WordCraft engine |
| Spreadsheets | `.xlsx` `.xlsm` `.xls` `.ods` `.csv` | `.xlsx` | GridCraft engine |
| Presentations | `.pptx` `.pptm` `.ppt` `.odp` | `.pptx` | DeckCraft engine |
| PDF | `.pdf` | `.pdf` | PdfCraft engine |
| Markdown | `.md` `.markdown` `.mdx` and more | `.md` | Built in (`crates/text-editor`) |
| Plain text and code | 20+ languages, plus unknown extensions whose content is text | `.txt` `.json` `.yaml` `.toml` `.xml` `.sql` `.sh` `.py` `.js` `.ts` `.css` `.go` `.rs` | Built in (`crates/text-editor`) |
| Rich text (HTML) | `.html` `.htm` `.xhtml` | `.html` | Built in (`crates/rich-editor`), in development |

The **＋** button on the tab bar opens the New panel, grouped into Office documents (documents, presentations, spreadsheets, PDF), Text & web (Markdown, plain text, rich text) and Code & data (JSON, YAML, TOML, XML, SQL, Shell, Python, JavaScript, TypeScript, CSS, Go, Rust). Every format in it can be edited and saved back in the same format.

### Four office editors in one: documents, spreadsheets, slides, PDF

- **Documents:** paragraph and character formatting, styles, tables, images, headers and footers, page breaks; WMF/EMF pictures; PDF export and printing.
- **Spreadsheets:** multiple sheets, formulas, number formats, borders and merged cells; mixed Latin and CJK text aligned on a shared baseline; PDF export (with embedded CJK fonts) and CSV.
- **Presentations:** slides, shapes, text boxes, images and layouts; slideshow and export.
- **PDF:** a real editor, not just a viewer. Edit existing text and images, add text and images, annotate (notes, text boxes, markup, shapes and replies), fill in text, and insert, delete, reorder, rotate, crop, extract and split pages. See [PDF capabilities](docs/pdf-capabilities.md) (Chinese).

All four run in a single process and share one host:

- **Unified ribbon:** the host draws the ribbon's tab row for every editor; each editor draws only its own commands, so switching file types feels the same.
- **Unified fonts:** open fonts ship once and system fonts are scanned once; all editors share one set of UI fonts, so switching tabs no longer reloads fonts.
- **Autosave and crash recovery:** the host periodically writes unsaved changes to a recovery folder; after an unexpected quit, relaunch and get them back.
- **Unified files and dialogs:** open, save and folder pickers use the native OS panels; confirmations, errors, settings and printing are Weave's own. While a dialog is open, tabs can't be switched or closed by accident, and files opened from outside wait in a queue.
- **Native on macOS:** real traffic-light window buttons, rounded corners and shadow, with file tabs placed in the title bar; app menu, shortcuts, opening from Finder and the system quit flow are wired in.

### Markdown with Obsidian-style live preview

- **Live preview:** headings, bold, links and more render in place; markers such as `#`, `**` and `[]()` appear only on the line you are editing. Source and reading views too.
- **Notes features:** task checkboxes, `[[wikilinks]]`, backlinks, tags and outline.
- **Sidebar:** file tree, full-text search, outline, tags and backlinks.
- **Command palette** (⇧⌘P) and **quick open** (⌥⌘O).
- **Insert tools:** images, footnotes, callouts, math and properties (front matter).
- **Focus mode** and **typewriter scrolling**.
- PDF export and printing go through the document engine's Markdown import.

### Code and plain-text editor

- **Syntax highlighting:** Rust, JavaScript, TypeScript, Python, Go, C/C++, C#, Java, Kotlin, Swift, PHP, Ruby, Lua, Shell, PowerShell, SQL, Dockerfile, Makefile, JSON, YAML, TOML, INI, HTML, CSS and more.
- **Editing tools:** line numbers, find and replace, search in folder, go to line, line commands (move, duplicate, delete, join), sort, de-duplicate, change case, trim trailing whitespace, convert indentation, and **JSON formatting**.
- **Keeps your encoding:** files are written back in their original encoding (including GBK, UTF-16 and BOMs) and line endings; a file you didn't change is saved byte for byte. Files with an unknown extension but text content open as plain text.

### Rich-text editor (in development)

A WYSIWYG HTML editor for laying out articles, in the spirit of WeChat article editors such as 135 Editor:

- **Open any HTML file** and switch between the **design view** (the laid-out page) and the **source view** (highlighted HTML). Both always agree.
- **Edit in the design view:** click to place the caret, drag or Shift-click to select, double-click for a word, triple-click for a paragraph, with input-method support. Copy carries HTML and plain text; paste keeps inline styles and drops scripts.
- **Home tab:** paragraph style, size, bold, italic, underline, strikethrough, text and highlight colors, clear formatting, alignment, line spacing and first-line indent.
- **Faithful:** unknown tags, comments, `<head>`, `<script>` and `<style>` are kept as is; an untouched file is written back unchanged; undo, saving and the source view all work on the same text.
- Its own HTML parser and CSS-subset layout, with no new third-party dependencies.
- Roadmap: component library, one-click recoloring and images → copy to WeChat → polish.

### Command line for AI agents and scripts

**This is the second pillar of Weave Office: it is AI-friendly.** The same executable is a command line. When the first argument is a verb, no window opens; it reads and writes documents directly on the four editors' engines. It is designed so AI agents can operate on office files reliably.

```sh
weave-office create quote.xlsx
weave-office set quote.xlsx /Sheet1/B2 value=1280
weave-office add contract.docx /body --type paragraph text="Both parties agree…"
weave-office view pitch.pptx outline
weave-office export contract.docx contract.pdf
weave-office help agent      # prints the full guide written for AI agents
```

- Verbs: `create` `info` `view` `get` `set` `add` `remove` `commands` `run` `batch` `export` `help`.
- Address content by path: `/body/p[3]`, `/Sheet1/A1:C10`, `/slide[2]/shape[1]`, `/page[4]`.
- `--json` returns one envelope, `{"ok":…,"data"|"error":…}`; errors carry a code, a suggested fix and the closest matching commands.
- `batch` writes only if every step succeeds. Saves go to a temporary file that is then renamed over the original, so a file is either the complete new version or untouched.

Full reference: [docs/cli.md](docs/cli.md) (Chinese); the agent guide is [crates/cli/AGENT.md](crates/cli/AGENT.md).

## Build from source

You need Rust 1.95.0 (pinned in `rust-toolchain.toml`) and Python 3. Large build inputs such as fonts and translations live on an external volume; point `WEAVE_DEV_VOLUME` at its mount point:

```sh
export WEAVE_DEV_VOLUME=/Volumes/MyDrive

python3 scripts/prepare-fonts.py
python3 scripts/prepare-document-fonts.py
python3 scripts/prepare-i18n.py
python3 scripts/cargo.py run --locked --offline
```

Build output goes to `$WEAVE_DEV_VOLUME/DevCache/WeaveOffice/target`. To try just the command line, no font preparation is needed:

```sh
cargo run -p weave-cli --example weave -- help
cargo test -p weave-cli
```

For a macOS `.app` see the [packaging notes](packaging/README.md); for font inputs see [fonts](docs/fonts.md).

## Project layout

```
src/                  Host: window, file tabs, ribbon, dialogs, autosave and recovery, editor adapters
crates/
  workspace/          File kinds, tabs, dirty state and save ordering
  ribbon/             The unified ribbon drawn by the host
  font-assets/        Shared open fonts
  system-fonts/       One system font scan for every editor
  pdf-backend/        PDF engine adapter
  text-editor/        Markdown and code editor
  rich-editor/        Rich-text (HTML) editor
  cli/                The command line, weave-office <verb>
vendor/               Pinned snapshots of the four upstream editors
integration/          Early integration experiments, kept for reference
packaging/ scripts/   Packaging and build scripts
tests/                Host regression tests
```

## Documentation

Most project docs are currently in Chinese.

- [Product goals and plan](PLAN.md)
- [Acceptance log](docs/acceptance.md): automated tests, hands-on checks and remaining work
- [Command line](docs/cli.md) · [PDF capabilities](docs/pdf-capabilities.md) · [Fonts](docs/fonts.md) · [UI languages](docs/i18n.md)
- [Upstream projects, pinned versions and hand-off notes](vendor/README.md)
- [App icon](assets/app-icon/README.md) · [Packaging](packaging/README.md)

## Acknowledgements and license

Weave Office is released under the [MIT License](LICENSE).

The four office editors are built on [WordCraft](https://github.com/storytold/wordcraft), [GridCraft](https://github.com/storytold/gridcraft), [DeckCraft](https://github.com/storytold/deckcraft) and [PdfCraft](https://github.com/storytold/pdfcraft), vendored as pinned snapshots under `vendor/`. The upstream source is licensed MIT OR Apache-2.0, and each upstream's LICENSE, NOTICE and asset attributions are kept in its directory; Weave Office uses its own name and icon. The command line's design draws on ideas from [OfficeCLI](https://github.com/iOfficeAI/OfficeCLI) and contains none of its code.

Thanks to the [LINUX DO](https://linux.do) community for its support and feedback.
