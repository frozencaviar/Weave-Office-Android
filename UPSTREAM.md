# Upstream

This repository is a fork of [Jamailar/Weave-Office](https://github.com/Jamailar/Weave-Office)
(MIT), pinned at:

```
f3054c51677cacab7e4ba945d50f7a45b2fd6ae2
```

The Android port lives in `apps/weave-office-android/` and `docs/android/`; the vendored
`rfd` and `arboard` copies in `vendor/` add the Android stubs the upstream crates lack.

## Updating from upstream

```sh
git remote add upstream https://github.com/Jamailar/Weave-Office.git   # once
git fetch upstream
git merge upstream/main
```

The Android port touches:

- `Cargo.toml` (the launcher workspace member, the Android target dependencies, the
  `rfd`/`arboard` patches);
- `src/file_dialog.rs` and `src/clipboard.rs` (the desktop-only paths, gated for Android);
- `vendor/rfd` and `vendor/arboard` (the Android stubs);
- `apps/weave-office-android/` and `docs/android/` (new files).

Keep the upstream pin in this file in step with the merge.
