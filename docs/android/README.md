# Weave Office on Android

> **Status:** a working prototype. The launcher, the SAF document bridge, the soft-keyboard and
> modifier bridges, the Shortcut Pad, touch gestures, the pen, the clipboard, printing, file
> associations and the updater are in place; the release APK builds and CI publishes it.
> [STATUS.md](STATUS.md) is the detailed log.

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
  1. launcher (`src/app.rs`, this crate) — the `Shell` around `WeaveApp`;
  2. bridges — SAF documents, the soft keyboard, the modifiers, the clipboard, the pointer's tool
     type, printing;
  3. vendored dependency patches (`vendor/rfd`, `vendor/arboard`) — the Android backends upstream
     lacks;
  4. build-time patches (the host's desktop-only paths, `src/file_dialog.rs`,
     `src/clipboard.rs`) — Android branches;
  5. shared templates — `WeaveOfficeActivity.java`, `bridge.rs`, `ime.rs`, `touch.rs`,
     `pad.rs`, and `crates/system-fonts`' Android font folders.

## Releases

`.github/workflows/android.yml` builds the APK on every push and pull request, and publishes a
release for every `android-v<version>` tag:

```sh
git tag android-v0.1.1
git push origin android-v0.1.1
```

The release carries the APK and the tag names the version, which is what the in-app updater reads
(`Updater.java`). The release key is committed
(`apps/weave-office-android/android/weave-release.jks`) so CI's APK updates a locally built one;
a fork that publishes its own releases should point `WEAVE_KEYSTORE` at its own key.

## Keeping up with upstream

This repository is a fork of [Jamailar/Weave-Office](https://github.com/Jamailar/Weave-Office) with
the Android shell added on top:

```sh
git remote add upstream https://github.com/Jamailar/Weave-Office.git   # once
git fetch upstream
git merge upstream/main
```

The port's changes are kept in `apps/weave-office-android/`, `docs/android/`,
`crates/system-fonts/`, and a few hooks in `src/app.rs` and `Cargo.toml`; the engines under
`vendor/` are not modified by the port, so an upstream engine change only needs the shell updated.

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
