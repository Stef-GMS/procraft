# Plan 07 — Verification, screenshots and mobile build packs

**Goal:** prove the shell and hand the user installable test builds.

**Files:** no source changes unless a gate fails; artifacts under `dist/`
(untracked).

**Steps**

- [x] Run the full gates from `D:\Dev\procraft`:
  `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test` (app + engine), and a release host build.
- [x] Compile-check the mobile targets if the toolchains are installed
  (`cargo check -p procraft --target aarch64-linux-android`); iOS needs macOS
  and stays with CI.
- [x] Forced-shell screenshot matrix (Windows): narrow 390x844 / 844x390 and
  tablet 1024x768 / 1366x1024, every theme, with and without drawers, options
  and popups. Save under `dist/mobile-shell/<date>/`.
- [x] Desktop regression matrix: wide/tablet/narrow with the override unset;
  compare against `dist/ui-2026-10-27` and `dist/liquify` to prove no drift.
- [x] Windows artifact: `dist/win-<sha>/procraft.exe` + mirrored
  `dist/procraft.exe`, with size and SHA-256.
- [x] Mobile packs: **ask before pushing**. Approved 2026-10-28, so
  `ui-b2-restyle` was pushed and the `ios-ipa` workflow ran; the IPA is in
  `dist/ios-aed004a/`.
- [ ] Android APK: run the `android-apk` (debug) workflow and download into
  `dist/android-<sha>/` (not requested yet).
- [x] Commit any doc updates: `docs(ui): mobile shell verification record`.

**Exit:** gates green, all screenshots present, artifacts recorded, user can
install and give the final device verdict.

**Log:** append a session to `chunk-26.json`.

## Result (2026-10-28)

### Gates
- `cargo fmt --all -- --check` clean.
- `cargo clippy --workspace --all-targets -- -D warnings` clean.
- `cargo test --workspace` green: 104 app tests (+1 ignored), 323 engine tests
  (+1 ignored).
- `cargo build --release -p procraft` succeeded.

### Mobile targets
- `cargo check -p procraft --target aarch64-linux-android` passes on this host.
- `cargo check -p procraft --target aarch64-apple-ios` also passes (type-check
  only; there is no iOS linker here).
- No Android SDK/NDK or `cargo-apk` on this machine, so no local APK; iOS
  packaging needs macOS. Both stay with the CI workflows.

### Screenshot matrix — `dist/mobile-shell/2026-10-28/` (52 shots)
- Sizes: phone portrait 390x844 (`487x1055` at 125%, safe top 47 pt),
  phone landscape 844x390 (`1055x488`, safe top 0), tablet 1024x768
  (`1280x960`, safe top 24 pt), tablet 1366x1024 (physical 1:1, safe top
  24 pt). `PROCRAFT_FORCE_SHELL=mobile` plus the matching
  `PROCRAFT_FORCE_LAYOUT`.
- Per size and theme (dark, light, paper): `-base`, `-tools`, `-layers`,
  `-popup` (File menu); plus `-gallery` in dark. Light/paper themes were
  re-checked after the phone-portrait drawer shots were retaken without the
  strip scroll (driver bug fixed mid-run).
- Raw run log: `dist/mobile-shell/2026-10-28/_capture.log`.

### Desktop regression — no drift
- Overrides unset, desktop shell: `desktop-wide-{dark,light,paper}` at
  1600x930, `desktop-tabletwindow-dark` at 1280x960 and
  `desktop-narrow-dark` at 700x900 (which folds the top bar into the View
  menu exactly as before).
- Pixel diff against `dist/ui-2026-10-27/wide-*`: 0.061% / 0.037% / 0.037%
  of sampled pixels differ, and the diff map
  (`desktop-wide-dark-diff.png`) confines them to the brush-ring cursor and
  the transparent window border included by `GetWindowRect`. App content is
  otherwise pixel-identical.
- `dist/liquify` is a pre-existing tool-panel reference; nothing in this plan
  touched it.

### Windows artifact
- `dist/win-aed004a/procraft.exe` (+ `README.md`, `LICENSE-MIT`) and the
  mirrored `dist/procraft.exe`.
- Size 27,235,328 bytes; SHA-256
  `2ef56d0fa6c03028a9047ca2a4beff5b3d2424d624831f4b8f2b7f814f3f10c7`
  (both copies). `dist/` stays untracked.

### Findings for Plan 08 (device-fix candidates)
1. File menu in phone landscape (390 pt tall) runs past the bottom edge with
   no scroll — `Open project`, `PSD export`, `Open PSD` are unreachable.
   Portrait, tablets and desktop fit.
2. Gallery is still a small desktop-style window parked low-left rather than
   a centered mobile sheet; unchanged from Plan 06, needs a device verdict.
3. Phone landscape Tools drawer shows only the first two rows before
   scrolling; usable but cramped.
4. Phone portrait strip needs a horizontal swipe to reach Gallery/File/
   Reference/More (~6 wheel notches at 390 pt wide); intentional, but worth
   confirming on device.

### Harness changes
- `capture.ps1`: `-ScrollFirst` (scroll before clicking, needed to reach
  File on the phone strip) and `-ParkCursor` (park the pointer on the canvas
  so a stale hover highlight does not leak into base shots).
- Matrix driver: `%TEMP%\opencode\ui-shots\plan07-matrix.ps1`,
  `plan07-bases.ps1`, pixel diff `diff.ps1`.

### Packs
- iOS: `ui-b2-restyle` pushed (commit 77b182b, code identical to aed004a);
  `ios-ipa` run
  <https://github.com/surfrpt1/procraft/actions/runs/40688842059> succeeded
  and the artifact was downloaded to `dist/ios-aed004a/ProCraft-ios-v1.ipa`.
  - 23,297,947 bytes; SHA-256
    `aea79349b0db0bd441dbf3d9f80dd2d2d49612a0b5341972995c5ac7c6fb2363`.
  - `Payload/ProCraft.app`: ProCraft 0.9.0, bundle `com.surfrpt1.procraft`,
    `MinimumOSVersion` 17.0, arm64 binary, 4 icon sizes, Info.plist.
  - Ad-hoc signed only (`codesign --sign -`), so it needs re-signing with a
    development profile (Sideloadly/AltStore style) before it installs on a
    device.
- Android: not built; the `android-apk` workflow has not been dispatched.
