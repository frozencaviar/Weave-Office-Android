# Android port status

> **Last updated:** 2026-11-04 · **Change:** the dev APK builds and verifies.

## Done

| Step | State |
|---|---|
| Fork + upstream pin | `f3054c51677cacab7e4ba945d50f7a45b2fd6ae2` (see [UPSTREAM.md](../../UPSTREAM.md)) |
| Launcher crate | `apps/weave-office-android` (cdylib + rlib): `android_main`, GameActivity, wgpu (Vulkan + GLES), logcat logging, panic hook, `HOME` pointed at the app's private storage |
| Gradle project | `apps/weave-office-android/android`: manifest, theme, `WeaveOfficeActivity` (immersive, edge to edge), icon from `assets/app-icon`, debug signing |
| Build script | `apps/weave-office-android/build.sh` (cargo-ndk + Gradle, font inputs required, `ANDROID_USER_HOME` fallback) |
| Host compiles for Android | `cargo check -p weave-office --lib --target aarch64-linux-android` — clean |
| Launcher compiles for Android | `cargo check -p weave-office-android --target aarch64-linux-android` — clean |
| Desktop-only paths gated | `src/file_dialog.rs` (rfd) and `src/clipboard.rs` (arboard) have Android branches |
| Vendored dependency stubs | `vendor/rfd` and `vendor/arboard` add the Android backends upstream lacks (dialogs cancelled, clipboard unsupported), patched in `Cargo.toml` |
| **Dev APK** | `PROFILE=dev apps/weave-office-android/build.sh` → `target/android/weave-office-0.1.1-arm64-v8a.apk` (567 MB, unstripped): package `local.weaveoffice.android`, versionCode 101, minSdk 26, targetSdk 35, `arm64-v8a`, debug-signed, `libweave_office_android.so` inside |

## Next

1. **Release APK:** `apps/weave-office-android/build.sh` (release profile) — stripped and optimised,
   much smaller than the dev build.
2. **Install and run:** `adb install -r`, `adb logcat -s WeaveOffice`; check the window, wgpu
   adapter and the workspace on a tablet.
3. **M3 — bridges:** SAF pickers (`SafActivity.java` + `saf_bridge.rs`), IME (hidden
   `EditText` + `ime_bridge.rs`), clipboard (JNI), replacing the stubs.
4. **M4 — touch:** Shortcut Pad (from the ArtCraft shells), phone layout, two-finger scroll.
5. **M5 — system:** printing (Android `PrintManager`), file associations (docx/xlsx/pptx/pdf/md),
   updates.
6. **M6 — CI:** a build workflow and a release tag.

## Environment notes

- The desktop build needs the pinned external font inputs; on a machine without the external
  volume, check them out by hand (see [build.md](build.md)).
- The Android build uses the same inputs plus `ANDROID_HOME`, `ANDROID_NDK_HOME`, JDK 17 and
  `cargo-ndk`.
- Gradle's debug keystore goes under `$ANDROID_USER_HOME` (default `~/.android`); the build
  script falls back to `target/android-home` when `$HOME` isn't writable.
