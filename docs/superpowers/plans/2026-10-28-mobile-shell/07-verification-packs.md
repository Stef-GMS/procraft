# Plan 07 — Verification, screenshots and mobile build packs

**Goal:** prove the shell and hand the user installable test builds.

**Files:** no source changes unless a gate fails; artifacts under `dist/`
(untracked).

**Steps**

- [ ] Run the full gates from `D:\Dev\procraft`:
  `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test` (app + engine), and a release host build.
- [ ] Compile-check the mobile targets if the toolchains are installed
  (`cargo check -p procraft --target aarch64-linux-android`); iOS needs macOS
  and stays with CI.
- [ ] Forced-shell screenshot matrix (Windows): narrow 390x844 / 844x390 and
  tablet 1024x768 / 1366x1024, every theme, with and without drawers, options
  and popups. Save under `dist/mobile-shell/<date>/`.
- [ ] Desktop regression matrix: wide/tablet/narrow with the override unset;
  compare against `dist/ui-2026-10-27` and `dist/liquify` to prove no drift.
- [ ] Windows artifact: `dist/win-<sha>/procraft.exe` + mirrored
  `dist/procraft.exe`, with size and SHA-256.
- [ ] Mobile packs: **ask before pushing**. If approved, push `ui-b2-restyle`,
  run `ios-ipa` and `android-apk` (debug) workflows, and download into
  `dist/ios-<sha>/`, `dist/android-<sha>/`.
- [ ] Commit any doc updates: `docs(ui): mobile shell verification record`.

**Exit:** gates green, all screenshots present, artifacts recorded, user can
install and give the final device verdict.

**Log:** append a session to `chunk-26.json`.
