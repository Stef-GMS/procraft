# M49 — Settings persistence + keybind remap (design)

Date: 2026-10-17. Status: implemented (build 998cbce). Batch 7 kickoff.

## Intent

Two user-facing needs, one foundation:

1. **Nothing persists user settings on mobile, and desktop persists only a
   few UI bits.** Theme, panel visibility, zoom, and recent files survive
   on desktop (eframe storage at `%APPDATA%\procraft\ui.json`), but iOS
   and Android are session-only (`persistence_path = None`), and none of
   the art-affecting preferences are captured at all.
2. **Remappable shortcuts.** A Discord user asked to remap keys; today
   the keyboard surface is hardcoded: Ctrl+Z / Ctrl+Y / Ctrl+Shift+Z,
   Esc, Enter. The user (product owner) wants custom shortcuts for
   actions — their examples: `B` brush, `E` eraser, `F` fill, and
   Shift+drag = brush size.

The settings store is the foundation; shortcuts are the first feature on
top of it.

## Goals

- A cross-platform settings store (Windows, iOS sideload, Android) that
  survives restarts, seeded once from the legacy desktop storage.
- Shortcuts: an action registry, default bindings (tool keys + existing
  undo/redo), a Shortcuts UI to rebind, persisted.
- A canvas drag modifier: "Brush size (drag)", default Shift+drag on
  canvas, configurable combination, live ring preview.
- Fix accidental shortcut firing while typing (text fields focused).

## Non-goals (this milestone)

- Remapping mouse/pen buttons or touch gestures. Keyboard keys and
  modifier-holds only.
- Remapping Esc/Enter (system-level cancel/confirm, stay fixed).
- Custom brush preset persistence / brush-popup customization (the user
  explicitly left this out).
- Cross-device settings sync.
- Perfect macOS key mapping (Ctrl semantics as today; Command mapping is
  later polish).

## Anti-goals

- Existing hardcoded behavior must not regress: Ctrl+Z / Ctrl+Y work as
  today; Ctrl+Shift+Z must redo as advertised — which fixes today's
  logical-matching quirk where it fired undo instead (see dispatcher
  section). Esc cancel, Shift+wheel horizontal pan, Ctrl+wheel /
  Ctrl+MMB zoom keep working as today when bindings are default.
- Capture mode must never leak keys into the canvas or tools.
- No heavyweight new dependencies.

## Constraints

- Repo gates (standing, CI parity): `cargo test` workspace green, scoped
  `cargo fmt`, `cargo clippy -p procraft-engine -p procraft
  --all-targets -- -D warnings`, and `cargo check` for both Android
  targets (aarch64, x86_64).
- TDD: failing test first per behavior (repo convention; view-only
  wiring may be flagged as an exception like prior milestones).
- Local commits per milestone; push only when the user asks. Batch 7
  closes with Windows + iOS sideload + Android builds and a GitHub
  release continuing the v0.8.x series.

## Where things live

- **Engine crate** (`crates/engine`) — pure model, no UI:
  new `shortcuts.rs`: `ActionId`, `KeyChord`, `Shortcuts` (defaults,
  serde, conflict lookup, chord formatting for display). Unit tests.
  `Settings` struct (serde: version, theme, show_tools, show_layers,
  zoom, recent, shortcuts) also here.
- **App crate** (`apps/procraft`) — IO + UI + dispatch:
  new `apps/procraft/src/settings.rs`: platform path resolution, atomic
  load/save of `Settings` (write `.tmp`, rename). `lib.rs`: dispatcher,
  Shortcuts window, drag modifier, focus guards, entry buttons.

## The settings store

- File: `<app-dir>/settings.json`, JSON, `{"version": 1, ...}`.
- Paths: desktop `%APPDATA%\procraft\` (already created for ui.json);
  Android: `AndroidApp::internal_data_path()` — the handle arrives in
  `android_main` (lib.rs:7330) and is stored into the existing
  `options.android_app`; capture the path there into a `OnceLock<PathBuf>`
  before `run_with`. iOS: `$HOME/Documents`.
- On first run with no `settings.json`, seed from the legacy eframe
  storage values (theme/show_tools/show_layers/zoom/recent, read in
  `ProCraftApp::new`, lib.rs:775-795), write the file, and set an
  `m49_seeded` marker key in eframe storage. Seeding happens **once,
  ever**, gated on the marker: a later missing, deleted, or corrupted
  `settings.json` (quarantined as `settings.json.bad`) does NOT re-seed
  frozen legacy values — it falls back to defaults with a status note
  ("Settings were unreadable; backup kept as settings.json.bad"). The
  legacy keys left in ui.json go dead (eframe storage has no delete);
  that is accepted.
- Write policy, two tiers (my call):
  - Discrete changes (rebind/clear/reset, theme, panel toggles, recent
    list): atomic write immediately.
  - Continuous values (zoom — updated per frame during Ctrl+MMB and
    pinch): dirty flag, flush when quiet for ≥1.5 s, plus on
    `App::save` (exit/background). A kill mid-gesture loses at most a
    brief view-value tail; accepted.
  - Atomic = write `.tmp`, rename over. Parse failure never blocks
    startup: defaults + status note + `.bad` backup (see seed rules
    above; the marker prevents stale re-seeds).
- This is my (the builder's) call: no migration beyond the one-time
  seed; no ron; version field present from day one.

## Shortcuts model (engine)

- `KeyChord { key: ChordKey, ctrl: bool, shift: bool, alt: bool }`.
  `ChordKey` covers letters, digits, arrows, F-keys, common punctuation
  (the exact enum set is a builder detail; keep serde names stable).
- `ActionId`: `Tool(<each of the 13 tools>)`, `Undo`, `Redo`, `RedoAlt`.
  V1 defaults: `B` Brush, `E` Eraser, `F` Fill, Ctrl+Z Undo, Ctrl+Y
  Redo, Ctrl+Shift+Z RedoAlt. All other actions default to unbound and
  are user-bindable. (My call: minimal safe defaults; more tool keys
  later if asked.)
- Drag modifiers are a separate small map: `DragActionId::BrushSize`
  bound to a modifier set, default `{shift}`. Same persistence, same UI
  list. Edited via Ctrl/Shift checkboxes in the window (modifier-only
  "capture" is unreliable across platforms; Alt stays reserved for the
  canvas pick loupe, so it is not offered). At least one modifier must
  stay checked.
- Conflict rule: a chord can belong to exactly one action. Capture
  refuses a taken chord and names the owner in the status line; clearing
  a binding uses Delete during capture or a per-row clear button; a
  "Reset all" button restores defaults. (My call: refuse, don't steal.)
- One action may have `Redo` and `RedoAlt` as separate actions — matches
  today's two redo combos, keeps the model one-chord-per-action.

## App side

- **Dispatcher**: once per frame, before panels, when
  `ctx.memory(|m| m.focused().is_none())` — scan the frame's key events
  for an **exact** chord match (`ctrl/shift/alt` all equal), remove the
  event (consume it), then run the action. Exact matching is required,
  not a style choice: egui's `consume_key` matches *logically* (extra
  Shift/Alt are ignored), which is how today's Ctrl+Shift+Z lands in the
  Ctrl+Z branch first — confirmed in egui 0.36.2 source
  (`Modifiers::matches_logically`) — so the RedoAlt branch at
  lib.rs:2768 is currently dead code and Ctrl+Shift+Z fires undo.
  Exact matching makes dispatch order-independent, enforces
  one-chord-one-action, and fixes that latent bug (Ctrl+Shift+Z → redo
  as advertised). Engine gets a testable `matches_exact` helper.
  Actions reuse existing code paths: tool selection must do exactly what
  toolbar clicks do (lib.rs tool radio handling), undo/redo call the same
  `doc.undo()/redo()` + `ghost_dirty` + status updates as
  lib.rs:2752-2777.
- **Focus guard fix**: the current Ctrl+Z and Esc handling
  (lib.rs:2751-2799) fires while a TextEdit is focused; the dispatcher
  and the (kept) Esc path get guarded on text focus. Esc when not
  focused keeps today's behavior.
- **Capture mode**: when the Shortcuts window is capturing, it consumes
  raw key events first (before dispatcher and Esc handling); Esc cancels
  capture and is consumed; Delete clears the row's binding; the capture
  state is a single `Option<ActionId or DragActionId>` in the app.
- **Drag modifier — Brush size**: with a paint tool (Brush/Eraser/
  Smudge) active, the bound modifier hold + primary press on canvas
  starts size-drag instead of painting: horizontal travel scales size as
  `1.25^(dx/120)` (mirrors the Ctrl+MMB zoom factor precedent,
  engine pointer.rs), clamped to the same brush size range as the UI
  slider; the existing brush cursor ring previews live; release keeps
  it, Esc reverts to the pre-drag size; no history entry, no undo.
- **Shortcuts window**: `egui::Window` following the About pattern
  (lib.rs:3547): title "Shortcuts", rows = action name + current chord
  button (click to capture) + clear; footer: Reset all. Entry points:
  "Shortcuts…" button in the wide-bar cluster near theme/About
  (view_controls, lib.rs:1160) and the same item inside the narrow-bar
  View menu (lib.rs:3240). Visible on all platforms.
- **Polish (cheap)**: tool buttons get a tooltip with their current
  binding; status line announces rebinds and conflicts.

## TDD targets

Engine: defaults spec'd; serde round-trip; unknown ids ignored and
missing ids defaulted on load; conflict lookup finds the owner; chord
formatting ("Ctrl+Shift+Z", "B"). App: `size_drag_factor(dx)` (identity
at 0, monotonic, clamped); modifier-match helper; tool-action application
through the same shared fn the toolbar uses. Wiring (window, buttons,
entry points) is view-only — gates + user repro, consistent with repo
precedent.

## What's left to the builder

- Exact `ChordKey` enum coverage; serde field names.
- Exact hook order in `update()` and where the size-drag branch slots
  into the canvas pointer chain (after gates, before painting).
- Brush size clamp constants (reuse the slider's).
- Tooltip/copy wording.

## My calls (check these while reading)

1. `settings.json` in the same per-app dir as ui.json; one-time seed from
   eframe storage; eframe keeps only window geometry afterwards.
2. Minimal default bindings: B/E/F + the existing three undo/redo combos;
   everything else unbound.
3. Conflict = refuse with the owner named; Delete clears; Reset all
   restores defaults.
4. Drag modifier configurable (default Shift), paint tools only, primary
   button only, Esc during the drag reverts (after release the size
   stays; Esc then behaves as today). UI: Ctrl/Shift checkboxes, Alt
   excluded (loupe).
5. Shortcuts entry in the wide-bar cluster + View menu; window on all
   platforms.
6. Focus guard: no shortcut dispatch while any text field is focused
   (fixes today's typing + Ctrl+Z / Esc quirk).
7. Exact chord matching in the dispatcher (order-independent; fixes the
   latent Ctrl+Shift+Z → undo bug caused by egui's logical matching).
8. Seed once, ever — gated by an `m49_seeded` marker in eframe storage;
   corruption/missing file later falls back to defaults, never re-seeds
   stale legacy values.
9. Two-tier writes: discrete changes immediate, continuous values
   (zoom) coalesced (flush when quiet ≥1.5 s + on save/exit).
