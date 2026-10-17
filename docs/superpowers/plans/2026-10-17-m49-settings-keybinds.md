# M49 — Settings Persistence + Keybind Remap — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A cross-platform `settings.json` store (theme, panels, zoom, recents, shortcuts) seeded once from the legacy eframe storage, plus rebindable keyboard shortcuts with a Shortcuts window, and a configurable `Shift+drag = brush size` canvas modifier.

**Architecture:** The engine crate owns the pure models (`shortcuts.rs`: actions, chords, conflict lookup, serde-safe storing; `settings.rs`: the `Settings` envelope with JSON helpers). The app crate owns IO (`settings.rs`: platform paths, atomic write, seed/corrupt policy) and UI (`lib.rs`: dispatcher with exact-chord matching, Shortcuts window, capture mode, size-drag). Persistence uses a per-frame snapshot diff: discrete fields save immediately, zoom coalesces (quiet ≥ 1.5 s + on exit).

**Tech Stack:** Rust, `eframe`/`egui` 0.36 (verified against registry source), `serde` + `serde_json` (already engine deps; the app adds no new dependencies). No GUI types in the engine crate.

**Spec:** `docs/superpowers/specs/2026-10-17-m49-settings-keybinds-design.md` (includes two addenda made during planning: drag modifiers are edited via Ctrl/Shift checkboxes — Alt stays reserved for the pick loupe; Esc reverts a size drag only while the drag is still active).

## Global Constraints

- Gates (standing, CI parity): `cargo test` workspace green; scoped `cargo fmt -p procraft-engine -p procraft`; `cargo clippy -p procraft-engine -p procraft --all-targets -- -D warnings`; `cargo check` for both Android targets (`aarch64-linux-android`, `x86_64-linux-android`). Never whole-tree `cargo fmt`.
- TDD: failing test first for every behavior; view-only wiring may be flagged as an exception (repo precedent), but every pure function named below has tests.
- Local commits per task (no push). Commit style: follow `git log --oneline -10`; suggested prefix `M49:`.
- No new dependencies in either crate.
- Engine crate stays GUI-free (no egui types). All egui↔engine mapping lives in `apps/procraft/src/lib.rs`.
- Keyboard semantics: exact chord matching (ctrl/shift/alt all equal). Key-repeat events (`repeat: true`) fire actions, matching today's `consume_key` (verified: it matches `pressed: true` with `..` and docs say "Includes key-repeat events").
- Zoom clamp is `0.1..=8.0` everywhere (lib.rs:1163/1167/5805/5813/6363). Brush size clamp is `1.0..=200.0` (lib.rs:4428). Zoom `<= 0` / non-finite is never restored from storage (legacy behavior, lib.rs:789-795).
- Excluded from bindable chords (system keys): Escape, Enter, Backspace, Delete.

## Review Focus

Failure modes the spec implies but no happy-path test covers — each is pinned in the task named; verify these first in review:

1. **Partly-valid / wrong-typed `settings.json`** must never crash or re-seed stale values: defaults + status note + `settings.json.bad` backup; the `m49_seeded` marker blocks re-seeding. (Tasks 3–4 tests + manual repro.)
2. **Ctrl+Shift+Z must fire RedoAlt, not Undo** — today's logical matching lands it in the Ctrl+Z branch first (dead RedoAlt branch at lib.rs:2768-2777). Exact-match tests in Tasks 1 and 5 pin it.
3. **Typing in a text field must not trigger shortcuts** (layer rename / Text dialog / status fields): tool letters stay inert, Esc does not clear the selection. `ctx.text_edit_focused()` guard (Task 5) + manual repro.
4. **Shift+drag with a paint tool must resize, not paint**: no stroke, no history entry, no stabilizer corruption; Shift+wheel pan and Ctrl+MMB zoom keep working; plain drag still paints. (Tasks 5/7 tests + manual repro.)
5. **Persistence edge paths**: mobile first run writes nothing until the first change; desktop seeds once, ever; `zoom <= 0` (Fit) is never restored; kill mid-zoom loses at most the 1.5 s tail. (Tasks 3–4 tests: `init_action`, `should_flush_zoom`.)

---

### Task 1: Engine — shortcuts model (`crates/engine/src/shortcuts.rs`)

**Files:**
- Create: `crates/engine/src/shortcuts.rs`
- Modify: `crates/engine/src/lib.rs:5` (mod list) and `:17-46` (pub use block)

**Interfaces (Produced — later tasks rely on these exact names):**
- `pub enum ToolId { Brush, Eraser, Smudge, Select, Lasso, Fill, Picker, Move, Hand, Liquify, Crop, Shapes, Text }` (Copy, Ord, serde `snake_case`)
- `pub enum ActionId { Tool(ToolId), Undo, Redo, RedoAlt }` (Copy, Ord, PartialEq, Debug) with `pub const ALL: [ActionId; 16]` (tools in the order above, then Undo, Redo, RedoAlt), `pub fn to_key(&self) -> String` (`"tool.brush"`, `"undo"`, `"redo"`, `"redo_alt"`), `pub fn from_key(k: &str) -> Option<ActionId>`, `pub fn label(&self) -> String` ("Brush" … "Undo", "Redo", "Redo (alt combo)")
- `pub enum ChordKey` — letters `A..Z`, `Num0..Num9`, `F1..F12`, `ArrowUp/Down/Left/Right`, `Home`, `End`, `PageUp`, `PageDown`, `Insert`, `Space`, `Tab`, `Comma`, `Period`, `Slash`, `Semicolon`, `Quote`, `Minus`, `Equals`, `OpenBracket`, `CloseBracket`, `Backslash`, `Backtick` (Copy, Ord, PartialEq, serde `snake_case` names) + `pub fn name(&self) -> &'static str` (display: `"B"`, `"0"`, `"F5"`, `"Up"`, `"PgUp"`, `","`, `"]"`, `"\\"`, `` "`" `` …)
- `pub struct KeyChord { pub key: ChordKey, pub ctrl: bool, pub shift: bool, pub alt: bool }` + `pub const fn new(...)`, `pub fn matches(&self, key: ChordKey, ctrl: bool, shift: bool, alt: bool) -> bool` (all four equal), `pub fn format(&self) -> String` (`"Ctrl+Shift+Z"`, `"B"`; prefix order Ctrl, Shift, Alt)
- `pub struct ModSet { pub ctrl: bool, pub shift: bool, pub alt: bool }` + `pub const SHIFT: ModSet`, `pub fn is_empty(&self) -> bool`, `pub fn matches(&self, ctrl: bool, shift: bool, alt: bool) -> bool`
- `pub enum DragActionId { BrushSize }` with `to_key`/`from_key` (`"brush_size"`)
- `pub struct Shortcuts { /* private maps; every ActionId present, value None = explicitly unbound */ }` + `Default`, `Clone`, `PartialEq`, `Debug`, and:
  - `pub fn get(&self, a: ActionId) -> Option<KeyChord>`
  - `pub fn owner_of(&self, chord: KeyChord) -> Option<ActionId>` (exact equal)
  - `pub fn set_binding(&mut self, a: ActionId, chord: KeyChord) -> Result<(), ActionId>` (Err = current owner, unchanged)
  - `pub fn clear_binding(&mut self, a: ActionId)`
  - `pub fn reset_all(&mut self)`
  - `pub fn drag_get(&self, d: DragActionId) -> ModSet`
  - `pub fn set_drag(&mut self, d: DragActionId, set: ModSet)`
  - `pub fn to_stored(&self) -> StoredShortcuts` — only entries differing from defaults (cleared defaults → explicit `null`)
  - `pub fn from_stored(s: &StoredShortcuts) -> (Shortcuts, Vec<String>)` — start from defaults; apply entries; unknown action keys skipped with a warning string; duplicate chord resolution: first (display order) keeps it, later skipped with warning
- `pub struct StoredShortcuts { pub bindings: BTreeMap<String, Option<KeyChord>>, pub drag: BTreeMap<String, ModSet> }` (`#[derive(Default, Clone, PartialEq, Debug, Serialize, Deserialize)]`)
- Defaults pinned: `B`→Tool(Brush), `E`→Tool(Eraser), `F`→Tool(Fill), Ctrl+Z→Undo, Ctrl+Y→Redo, Ctrl+Shift+Z→RedoAlt; every other action unbound; drag `{shift}`.

- [ ] **Step 1: Write the failing tests** in `shortcuts.rs` (`#[cfg(test)] mod tests`, style as `pointer.rs:41-64`)

```rust
#[test]
fn defaults_pin_the_agreed_bindings() {
    let s = Shortcuts::default();
    assert_eq!(s.get(ActionId::Tool(ToolId::Brush)),  Some(KeyChord::new(ChordKey::B, false, false, false)));
    assert_eq!(s.get(ActionId::Tool(ToolId::Eraser)), Some(KeyChord::new(ChordKey::E, false, false, false)));
    assert_eq!(s.get(ActionId::Tool(ToolId::Fill)),   Some(KeyChord::new(ChordKey::F, false, false, false)));
    assert_eq!(s.get(ActionId::Undo), Some(KeyChord::new(ChordKey::Z, true, false, false)));
    assert_eq!(s.get(ActionId::Redo), Some(KeyChord::new(ChordKey::Y, true, false, false)));
    assert_eq!(s.get(ActionId::RedoAlt), Some(KeyChord::new(ChordKey::Z, true, true, false)));
    assert_eq!(s.get(ActionId::Tool(ToolId::Lasso)), None);
    assert_eq!(s.drag_get(DragActionId::BrushSize), ModSet::SHIFT);
}

#[test]
fn exact_match_redoalt_is_not_undo() {
    // The latent bug: logical matching would let Ctrl+Shift+Z match Ctrl+Z.
    let ctrl_z = KeyChord::new(ChordKey::Z, true, false, false);
    assert!(!ctrl_z.matches(ChordKey::Z, true, true, false));
    let s = Shortcuts::default();
    assert_eq!(s.owner_of(KeyChord::new(ChordKey::Z, true, true, false)), Some(ActionId::RedoAlt));
}

#[test]
fn conflict_refuses_and_names_owner() {
    let mut s = Shortcuts::default();
    let err = s.set_binding(ActionId::RedoAlt, KeyChord::new(ChordKey::Z, true, false, false));
    assert_eq!(err, Err(ActionId::Undo));
    assert_eq!(s.get(ActionId::RedoAlt), Some(KeyChord::new(ChordKey::Z, true, true, false)));
}

#[test]
fn clear_reset_and_unbound_survive() {
    let mut s = Shortcuts::default();
    s.clear_binding(ActionId::Undo);
    assert_eq!(s.get(ActionId::Undo), None);
    s.set_binding(ActionId::Redo, KeyChord::new(ChordKey::G, false, false, false)).unwrap();
    s.reset_all();
    assert_eq!(s.get(ActionId::Undo), Some(KeyChord::new(ChordKey::Z, true, false, false)));
    assert_eq!(s.get(ActionId::Redo), Some(KeyChord::new(ChordKey::Y, true, false, false)));
}

#[test]
fn stored_form_is_sparse_and_round_trips() {
    let mut s = Shortcuts::default();
    assert!(s.to_stored().bindings.is_empty(), "defaults store nothing");
    s.clear_binding(ActionId::Undo);
    s.set_binding(ActionId::Tool(ToolId::Brush), KeyChord::new(ChordKey::G, false, false, false)).unwrap();
    s.set_drag(DragActionId::BrushSize, ModSet { ctrl: true, shift: true, alt: false });
    let json = serde_json::to_string(&s.to_stored()).unwrap();
    let back: StoredShortcuts = serde_json::from_str(&json).unwrap();
    let (s2, warns) = Shortcuts::from_stored(&back);
    assert!(warns.is_empty());
    assert_eq!(s2, s);
    assert!(json.contains("\"undo\":null"));
    assert!(json.contains("\"tool.brush\""));
}

#[test]
fn stored_unknown_ids_are_skipped_not_resurrected() {
    let mut stored = StoredShortcuts::default();
    stored.bindings.insert("tool.sparkle".into(), Some(KeyChord::new(ChordKey::Q, false, false, false)));
    stored.bindings.insert("undo".into(), None);
    let (s, warns) = Shortcuts::from_stored(&stored);
    assert_eq!(warns.len(), 1);
    assert!(warns[0].contains("tool.sparkle"));
    assert_eq!(s.get(ActionId::Undo), None);            // explicit null honored
    assert_eq!(s.get(ActionId::Tool(ToolId::Brush)), Some(KeyChord::new(ChordKey::B, false, false, false)));
}

#[test]
fn format_names_chords_like_the_ui() {
    assert_eq!(KeyChord::new(ChordKey::Z, true, true, false).format(), "Ctrl+Shift+Z");
    assert_eq!(KeyChord::new(ChordKey::B, false, false, false).format(), "B");
    assert_eq!(KeyChord::new(ChordKey::F5, false, false, true).format(), "Alt+F5");
    assert_eq!(KeyChord::new(ChordKey::Num0, true, false, false).format(), "Ctrl+0");
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p procraft-engine shortcuts`
Expected: FAIL (module/file not found — compile error is the failure signal).

- [ ] **Step 3: Implement `shortcuts.rs`**

Maps: `bindings: BTreeMap<ActionId, Option<KeyChord>>` initialized by `default_bindings()` containing all 16 actions; `drag: BTreeMap<DragActionId, ModSet>`. `ActionId` needs `Ord` (derive). Serde on `ChordKey`/`KeyChord`/`ModSet` via `#[derive(Serialize, Deserialize)]` + `#[serde(rename_all = "snake_case")]` where needed. `to_stored` compares against `Shortcuts::default()` entry-by-entry. `from_stored` iterates `ActionId::ALL` order for duplicate resolution (first keeps). Update `crates/engine/src/lib.rs`: add `mod shortcuts;` and extend the `pub use` block with `shortcuts::{ActionId, ChordKey, DragActionId, KeyChord, ModSet, Shortcuts, StoredShortcuts, ToolId}`.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p procraft-engine shortcuts`
Expected: all 7 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/engine/src/shortcuts.rs crates/engine/src/lib.rs
git commit -m "M49: engine shortcuts model (chords, actions, conflict lookup, sparse store)"
```

---

### Task 2: Engine — `Settings` envelope (`crates/engine/src/settings.rs`)

**Files:**
- Create: `crates/engine/src/settings.rs`
- Modify: `crates/engine/src/lib.rs` (mod + pub use)

**Interfaces:**
- Consumes: `StoredShortcuts`, `Shortcuts` (Task 1)
- Produces: `pub const SETTINGS_VERSION: u32 = 1;` and

```rust
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]           // missing fields fall back to Default; unknown fields ignored
pub struct Settings {
    pub version: u32,       // 1
    pub theme: String,      // "paper" (app's fresh default is Theme::Paper, lib.rs:626)
    pub show_tools: bool,   // true
    pub show_layers: bool,  // true
    pub zoom: f32,          // 1.0
    pub recent: Vec<String>,          // []
    pub shortcuts: StoredShortcuts,   // Shortcuts::default().to_stored()
}
impl Default for Settings { /* values above */ }
impl Settings {
    pub fn from_json(s: &str) -> Result<Settings, String>;
    pub fn to_json_pretty(&self) -> String;
}
```

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn defaults_are_pinned() {
    let s = Settings::default();
    assert_eq!(s.version, 1);
    assert_eq!(s.theme, "paper");
    assert!(s.show_tools && s.show_layers);
    assert_eq!(s.zoom, 1.0);
    assert!(s.recent.is_empty());
    assert_eq!(s.shortcuts, crate::shortcuts::Shortcuts::default().to_stored());
}

#[test]
fn json_round_trip_is_lossless() {
    let mut s = Settings::default();
    s.theme = "dark".into();
    s.show_tools = false;
    s.zoom = 2.5;
    s.recent = vec!["a.procraft".into(), "b.procraft".into()];
    let s2 = Settings::from_json(&s.to_json_pretty()).unwrap();
    assert_eq!(s2, s);
}

#[test]
fn partial_and_extra_json_fall_back_to_defaults() {
    let s = Settings::from_json(r#"{"theme":"light","future_field":42}"#).unwrap();
    assert_eq!(s.theme, "light");
    assert_eq!(s.zoom, 1.0);           // missing field -> default
    assert!(!s.shortcuts.bindings.contains_key("undo")); // none customized
}

#[test]
fn wrong_types_are_a_clean_error() {
    assert!(Settings::from_json(r#"{"version":"one"}"#).is_err());
    assert!(Settings::from_json("not json").is_err());
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p procraft-engine settings`
Expected: FAIL (module not found).

- [ ] **Step 3: Implement `settings.rs`** (derive-based; `from_json` = `serde_json::from_str` mapping err to `to_string()`; `to_json_pretty` = `to_string_pretty` with `unwrap_or_default`). Declare `mod settings; pub use settings::{Settings, SETTINGS_VERSION};` in `crates/engine/src/lib.rs`.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p procraft-engine settings`
Expected: 4 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/engine/src/settings.rs crates/engine/src/lib.rs
git commit -m "M49: engine settings envelope with JSON helpers"
```

---

### Task 3: App — settings IO module (`apps/procraft/src/settings.rs`)

**Files:**
- Create: `apps/procraft/src/settings.rs`
- Modify: `apps/procraft/src/lib.rs` (add `mod settings;` after line 25; extend the `use procraft_engine::{...}` block with `Settings`)
- Modify: `apps/procraft/src/lib.rs:7328-7334` (`android_main` captures the data dir)

**Interfaces:**
- Consumes: `procraft_engine::Settings` (Task 2)
- Produces (all in `crate::settings`):

```rust
pub const MARKER_KEY: &str = "m49_seeded";
pub const SETTINGS_FILE: &str = "settings.json";
pub enum Inits { LoadFile, SeedLegacy, UseDefaults }
pub fn init_action(file_exists: bool, marker_set: bool) -> Inits;
pub fn settings_dir() -> Option<std::path::PathBuf>;
pub fn settings_path(dir: &std::path::Path) -> std::path::PathBuf;      // dir.join(SETTINGS_FILE)
pub fn read(dir: &std::path::Path) -> Result<Settings, String>;          // io + parse errors
pub fn save_now(dir: &std::path::Path, s: &Settings) -> Result<(), String>; // write .tmp, rename over
pub fn quarantine(dir: &std::path::Path) -> Option<std::path::PathBuf>;  // settings.json -> settings.json.bad
pub fn should_flush_zoom(dirty: bool, last_change: f64, now: f64) -> bool; // dirty && now - last_change >= 1.5
#[cfg(target_os = "android")] pub fn set_android_data_dir(p: std::path::PathBuf); // OnceLock, first wins
```

Path rules: desktop `%APPDATA%\procraft` (fallback `$HOME\procraft`; same base `build_options` uses, lib.rs:7295-7303); Android `ANDROID_DATA_DIR` OnceLock (set from `AndroidApp::internal_data_path()` in `android_main`); iOS `$HOME/Documents`.

- [ ] **Step 1: Write the failing tests** (in `settings.rs`, `#[cfg(test)] mod tests`)

```rust
#[test]
fn init_action_covers_the_three_ways_in() {
    assert_eq!(init_action(true, false), Inits::LoadFile);
    assert_eq!(init_action(true, true), Inits::LoadFile);
    assert_eq!(init_action(false, false), Inits::SeedLegacy);
    assert_eq!(init_action(false, true), Inits::UseDefaults); // marker: seed once, ever
}

#[test]
fn zoom_flushes_only_after_the_quiet_period() {
    assert!(!should_flush_zoom(false, 100.0, 200.0));
    assert!(!should_flush_zoom(true, 100.0, 101.0));
    assert!(should_flush_zoom(true, 100.0, 101.5));
    assert!(should_flush_zoom(true, 100.0, 130.0));
}

#[test]
fn settings_path_is_the_app_dir() {
    let d = std::path::Path::new("base");
    assert_eq!(settings_path(d), d.join("settings.json"));
    assert_eq!(SETTINGS_FILE, "settings.json");
    assert_eq!(MARKER_KEY, "m49_seeded");
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p procraft settings::`
Expected: FAIL (module not found).

- [ ] **Step 3: Implement `settings.rs`**

IO is thin and untested (repo convention for view-only wiring): `read` = `fs::read_to_string` + `Settings::from_json`; `save_now` = `fs::write` to `settings_path(dir).with_extension("json.tmp")` then `fs::rename` over the real path (std rename replaces existing on Windows/Unix); `quarantine` = remove any stale `.bad`, then `fs::rename` to `.bad`, returning the new path. `settings_dir()` per-platform cfg as above. In `lib.rs` `android_main` (7330-7334): `if let Some(p) = app.internal_data_path() { crate::settings::set_android_data_dir(p); }` before storing `options.android_app`.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p procraft settings::`
Expected: 3 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add apps/procraft/src/settings.rs apps/procraft/src/lib.rs
git commit -m "M49: app settings IO (paths, atomic save, seed policy, android data dir)"
```

---

### Task 4: App — load/save wiring (seed, marker, corrupt recovery, two-tier writes)

**Files:**
- Modify: `apps/procraft/src/lib.rs` — struct fields (~386-590), app literal (~600-774), storage block `775-807` (replaced), `impl eframe::App` `save` `2731-2744`, top of `fn ui` `2746` region.

**Interfaces:**
- Consumes: Tasks 1-3 (`Shortcuts`, `Settings`, `settings::*`)
- Produces (app-private):
  - New `ProCraftApp` fields: `shortcuts: Shortcuts`, `settings_dir: Option<PathBuf>`, `settings_snapshot: Settings`, `settings_note: Option<String>`, `zoom_dirty: bool`, `zoom_changed_at: f64`, `show_shortcuts: bool`, `capture: Option<ActionId>`, `size_drag: Option<SizeDrag>` (last three used by Tasks 5-7; declare `struct SizeDrag { base: f32, last_x: f32 }` now).
  - `fn settings_snapshot_from_fields(&self) -> Settings` (theme via `self.theme.as_str()`, clone recent + `self.shortcuts.to_stored()`)
  - `fn apply_settings(&mut self, s: &Settings)` — theme via `Theme::from_str(&s.theme)` (keep current on `None`); `show_tools`/`show_layers` copied; zoom copied only if finite and `> 0.0` (legacy semantics); recent cloned+truncated to `MAX_RECENT`; `self.shortcuts = Shortcuts::from_stored(&s.shortcuts).0`.
  - `fn settings_sync(&mut self, ctx: &egui::Context)` and `fn flush_settings_now(&mut self)` + pure `fn settings_differ(a: &Settings, b: &Settings) -> bool` (zoom excluded — handled separately); `fn save_settings_file(&self, s: &Settings) -> Result<(), String>` (no-op `Ok(())` when `settings_dir` is None, else `crate::settings::save_now`).

**Seed/first-run matrix (must match spec):**
| `settings.json` | `m49_seeded` | Action |
|---|---|---|
| exists | any | `LoadFile`: apply; corrupt → quarantine + note + `UseDefaults` behavior |
| missing | not set | `SeedLegacy`: existing legacy reads (theme/show_tools/show_layers/zoom/recent at 777-807) into fields, write `settings.json`, set marker via `cc.storage.set_string(MARKER_KEY, "1")` |
| missing | set | `UseDefaults`: fields keep their literal fresh defaults, nothing written |
| no dir at all | — | fields keep literal defaults, no file IO ever |

- [ ] **Step 1: Write the failing test** (lib.rs tests mod, ~7336)

```rust
#[test]
fn settings_differ_ignores_zoom_but_catches_discrete_changes() {
    let a = procraft_engine::Settings::default();
    let mut b = a.clone();
    b.zoom = 2.0;
    assert!(!settings_differ(&a, &b), "zoom is coalesced elsewhere");
    b.recent = vec!["x.procraft".into()];
    assert!(settings_differ(&a, &b));
    let mut c = a.clone();
    c.show_tools = !c.show_tools;
    assert!(settings_differ(&a, &c));
    let mut d = a.clone();
    d.shortcuts.bindings.insert("undo".into(), None);
    assert!(settings_differ(&a, &d));
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p procraft settings_differ`
Expected: FAIL (function not defined).

- [ ] **Step 3: Implement**

Order inside `ProCraftApp::new`: build the literal (new fields initialized `Shortcuts::default()`, `None`, `Settings::default()`, `0.0`, `false`); then, reading `cc.storage` once before the literal is consumed: capture legacy values into locals; compute `dir`/`marker_set`/`file_exists`; `match init_action(...)` implementing the matrix above (corrupt path: `settings::quarantine`, `settings_note = Some(format!("Settings were unreadable ({err}); backup kept as settings.json.bad."))`, notes surface in the status line — set `app.status` too); finally `app.settings_snapshot = app.settings_snapshot_from_fields();`. Replace the old always-overwrite `save()` (2731-2744): flush via `flush_settings_now()` (writes only when `settings_differ` or `zoom_dirty`), keep `self.clear_recovery()`, stop writing the legacy keys.

`settings_sync` called as the FIRST thing in `fn ui` (right after the `theme_applied` block, 2747-2750) — syncs the previous frame's changes, robust to early returns:

```rust
// observed now; discrete edits save immediately, zoom coalesces.
let now = ctx.input(|i| i.time);
let cur = self.settings_snapshot_from_fields();
if cur.zoom != self.settings_snapshot.zoom {
    self.zoom_dirty = true;
    self.zoom_changed_at = now;
    self.settings_snapshot.zoom = cur.zoom;
}
if settings_differ(&cur, &self.settings_snapshot) {
    self.settings_snapshot = cur.clone();
    let _ = self.save_settings_file(&cur);     // save_now when dir is Some
    self.zoom_dirty = false;                    // snapshot refreshed wholesale
} else if crate::settings::should_flush_zoom(self.zoom_dirty, self.zoom_changed_at, now) {
    self.zoom_dirty = false;
    self.settings_snapshot = cur;
    let _ = self.save_settings_file(&self.settings_snapshot.clone());
}
```

`flush_settings_now` = same final `save_now` without the timing gate (used by `save()` and `App::save` on desktop exit / iOS backgrounding).

- [ ] **Step 4: Run to verify pass + gates for touched crate**

Run: `cargo test -p procraft settings_differ` then `cargo test -p procraft`
Expected: new test PASS; suite green (view-only wiring exception noted in commit).

- [ ] **Step 5: Commit**

```bash
git add apps/procraft/src/lib.rs
git commit -m "M49: settings load/save wiring (seed once, corrupt recovery, two-tier writes)"
```

---

### Task 5: App — dispatcher with exact chord matching

**Files:**
- Modify: `apps/procraft/src/lib.rs` — replace the Ctrl+Z/Y/Shift+Z block `2751-2777`; guard the Esc block `2778-2799`; add fns near `impl ProCraftApp`.

**Interfaces:**
- Consumes: `Shortcuts`, `ActionId`, `KeyChord`, `ChordKey`, `ToolId` (Task 1); `self.shortcuts` (Task 4); `Tool` enum (lib.rs:94-109)
- Produces (app-private, unit-tested):
  - `fn chord_key_from_egui(k: egui::Key) -> Option<ChordKey>` (exhaustive match; unmapped variants → None)
  - `fn chord_from_event(ev: &egui::Event) -> Option<KeyChord>` — matches `egui::Event::Key { key, pressed: true, modifiers, .. }`; repeated events included (matches today's `consume_key`)
  - `fn take_shortcut_action(shortcuts: &Shortcuts, ctx: &egui::Context) -> Option<ActionId>` — finds the FIRST `pressed: true` event whose exact chord has an owner, removes that event from `ctx.input_mut(|i| i.events)` and returns the action
  - `fn tool_from_id(id: ToolId) -> Tool` (exhaustive)
  - methods: `fn dispatch_shortcuts(&mut self, ctx)`, `fn apply_action(&mut self, action: ActionId)`
  - Imports: extend the big `use procraft_engine::{...}` block (lib.rs:9-21) with `ActionId, ChordKey, DragActionId, KeyChord, ModSet, Shortcuts, ToolId`.

- [ ] **Step 1: Write the failing tests** (lib.rs tests mod)

```rust
fn key_ev(key: egui::Key, mods: egui::Modifiers) -> egui::Event {
    egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: mods }
}

#[test]
fn chord_from_event_maps_letters_digits_and_fkeys() {
    let b = chord_from_event(&key_ev(egui::Key::B, egui::Modifiers::NONE)).unwrap();
    assert_eq!(b, KeyChord::new(ChordKey::B, false, false, false));
    let n0 = chord_from_event(&key_ev(egui::Key::Num0, egui::Modifiers::CTRL)).unwrap();
    assert_eq!(n0, KeyChord::new(ChordKey::Num0, true, false, false));
    let f5 = chord_from_event(&key_ev(egui::Key::F5, egui::Modifiers::NONE)).unwrap();
    assert_eq!(f5, KeyChord::new(ChordKey::F5, false, false, false));
    assert!(chord_from_event(&key_ev(egui::Key::Escape, egui::Modifiers::NONE)).is_none());
    assert!(chord_from_event(&key_ev(egui::Key::Enter, egui::Modifiers::NONE)).is_none());
}

#[test]
fn exact_matching_sends_ctrl_shift_z_to_redoalt() {
    let s = Shortcuts::default();
    let ev = key_ev(egui::Key::Z, egui::Modifiers::CTRL | egui::Modifiers::SHIFT);
    let chord = chord_from_event(&ev).unwrap();
    assert_eq!(s.owner_of(chord), Some(ActionId::RedoAlt));
    let ev2 = key_ev(egui::Key::Z, egui::Modifiers::CTRL);
    assert_eq!(s.owner_of(chord_from_event(&ev2).unwrap()), Some(ActionId::Undo));
}

#[test]
fn tool_id_round_trips_to_tool() {
    assert_eq!(tool_from_id(ToolId::Brush), Tool::Brush);
    assert_eq!(tool_from_id(ToolId::Text), Tool::Text);
    assert_eq!(tool_from_id(ToolId::Liquify), Tool::Liquify);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p procraft chord_from_event`
Expected: FAIL (functions not defined). (`Modifiers::CTRL | Modifiers::SHIFT` — if `|` is not implemented, use `egui::Modifiers::CTRL.plus(egui::Modifiers::SHIFT)` which the codebase already uses at 2770.)

- [ ] **Step 3: Implement**

`dispatch_shortcuts` replaces 2751-2777:

```rust
fn dispatch_shortcuts(&mut self, ctx: &egui::Context) {
    if self.capture.is_some() { return; }          // capture mode owns the keys
    if ctx.text_edit_focused() { return; }         // typing guard (spec call #6)
    while let Some(action) = take_shortcut_action(&self.shortcuts, ctx) {
        self.apply_action(action);
    }
}
```

`take_shortcut_action` (pure w.r.t. app state; ctx for events only):
`ctx.input_mut(|i| { let pos = i.events.iter().position(|ev| chord_from_event(ev).and_then(|c| shortcuts.owner_of(c)).is_some())?; let ev = i.events.remove(pos); shortcuts.owner_of(chord_from_event(&ev)?) })`. The loop terminates: each iteration removes one event.

`apply_action` reuses today's code paths and status strings verbatim (lib.rs:2752-2777): `Tool(t)` → `self.tool = tool_from_id(t)` (identical effect to the toolbar's `radio_value`, 3749+); `Undo` → `if self.doc.undo() { self.ghost_dirty = true; self.status = "Undone.".into(); }`; `Redo | RedoAlt` → `redo()` with "Redone.".

Esc block (2778+): wrap the existing condition with `!ui.ctx().text_edit_focused() &&` — the rest of the Esc behavior stays byte-identical. Call site: insert `self.dispatch_shortcuts(ui.ctx());` where 2751's comment block was (before the Esc block).

- [ ] **Step 4: Run to verify pass + suite**

Run: `cargo test -p procraft` — all PASS, suite green.
Manual repro (flag as view verification): open layer rename, type `b`/`f` → tool unchanged; press Esc while renaming → selection kept; click canvas, Ctrl+Shift+Z → redo (was undo before this task).

- [ ] **Step 5: Commit**

```bash
git add apps/procraft/src/lib.rs
git commit -m "M49: exact-chord dispatcher, typing guard (fixes Ctrl+Shift+Z redo)"
```

---

### Task 6: App — Shortcuts window, capture mode, entry point

**Files:**
- Modify: `apps/procraft/src/lib.rs` — `view_controls` (1160-1206, add entry button), window after About (3568), capture call in `fn ui`, toolbar tooltips (3749+), new fns.

**Interfaces:**
- Consumes: Tasks 1/4/5 (`Shortcuts` API, `capture` field, `chord_from_event`)
- Produces (app-private):
  - `enum CaptureOutcome { Cancel, Clear, Bind(KeyChord), Ignore }`
  - `fn resolve_capture_event(ev: &egui::Event) -> CaptureOutcome` — Escape → `Cancel` FIRST (before chord mapping), Delete/Backspace → `Clear`, else `chord_from_event` → `Bind`, otherwise `Ignore`
  - `fn handle_capture(&mut self, ctx: &egui::Context)` — called in `fn ui` right BEFORE `dispatch_shortcuts`; removes the first `pressed: true` event and acts: `Cancel` → `capture = None`; `Clear` → `clear_binding`, `capture = None`, status; `Bind(c)` → `set_binding`: Ok → `capture = None`, status `"Bound {label} to {chord.format()}"`; Err(owner) → status `"{chord} is used by {owner label}"`, capture stays active
  - `fn shortcuts_window(&mut self, ctx: &egui::Context)` + `fn action_label(&self, a: ActionId) -> String` (Tool → `tool_label(tool_from_id(t), self.has_symbols)`, others → `ActionId::label()`)

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn capture_resolution_pins_esc_delete_and_bind() {
    let esc = key_ev(egui::Key::Escape, egui::Modifiers::NONE);
    assert!(matches!(resolve_capture_event(&esc), CaptureOutcome::Cancel));
    let del = key_ev(egui::Key::Delete, egui::Modifiers::NONE);
    assert!(matches!(resolve_capture_event(&del), CaptureOutcome::Clear));
    let back = key_ev(egui::Key::Backspace, egui::Modifiers::NONE);
    assert!(matches!(resolve_capture_event(&back), CaptureOutcome::Clear));
    let b = key_ev(egui::Key::B, egui::Modifiers::NONE);
    assert!(matches!(resolve_capture_event(&b), CaptureOutcome::Bind(c)
        if c == KeyChord::new(ChordKey::B, false, false, false)));
    let enter = key_ev(egui::Key::Enter, egui::Modifiers::NONE);
    assert!(matches!(resolve_capture_event(&enter), CaptureOutcome::Ignore));
    let release = egui::Event::Key { key: egui::Key::B, physical_key: None, pressed: false, repeat: false, modifiers: egui::Modifiers::NONE };
    assert!(matches!(resolve_capture_event(&release), CaptureOutcome::Ignore));
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p procraft capture_resolution`
Expected: FAIL (not defined).

- [ ] **Step 3: Implement the window** (view-only; About pattern, lib.rs:3547-3568)

`egui::Window::new("Shortcuts").open(&mut self.show_shortcuts).anchor(CENTER_CENTER).resizable(false).collapsible(false).show(...)` containing a `ScrollArea::vertical().max_height(420.0)`: one row per `ActionId::ALL` — label (min width ~150), button showing `shortcuts.get(a).map(|c| c.format()).unwrap_or("—")`; while `capture == Some(a)` the button reads "Press keys… (Esc cancels, Del clears)"; clicking sets `capture = Some(a)`; a small "✕" per row clears. Then separator, the drag row: label "Drag on canvas — Brush size", two checkboxes Ctrl/Shift bound to a local `ModSet` copy (Alt intentionally absent — reserved for the loupe), applying via `set_drag` only when the result is non-empty; "Reset all" button → `reset_all()` + status. Entry button in `view_controls` after the About button (1205): `if ui.button(/* "⌨ Shortcuts" or "Shortcuts" per has_symbols */).clicked() { if in_menu { ui.close(); } self.show_shortcuts = !self.show_shortcuts; }`. Toolbar polish: chain `.on_hover_text` on each `radio_value` (3749+) showing `"… — binding"` or `"— unbound"`. Wire `handle_capture` before `dispatch_shortcuts` in `fn ui`.

- [ ] **Step 4: Run to verify pass + suite**

Run: `cargo test -p procraft` — all PASS.
Manual repro: open window, rebind Brush to `G`, reload app → still `G` (settings.json); bind a taken chord → refusal names the owner; Delete clears; Reset all restores B/E/F; capture swallows keys (canvas does not react).

- [ ] **Step 5: Commit**

```bash
git add apps/procraft/src/lib.rs
git commit -m "M49: shortcuts window, capture mode, entry button, tool binding tooltips"
```

---

### Task 7: Shift+drag = brush size

**Files:**
- Modify: `crates/engine/src/pointer.rs` (add `size_drag_factor` + tests, export at engine `lib.rs:38` pub use)
- Modify: `apps/procraft/src/lib.rs` — canvas block after the Ctrl+MMB block (ends 6372, before `if self.tool == Tool::Select` at 6374); Esc branch (2778+); `SizeDrag` usage.

**Interfaces:**
- Produces: `pub fn size_drag_factor(dx: f32) -> f32` (engine) — `1.25f32.powf(dx / 120.0)` (drag right grows; same 1.25/120 px step as `zoom_drag_factor`, pointer.rs:20-22); app `fn drag_size(cur: f32, dx: f32) -> f32` = `(cur * size_drag_factor(dx)).clamp(1.0, 200.0)` (the slider range, lib.rs:4428)
- Consumes: `Shortcuts::drag_get` + `ModSet::matches` (Task 1); `self.size_drag` (Task 4)

- [ ] **Step 1: Write the failing tests**

Engine (`pointer.rs` tests mod, next to `zoom_drag_factor_matches_wheel_steps`):

```rust
#[test]
fn size_drag_factor_mirrors_the_zoom_step() {
    assert_eq!(size_drag_factor(0.0), 1.0);
    assert_eq!(size_drag_factor(120.0), 1.25);            // drag right = grow
    assert!((size_drag_factor(-120.0) - 0.8).abs() < 1e-6);
    assert!(size_drag_factor(60.0) > 1.0 && size_drag_factor(60.0) < 1.25);
}
```

App (lib.rs tests mod):

```rust
#[test]
fn drag_size_clamps_to_the_slider_range() {
    assert_eq!(drag_size(50.0, 0.0), 50.0);
    assert_eq!(drag_size(50.0, 120.0), 62.5);
    assert_eq!(drag_size(1.0, -120.0), 1.0);      // floor
    assert_eq!(drag_size(200.0, 120.0), 200.0);   // ceiling
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p procraft-engine size_drag_factor` and `cargo test -p procraft drag_size`
Expected: FAIL (not defined).

- [ ] **Step 3: Implement the canvas block**

After the Ctrl+MMB block (6372), before the tool blocks (6374):

```rust
// Brush-size drag (default Shift, configurable): while the bound
// modifiers are held with a paint tool, a primary drag scales size by
// horizontal travel; release keeps it, Esc during the drag reverts.
let size_mod = self.shortcuts.drag_get(DragActionId::BrushSize);
let size_mods = ui.ctx().input(|i| size_mod.matches(i.modifiers.ctrl, i.modifiers.shift, i.modifiers.alt));
let paint_tool = matches!(self.tool, Tool::Brush | Tool::Eraser | Tool::Smudge);
if let Some(sd) = &mut self.size_drag {
    if resp.dragged_by(egui::PointerButton::Primary) {
        if let Some(pos) = resp.hover_pos() {
            self.brush.size = drag_size(self.brush.size, pos.x - sd.last_x);
            sd.last_x = pos.x;
        }
    }
    if resp.drag_stopped() {
        self.status = format!("Brush size {:.0}", self.brush.size);
        self.size_drag = None;
    }
    self.painting = false;
    self.last = None;
    return;
} else if paint_tool && size_mods && !gesture && resp.drag_started_by(egui::PointerButton::Primary) {
    self.size_drag = Some(SizeDrag {
        base: self.brush.size,
        last_x: resp.hover_pos().map(|p| p.x).unwrap_or(0.0),
    });
    self.painting = false;
    self.last = None;
    return;
}
```

Esc: extend the guarded block (2778+): if `self.size_drag.take()` is `Some(sd)` → `self.brush.size = sd.base`, `self.painting = false; self.last = None;`, status "Brush size reverted." — else the existing selection-clearing behavior runs unchanged. (Ring preview at 5996 reads `self.brush.size` live — no extra work. Mobile is unaffected: no modifier keys.)

Deliberate interaction notes (verified against the repo): start-gate requires the exact modifier set, so plain drag still paints and Shift+wheel (5889) / Ctrl+MMB (6356) are untouched; `gesture` (two-finger) blocks it on touch; Alt is not offered in the UI, so the loupe (6202) cannot be shadowed.

- [ ] **Step 4: Run to verify pass + suite**

Run: `cargo test -p procraft-engine` and `cargo test -p procraft` — all PASS.
Manual repro: Brush tool, Shift+drag right → ring grows, no stroke, no undo entry; release keeps; Shift+drag left past 1 px → floors at 1; Shift+drag then Esc mid-drag → size snaps back; plain drag paints normally; Shift+wheel pans.

- [ ] **Step 5: Commit**

```bash
git add crates/engine/src/pointer.rs crates/engine/src/lib.rs apps/procraft/src/lib.rs
git commit -m "M49: Shift+drag brush size (engine factor + canvas gesture)"
```

---

### Task 8: Milestone close — full gates, docs, roadmap

**Files:**
- Modify: `ROADMAP.md` (Batch 7 / M49 lines — match the file's existing done-marker style), `docs/superpowers/specs/2026-10-17-m49-settings-keybinds-design.md` (status line at top → "Status: implemented (build `<sha>`)").

- [ ] **Step 1: Full gates (CI parity)**

Run, in order, expecting green:
1. `cargo test` (workspace)
2. `cargo fmt -p procraft-engine -p procraft --check` (then fix with the same minus `--check` if needed; scoped only)
3. `cargo clippy -p procraft-engine -p procraft --all-targets -- -D warnings`
4. `cargo check -p procraft --target aarch64-linux-android` and `cargo check -p procraft --target x86_64-linux-android` (same setup the CI workflow uses)

- [ ] **Step 2: Cross-platform smoke (manual, flag as view verification)**

Desktop: restart → theme/panels/zoom/recents/shortcuts restored; delete `settings.json` manually → defaults + `settings.json.bad` + status note; rebinding survives restart. Android (when built later): fresh install → no file until a change; change theme → file appears in app data dir; restart → restored. Confirm `m49_seeded` marker exists in `ui.json` after the first desktop upgrade run.

- [ ] **Step 3: Update ROADMAP + spec status, commit**

```bash
git add ROADMAP.md docs/superpowers/specs/2026-10-17-m49-settings-keybinds-design.md
git commit -m "M49: close-out — gates green, roadmap updated"
```

(Follow-up beyond this plan, per ROADMAP: close-out builds — Windows + iOS sideload + Android — and the GitHub release continuing the v0.8.x series. M50 editable text layers remains queued.)
