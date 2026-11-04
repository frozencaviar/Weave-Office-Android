//! One catalog of the installed fonts for the whole application.
//!
//! Each editor used to walk the platform's font folders on its own background thread and open
//! every font file it found, so a start-up read the same files three times (one editor read each
//! file whole). This crate walks the folders once per process and keeps, for each face, a small
//! font holding only the tables the editors' catalogs read: `name` (family, style and
//! localized names, without long texts such as the license), `OS/2` (code pages) and `fvar`
//! (named instances). Each editor parses those small fonts with its own naming rules, so the
//! families it lists are what its own scan found, while the disk is read once.
//!
//! No font database, fallback policy or parser lives here: the crate only reads bytes.
#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError, RwLock};

/// Faces a collection may list (a damaged header can claim any number).
const MAX_FACES: u32 = 256;
/// The largest table copied from a file.
const MAX_TABLE: u32 = 1 << 20;
/// Folder depth the walk follows below each font folder.
const MAX_SCAN_DEPTH: usize = 12;
/// Directory entries the walk looks at in all: a link to `/` mustn't become a disk crawl.
const MAX_SCAN_ENTRIES: usize = 200_000;
/// The largest font file cataloged (the biggest CJK collections are a few hundred MB).
pub const MAX_FONT_FILE: u64 = 512 << 20;

/// Glyph outline tables: a face with none of them has nothing to draw with an outline.
pub const OUTLINE_TABLES: [&[u8; 4]; 4] = [b"glyf", b"CFF ", b"CFF2", b"VARC"];
/// Color bitmap tables (Apple Color Emoji, Noto Color Emoji).
pub const COLOR_BITMAP_TABLES: [&[u8; 4]; 2] = [b"sbix", b"CBDT"];

/// The `name` records the editors' catalogs read: family, style, full and PostScript names
/// (1, 2, 4, 6), typographic family and style (16, 17), WWS names (21, 22), the variations
/// PostScript prefix (25), and the font-specific ids (256 and up) a variable font's named
/// instances use for their style names. Copyright, license and description texts are dropped.
fn kept_name(id: u16) -> bool {
    matches!(id, 1 | 2 | 4 | 6 | 16 | 17 | 21 | 22 | 25) || id >= 256
}

/// One face of an installed font file.
#[derive(Clone, Debug)]
pub struct Face {
    /// The font file.
    pub path: PathBuf,
    /// The file's size in bytes.
    pub file_len: u64,
    /// The face's index within the file (0 unless it is a collection).
    pub index: u32,
    /// The table tags of the face, in its table directory's order.
    pub tags: Vec<[u8; 4]>,
    /// A font holding just the face's `name`, `OS/2` and `fvar` tables (those it has), readable
    /// by any OpenType parser as face 0.
    pub tables: Arc<[u8]>,
}

impl Face {
    /// Whether the face has a table tagged `tag`.
    pub fn has_table(&self, tag: &[u8; 4]) -> bool {
        self.tags.iter().any(|t| t == tag)
    }

    /// Whether the face has glyph outlines ([`OUTLINE_TABLES`]).
    pub fn has_outlines(&self) -> bool {
        OUTLINE_TABLES.iter().any(|t| self.has_table(t))
    }

    /// Whether the face has color bitmaps ([`COLOR_BITMAP_TABLES`]).
    pub fn has_color_bitmaps(&self) -> bool {
        COLOR_BITMAP_TABLES.iter().any(|t| self.has_table(t))
    }
}

/// The installed fonts as one scan found them.
#[derive(Debug, Default)]
pub struct Catalog {
    /// The folders the scan walked.
    pub dirs: Vec<PathBuf>,
    /// Every face found, files in walk order and faces in file order.
    pub faces: Vec<Face>,
    /// Counts the scans of this process: a later scan has a larger generation.
    pub generation: u64,
}

impl Catalog {
    /// The faces grouped by file, in walk order.
    pub fn files(&self) -> impl Iterator<Item = (&Path, &[Face])> {
        self.faces.chunk_by(|a, b| a.path == b.path).map(|run| (run[0].path.as_path(), run))
    }
}

/// The shared catalog and a lock that lets one scan run at a time.
struct Shared {
    current: RwLock<Option<Arc<Catalog>>>,
    scanning: Mutex<u64>,
}

fn shared() -> &'static Shared {
    static SHARED: OnceLock<Shared> = OnceLock::new();
    SHARED.get_or_init(|| Shared { current: RwLock::new(None), scanning: Mutex::new(0) })
}

/// The installed fonts, scanned on first use (a caller arriving during the scan waits for it).
pub fn catalog() -> Arc<Catalog> {
    if let Some(done) = scanned() {
        return done;
    }
    let s = shared();
    let mut generation = s.scanning.lock().unwrap_or_else(PoisonError::into_inner);
    // Another caller may have finished the scan while this one waited.
    if let Some(done) = scanned() {
        return done;
    }
    scan_locked(s, &mut generation)
}

/// Scan the font folders again (fonts installed or removed since) and return the new catalog.
/// A rescan another caller started while this one waited counts: its result is returned.
pub fn rescan() -> Arc<Catalog> {
    let s = shared();
    let seen = scanned().map_or(0, |c| c.generation);
    let mut generation = s.scanning.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(done) = scanned().filter(|c| c.generation > seen) {
        return done;
    }
    scan_locked(s, &mut generation)
}

fn scan_locked(s: &Shared, generation: &mut u64) -> Arc<Catalog> {
    *generation += 1;
    let dirs = font_dirs();
    let faces = scan_dirs(&dirs);
    let next = Arc::new(Catalog { dirs, faces, generation: *generation });
    *s.current.write().unwrap_or_else(PoisonError::into_inner) = Some(next.clone());
    next
}

/// The catalog if a scan has finished, else `None`: never blocks on the disk.
pub fn scanned() -> Option<Arc<Catalog>> {
    shared().current.read().unwrap_or_else(PoisonError::into_inner).clone()
}

/// Start the first scan on a background thread unless one has run or is running. A failed
/// spawn leaves the scan to the first [`catalog`] call.
pub fn start_scan() {
    static STARTED: OnceLock<()> = OnceLock::new();
    STARTED.get_or_init(|| {
        if cfg!(not(target_arch = "wasm32")) && scanned().is_none() {
            let _ = std::thread::Builder::new().name("font-scan".into()).spawn(|| {
                catalog();
            });
        }
    });
}

/// The platform's font folders, the system's and the user's: every folder any editor looked in.
/// Empty on wasm.
pub fn font_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if cfg!(target_arch = "wasm32") {
        return dirs;
    }
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from);
    if cfg!(target_os = "macos") {
        dirs.extend(["/System/Library/Fonts", "/System/Library/Fonts/Supplemental", "/Library/Fonts"].map(Into::into));
        if let Some(h) = &home {
            dirs.push(h.join("Library/Fonts"));
            // Fonts Microsoft Office downloads on demand (Aptos…), used where Office put them.
            dirs.push(h.join("Library/Group Containers/UBF8T346G9.Office/FontCache/4/CloudFonts"));
        }
        // Fonts Office installs inside its apps.
        for app in ["Microsoft PowerPoint", "Microsoft Word", "Microsoft Excel"] {
            dirs.push(PathBuf::from(format!("/Applications/{app}.app/Contents/Resources/DFonts")));
        }
        // Fonts macOS ships as assets (PingFang, Yu Mincho, …): com_apple_MobileAsset_Font<N>
        // (`Assets` before macOS 11, `AssetsV2` since).
        for parent in ["/System/Library/AssetsV2", "/System/Library/AssetsV2/PreinstalledAssetsV2/InstallWithOs", "/System/Library/Assets"] {
            let Ok(entries) = std::fs::read_dir(parent) else { continue };
            for e in entries.flatten() {
                if e.file_name().to_string_lossy().starts_with("com_apple_MobileAsset_Font") {
                    dirs.push(e.path());
                }
            }
        }
    } else if cfg!(windows) {
        let root = std::env::var_os("WINDIR").map(PathBuf::from).unwrap_or_else(|| "C:\\Windows".into());
        dirs.push(root.join("Fonts"));
        if let Some(l) = std::env::var_os("LOCALAPPDATA") {
            let l = PathBuf::from(l);
            dirs.push(l.join("Microsoft\\Windows\\Fonts"));
            // Fonts Microsoft Office downloads on demand (Aptos…).
            dirs.push(l.join("Microsoft\\FontCache\\4\\CloudFonts"));
        }
    } else if cfg!(target_os = "android") {
        // Android keeps every system face in /system/fonts; the product and vendor partitions
        // add their own (OEM faces, and /product/fonts on newer releases).
        dirs.extend(["/system/fonts", "/product/fonts", "/system_ext/fonts", "/vendor/fonts"].map(Into::into));
    } else {
        dirs.extend(["/usr/share/fonts", "/usr/local/share/fonts"].map(Into::into));
        if let Some(h) = &home {
            dirs.push(h.join(".fonts"));
            dirs.push(h.join(".local/share/fonts"));
        }
        let env = |k: &str| std::env::var_os(k).map(|v| v.to_string_lossy().into_owned());
        dirs.extend(xdg_font_dirs(env("XDG_DATA_HOME").as_deref(), env("XDG_DATA_DIRS").as_deref(), home.as_deref()));
        // Inside Flatpak the host's system, local and user fonts are mounted here (after
        // 1.19.2 also ~/.fonts, at user-fonts-legacy when the XDG user font folder exists too).
        dirs.extend(["/run/host/fonts", "/run/host/local-fonts", "/run/host/user-fonts", "/run/host/user-fonts-legacy"].map(Into::into));
        // Folders fontconfig is configured with (NixOS, Guix, custom setups).
        for conf in fontconfig_files() {
            if let Ok(text) = std::fs::read_to_string(&conf) {
                dirs.extend(fontconfig_dirs(&text, env("XDG_DATA_HOME").as_deref(), home.as_deref()));
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    dirs.retain(|d| seen.insert(d.clone()));
    dirs
}

/// `<data dir>/fonts` for `XDG_DATA_HOME` (default `~/.local/share`) and each `XDG_DATA_DIRS`
/// entry (default `/usr/local/share:/usr/share`).
fn xdg_font_dirs(data_home: Option<&str>, data_dirs: Option<&str>, home: Option<&Path>) -> Vec<PathBuf> {
    let mut v = vec![];
    match data_home.filter(|d| !d.is_empty()) {
        Some(d) => v.push(Path::new(d).join("fonts")),
        None => {
            if let Some(h) = home {
                v.push(h.join(".local/share/fonts"));
            }
        }
    }
    let dirs = data_dirs.filter(|d| !d.is_empty()).unwrap_or("/usr/local/share:/usr/share");
    v.extend(dirs.split(':').filter(|d| d.starts_with('/')).take(64).map(|d| Path::new(d).join("fonts")));
    v
}

/// The fontconfig configuration files: `/etc/fonts/fonts.conf`, `local.conf` and `conf.d/*.conf`.
fn fontconfig_files() -> Vec<PathBuf> {
    let mut v = vec![PathBuf::from("/etc/fonts/fonts.conf"), PathBuf::from("/etc/fonts/local.conf")];
    if let Ok(entries) = std::fs::read_dir("/etc/fonts/conf.d") {
        let mut files: Vec<_> = entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "conf")).collect();
        files.sort();
        files.truncate(512);
        v.extend(files);
    }
    v
}

/// The `<dir>` folders of a fontconfig file (`prefix="xdg"` is under `XDG_DATA_HOME`, a leading
/// `~` under home; relative entries are skipped).
fn fontconfig_dirs(text: &str, data_home: Option<&str>, home: Option<&Path>) -> Vec<PathBuf> {
    let mut v = vec![];
    let mut rest = text;
    while let Some(start) = rest.find("<dir") {
        let after = &rest[start + 4..];
        // `<dir>` or `<dir …>`, not `<dirname>`.
        if !(after.starts_with('>') || after.starts_with(' ')) {
            rest = after;
            continue;
        }
        let Some(gt) = after.find('>') else { break };
        let attrs = &after[..gt];
        let body = &after[gt + 1..];
        let Some(end) = body.find("</dir>") else { break };
        let path = body[..end].trim();
        rest = &body[end..];
        if attrs.trim_end().ends_with('/') || path.is_empty() || v.len() >= 64 {
            continue;
        }
        let p = if attrs.contains("prefix=\"xdg\"") {
            let base = data_home.filter(|d| !d.is_empty()).map(PathBuf::from).or_else(|| home.map(|h| h.join(".local/share")));
            base.map(|b| b.join(path))
        } else if let Some(r) = path.strip_prefix("~/") {
            home.map(|h| h.join(r))
        } else if path.starts_with('/') {
            Some(PathBuf::from(path))
        } else {
            None
        };
        v.extend(p);
    }
    v
}

/// Catalog every face under `dirs`, recursing into subfolders (links followed, each folder read
/// once however links lead back to it). Only regular font files are opened: opening a FIFO named
/// like a font would block until something writes to it.
pub fn scan_dirs(dirs: &[PathBuf]) -> Vec<Face> {
    let mut found = Vec::new();
    if cfg!(target_arch = "wasm32") {
        return found;
    }
    let mut stack: Vec<(PathBuf, usize)> = dirs.iter().rev().map(|d| (d.clone(), 0)).collect();
    let mut visited = std::collections::HashSet::new();
    let mut entries = 0usize;
    while let Some((dir, depth)) = stack.pop() {
        if !visited.insert(std::fs::canonicalize(&dir).unwrap_or_else(|_| dir.clone())) {
            continue;
        }
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        let mut listed: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
        // A fixed order, so two scans of one folder list its faces alike.
        listed.sort();
        for p in listed {
            entries += 1;
            if entries > MAX_SCAN_ENTRIES {
                stack.clear();
                break;
            }
            let Ok(meta) = std::fs::metadata(&p) else { continue };
            if meta.is_dir() {
                if depth < MAX_SCAN_DEPTH {
                    stack.push((p, depth + 1));
                }
                continue;
            }
            if !is_font_name(&p) || !meta.is_file() || meta.len() <= 12 || meta.len() > MAX_FONT_FILE {
                continue;
            }
            found.extend(file_faces(&p, meta.len()));
        }
    }
    found
}

/// Whether `path` is named like an OpenType font or collection.
fn is_font_name(path: &Path) -> bool {
    path.extension().and_then(std::ffi::OsStr::to_str).is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "ttf" | "otf" | "ttc" | "otc"))
}

/// The faces of the font file at `path`, reading only its table directories and the tables kept.
pub fn file_faces(path: &Path, file_len: u64) -> Vec<Face> {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut file) = std::fs::File::open(path) else { return vec![] };
    let mut read_at = |offset: u64, len: usize| -> Option<Vec<u8>> {
        if offset.checked_add(len as u64)? > file_len {
            return None;
        }
        let mut buf = vec![0; len];
        file.seek(SeekFrom::Start(offset)).ok()?;
        file.read_exact(&mut buf).ok()?;
        Some(buf)
    };
    let Some(head) = read_at(0, 12) else { return vec![] };
    // A collection lists where each face's table directory starts.
    let starts: Vec<u32> = if head.starts_with(b"ttcf") {
        let n = be32(&head, 8).unwrap_or(0).min(MAX_FACES) as usize;
        read_at(12, n * 4).map(|b| b.chunks_exact(4).map(|c| u32::from_be_bytes([c[0], c[1], c[2], c[3]])).collect()).unwrap_or_default()
    } else {
        vec![0]
    };
    let mut faces = Vec::new();
    for (index, start) in starts.into_iter().enumerate() {
        let Some(dir) = read_at(start.into(), 12) else { continue };
        let Some(count) = dir.get(4..6).map(|b| usize::from(u16::from_be_bytes([b[0], b[1]]))) else { continue };
        let Some(records) = read_at(u64::from(start) + 12, count * 16) else { continue };
        let records: Vec<&[u8]> = records.chunks_exact(16).collect();
        let tags: Vec<[u8; 4]> = records.iter().map(|r| [r[0], r[1], r[2], r[3]]).collect();
        let mut table = |tag: &[u8; 4]| -> Option<Vec<u8>> {
            let rec = records.iter().find(|r| r.starts_with(tag))?;
            let (offset, len) = (be32(rec, 8)?, be32(rec, 12)?);
            if len > MAX_TABLE {
                return None;
            }
            read_at(offset.into(), len as usize)
        };
        let Some(name) = table(b"name") else { continue };
        let mut kept: Vec<([u8; 4], Vec<u8>)> = table(b"OS/2").map(|t| (*b"OS/2", t)).into_iter().collect();
        kept.extend(table(b"fvar").map(|t| (*b"fvar", t)));
        kept.push((*b"name", compact_name_table(&name).unwrap_or(name)));
        faces.push(Face { path: path.to_path_buf(), file_len, index: index as u32, tags, tables: mini_font(&kept).into() });
    }
    faces
}

fn be16(b: &[u8], at: usize) -> Option<u16> {
    b.get(at..at + 2).map(|s| u16::from_be_bytes([s[0], s[1]]))
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    b.get(at..at + 4).map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
}

/// `name` with only the records [`kept_name`] keeps (and, in a version 1 table, its language
/// tags). `None` when the table can't be read, so the caller keeps it whole.
fn compact_name_table(table: &[u8]) -> Option<Vec<u8>> {
    let version = be16(table, 0)?;
    let count = usize::from(be16(table, 2)?);
    let storage = usize::from(be16(table, 4)?);
    if version > 1 {
        return None;
    }
    let records_end = 6 + count * 12;
    let lang_tags: Vec<(u16, u16)> = if version == 1 {
        let n = usize::from(be16(table, records_end)?);
        (0..n).map(|i| Some((be16(table, records_end + 2 + i * 4)?, be16(table, records_end + 4 + i * 4)?))).collect::<Option<_>>()?
    } else {
        Vec::new()
    };
    let text = |len: u16, offset: u16| table.get(storage + usize::from(offset)..storage + usize::from(offset) + usize::from(len));
    let mut records: Vec<[u16; 4]> = Vec::new();
    let mut strings: Vec<&[u8]> = Vec::new();
    for i in 0..count {
        let at = 6 + i * 12;
        let field = |k: usize| be16(table, at + k * 2);
        let (platform, encoding, language, id, len, offset) = (field(0)?, field(1)?, field(2)?, field(3)?, field(4)?, field(5)?);
        if !kept_name(id) {
            continue;
        }
        let Some(s) = text(len, offset) else { continue };
        records.push([platform, encoding, language, id]);
        strings.push(s);
    }
    let lang_strings: Vec<&[u8]> = lang_tags.iter().map(|&(len, offset)| text(len, offset).unwrap_or(&[])).collect();
    let header = 6 + records.len() * 12 + if version == 1 { 2 + lang_strings.len() * 4 } else { 0 };
    let mut out = Vec::with_capacity(header + strings.iter().chain(&lang_strings).map(|s| s.len()).sum::<usize>());
    let push = |out: &mut Vec<u8>, v: usize| -> Option<()> {
        out.extend_from_slice(&u16::try_from(v).ok()?.to_be_bytes());
        Some(())
    };
    push(&mut out, version.into())?;
    push(&mut out, records.len())?;
    push(&mut out, header)?;
    let mut offset = 0usize;
    for (r, s) in records.iter().zip(&strings) {
        for v in r {
            push(&mut out, (*v).into())?;
        }
        push(&mut out, s.len())?;
        push(&mut out, offset)?;
        offset += s.len();
    }
    if version == 1 {
        push(&mut out, lang_strings.len())?;
        for s in &lang_strings {
            push(&mut out, s.len())?;
            push(&mut out, offset)?;
            offset += s.len();
        }
    }
    for s in strings.iter().chain(&lang_strings) {
        out.extend_from_slice(s);
    }
    Some(out)
}

/// A font file holding just `tables` (sorted by tag), each padded to four bytes.
pub fn mini_font(tables: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
    let mut tables: Vec<&([u8; 4], Vec<u8>)> = tables.iter().collect();
    tables.sort_by_key(|(tag, _)| *tag);
    let mut font = Vec::new();
    font.extend_from_slice(&0x0001_0000_u32.to_be_bytes());
    // numTables, then searchRange, entrySelector and rangeShift (which readers don't need).
    font.extend_from_slice(&u16::try_from(tables.len()).unwrap_or(0).to_be_bytes());
    font.extend_from_slice(&[0; 6]);
    let mut offset = 12 + 16 * tables.len();
    for (tag, data) in &tables {
        font.extend_from_slice(tag);
        font.extend_from_slice(&[0; 4]);
        for v in [offset, data.len()] {
            font.extend_from_slice(&u32::try_from(v).unwrap_or(0).to_be_bytes());
        }
        offset += data.len().next_multiple_of(4);
    }
    for (_, data) in &tables {
        font.extend_from_slice(data);
        font.resize(font.len().next_multiple_of(4), 0);
    }
    font
}

#[cfg(test)]
mod tests;
