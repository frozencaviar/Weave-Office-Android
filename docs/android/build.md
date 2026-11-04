# Building the Android APK

> **Last updated:** 2026-11-04 · **Change:** first version, with the launcher.

`apps/weave-office-android` builds one APK: arm64-v8a, minimum Android 8.0 (API 26), built for
Android 15 (API 35). `build.sh` compiles the Rust library with the NDK and then runs Gradle.

## Requirements

| What | Version |
|---|---|
| Rust | the workspace's `rust-version` (1.95), with `rustup target add aarch64-linux-android` |
| `cargo-ndk` | required (`cargo install cargo-ndk`) |
| JDK | 17 |
| Android SDK | platform 35 and build-tools 35.0.0 |
| Android NDK | r27 or newer |
| Gradle | 8.11.1, through the committed wrapper |
| Font inputs | `CRAFT_FONTS_DIR`, `WEAVE_DOCUMENT_FONTS_DIR`, `CRAFT_I18N_DIR` (the desktop build's pinned external checkouts) |

## Environment

| Variable | Default | What it does |
|---|---|---|
| `ANDROID_HOME` | — (required) | The Android SDK |
| `ANDROID_NDK_HOME` | — (required) | The NDK (r27 or newer) |
| `CRAFT_FONTS_DIR` | — (required) | The pinned `storytold/craft-fonts` checkout |
| `WEAVE_DOCUMENT_FONTS_DIR` | — (required) | Noto Serif CJK SC + OFL + manifest |
| `CRAFT_I18N_DIR` | — | The pinned `storytold/craft-i18n` checkout (English-only without it) |
| `PROFILE` | `release` | `dev` builds an unoptimised library |
| `RELEASE_BUILD` | `0` | The Android build number of this version |

## Preparing the font inputs

The desktop build's scripts require an external volume; on a machine without one, check the pinned
revisions out by hand (they must stay outside the repository):

```sh
git clone --filter=blob:none --no-checkout https://github.com/storytold/craft-fonts
git -C craft-fonts checkout --detach 37827adc41fd165f9a9234ac40f94dbad353aa69

mkdir weave-document-fonts && cd weave-document-fonts
cp <repo>/scripts/document-fonts-manifest.txt manifest.txt
curl -fLO https://raw.githubusercontent.com/notofonts/noto-cjk/f8d157532fbfaeda587e826d4cd5b21a49186f7c/Serif/OTF/SimplifiedChinese/NotoSerifCJKsc-Regular.otf
curl -fL -o OFL.txt https://raw.githubusercontent.com/notofonts/noto-cjk/f8d157532fbfaeda587e826d4cd5b21a49186f7c/Serif/LICENSE

git clone https://github.com/storytold/craft-i18n
git -C craft-i18n checkout --detach $(cat <repo>/vendor/wordcraft/craft-i18n.rev)
```

## Building

```sh
export ANDROID_HOME=/path/to/android-sdk
export ANDROID_NDK_HOME=$ANDROID_HOME/ndk/27.3.13750724
export CRAFT_FONTS_DIR=/path/to/craft-fonts
export WEAVE_DOCUMENT_FONTS_DIR=/path/to/weave-document-fonts
export CRAFT_I18N_DIR=/path/to/craft-i18n
apps/weave-office-android/build.sh
adb install -r target/android/weave-office-0.1.1-arm64-v8a.apk
```

`build.sh` builds `weave-office-android` for `aarch64-linux-android`, copies
`libweave_office_android.so` into `android/app/src/main/jniLibs/arm64-v8a/`, runs
`./gradlew --no-daemon assembleRelease` and copies the APK to `target/android/`.

Logs: `adb logcat -s WeaveOffice`.
