# Plan 01 — Shell seam and dev override

**Goal:** one pure decision point that says Desktop or Mobile, testable on the
host, so later plans can branch without scattering `cfg!` checks.

**Files:** `apps/procraft/src/lib.rs` (helpers near `BarLayout`, tests near the
existing `bar_layout_splits_at_phone_width` / `forced_layout_wins_over_the_width_rules`).

**Steps**

- [x] Write failing tests:
  ```rust
  #[test]
  fn native_mobile_gets_the_mobile_shell() {
      assert_eq!(resolve_shell(None, true), Shell::Mobile);
      assert_eq!(resolve_shell(None, false), Shell::Desktop);
  }

  #[test]
  fn forced_shell_overrides_the_native_default() {
      assert_eq!(resolve_shell(Some(true), false), Shell::Mobile);
      assert_eq!(resolve_shell(Some(false), true), Shell::Desktop);
  }
  ```
- [x] Add `enum Shell { Desktop, Mobile }`, `forced_shell()` reading
  `PROCRAFT_FORCE_SHELL=mobile|desktop`, `resolve_shell(forced, native)` and
  `fn mobile_shell() -> bool` using
  `cfg!(any(target_os = "ios", target_os = "android"))`.
- [x] In `ui()`, compute `let shell = mobile_shell();` and hide the bottom
  `status` panel when `shell == Shell::Mobile` (Desktop keeps it; the mobile
  toast arrives in Plan 06). This is the only visible change.
- [x] Run `cargo test -p procraft --lib`, fmt and clippy.
- [x] Verify desktop is unchanged (`PROCRAFT_FORCE_SHELL` unset); then run with
  `PROCRAFT_FORCE_SHELL=mobile` and confirm only the status row disappears
  while the rest of the docked shell still works.
- [x] Commit: `feat(ui): mobile shell gate with a forced dev override`.

**Exit:** pure shell resolution covered by tests; no desktop change; mobile
build still uses the docked layout for now.

**Log:** append a session to `chunk-26.json`.

**Result (2026-10-28):** done. 90 app tests green (2 new), clippy `-D warnings`
and fmt clean. Desktop wide screenshot unchanged; forced-mobile tablet shows
the canvas reaching the bottom edge with no status row
(`dist/mobile-shell/plan01-*.png`).
