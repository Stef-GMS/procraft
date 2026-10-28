# Mobile Shell Reorganization — Plan Set

**Spec:** `docs/superpowers/specs/2026-10-28-mobile-shell-design.md`
**Goal:** Procreate-style canvas-first shell for iOS `.ipa` and Android `.apk`
only. Floating corner clusters, slide-over sidebars, full-screen canvas, and a
phone bottom strip instead of the tablet's top-left cluster.

**Rule:** desktop is untouched. Everything new is behind the mobile shell gate
(`cfg!(ios/android)` or `PROCRAFT_FORCE_SHELL=mobile` on Windows).

| Plan | Title | Visible change | Risk |
|---|---|---|---|
| 01 | Shell seam and override | status row hidden on mobile only | low |
| 02 | Extract panel and File-menu bodies | none (pure refactor) | medium |
| 03 | Canvas-first mobile shell skeleton | mobile only | high |
| 04 | Tablet clusters and options capsule | mobile tablet only | medium |
| 05 | Phone bottom strip | mobile phone only | high |
| 06 | Overlay polish, toast, modal sizing | mobile only | medium |
| 07 | Verification, screenshots, build packs | none | low |
| 08 | Device-verdict fixes | mobile + desktop icon/readout polish | low |

## Workflow

For every plan:

1. Write the failing tests first where the plan has a pure seam.
2. Implement; keep `cargo fmt --check`, `cargo clippy -D warnings` and both test
   suites green.
3. Verify visually with `PROCRAFT_FORCE_SHELL=mobile` and the existing
   `%TEMP%\opencode\ui-shots` harness (never replace the desktop screenshots).
4. Commit the plan on `ui-b2-restyle` (never push).
5. Append a session to `D:\conversationLog\chunk-26.json` describing what
   changed, what was verified, and what the next plan is.

## Device verdicts

- After Plan 04: iPad arrangement.
- After Plan 05: iPhone arrangement.
- After Plan 06: final pass.
- After the `aed004a` IPA (2026-10-28): icons + size readout fixed; large-brush
  lag confirmed as CPU stamping and deferred to a rendering milestone.
- `.ipa`/`.apk` packs need a push; ask before triggering the workflows.
