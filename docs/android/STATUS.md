# Android port status

> **Last updated:** 2026-11-04 · **Change:** the Shortcut Pad's keys were redrawn as compact
> squares with Lucide-style line icons and hover tooltips.

## Done

| Step | State |
|---|---|
| Fork + upstream pin | `f3054c51677cacab7e4ba945d50f7a45b2fd6ae2` (see [UPSTREAM.md](../../UPSTREAM.md)) |
| Launcher crate | `apps/weave-office-android` (cdylib + rlib): `android_main`, GameActivity, wgpu (Vulkan + GLES), logcat logging, panic hook, `HOME` pointed at the app's private storage |
| Shell wrapper | `Shell` implements `eframe::App` around `WeaveApp` and injects the soft keyboard's text (winit drops GameActivity's text events) |
| Gradle project | `apps/weave-office-android/android`: manifest, theme, `WeaveOfficeActivity` (immersive, edge to edge), icon from `assets/app-icon`, debug signing |
| Build script | `apps/weave-office-android/build.sh` (cargo-ndk + Gradle, font inputs required, `ANDROID_USER_HOME` fallback) |
| Host compiles for Android | `cargo check -p weave-office --lib --target aarch64-linux-android` — clean |
| Launcher compiles for Android | `cargo check -p weave-office-android --target aarch64-linux-android` — clean |
| Desktop-only paths gated | `src/file_dialog.rs` (rfd) and `src/clipboard.rs` (arboard) have Android branches |
| Vendored dependency stubs | `vendor/rfd` and `vendor/arboard` add the Android backends upstream lacks (dialogs cancelled, clipboard unsupported), patched in `Cargo.toml` |
| **SAF bridge** | `src/bridge.rs` + `WeaveOfficeActivity`: Open/Save through Android's document pickers; picked documents are copied into `bridge/inbox/<id>/`, saves write `bridge/out/<id>/` and the Activity copies them back to the document (polled, once per write) |
| **IME bridge** | `src/ime.rs` + the Shell wrapper: the keyboard's text state is polled each frame and replayed as egui Text/Backspace/Enter events (the action key arrives as a newline); the keyboard is put away when focus leaves. Polling is driven by whether the app *published an IME descriptor*, not by egui's keyboard focus: a canvas text editor (a slide's shape, a page) keeps egui's focus empty on purpose, so asking about focus left the soft keyboard's text undelivered there |
| **Shortcut Pad** | `src/pad.rs`: a light card on either edge with 2–10 compact 36 pt square keys (6 pt apart, 8 pt padding) and a wheel. The keys carry Lucide-style 18 pt line icons (1.75 pt stroke, round caps, #4B5563) and show their names in a tooltip on hover or a long press. At rest the wheel is a minimal 36 pt ring (hairline #E5E7EB, twelve 3 pt ticks, a 4 pt hollow hub); pressing it opens a 150 pt dial anchored at the strip's edge — frosted white, an orange ring and 36 ticks that rotate with the turn (a longer tick marks the rotation), an arc that shows the zoom's place in the editor's own range (10–500% in Word, 10–400% in Excel and PowerPoint, 8–6400% in PDF), and the active document's zoom in the middle. Turning it follows the zoom continuously (a full turn doubles it, like a pinch) through `view.zoom`, or runs the wheel's two actions one step per twelfth of a turn when they are not zoom commands. The strip folds with a height/opacity animation (the arrow turns 180°), drags to either edge and snaps there, and a long-press on the arrow opens its settings (`shortcut-pad.json`) |
| **Touch gestures** | `src/touch.rs`: a gesture recogniser turns fingers into the mouse events a finger cannot make. A tap clicks (selects a shape, puts the caret); a drag scrolls; a long press then drag drags (moves a shape, sweeps a marquee, selects text); a long press released in place is a Ctrl+click, so a second tap adds to the selection; two fingers scroll by their centroid. The pad's setting can make a drag drag straight away instead, like a mouse, and an armed drawing tool (pen, highlighter, eraser) does the same so a stroke draws. winit's Android backend reports the stylus as another touch with no tool type of its own, so the Activity writes the pointer's `MotionEvent` tool type to `bridge/tool.txt` whenever it changes and the shell makes a pen drag drag straight away; the pen's pressure rides along in the touch event's `force`, which the ink tool already turns into stroke width. A click's release is held back one frame because the canvas reads a press and a release in the same frame as one event and never finishes the click; the pointer egui-winit made from the finger is released while scrolling so the canvas does not drag a selection |
| **Fonts** | The bundled craft-fonts faces plus the device's own: `crates/system-fonts` now lists Android's font folders (`/system/fonts`, `/product/fonts`, `/system_ext/fonts`, `/vendor/fonts`), so the editors' font menus offer the system families (Noto Sans CJK, Noto Serif CJK, the OEM faces) beside the bundled ones. Proprietary Office fonts are never shipped; documents that name them draw in the metric-compatible open substitute (Times New Roman → Liberation Serif, Calibri → Carlito, Cambria → Caladea) |
| **Keyboard modifiers** | The Shell tracks Ctrl, Shift and Alt from their own key events and hands egui the `ModifiersChanged` winit's Android backend never emits, so Ctrl+C, Ctrl+Z, Shift+arrows and the rest work on a hardware keyboard |
| **Open with / Share** | The Activity accepts `ACTION_VIEW`/`ACTION_SEND` for docx/xlsx/pptx/pdf/md/txt, copies the document into `bridge/incoming/`, and the shell moves it to `bridge/documents/` and opens it once (`WeaveApp::open_external`) |
| **Phone layout** | A phone-class screen (smallest side under 600 pt) gets the interface at 75% |
| **Clipboard** | The Activity publishes the system clipboard to `bridge/clip.txt` (a change listener), the app's copy writes `bridge/clip-set.txt`, and the host's clipboard reads/writes through the shell's hook (`src/clipboard.rs`); in-app rich payloads still work |
| **Printing** | The `android.print` action (a pad key can be pointed at it) exports the active document to a PDF in the bridge and the Activity hands it to Android's `PrintManager` (a `PrintDocumentAdapter` streams the file) |
| **Updates** | `Updater.java`: checks the GitHub releases for an `android-v*` tag with an APK, compares versions, downloads and installs through Android's package installer; a startup check and the `android.update` action trigger it |
| **Release APK** | `apps/weave-office-android/build.sh` → `target/android/weave-office-0.1.1-arm64-v8a.apk` (236 MB): package `local.weaveoffice.android`, versionCode 101, minSdk 26, targetSdk 35, `arm64-v8a`, debug-signed |

| **CI** | `.github/workflows/android.yml`: Rust 1.95 + Android target, JDK 17, SDK/NDK, cargo-ndk, the pinned font inputs, the host tests and the APK build, uploaded as an artifact |

## Validated on the Android emulator

The APK was installed and run on an Android 15 (API 35) arm64 emulator (SwiftShader):

- the app starts, the Home screen draws all seven document types, and the Shortcut Pad is on
  the right edge (see [home.png](images/home.png));
- a presentation opens with its ribbon, slide panel, canvas and status bar
  ([presentation.png](images/presentation.png));
- the soft keyboard opens when a text box is tapped;
- wgpu picks Vulkan (SwiftShader) with the GL fallback available; the logcat says which adapter
  it got.

The Shortcut Pad sits against the window's edge and its keys work: Undo with no document opens
the host's "open a file first" dialog, a long press on a key shows its name, and a long press on
the fold arrow opens the pad's settings ([pad-settings.png](images/pad-settings.png)). Pressing
the wheel opens the dial with the document's zoom in its middle and an orange progress arc that
follows the turn ([pad-dial.png](images/pad-dial.png)).

## Next

1. **Install and run:** `adb install -r`, `adb logcat -s WeaveOffice`; check the window, the wgpu
   adapter, opening and saving a document through the pickers, and typing on the soft keyboard.
2. **M4 — touch:** Shortcut Pad (from the ArtCraft shells), phone layout, two-finger scroll.
3. **M5 — system:** printing (Android `PrintManager`), file associations (docx/xlsx/pptx/pdf/md),
   updates, clipboard (JNI).
4. **M6 — CI:** a build workflow and a release tag.
5. **Size:** the APK is 236 MB (the four engines and the embedded fonts); a later pass can trim
   the embedded faces and split the debug symbols.

## Environment notes

- The desktop build needs the pinned external font inputs; on a machine without the external
  volume, check them out by hand (see [build.md](build.md)).
- The Android build uses the same inputs plus `ANDROID_HOME`, `ANDROID_NDK_HOME`, JDK 17 and
  `cargo-ndk`.
- Gradle's debug keystore goes under `$ANDROID_USER_HOME` (default `~/.android`); the build
  script falls back to `target/android-home` when `$HOME` isn't writable.