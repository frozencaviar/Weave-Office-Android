# macOS local bundle

## Five-architecture release

Release installers are two macOS disk images (`arm64`, `x64`) and three Windows NSIS
installers (`x64`, `arm64`, `x86`). All builds use the root Cargo version and the same
locked dependencies, pinned fonts, translations and bundled attribution.

On the Mac, prepare the inputs below, install both Rust targets, and prefetch the
target-specific dependencies before invoking the offline bundle builder:

```sh
export WEAVE_DEV_VOLUME=/Volumes/MyDrive
rustup target add aarch64-apple-darwin x86_64-apple-darwin
cargo fetch --locked --target aarch64-apple-darwin --target x86_64-apple-darwin
python3 scripts/build-macos-app.py --target aarch64-apple-darwin --output "$WEAVE_DEV_VOLUME/DevCache/WeaveOffice/packages/arm64/Weave Office.app"
python3 scripts/build-macos-app.py --target x86_64-apple-darwin --output "$WEAVE_DEV_VOLUME/DevCache/WeaveOffice/packages/x64/Weave Office.app"
python3 scripts/package-macos-dmg.py --app "$WEAVE_DEV_VOLUME/DevCache/WeaveOffice/packages/arm64/Weave Office.app" --output "$WEAVE_DEV_VOLUME/DevCache/WeaveOffice/packages/WeaveOffice-0.1.2-macos-arm64.dmg"
python3 scripts/package-macos-dmg.py --app "$WEAVE_DEV_VOLUME/DevCache/WeaveOffice/packages/x64/Weave Office.app" --output "$WEAVE_DEV_VOLUME/DevCache/WeaveOffice/packages/WeaveOffice-0.1.2-macos-x64.dmg"
```

Windows builds run on the local Linux builder through the SSH alias `jamdebian`,
using Rust 1.95.0, `cargo-xwin`, LLVM/Clang and NSIS. Sync the source checkout
(excluding `.git`, `target` directories and the machine-local `.cargo/config.toml`)
to a dedicated directory such as `/home/jam/build/weave-office-release/source`.
Sync `craft-fonts`, `craft-i18n` (including their Git metadata for revision checks),
and `weave-document-fonts` to a sibling `inputs` directory. Keep all build outputs
outside the source checkout. On the Linux builder:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
export RUSTUP_TOOLCHAIN=1.95.0
rustup target add x86_64-pc-windows-msvc aarch64-pc-windows-msvc i686-pc-windows-msvc
cd /home/jam/build/weave-office-release/source
python3 scripts/build-windows-app.py \
  --inputs /home/jam/build/weave-office-release/inputs \
  --target-dir /home/jam/build/weave-office-release/target \
  --output /home/jam/build/weave-office-release/installers
```

Use `--arch x64`, `--arch arm64` or `--arch x86` to build a subset, and `--offline`
after the dependencies have been fetched. The script verifies the font and catalog
inputs, statically links the MSVC runtime, verifies each application's PE machine
type, and packages the executable and attribution. Release executables use the
Windows GUI subsystem, so starting the app opens no console window; command-line
verbs (`weave-office help`) still write to a redirected or piped output, or to the
console they were typed into. `build.rs` embeds `assets/app-icon/weave-office.ico`
and version information with `llvm-rc` (`WEAVE_REQUIRE_WINRES=1` makes a missing
resource compiler an error), and the same icon is used by the installer, the
uninstaller and the Start menu shortcut. Regenerate the `.ico` and the 256 px window
icon from the approved master with `python3 scripts/generate_windows_icon.py`
(requires Pillow). Installers install for the
current user, add a Start menu shortcut and register an uninstaller without
requiring administrator privileges or changing file associations. Existing user
documents and settings are preserved on uninstall. Use a dedicated `XWIN_CACHE_DIR`
and a valid `clang-cl` on `PATH` if sharing a machine with other builds. Parallel
build processes need separate target directories and `XWIN_CACHE_DIR` paths, since
`cargo-xwin` replaces its compiler symlink on startup. The x86 build explicitly
enables SSE2 for Clang's native C/C++ compilation, as required by AWS-LC.

For signed releases, pass `--sign-identity "Developer ID Application: …"` to the
macOS builder, notarize and staple the resulting application, then package, sign,
notarize and staple the DMG. Windows accepts `--sign-command` (or
`WEAVE_WINDOWS_SIGN_COMMAND`) as a JSON object with `cmd` and an `args` list;
`%1` is replaced with the file to sign. It signs the application before packaging
and uses NSIS finalizers to sign both the uninstaller and the finished installer.
This also supports a private SSH signing relay while credentials remain on the Mac.

Before publishing, run the existing workspace tests, check each executable's
architecture and dependencies, verify the DMGs, and smoke-test installation,
launch and uninstall on available runtimes. Document untested architectures and
OS versions in the release notes. Include `SHA256SUMS.txt` for the five installers.
The default macOS build is ad-hoc signed and not notarized; Windows installers
are unsigned unless a signing command is supplied. Windows packaging requires
Python 3.11 or newer for `tomllib`.

## Native macOS bundle details

Run from a macOS machine with Rust 1.95.0 and Python 3:

```sh
python3 scripts/build-macos-app.py
```

The default output is `$WEAVE_DEV_VOLUME/DevCache/WeaveOffice/target/bundle/Weave Office.app` (derived from Cargo metadata). Use `--debug` for the dev profile, or `--output /path/to/Weave Office.app` for another destination. Existing destinations are rejected by default. To rebuild the same app path, pass `--replace`; this only replaces a regular `.app` bundle whose `Contents/Info.plist` has `CFBundleIdentifier` set to `local.weaveoffice.desktop`. The new bundle is fully built and signed in a temporary directory beside the destination before replacement. If installation fails, the script restores the old bundle; if that restoration itself fails, it leaves the old bundle in a named backup beside the destination and prints its path. Cargo runs with `--locked --offline`; dependencies must already be cached.

For development, use the same external-cache check before Cargo commands:

```sh
python3 scripts/cargo.py build
python3 scripts/cargo.py test
python3 scripts/cargo.py run -- --help
```

Both scripts require the external volume named by `WEAVE_DEV_VOLUME` (its mount point, e.g. `/Volumes/MyDrive`) to be mounted and the effective Cargo target directory to resolve inside that volume. A missing volume or a `CARGO_TARGET_DIR` override pointing elsewhere fails before compilation. The machine-local `.cargo/config.toml` provides the default target directory; the scripts do not change global Cargo settings.

Both scripts also require the pinned open fonts at `$WEAVE_DEV_VOLUME/DevCache/WeaveOffice/inputs/craft-fonts` and the Word document serif at `$WEAVE_DEV_VOLUME/DevCache/WeaveOffice/inputs/weave-document-fonts`. Prepare them once with `python3 scripts/prepare-fonts.py` and `python3 scripts/prepare-document-fonts.py`. Every build, test and run verifies the craft-fonts checkout revision, clean state, manifests, font SHA-256 values and OFL notices. It also verifies the document serif's fixed manifest, OTF SHA-256 and OFL SHA-256 before setting the corresponding absolute Cargo input paths. Missing or invalid fonts stop the command with a preparation hint; see [font input details](../docs/fonts.md).

WordCraft's Simplified Chinese interface also requires the pinned translation catalogs at `$WEAVE_DEV_VOLUME/DevCache/WeaveOffice/inputs/craft-i18n`. Prepare them with `python3 scripts/prepare-i18n.py`. Both scripts verify the pinned revision, clean checkout, licenses and Chinese catalogs, then set absolute `CRAFT_I18N_DIR` and `CRAFT_I18N_REQUIRED=1` for Cargo. Missing or invalid catalogs stop the command with a preparation hint; see [translation input details](../docs/i18n.md).

The bundle contains a copied executable, an `Info.plist` registering DOCX, XLSX, PPTX and PDF as editable document types, and upstream, embedded-font and translation licenses and attribution in `Contents/Resources/licenses`. Font binaries and translation catalogs are compiled into the executable; they are not copied as separate bundle files. The font and translation notices record their pinned source commits, including the document serif. The bundle receives a local ad-hoc code signature and passes `codesign --verify --strict` before installation. Copy the finished `.app` to a local disk to run it without the external volume mounted. The script does not install the app, change default handlers, notarize it, or include upstream brand assets. macOS 11.0 is the declared minimum; actual compatibility should be checked on the oldest supported machine before distribution.

`CFBundleLocalizations` declares `en` and `zh-Hans`, matching the two interface languages handled by the application. This lets AppKit choose supported native-panel localization using the system's language preferences. The Chinese-system GUI check confirms that Open and Save As use native Chinese labels after this declaration; the panels themselves remain AppKit components.

Original app and document icons are rendered from the vector artwork in `scripts/macos-icons.swift`. Generate their `.icns` files with `python3 scripts/generate_macos_icons.py`; the generator keeps intermediate iconsets under `$WEAVE_DEV_VOLUME/DevCache/WeaveOffice/macos-icons` and checks that the cache remains on the external volume. The bundle builder runs this generator, includes `WeaveOffice.icns`, `WeaveWord.icns`, `WeaveSheet.icns`, `WeaveSlides.icns` and `WeavePDF.icns` in `Contents/Resources`, and sets `CFBundleIconFile` and each document type's `CFBundleTypeIconFile`. `iconutil` validates the generated 10-size iconsets; no third-party artwork is bundled.

On 2026-11-02, the optimized release bundle at `~/Applications/Weave Office.app` was updated to arm64 Mach-O UUID `1964B381-ECC0-3DF0-B7A0-E83FD1D70A51`. It retains the font-preview and modal/batch-open fixes and adds shared immutable font storage. The bundle is 242,432,022 bytes, down from 407,542,358 bytes (40.5%); the 58 audited font files each appear exactly once. The root release profile strips symbols while retaining unwind guards, and the host selects only Glow; standalone PdfCraft retains WGPU by default. The first full release rebuild took 13m25s; the cached installation build took 1.23s. Build products and large font/translation inputs remained on the external volume. Strict code-signature verification passed. This exact binary opened five mixed documents while Seatbelt denied reads from the external volume, source/reference repositories and other office suites; a held rerun displayed all four types and exited normally with code 0. Copied documents and the custom settings file remained unchanged. An earlier detached test process exited without a captured return code; its cause was not established and is retained in the acceptance record. This is a local ad-hoc signed build. Network isolation, public distribution signing, notarization, fresh-machine installation and minimum-OS compatibility remain unverified.

On 2026-11-01, an empty target at `$WEAVE_DEV_VOLUME/DevCache/WeaveOffice/rebuild-20261101/target` built the complete `weave-office` binary with `--locked --offline -j3` in about 5m 28s. The resulting executable reported `Weave Office 0.1.0`; the later application-menu update also rebuilt there successfully. This reuses downloaded Cargo sources and verified font/translation inputs, but no previous compilation artifacts in that target. It does not claim a fresh-machine installation or minimum-OS check. See [acceptance evidence](../docs/acceptance.md).
