# Weave Office on Android

> **Status:** the launcher, the Gradle project, the SAF document bridge and the soft-keyboard
> bridge are in place; the release APK builds. The Shortcut Pad, printing, file associations and
> updates are the next layers.

Weave Office on Android tablets and phones: the same workspace (documents, spreadsheets,
presentations, PDF, Markdown, code) in an arm64-v8a APK for Android 8.0 (API 26) and newer, built
for Android 15 (API 35).

It is an **unofficial community port**, not made or supported by the Weave Office developers.

## How it fits together

```text
WeaveOfficeActivity.java (extends GameActivity)      libweave_office_android.so (this crate)
 ├─ Storage Access Framework pickers  ◀──files──▶  bridge.rs (request.json / answer.json / inbox/ / out/)
 ├─ on-screen keyboard text ──────────polled────▶  ime.rs ─▶ egui events
 └─ GameActivity ──▶ android-activity ──▶ winit ──▶ eframe (wgpu: Vulkan, GLES fallback)
                                                     └─▶ WeaveApp (unchanged)
```

- **Rendering:** eframe's native runner on winit's GameActivity backend. wgpu is limited to Vulkan
  and OpenGL ES, so devices without a usable Vulkan driver draw through GLES.
- **The app itself is unchanged:** the launcher constructs the same `WeaveApp` as the desktop app,
  wrapped in a `Shell` that adds the soft keyboard's text.
- **Files:** the host's own Open/Save dialogs go through `file_dialog::android` (a picker hook the
  shell installs). The shell turns each request into a SAF picker over the file bridge: picked
  documents are copied into `bridge/inbox/<id>/` and opened from there; a save writes
  `bridge/out/<id>/` and the Activity copies it back to the document it stands for.
- **Soft keyboard:** winit 0.30 opens the keyboard but drops GameActivity's text events, so the
  shell polls the keyboard's text state each frame and replays the changes as egui Text and
  Backspace events (`src/ime.rs`).
- **The five layers** (following the ArtCraft Android set):
  1. launcher (`src/app.rs`, this crate) — done;
  2. bridges (SAF, IME) — done; clipboard — next;
  3. vendored dependency patches (winit/egui-winit/android-activity) — next;
  4. build-time patches (the host's desktop-only paths, `src/file_dialog.rs`,
     `src/clipboard.rs`) — done;
  5. shared templates (`SafActivity.java`, `saf_bridge.rs`, `ime_bridge.rs`,
     `system_fonts.rs`) — the Activity and the two bridges are in place; system fonts next.

## Build

```sh
export ANDROID_HOME=/path/to/android-sdk
export ANDROID_NDK_HOME=$ANDROID_HOME/ndk/27.3.13750724
export CRAFT_FONTS_DIR=/path/to/craft-fonts
export WEAVE_DOCUMENT_FONTS_DIR=/path/to/weave-document-fonts
export CRAFT_I18N_DIR=/path/to/craft-i18n
apps/weave-office-android/build.sh
adb install -r target/android/weave-office-0.1.1-arm64-v8a.apk
```

The font inputs are the pinned external checkouts the desktop build needs too; see
[build.md](build.md) for how to prepare them. [STATUS.md](STATUS.md) tracks what works and what is
next.
