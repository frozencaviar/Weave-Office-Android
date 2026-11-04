# Android port status

> **Last updated:** 2026-11-04 · **Change:** the SAF and IME bridges landed; the release APK
> builds.

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
| **IME bridge** | `src/ime.rs` + the Shell wrapper: the keyboard's text state is polled each frame and replayed as egui Text/Backspace events; the keyboard is put away when focus leaves |
| **Shortcut Pad** | `src/pad.rs`: a strip on either edge with 2–10 touch keys and a wheel; keys run host commands (`edit.undo`, `file.save`, `file.print`) or editor commands (`view.zoomIn`), the wheel turns two more, and the settings window edits them (`shortcut-pad.json`) |
| **Release APK** | `apps/weave-office-android/build.sh` → `target/android/weave-office-0.1.1-arm64-v8a.apk` (236 MB): package `local.weaveoffice.android`, versionCode 101, minSdk 26, targetSdk 35, `arm64-v8a`, debug-signed |

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