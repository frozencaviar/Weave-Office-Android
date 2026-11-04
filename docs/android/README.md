# Weave Office on Android

> **Status:** first layer (launcher + Gradle project). The app builds for `aarch64-linux-android`;
> the document pickers, soft keyboard, clipboard and Shortcut Pad are the next layers.

Weave Office on Android tablets and phones: the same workspace (documents, spreadsheets,
presentations, PDF, Markdown, code) in an arm64-v8a APK for Android 8.0 (API 26) and newer, built
for Android 15 (API 35).

It is an **unofficial community port**, not made or supported by the Weave Office developers.

## How it fits together

```text
WeaveOfficeActivity.java (extends GameActivity)      libweave_office_android.so (this crate)
 ├─ (next) Storage Access Framework pickers  ◀──files──▶  bridge.rs
 ├─ (next) on-screen keyboard height ─────────JNI──────▶  screen rect
 └─ GameActivity ──▶ android-activity ──▶ winit ──▶ eframe (wgpu: Vulkan, GLES fallback)
                                                     └─▶ WeaveApp (unchanged)
```

- **Rendering:** eframe's native runner on winit's GameActivity backend. wgpu is limited to Vulkan
  and OpenGL ES, so devices without a usable Vulkan driver draw through GLES.
- **The app itself is unchanged:** the launcher constructs the same `WeaveApp` as the desktop app.
- **The five layers** (following the ArtCraft Android set):
  1. launcher (`src/app.rs`, this crate);
  2. JNI bridges (SAF, IME, clipboard) — next;
  3. vendored dependency patches (winit/egui-winit/android-activity) — next;
  4. build-time patches (the host's desktop-only paths, `src/file_dialog.rs`,
     `src/clipboard.rs`) — started;
  5. shared templates (`SafActivity.java`, `saf_bridge.rs`, `ime_bridge.rs`,
     `system_fonts.rs`) — next.

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
[build.md](build.md) for how to prepare them.
