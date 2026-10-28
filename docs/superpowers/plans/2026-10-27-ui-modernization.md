# ProCraft UI Modernization (B2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Restyle ProCraft's existing layout into a modern Procreate-like design system using per-device tokens plus four custom primitives, without moving any panel or changing any behaviour.

**Architecture:** A new `apps/procraft/src/design.rs` module owns every colour, radius, spacing, control-height and type-size value, converts them into a complete `egui::Style`, and implements four drawing primitives (pill button, section card, thick slider, sun/moon toggle). `lib.rs` keeps its current structure and just calls into it, replacing `Theme::visuals`, scattered literals, and `ui.button`/`ui.heading` call sites.

**Tech Stack:** Rust, egui 0.36.2 / eframe 0.36.2, single `lib.rs` app crate with a `procraft-engine` engine crate. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-10-27-ui-modernization-design.md`

## Global Constraints

- No new dependencies; no new files besides `apps/procraft/src/design.rs`.
- egui 0.36.2 API only: styling goes through `Context::style_of`, `Context::set_style_of`, `Context::set_theme`. There is **no** `Context::style()` or `set_style()`.
- Do not change `bar_layout()`'s width rules, panel `default_size`, or panel ids (`tools-compact`, `layers-wide`, `menu`, `tool-options`, `status`).
- Do not move, merge, or delete UI controls. No layout reorganization. Menus, scroll areas, `DragValue`s and stock sliders stay stock widgets.
- Palette values are fixed by the spec table; only failing contrast/fit tests may move them, and the commit message must say which.
- Touch classes (`Tablet`, `Narrow`) use `control_h = 44.0`; `Wide` uses `26.0`.
- Existing 77 tests must keep passing. Expected constants in `quick_bar_touch_geometry_is_finger_sized` may be updated; coverage may not be deleted.
- Chrome must not gain a row: `Wide`/`Tablet` top bar 1 row, `Narrow` top bar ≤ 2 rows, tool-options bar 1 row.
- `cargo` must run with cwd `D:\Dev\procraft`. A piped `cargo build` may report exit code 1 from stderr progress; judge by the `Finished` line.

## Review Focus

- **Resizing or rotating across the 600 px boundary while running.** Control heights change with device class, so the style cache must key on `(Theme, BarLayout)`, not theme alone. A reasonable user expects the bar to re-measure immediately, not after restart.
- **An existing profile with `"theme": "paper"`.** It must load as Paper, and Paper must remain selectable at every width. A reasonable user expects their saved theme to be respected and not silently replaced by the new default.
- **A phone in portrait with the Selection tool (7 quick controls).** The options row scrolls horizontally; slider tracks must not swallow the drags, or the row becomes un-scrollable. A reasonable user expects to flick the row sideways.
- **The Swatches section at any saved panel width.** `swatch_grid` pins 224 px so Starter lays out 3 × 8; card padding must not shrink it. A reasonable user expects the palette to keep its grid.
- **An OS light/dark switch while the app is open.** `ThemePreference` must be pinned to the app's theme so an unstyled egui slot never appears mid-session. A reasonable user expects the window not to half-repaint in a different palette.

---

### Task 1: Tokens and the style pipeline

**Files:**
- Create: `apps/procraft/src/design.rs`
- Modify: `apps/procraft/src/lib.rs` (module declaration near the top; `Theme`/`BarLayout` visibility; `theme_applied` field at `:779` and `:1053`; style application at `:5122-5125`; remove `Theme::visuals` at `:442-455`)
- Test: inside `apps/procraft/src/design.rs`

**Interfaces:**
- Consumes: existing `crate::Theme`, `crate::BarLayout`, `crate::bar_layout`.
- Produces:
  - `pub(crate) struct Tokens` with public fields `class: BarLayout`; `surface_0..surface_3: Color32`, `stroke`, `text_hi`, `text_lo`, `accent`, `accent_soft`, `danger: Color32`; `radius_control/radius_card: u8`; `pad_x`, `pad_y`, `pad_x_card`, `pad_y_card`, `gap`, `control_h`, `slider_track`, `slider_width_wide`, `slider_width_touch`, `type_body`, `type_small`, `type_heading: f32`.
  - `pub(crate) fn tokens(theme: Theme, class: BarLayout) -> Tokens`
  - `impl Tokens { pub(crate) fn apply(&self, base: &egui::Style) -> egui::Style }`
  - `pub(crate) fn contrast_ratio(a: Color32, b: Color32) -> f32`
  - `pub(crate) fn rgb(r: u8, g: u8, b: u8) -> Color32`, `pub(crate) fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color32`

- [ ] **Step 1: Write the failing tests**

Create `apps/procraft/src/design.rs` containing only `use` lines and this test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BarLayout, Theme};

    #[test]
    fn touch_classes_get_finger_sized_controls() {
        assert_eq!(tokens(Theme::Dark, BarLayout::Wide).control_h, 26.0);
        assert_eq!(tokens(Theme::Dark, BarLayout::Tablet).control_h, 44.0);
        assert_eq!(tokens(Theme::Dark, BarLayout::Narrow).control_h, 44.0);
        assert_eq!(tokens(Theme::Dark, BarLayout::Tablet).slider_track, 10.0);
        assert_eq!(tokens(Theme::Dark, BarLayout::Wide).slider_track, 6.0);
    }

    #[test]
    fn secondary_text_clears_aa_contrast_on_panels() {
        for theme in [Theme::Dark, Theme::Light, Theme::Paper] {
            let t = tokens(theme, BarLayout::Wide);
            let ratio = contrast_ratio(t.text_lo, t.surface_1);
            assert!(ratio >= 4.5, "{theme:?} text_lo on surface_1 = {ratio:.2}");
        }
    }

    #[test]
    fn primary_text_clears_aaa_contrast_on_panels() {
        for theme in [Theme::Dark, Theme::Light, Theme::Paper] {
            let t = tokens(theme, BarLayout::Wide);
            let ratio = contrast_ratio(t.text_hi, t.surface_1);
            assert!(ratio >= 7.0, "{theme:?} text_hi on surface_1 = {ratio:.2}");
        }
    }

    #[test]
    fn device_class_changes_metrics_so_the_cache_must_key_on_both() {
        let wide = tokens(Theme::Dark, BarLayout::Wide);
        let tablet = tokens(Theme::Dark, BarLayout::Tablet);
        assert_ne!(wide.control_h, tablet.control_h);
        assert_ne!(wide.pad_x, tablet.pad_x);
    }

    #[test]
    fn apply_writes_the_palette_and_metrics_into_the_style() {
        let t = tokens(Theme::Dark, BarLayout::Wide);
        let s = t.apply(&egui::Style::default());
        assert_eq!(s.visuals.panel_fill, t.surface_1);
        assert_eq!(s.visuals.extreme_bg_color, t.surface_0);
        assert_eq!(s.visuals.widgets.inactive.bg_stroke.color, t.stroke);
        assert_eq!(s.spacing.interact_size.y, 26.0);
        assert_eq!(s.spacing.slider_rail_height, 6.0);
        // The palette wins over whatever the base style was, so writing the
        // Light slot with Paper never inherits a dark base.
        let paper = tokens(Theme::Paper, BarLayout::Wide);
        let dark_base = tokens(Theme::Dark, BarLayout::Wide).apply(&egui::Style::default());
        assert_eq!(paper.apply(&dark_base).visuals.panel_fill, paper.surface_1);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p procraft --lib design:: 2>&1 | Select-String -Pattern 'error\\[|test result'`
Expected: compile error — `tokens` / `contrast_ratio` not defined.

- [ ] **Step 3: Implement the token table and `apply`**

In `design.rs`, add `use egui::{Color32, CornerRadius, FontId, Stroke, Style, TextStyle};` and `use crate::{BarLayout, Theme};`, the two structs above, and the palette exactly as the spec's table (Dark `surface_0 = rgb(0x13,0x13,0x15)` … Paper `surface_3 = rgb(0xE7,0xDC,0xC9)`).

`tokens()` returns colours by `match theme` and geometry by `match class`:

| field | Wide | Tablet | Narrow |
|---|---|---|---|
| `control_h` | 26.0 | 44.0 | 44.0 |
| `gap` | 6.0 | 8.0 | 6.0 |
| `pad_x` / `pad_y` | 10.0 / 6.0 | 14.0 / 10.0 | 10.0 / 8.0 |
| `pad_x_card` / `pad_y_card` | 6.0 / 8.0 | 8.0 / 10.0 | 8.0 / 10.0 |
| `radius_control` | 8 | 12 | 12 |
| `radius_card` | 10 | 14 | 14 |
| `slider_track` | 6.0 | 10.0 | 10.0 |
| `slider_width_wide` | 110.0 | 110.0 | 110.0 |
| `slider_width_touch` | 130.0 | 130.0 | 130.0 |
| `type_body` / `type_small` / `type_heading` | 14.0 / 12.0 / 15.0 | 15.0 / 13.0 / 16.0 | 15.0 / 13.0 / 16.0 |

`accent_soft = rgba(accent.r, accent.g, accent.b, 61)` (24%).

`apply` clones `base`, then sets, in this order: `visuals.panel_fill = surface_1`; `window_fill = surface_2`; `extreme_bg_color = surface_0`; `faint_bg_color = surface_2`; `window_stroke = Stroke::new(1.0, stroke)`; for each of `noninteractive / inactive / hovered / active / open` set `corner_radius = CornerRadius::same(radius_control)`; `noninteractive.fg_stroke = Stroke::new(1.0, text_hi)`; `inactive.weak_bg_fill = surface_2`, `inactive.bg_fill = surface_3`, `inactive.bg_stroke = Stroke::new(1.0, stroke)`, `inactive.fg_stroke = Stroke::new(1.0, text_hi)`; `hovered.weak_bg_fill = surface_3`, `hovered.bg_stroke = Stroke::new(1.0, accent)`; `active` same as `hovered`; `open = hovered`; `selection.bg_fill = accent_soft`, `selection.stroke = Stroke::new(1.0, accent)`; `visuals.override_text_color = None`; `spacing.item_spacing = vec2(gap, gap)`; `spacing.button_padding = vec2(pad_x, pad_y)`; `spacing.interact_size = vec2(control_h * 1.4, control_h)` (the height is the minimum every clickable must meet, so this is what makes touch targets 44 px); `spacing.slider_rail_height = slider_track`; `spacing.slider_width = 140.0`; and `text_styles` entries `Body = FontId::proportional(type_body)`, `Button = FontId::proportional(type_body)`, `Small = FontId::proportional(type_small)`, `Heading = FontId::proportional(type_heading)`.

Note `set_theme` only sets a preference and does not rebuild styles, so writing the slot first and then selecting it is safe.

`contrast_ratio` uses WCAG relative luminance: linearise each channel (`c/12.92` below `0.04045`, else `((c+0.055)/1.055)^2.4`), combine as `0.2126r + 0.7152g + 0.0722b`, then `(lighter + 0.05) / (darker + 0.05)`.

- [ ] **Step 4: Run the design tests**

Run: `cargo test -p procraft --lib design:: 2>&1 | Select-String -Pattern 'test result|FAILED|panicked'`
Expected: `test result: ok. 5 passed`.

- [ ] **Step 5: Wire the module into `lib.rs`**

Add `mod design;` beside the existing `mod settings;`. Change `enum Theme` to `pub(crate) enum Theme` and add `Debug` to its derive. Change `enum BarLayout` to `pub(crate) enum BarLayout`. Change the field at `:779` to `theme_applied: Option<(Theme, BarLayout)>` (initialiser at `:1053` stays `None`).

Delete `impl Theme { fn visuals(...) }` entirely (`:442-455`).

Replace the block at `:5122-5125` with:

```rust
let class = bar_layout(
    ui.ctx().viewport_rect().width(),
    cfg!(any(target_os = "ios", target_os = "android")),
);
if self.theme_applied != Some((self.theme, class)) {
    let t = design::tokens(self.theme, class);
    let slot = if self.theme == Theme::Dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    };
    let base = ui.ctx().style_of(slot);
    ui.ctx().set_style_of(slot, t.apply(&base));
    ui.ctx().set_theme(if self.theme == Theme::Dark {
        egui::ThemePreference::Dark
    } else {
        egui::ThemePreference::Light
    });
    self.theme_applied = Some((self.theme, class));
}
```

Also replace the hard-coded warning colour at `:1607` (`ui.colored_label(egui::Color32::from_rgb(200, 120, 40), note)`) with the current theme's `danger` token — that note reports settings failures, so red is the right semantic. This is the only chrome colour the restyle replaces; every other `Color32::from_rgb` in the file is canvas/artwork semantics and stays untouched.

- [ ] **Step 6: Run the whole suite and build**

Run: `cargo test -p procraft --lib 2>&1 | Select-String -Pattern 'test result|failures:'`
Expected: `82 passed` (77 + 5), 0 failed.

Run: `cargo build -p procraft 2>&1 | Select-Object -Last 3`
Expected: `Finished` line, no errors.

- [ ] **Step 7: Commit**

```bash
git add apps/procraft/src/design.rs apps/procraft/src/lib.rs
git commit -m "feat(ui): per-device design tokens and the egui style pipeline"
```

---

### Task 2: Pill button and the top bar

**Files:**
- Modify: `apps/procraft/src/design.rs`
- Modify: `apps/procraft/src/lib.rs` (`view_controls` at `:2155-2208`; top-bar row at `:5412`; Undo/Redo/Clear at `:5640-5646`)

**Interfaces:**
- Consumes: `Tokens`, `tokens` from Task 1.
- Produces:
  - `pub(crate) struct PillSpec<'a> { pub glyph: Option<&'a str>, pub label: Option<&'a str>, pub selected: bool, pub enabled: bool }`
  - `pub(crate) fn pill_button(ui: &mut egui::Ui, t: &Tokens, spec: PillSpec<'_>) -> egui::Response`
  - `pub(crate) fn pill_width(t: &Tokens, text_width: f32) -> f32`

- [ ] **Step 1: Write the failing test**

Append to `design.rs`'s test module:

```rust
#[test]
fn pill_is_at_least_one_control_tall_and_half_height_rounded() {
    let t = tokens(Theme::Dark, BarLayout::Tablet);
    assert_eq!(pill_width(&t, 0.0), 44.0);
    assert_eq!(pill_width(&t, 20.0), 44.0);
    assert!(pill_width(&t, 80.0) >= 80.0 + 2.0 * t.pad_x);
    assert_eq!(pill_radius(&t), 22);
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p procraft --lib design::tests::pill_ 2>&1 | Select-String -Pattern 'error\\[|test result'`
Expected: compile error — `pill_width` / `pill_radius` not defined.

- [ ] **Step 3: Implement the pill primitive**

```rust
pub(crate) fn pill_width(t: &Tokens, text_width: f32) -> f32 {
    (text_width + 2.0 * t.pad_x).max(t.control_h)
}

pub(crate) fn pill_radius(t: &Tokens) -> u8 {
    (t.control_h * 0.5).round() as u8
}
```

`pill_button` allocates `vec2(pill_width(...), control_h)` with `Sense::click()`, paints a `rect_filled` at `CornerRadius::same(pill_radius(t))` using `surface_3` when hovered, `accent` when `selected`, else `surface_2`, plus a 1 px `accent` stroke when hovered or selected, then paints the glyph and label centred in `text_hi` (or `text_lo` when `!enabled`). It returns the response with `.on_hover_text` for `label` when only a glyph is shown.

- [ ] **Step 4: Run the test**

Run: `cargo test -p procraft --lib design::tests::pill_ 2>&1 | Select-String -Pattern 'test result|FAILED'`
Expected: `1 passed`.

- [ ] **Step 5: Adopt pills in the top bar**

Add a helper on `ProCraftApp`:

```rust
fn pill(&mut self, ui: &mut egui::Ui, label: &str) -> bool {
    let t = design::tokens(self.theme, bar_layout(
        ui.ctx().viewport_rect().width(),
        cfg!(any(target_os = "ios", target_os = "android")),
    ));
    design::pill_button(
        ui,
        &t,
        design::PillSpec { glyph: None, label: Some(label), selected: false, enabled: true },
    )
    .clicked()
}
```

Replace `ui.button("...")` with `self.pill(ui, "...")` for: `Gallery`, `Reference` (`:5413`, `:5416`), `Undo`, `Redo`, `Clear layer` (`:5640-5646`), and inside `view_controls`: `Fit`, `Reset rotation`, `About`, `Shortcuts`. Leave the zoom `−`/`+` buttons, the `Rotate°` slider, the File/View/browser `MenuButton`s, and every menu item as they are.

In `view_controls`, pass the existing `about`/`shortcuts` strings (already glyph-gated by `has_symbols`) as the label.

- [ ] **Step 6: Run the suite and eyeball the bar**

Run: `cargo test -p procraft --lib 2>&1 | Select-String -Pattern 'test result|failures:'`
Expected: `83 passed`, 0 failed.

Run: `cargo build --release -p procraft` then launch `target\release\procraft.exe`. Expected: the named top-bar buttons are now pills; the zoo of stock buttons is gone; no button is clipped at 1200 px.

- [ ] **Step 7: Commit**

```bash
git add apps/procraft/src/design.rs apps/procraft/src/lib.rs
git commit -m "feat(ui): pill buttons in the top bar"
```

---

### Task 3: Section cards in both panels

**Files:**
- Modify: `apps/procraft/src/design.rs`
- Modify: `apps/procraft/src/lib.rs` (left panel `:6273`, `:6567`, `:6738`, `:6755`, dynamics block `:6987-7124`; right panel from `:7141`)

**Interfaces:**
- Consumes: `Tokens`, `tokens`.
- Produces:
  - `pub(crate) fn section_card(ui: &mut egui::Ui, t: &Tokens, title: Option<&str>, flush: bool, add: impl FnOnce(&mut egui::Ui))`
  - `pub(crate) fn card_pad(t: &Tokens, flush: bool) -> egui::Vec2`
  - `pub(crate) fn card_available_width(panel_width: f32, t: &Tokens, flush: bool) -> f32` (`panel_width - 16.0 - 2.0 * card_pad.x`)

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn flush_cards_leave_the_swatch_grid_its_full_224px() {
    let t = tokens(Theme::Dark, BarLayout::Wide);
    assert_eq!(card_pad(&t, false).x, 6.0);
    assert_eq!(card_pad(&t, true).x, 0.0);
    assert_eq!(card_available_width(240.0, &t, true), 224.0);
    assert!(card_available_width(240.0, &t, false) < 224.0);
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p procraft --lib design::tests::flush_ 2>&1 | Select-String -Pattern 'error\\[|test result'`
Expected: compile error — `card_pad` not defined.

- [ ] **Step 3: Implement `section_card`**

```rust
pub(crate) fn card_pad(t: &Tokens, flush: bool) -> egui::Vec2 {
    egui::vec2(if flush { 0.0 } else { t.pad_x_card }, t.pad_y_card)
}
```

`section_card` wraps `add` in `egui::Frame::NONE.fill(t.surface_2).stroke(Stroke::new(1.0, t.stroke)).corner_radius(CornerRadius::same(t.radius_card)).inner_margin(Margin::from(card_pad(t, flush)))` — egui 0.36 has no `Frame::new()`, so start from `Frame::NONE`, and `Margin: From<Vec2>` exists — paints the optional title (uppercase, `type_small`, `text_lo`) on its own row with 6 px below, then runs `add`.

`card_available_width` implements the formula above (the `16.0` is the panel's own ~8 px margin each side).

- [ ] **Step 4: Run the test**

Expected: `1 passed`.

- [ ] **Step 5: Adopt cards**

Left panel — wrap each heading section in one card, keeping every inner widget and `ui.collapsing` exactly as-is:

| Section | Range | Title | flush |
|---|---|---|---|
| Tool | `ui.heading("Tool")` → separator at `:6418` | `Some("Tool")` | false |
| Brushes | `:6567` → separator at `:6737` | `Some("Brushes")` | false |
| Color | `:6738` → separator at `:6754` | `Some("Color")` | false |
| Swatches | `:6755` → separator at `:6976` | `Some("Swatches")` | **true** |
| Brush dynamics (`Pressure` … `Guides`) | `:6987-7124` | `None` | false |

Remove the now-redundant `ui.heading(...)` lines where the card title replaces them, and the three `ui.separator()` calls that only separated those sections. Keep separators *inside* a section.

Right panel: wrap the `Layers` heading section in `section_card(ui, &t, Some("Layers"), false, ...)` and the layer-options block in a second card. Use `flush = true` on the Layers card if the row name column visibly elides earlier than before; note the choice in the commit.

- [ ] **Step 6: Run the suite and check the palettes**

Run: `cargo test -p procraft --lib 2>&1 | Select-String -Pattern 'test result|failures:'`
Expected: `84 passed`.

Launch the release build. Expected: each section sits in a rounded card; the Starter swatches are still **3 rows of 8**; every collapsible still collapses.

- [ ] **Step 7: Commit**

```bash
git add apps/procraft/src/design.rs apps/procraft/src/lib.rs
git commit -m "feat(ui): section cards for the tools and layers panels"
```

---

### Task 4: One touch-height source and a device-class override

**Files:**
- Modify: `apps/procraft/src/lib.rs` (`quick_hit_height`; literals at `:2221`, `:2454`, `:5396`, `:5423`, `:5661`; FAB sizes at `:8495`, `:8523`; `bar_layout` callers at `:5391`, `:5122`, `:5675`)

**Interfaces:**
- Consumes: `design::tokens`.
- Produces:
  - `fn quick_hit_height(class: BarLayout) -> f32` now returns `design::tokens(Theme::Dark, class).control_h`
  - `fn resolve_layout(forced: Option<BarLayout>, width: f32, mobile: bool) -> BarLayout`
  - `fn forced_layout() -> Option<BarLayout>`
  - `fn effective_layout(width: f32, mobile: bool) -> BarLayout`

- [ ] **Step 1: Write the failing tests**

In `lib.rs`'s test module, replace the body of `quick_bar_touch_geometry_is_finger_sized` with:

```rust
assert_eq!(quick_hit_height(BarLayout::Wide), 26.0);
assert_eq!(quick_hit_height(BarLayout::Narrow), 44.0);
assert_eq!(quick_hit_height(BarLayout::Tablet), 44.0);
assert!(quick_scrolls(BarLayout::Narrow));
assert!(!quick_scrolls(BarLayout::Wide));
```

Add:

```rust
#[test]
fn forced_layout_wins_over_the_width_rules() {
    assert_eq!(resolve_layout(Some(BarLayout::Tablet), 380.0, false), BarLayout::Tablet);
    assert_eq!(resolve_layout(Some(BarLayout::Narrow), 1400.0, false), BarLayout::Narrow);
    assert_eq!(resolve_layout(None, 380.0, false), BarLayout::Narrow);
    assert_eq!(resolve_layout(None, 844.0, true), BarLayout::Tablet);
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p procraft --lib quick_bar_touch 2>&1 | Select-String -Pattern 'left:|right:|error\\[|test result'`
Expected: FAIL — `left: 20.0, right: 26.0`.

- [ ] **Step 3: Implement**

`quick_hit_height` returns `design::tokens(Theme::Dark, class).control_h`. Replace each literal `40.0`/`egui::vec2(8.0, 8.0)` in the five sites with the class's token value — `let t = design::tokens(self.theme, <class>);` then `t.control_h`, `vec2(t.gap, t.gap)`. At `:2221` and inside `view_controls` use the class computed from the viewport width.

Add:

```rust
fn resolve_layout(forced: Option<BarLayout>, width: f32, mobile: bool) -> BarLayout {
    forced.unwrap_or_else(|| bar_layout(width, mobile))
}

fn forced_layout() -> Option<BarLayout> {
    match std::env::var("PROCRAFT_FORCE_LAYOUT").ok()?.as_str() {
        "wide" => Some(BarLayout::Wide),
        "tablet" => Some(BarLayout::Tablet),
        "narrow" => Some(BarLayout::Narrow),
        _ => None,
    }
}

fn effective_layout(width: f32, mobile: bool) -> BarLayout {
    resolve_layout(forced_layout(), width, mobile)
}
```

Switch every UI call site (`:5122`, `:5391`, `:5675`) from `bar_layout(...)` to `effective_layout(...)`. Leave `bar_layout` itself untouched so the `bar_layout_*` tests still pass. Change the two FAB `[44.0, 44.0]` sizes to `[t.control_h, t.control_h]`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p procraft --lib 2>&1 | Select-String -Pattern 'test result|failures:'`
Expected: `85 passed`.

- [ ] **Step 5: Check the higher-density touch layout and the override**

Run: `$env:PROCRAFT_FORCE_LAYOUT='tablet'; cargo build --release -p procraft; .\target\release\procraft.exe`
Expected: on a 1400 px desktop window the bar uses 44 px controls and the tablet fold (View menu present), proving the override works. Unset the variable afterwards.

- [ ] **Step 6: Commit**

```bash
git add apps/procraft/src/lib.rs
git commit -m "feat(ui): single touch-height source and a forced device-class override"
```

---

### Task 5: Thick slider in the quick bar

**Files:**
- Modify: `apps/procraft/src/design.rs`
- Modify: `apps/procraft/src/lib.rs` (`quick_slider` at `:3162-3197`)

**Interfaces:**
- Consumes: `Tokens`, `control_h`, `slider_track`, `slider_width_wide/touch`.
- Produces:
  - `pub(crate) fn thick_slider(ui: &mut egui::Ui, t: &Tokens, value: &mut f32, range: RangeInclusive<f32>, step: f64, label: &str, default: f32) -> egui::Response`
  - `pub(crate) fn value_from_track(x: f32, width: f32, range: &RangeInclusive<f32>) -> f32`
  - `pub(crate) fn fill_fraction(v: f32, range: &RangeInclusive<f32>) -> f32`
  - `pub(crate) fn slider_track_width(t: &Tokens) -> f32`

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn track_position_maps_to_the_range_and_clamps() {
    let r = 0.0..=100.0;
    assert_eq!(value_from_track(0.0, 200.0, &r), 0.0);
    assert_eq!(value_from_track(100.0, 200.0, &r), 50.0);
    assert_eq!(value_from_track(200.0, 200.0, &r), 100.0);
    assert_eq!(value_from_track(-40.0, 200.0, &r), 0.0);
    assert_eq!(value_from_track(999.0, 200.0, &r), 100.0);
    assert_eq!(fill_fraction(25.0, &r), 0.25);
    assert_eq!(fill_fraction(500.0, &r), 1.0);
}

#[test]
fn slider_hit_area_is_only_the_track() {
    let wide = tokens(Theme::Dark, BarLayout::Wide);
    let tablet = tokens(Theme::Dark, BarLayout::Tablet);
    assert_eq!(slider_track_width(&wide), 110.0);
    assert_eq!(slider_track_width(&tablet), 130.0);
    assert!(slider_track_width(&tablet) < tablet.control_h * 7.0);
}
```

- [ ] **Step 2: Run to verify it fails**

Expected: compile error — `value_from_track` not defined.

- [ ] **Step 3: Implement**

`slider_track_width` returns `slider_width_touch` when `t.class != BarLayout::Wide`, else `slider_width_wide` (the `class` field exists from Task 1).

`value_from_track` and `fill_fraction` clamp to the range and handle `lo == hi` by returning `lo`.

`thick_slider` draws, inside one `ui.horizontal` with `vec2(4.0, 4.0)` spacing:
1. `ui.label(format!("{label} {value_text}"))` where `value_text` uses integers when `step.fract() == 0.0`, else two decimals;
2. a `-` button sized `[24.0 or t.control_h, t.control_h]` that subtracts `step`;
3. the track: `let (rect, resp) = ui.allocate_exact_size(vec2(slider_track_width(t), t.control_h), Sense::click_and_drag());` then paint `surface_3` full-track at `CornerRadius::same((t.slider_track/2.0) as u8)`, and the filled portion of width `rect.width() * fill_fraction(...)` in `accent`;
4. a `+` button.

On `resp.clicked() || resp.dragged()`, set `*value = value_from_track(resp.interact_pointer_pos().x - rect.left(), rect.width(), range)`. On `resp.double_clicked()`, set `*value = default`. Call `resp.widget_info(|| egui::WidgetInfo::slider(true, *value as f64, format!("{label} {value_text}")))` — the 0.36 signature is `slider(enabled, value, label)`.

- [ ] **Step 4: Run the tests**

Expected: `2 passed`.

- [ ] **Step 5: Rewrite `quick_slider` to use it**

Change the signature to take tokens instead of a bare height:

```rust
fn quick_slider<Num: egui::emath::Numeric>(
    ui: &mut egui::Ui,
    value: &mut Num,
    range: std::ops::RangeInclusive<Num>,
    step: f64,
    text: &str,
    t: &design::Tokens,
) {
    let lo = range.start().to_f64() as f32;
    let hi = range.end().to_f64() as f32;
    let mut v = value.to_f64() as f32;
    let default = lo;
    design::thick_slider(ui, t, &mut v, lo..=hi, step, text, default);
    *value = Num::from_f64(v as f64);
}
```

Plumbing: `tool_options_bar` keeps `let hit = quick_hit_height(layout)` (still used for `interact_size.y`) and adds `let t = design::tokens(self.theme, layout);`. Pass both down: `quick_controls_ui(ui, hit, &t)` → `quick_control_ui(ui, control, hit, &t)` → `quick_slider(ui, …, t)`. The duplicated height (`hit` vs `t.control_h`) is deliberate: `hit` drives layout spacing, tokens drive the rail, and `quick_hit_height` stays the single tested accessor rather than becoming dead code.

Update all `quick_slider(...)` call sites in `quick_control_ui` to pass `t` as the last argument and drop the old `hit` argument from those calls.

- [ ] **Step 6: Run the suite and check scrolling**

Run: `cargo test -p procraft --lib 2>&1 | Select-String -Pattern 'test result|failures:'`
Expected: `87 passed`.

With `PROCRAFT_FORCE_LAYOUT=narrow`, set the window to ~400 px and pick Selection. Expected: thick accent-filled rails; the `-`/`+` steppers still work; dragging the row between controls scrolls it; dragging a rail sets the value.

- [ ] **Step 7: Commit**

```bash
git add apps/procraft/src/design.rs apps/procraft/src/lib.rs
git commit -m "feat(ui): thick quick-bar sliders"
```

---

### Task 6: Three-state theme toggle (☀ / ☾ / 🖹)

**Files:**
- Modify: `apps/procraft/src/design.rs`
- Modify: `apps/procraft/src/lib.rs` (`Panel::top("menu")` at `:5412-5680`; `view_controls` at `:2155`)

**Interfaces:**
- Consumes: `Tokens`, `pill_button`, `Theme`.
- Produces:
  - `pub(crate) fn theme_toggle(ui: &mut egui::Ui, t: &Tokens, theme: &mut Theme) -> bool`
  - `pub(crate) fn next_theme(theme: Theme) -> Theme`
  - `pub(crate) fn theme_glyph(theme: Theme) -> &'static str`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn toggle_cycles_dark_light_paper_with_one_glyph_each() {
    assert_eq!(next_theme(Theme::Dark), Theme::Light);
    assert_eq!(next_theme(Theme::Light), Theme::Paper);
    assert_eq!(next_theme(Theme::Paper), Theme::Dark);
    assert_eq!(theme_glyph(Theme::Dark), "☾");
    assert_eq!(theme_glyph(Theme::Light), "☀");
    assert_eq!(theme_glyph(Theme::Paper), "🖹");
}
```

- [ ] **Step 2: Run to verify it fails**

Expected: compile error — `next_theme` not defined.

- [ ] **Step 3: Implement**

`next_theme` cycles `Dark → Light → Paper → Dark`.

`theme_glyph` shows the **current** theme, so the pill reads as a state, not as
the action: `Dark = "☾"`, `Light = "☀"`, `Paper = "🖹"`. There is no text
fallback: the pill is icon-only at every width, and the names live in the
tooltip and in the combo rows' tooltips.

`theme_toggle` calls `pill_button` with `PillSpec { glyph: Some(theme_glyph(*theme)), label: None, selected: false, enabled: true }`, plus `.on_hover_text(format!("{} theme · click for {}.", theme.name(), next.name()))` on the response. If clicked, `*theme = next` and return `true`. `theme_control` shows the same glyphs: `selected_text(design::theme_glyph(self.theme))` and one `selectable_value` row per theme labelled with its glyph and `.on_hover_text(theme.name())`.

- [ ] **Step 4: Run the test**

Expected: `1 passed`.

- [ ] **Step 5: Place it in the top bar**

In `Panel::top("menu")`'s `horizontal_wrapped`, render the toggle **before** the width-dependent cluster so it is never inside the fold:

```rust
let t = design::tokens(self.theme, layout);
design::theme_toggle(ui, &t, &mut self.theme);
```

Because `self.theme` is borrowed mutably, capture the result into a local and let the existing `theme_applied` check pick the change up on the next frame. Leave `view_controls`'s `include_theme` combo exactly where it is — at `Wide` the toggle and the inline combo coexist; on touch the combo stays in the View menu — but make its face and rows icon-only.

In `theme_control`, replace `ui.spacing_mut().interact_size.y = 40.0` with the token value for the current class.

- [ ] **Step 6: Run the suite and check every width**

Run: `cargo test -p procraft --lib 2>&1 | Select-String -Pattern 'test result|failures:'`
Expected: `88 passed`.

Expected by eye: at 1400 px the toggle sits beside the theme combo; at 400 px it is visible without opening the View menu; tapping it cycles Dark → Light → Paper and repaints immediately; the pill shows the current theme (☾ / ☀ / 🖹); the top bar does not gain a row at any width.

- [ ] **Step 7: Commit**

```bash
git add apps/procraft/src/design.rs apps/procraft/src/lib.rs
git commit -m "feat(ui): three-state theme toggle in the top bar"
```

---

### Task 7: Dark default, artifacts and verification

**Files:**
- Modify: `crates/engine/src/settings.rs` (`:37` default, `:68` test)
- Modify: `docs/superpowers/specs/2026-10-27-ui-modernization-design.md` only if a token had to move

**Interfaces:**
- Consumes: everything above.
- Produces: no new code interfaces; produces `dist\win-73e06da\procraft.exe`, a fresh iOS `.ipa`, a fresh Android `.apk`, and screenshots.

- [ ] **Step 1: Change the default theme and its test**

`crates/engine/src/settings.rs`: `theme: "paper".into()` → `theme: "dark".into()`, and the assertion at `:68` `assert_eq!(s.theme, "paper")` → `"dark"`.

- [ ] **Step 2: Run both crates' tests**

Run: `cargo test -p procraft-engine 2>&1 | Select-String -Pattern 'test result|failures:'`
Run: `cargo test -p procraft --lib 2>&1 | Select-String -Pattern 'test result|failures:'`
Expected: engine green, app `88 passed`, 0 failed.

- [ ] **Step 3: Verify the fresh-profile and existing-profile paths**

```powershell
$exe='D:\Dev\procraft\target\release\procraft.exe'
$t='C:\Users\saran\AppData\Local\Temp\opencode\ui-fresh'
Remove-Item -Recurse -Force $t -ErrorAction SilentlyContinue; New-Item -ItemType Directory -Force $t|Out-Null
$env:APPDATA=$t
$p=Start-Process $exe -PassThru; Start-Sleep -Seconds 9; $p.CloseMainWindow()|Out-Null; $p.WaitForExit(12000)|Out-Null
Get-Content (Join-Path $t 'procraft\settings.json')
```

Expected: `"theme": "dark"`. Then write a `settings.json` with `"theme": "paper"`, relaunch, and confirm the app stays Paper (no rewrite to dark).

- [ ] **Step 4: Screenshot all three themes at all three classes**

For each of `PROCRAFT_FORCE_LAYOUT=wide|tablet|narrow` and each theme (Dark, Light, Paper), launch the release exe, capture the window, and save to `D:\Dev\procraft\dist\ui-2026-10-27\<class>-<theme>.png`. Verify in the images: rounded cards, pills, thick rails, no clipped static text, no gained rows.

- [ ] **Step 5: Rebuild the artifacts**

Run: `cargo build --release -p procraft` then copy to `dist\win-73e06da\procraft.exe` and `dist\procraft.exe`, recording size and SHA-256.

For iOS and Android use the existing workflows (`gh workflow run android-apk.yml --repo surfrpt1/procraft --ref main`) and the iOS IPA workflow used for `73e06da`; download the artifacts into `dist\ios-<sha>\` and `dist\android-<sha>\`. If a workflow cannot be triggered, report which platform is missing rather than skipping silently.

- [ ] **Step 6: Final verification and commit**

`git status --short` should show only `settings.rs` (and the spec if a token moved). Commit:

```bash
git add crates/engine/src/settings.rs
git commit -m "feat(settings): dark is the default theme for new installs"
```

Report: test counts, exe size + hash, conditional on: swatches still 3 × 8, no chrome row gained, all nine screenshots present.

---

### Task 8: Swatch card legibility

**Files:**
- Modify: `apps/procraft/src/lib.rs` (`swatch_grid` / `swatch_button` near `:3205`, the Swatches card near `:6835`)

**Problem:** the Swatches card was built `flush` (zero horizontal inner
margin), so `SWATCHES`, `Starter (…):` and the first swatch column touched the
card border and read as clipped; the swatches used the control radius (8 on
wide, 12 on touch), turning 24 pt squares into soft blobs at 125% scaling; and
the one-line `Palettes: ◀ name (1/1 · 0 swatches) ▶ + New` row overflowed the
card and clipped the palette name.

- [ ] **Step 1: Normal card padding** — call
  `design::section_card(ui, &t, Some("Swatches"), false, …)`; the grid still
  fits 8 per row inside the padded content width.
- [ ] **Step 2: Draw swatches by hand** — fixed `SWATCH = 24.0`,
  `SWATCH_GAP = 4.0`; `swatch_button` allocates a 24 pt click rect, snaps the
  painted rect to the physical pixel grid (`round(x * ppp) / ppp`), fills it
  with radius `4` and draws a 1 pt neutral / 2 pt accent (selected) inside
  stroke. No `Button` widget, so the control radius cannot round it into a
  blob.
- [ ] **Step 3: Reflow the palette row** — actions (`◀ ▶ + New`) on one
  wrapped row, the active palette name/counts on the next label so long names
  wrap instead of being clipped.
- [ ] **Step 4: Verify** — `cargo test -p procraft`, `cargo clippy --workspace
  --all-targets -- -D warnings`, then screenshot the scrolled Swatches card at
  wide (dark/light/paper) and confirm crisp 8-per-row squares, padded labels
  and a fully visible palette name.

