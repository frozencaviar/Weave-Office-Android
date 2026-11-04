#!/usr/bin/env bash
# Build Weave Office's Android APK (arm64-v8a): the Rust library with cargo-ndk, then the APK with
# Gradle. Needs ANDROID_HOME (SDK with platform 35 and build-tools), ANDROID_NDK_HOME (r27+),
# JDK 17, "rustup target add aarch64-linux-android", "cargo install cargo-ndk", and the pinned
# font inputs (CRAFT_FONTS_DIR, WEAVE_DOCUMENT_FONTS_DIR, CRAFT_I18N_DIR; see docs/android/build.md).
#
#   apps/weave-office-android/build.sh              # release APK
#   PROFILE=dev apps/weave-office-android/build.sh  # unoptimised library, faster to build
#
# The APK lands in target/android/weave-office-<version>[-<build>]-arm64-v8a.apk.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
PROFILE="${PROFILE:-release}"
API="${ANDROID_API:-26}"

: "${ANDROID_HOME:?set ANDROID_HOME to the Android SDK}"
: "${ANDROID_NDK_HOME:?set ANDROID_NDK_HOME to the Android NDK (r27 or newer)}"
: "${CRAFT_FONTS_DIR:?set CRAFT_FONTS_DIR to the pinned craft-fonts checkout}"
: "${WEAVE_DOCUMENT_FONTS_DIR:?set WEAVE_DOCUMENT_FONTS_DIR to the pinned document font input}"
command -v cargo-ndk >/dev/null || { echo "error: cargo-ndk not found (cargo install cargo-ndk)" >&2; exit 1; }

export CRAFT_FONTS_REQUIRED=1
export WEAVE_DOCUMENT_FONTS_REQUIRED=1
if [[ -n "${CRAFT_I18N_DIR:-}" ]]; then export CRAFT_I18N_REQUIRED=1; fi

# Gradle's debug signing config writes a debug keystore under $ANDROID_USER_HOME (default
# ~/.android). A sandboxed or read-only home can't take it, so fall back to the target directory.
if [[ -z "${ANDROID_USER_HOME:-}" && ! -w "${HOME:-/nonexistent}" ]]; then
    export ANDROID_USER_HOME="${CARGO_TARGET_DIR:-$ROOT/target}/android-home"
    mkdir -p "$ANDROID_USER_HOME"
fi

# Keep this machine's folders (and the user name in them) out of the library: rustc records
# source paths for panic messages.
remap() { printf ' --remap-path-prefix=%s=%s' "$1" "$2"; }
REMAP="$(remap "${CARGO_HOME:-$HOME/.cargo}" /cargo)$(remap "$ROOT" /weave-office)$(remap "${CARGO_TARGET_DIR:-$ROOT/target}" /target)"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS="${CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS:-}$REMAP"

cd "$ROOT"
cargo ndk -t arm64-v8a -P "$API" -o "$HERE/android/app/src/main/jniLibs" \
    build -p weave-office-android --profile "$PROFILE"

cd "$HERE/android"
if [[ "${OS:-}" == "Windows_NT" ]]; then ./gradlew.bat --no-daemon assembleRelease; else ./gradlew --no-daemon assembleRelease; fi

VERSION="${APP_VERSION:-$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)}"
if [[ "${RELEASE_BUILD:-0}" != 0 ]]; then VERSION="$VERSION-$RELEASE_BUILD"; fi
OUT="${CARGO_TARGET_DIR:-$ROOT/target}/android"
mkdir -p "$OUT"
cp app/build/outputs/apk/release/app-release.apk "$OUT/weave-office-$VERSION-arm64-v8a.apk"
echo "APK: $OUT/weave-office-$VERSION-arm64-v8a.apk"
