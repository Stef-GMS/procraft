# Plan 10 — Dotted workspace board

**Goal:** give the area where the page sits a Freeform-style dotted board so the
sheet reads as a page floating on a workspace instead of a white rectangle
pinned into a flat panel.

**Scope:** the canvas viewport on every layout (phone, tablet, desktop). No new
settings, no toolbar changes; artwork pixels are never touched.

**Inputs:** user request 2026-10-28 while testing the `e22e402` iPad build: "you
see that canvas where page is placed i want it to have some canvas type lining
or dots do that".

**Steps**

- [x] Pure helpers with tests: `board_dot_step`, `board_dot_span`,
      `board_fit_zoom`, `center_offset`.
- [x] `design::board_dot(theme)` — a faint white tint in dark, near-black tint
      in light/paper.
- [x] Paint a canvas-fixed dot grid behind the page (skipped under the page,
      clipped to the panel rather than the scroll viewport).
- [x] Fit zoom leaves a board margin; a page that fits floats centred.
- [x] Phone (dark/light), tablet portrait and desktop captures.
- [x] `cargo fmt --check`, `clippy -D warnings`, both test suites, log session,
      commit.

**Exit:** the page floats on a subtle dotted board at any zoom where the board
is visible, in all three themes, on phone, tablet and desktop.

**Screenshots:** `dist/mobile-shell/2026-10-28/plan10-{phone-board-dots-dark,
phone-board-dots-light,tablet-board-dots-dark,desktop-board-dots-dark}.png`.

## Design notes

- Dots live in canvas coordinates, so they pan and rotate with the page. The
  step is `32 * 2^k` canvas px, picked so the on-screen spacing stays between
  24 and 48 pt: density reads the same at every zoom and the grid always lands
  on the same points across octaves.
- Dots are skipped where the page covers them and painted before the page
  texture, so the artwork and its texture are untouched; the page edge stays
  crisp because the grid never draws on top of it.
- The scroll area clips its painter to the page viewport, which hid every dot
  outside the page; the grid paints through a panel-clipped painter instead.
- Fit used to fill the viewport exactly, leaving no board. It now keeps a 28 pt
  margin, and content that fits is centred, so the sheet floats instead of
  pinning to the top-left. Content that overflows is not moved, so zoomed-in
  panning behaves exactly as before.
- Startup zoom is still 100% (a 1280x800 page larger than a phone screen fills
  the viewport, and no board exists at that zoom). Pinch out or tap Fit to see
  the board on a small screen.
