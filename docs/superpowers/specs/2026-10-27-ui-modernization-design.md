# ProCraft UI modernization — design

Date: 2026-10-27
Status: awaiting approval
Scope: visual/design system restyle of the existing UI. No layout
reorganization. That is a separate project ("A") planned afterwards.

## 1. Intent

ProCraft's functionality is ahead of its appearance. The app currently
renders as stock egui: three `Theme` variants that return `Visuals::dark()` /
`Visuals::light()` (with `Paper` overriding three colours), egui's default
corner radius, default control heights, default button padding, and five
separate hard-coded `interact_size.y = 40.0` sites. Nothing about the UI is
designed; it is whatever egui ships with.

The goal is to make ProCraft look and feel like a contemporary painting app
on desktop, tablet and phone — specifically in the visual language of modern
Procreate — **without** changing where things live on screen. Same top bar,
same tool-options row, same left tools panel, same right layers panel, same
canvas. Only how they are drawn, sized, coloured and grouped changes.

This is deliberately the low-risk half of the Procreate modernization. The
structural half (floating corner clusters, slide-over panels, canvas-first
chrome, per-device arrangement) is project A and is out of scope here.

## 2. Why now

- Open-work item 3 (phone/tablet issues) already has in-tree fixes for
  status-bar overlap and top-bar pressability; the remaining complaints are
  visual weight, touch size and dark-mode discoverability — which is this
  project.
- The desktop, iPad and phone builds share one layout with one density.
  Restyling is the cheapest way to make all three feel intentional.
- The user's saved settings already select the dark theme, and Paper's warm
  palette is a signature look. A token system lets both coexist properly
  instead of only 3 colour overrides.

## 3. Goals

1. One place that owns colour, radius, spacing, control height and type
   size — no more scattered `Color32::from_rgb` or `40.0` literals in UI code.
2. Every theme (Dark, Light, Paper) fully specified by that one place, and
   visually coherent in all three.
3. Procreate-like visual language: dark neutral surfaces, soft rounded
   cards, pill-shaped controls, thick draggable sliders, generous spacing.
4. Finger-sized controls on every touch surface (44 px), with desktop
   staying dense.
5. Theme reachable in one tap from anywhere (☀ / ☾ / 🖹 toggle), not
   buried in a menu.
6. No functional regressions; the existing test suite stays green.

## 4. Non-goals

- No layout reorganization. Panels stay where they are; no floating corner
  clusters, no slide-over overlays, no bottom tool strip. (Project A.)
- No change to painting/engine behaviour, document format, settings format
  version, shortcuts, or menu contents beyond the theme control.
- No icon font, SVG icon set, or image assets. Existing glyphs and text
  labels only, gated by the existing `has_symbols` capability flag.
- No animations, transitions, or motion design.
- No new fonts. egui's default proportional face stays.
- No custom menu/scrollbar/numeric-field widgets. Those stay stock egui.
- No new dependencies.

## 5. Anti-goals

These would count as failure even if the app compiles and looks nicer:

- Chrome gaining a row on any device. Touch bars *will* get taller, because
  finger-sized targets are the point; a 44 px row replacing a 20 px row is
  accepted. A third row on a phone is not. Budget in §8.10.
- Panels getting wider, or the canvas narrower, to make room for cards.
- Any of the 77 existing tests being deleted to make room for the restyle
  (updating expected constants is fine; deleting coverage is not).
- Rebuilding the style every frame (visible jank, battery drain on iPad).
- Theme changes not applying until restart, or applying only to part of the
  window.
- Paper or Light becoming unreachable.
- Touch targets that look big but have a smaller hit area than they paint
  (or vice versa) — hit rect and painted rect must match.
- Buttons that get larger on desktop, stealing working space from artists.

## 6. Current state (what the restyle touches)

Single file: `apps/procraft/src/lib.rs` (~10.9k lines).

| Area | Where | Today |
|---|---|---|
| Theme enum + visuals | `lib.rs:418-481` | 3 variants; Dark/Light return stock visuals; Paper overrides `window_fill`, `panel_fill`, `faint_bg_color` |
| Style application | `lib.rs:5122-5125` | `ctx.set_visuals(...)`, cached by `theme_applied: Option<Theme>` |
| Top bar | `egui::Panel::top("menu")`, `lib.rs:5366` | inline `horizontal_wrapped` of File menu, browsers, edit buttons, zoom stepper, fit, rotate, theme combo, About |
| Tool options row | `egui::Panel::top("tool-options")`, `lib.rs:5680` | `tool_options_bar` → `quick_controls_ui` → `quick_slider` |
| Left panel | `egui::Panel::left("tools-compact")`, `lib.rs:6267` | tools palette + brush settings, flat sections |
| Right panel | `egui::Panel::right("layers-wide")`, `lib.rs:7141` | layer list + layer options, flat sections |
| Status bar | `egui::Panel::bottom("status")`, `lib.rs:8501` | one label |
| Canvas | `egui::CentralPanel`, `lib.rs:8514` | painted image + interaction |
| Collapsed-panel FABs | `egui::Area` (`layers-fab`, tools FAB) | 44×44 buttons |
| Width eras | `BarLayout` / `bar_layout`, `lib.rs:3296-3317` | `Wide` (desktop ≥600), `Tablet` (mobile ≥600), `Narrow` (<600) |
| Touch heights | `quick_hit_height`, `lib.rs` (~near 5990) | 20.0 / 34.0 / 40.0 |
| Hard-coded heights | `lib.rs:2221, 2454, 5396, 5423, 5661` | `interact_size.y = 40.0` repeated |
| Default theme | `crates/engine/src/settings.rs:37` | `"paper"`, asserted at `settings.rs:68` |

Three consequences worth stating plainly:

- `Theme::visuals()` can only express *colours*, because `set_visuals` does
  not touch spacing, radii, or text styles. A restyle needs `Style`, not
  `Visuals`.
- The `theme_applied` cache key is the theme alone. Device class can change
  without restart (resize a desktop window across 600 px, rotate a tablet),
  so once control heights become device-dependent the cache must include
  the device class or the metrics will not update.
- There are 22 `Color32::from_rgb` sites, but only four are chrome
  (449-451 Paper, 1607 warning label). The rest are canvas overlays,
  selection outlines, guides and layer-thumbnail borders — these are
  *semantic artwork colours*, not UI chrome, and are explicitly left alone
  except where they double as accent (see §8.6).

## 7. Approaches considered

### B1 — Tokens only
A `Tokens` struct per (theme, device) converted into a full `egui::Style`,
applied once. All widgets stay stock.

- Pros: smallest possible change, no new drawing code, near-zero regression
  risk.
- Cons: the result still reads as "egui with better colours". Rounded corners
  and padding help, but stock egui buttons, sliders and panel sections keep
  their recognisable look, so it does not deliver the requested Procreate
  feel.

### B2 — Tokens + a small set of custom primitives **(chosen)**
B1, plus four hand-drawn pieces that carry the visual identity: a pill
button, a section card, a thick quick slider, and a three-state theme toggle.

- Pros: delivers the visible jump; bounded (~400-550 new lines, mostly one
  new module); leaves menus, scroll areas, numeric fields, canvas and all
  interaction semantics untouched; each primitive is independently testable
  and revertible.
- Cons: four new drawing paths to keep consistent; custom slider needs care
  to keep a correct hit rect and accessible widget info.

### B3 — Full custom widget kit
Replace everything, including menus, scrollbars, combo boxes and drag values.

- Rejected: it is project A-sized work, overlaps with a redesign that will
  move or delete half of these widgets anyway, and delays any visible result
  past the point of usefulness.

The user chose **B now, A after**, and **B2** within B.

## 8. Design

### 8.1 Device classes

Reuse the existing `BarLayout` as the device class — it is already computed
from viewport width and platform, and already drives the top bar's folding
behaviour.

| Class | Condition | Feel |
|---|---|---|
| `Wide` | desktop, width ≥ 600 | dense, mouse-first |
| `Tablet` | iOS/Android, width ≥ 600 | finger-first, roomy |
| `Narrow` | width < 600, any platform | finger-first, compact |

`bar_layout()` itself does not change; existing tests keep their exact
expectations.

### 8.2 Tokens

One struct, built by a pure function `tokens(theme: Theme, class: BarLayout)
-> Tokens`. Fields:

```
colour   surface_0   window / canvas surround
         surface_1   panels and bars
         surface_2   cards, buttons, popups
         surface_3   controls: slider tracks, text fields, wells
         stroke      hairline borders
         text_hi     primary text
         text_lo     secondary text, labels, units
         accent      selection, active tool, slider fill
         accent_soft accent at ~24 alpha, for selected pill backgrounds
         danger      destructive/warning text

geometry radius_small (4)   radius_control (8|12)   radius_card (12)
         pad_x, pad_y       panel and card interior padding
         gap                item spacing between controls
         control_h          standard button/row height
         hit_min            minimum touch target (touch classes only)
         slider_track       painted track height
         type_body, type_small, type_heading
```

Table (values are the design; the builder may nudge a value only if a
contrast or fit test forces it, and must say so in the commit):

| | Wide | Tablet | Narrow |
|---|---|---|---|
| `control_h` | 26 | 44 | 44 |
| `hit_min` | — (mouse) | 44 | 44 |
| `gap` | 6 | 8 | 6 |
| `pad_x` / `pad_y` | 10 / 6 | 14 / 10 | 10 / 8 |
| `radius_control` | 8 | 12 | 12 |
| `radius_card` | 10 | 14 | 14 |
| `slider_track` | 6 | 10 | 10 |
| `type_body` | 14 | 15 | 15 |
| `type_small` | 12 | 13 | 13 |
| `type_heading` | 15 | 16 | 16 |

Colours:

| Token | Dark (default) | Light | Paper |
|---|---|---|---|
| `surface_0` | `#131315` | `#EDEFF2` | `#F4EEE4` |
| `surface_1` | `#1B1B1E` | `#F7F8FA` | `#FAF6EE` |
| `surface_2` | `#26262A` | `#FFFFFF` | `#FFFDF8` |
| `surface_3` | `#33333A` | `#DFE3E9` | `#E7DCC9` |
| `stroke` | `#3D3D45` | `#CDD3DB` | `#D6C9B3` |
| `text_hi` | `#F0F0F3` | `#16181C` | `#2A231B` |
| `text_lo` | `#9C9CA6` | `#626A76` | `#6E6252` |
| `accent` | `#4C8DFF` | `#2F6FEB` | `#B5651D` |
| `danger` | `#FF6B6B` | `#C0392B` | `#A63A21` |

`text_lo` on `surface_1` must clear 4.5:1 contrast in all three themes; the
chosen values are 5.9 / 4.6 / 4.9 respectively and a host test asserts it.
`accent` fills carry white text; at body sizes this is AA-large only, which
is accepted for selected-pill labels (bold, ≥14 px).

### 8.3 From tokens to `egui::Style`

`Tokens::apply(base: &egui::Style) -> egui::Style`:

- `visuals.panel_fill = surface_1`, `window_fill = surface_2`,
  `extreme_bg_color = surface_0`, `faint_bg_color = surface_2`,
  `window_stroke` = 1 px `stroke`.
- `visuals.widgets.noninteractive`: `bg_fill = surface_1`,
  `fg_stroke = text_hi`, `corner_radius = radius_control`.
- `visuals.widgets.inactive`: `weak_bg_fill = surface_2`,
  `bg_fill = surface_3`, `bg_stroke = stroke`, `fg_stroke = text_hi`.
- `visuals.widgets.hovered`: `weak_bg_fill = surface_3`,
  `bg_stroke = accent`, `fg_stroke = text_hi`, `expansion = 0`.
- `visuals.widgets.active`: `weak_bg_fill = surface_3`,
  `bg_stroke = accent`, `fg_stroke = text_hi`.
- `visuals.widgets.open = hovered`.
- `visuals.selection.bg_fill = accent_soft`, `selection.stroke = accent`.
- `visuals.override_text_color = None` (widgets specify their own); secondary
  text is set per-widget by the primitives, not globally.
- `spacing.item_spacing = (gap, gap)`, `button_padding = (pad_x, pad_y)`,
  `interact_size = (control_h * 1.4, control_h)`,
  `slider_rail_height = slider_track`, `slider_width = 140`.
- Text styles: `Body = type_body`, `Button = type_body`,
  `Small = type_small`, `Heading = type_heading` (bold).

egui 0.36.2 has **no** `Context::style()` / `set_style()`. It keeps two
style slots (`egui::Theme::Dark` and `egui::Theme::Light`) plus a
`ThemePreference`. So the three app themes map onto two slots:

| App theme | egui slot written | `ThemePreference` |
|---|---|---|
| Dark | `Theme::Dark` | `Dark` |
| Light | `Theme::Light` | `Light` |
| Paper | `Theme::Light` | `Light` |

Application, on every change of `(Theme, BarLayout)`:

1. `let slot = if theme == Theme::Dark { egui::Theme::Dark } else { egui::Theme::Light };`
2. `let base = ctx.style_of(slot);`
3. `ctx.set_style_of(slot, tokens.apply(&base));`
4. `ctx.set_theme(if theme == Theme::Dark { ThemePreference::Dark } else { ThemePreference::Light });`

Pinning `ThemePreference` is required: left at `System`, an OS light/dark
switch would swap in an unstyled slot mid-session (an anti-goal) and leak
stock egui styling into half the window. Switching Light ⇄ Paper rewrites
the same `Light` slot. The cache key becomes `Option<(Theme, BarLayout)>` so
a resize or rotation across the 600 px boundary reapplies metrics.
Rebuilding is cheap but must not happen every frame (anti-goal).

### 8.4 Primitive 1 — `pill_button`

```
pill_button(ui, tokens, spec) -> Response
spec { glyph: Option<&str>, label: Option<&str>, selected: bool, enabled: bool }
```

- Height = `control_h`; width = text/glyph width + 2 × `pad_x`, minimum
  `control_h` for icon-only.
- Painted with `corner_radius = control_h / 2` (true pill), fill
  `surface_2` (normal) / `surface_3` (hover) / `accent` (selected), stroke
  1 px `accent` when hovered/selected.
- Glyph above/beside label: label only when it fits; icon-only pills show
  the glyph. Tooltips carry the full name.
- Hit rect **equals** painted rect (`ui.allocate_response` from the painted
  size, not `add_sized` around a label).
- `has_symbols == false` ⇒ glyph is omitted and the label is shown.
- Adopted in: plain top-bar buttons (import/open, edit actions, fit,
  rotate, About) and the tools palette rows. **Menu triggers keep stock
  `MenuButton`** (File, View, browser menus) — they inherit the token radius
  and padding from the style, and are allowed to look slightly different
  from the pills beside them. Replacing `MenuButton`'s trigger would mean
  hand-wiring a popup and a fifth drawing path, i.e. a custom menu widget,
  which §4 rules out.

### 8.5 Primitive 2 — `section_card`

```
section_card(ui, tokens, title: Option<&str>, flush: bool,
             add: impl FnOnce(&mut Ui))
```

- A `Frame` with `fill = surface_2`, `stroke = 1 px stroke`,
  `corner_radius = radius_card`, inner margin = `(pad_x_card, pad_y_card)`.
  `pad_x_card` = 6 (Wide) / 8 (touch), `pad_y_card` = 8 (Wide) / 10 (touch).
  This is deliberately smaller than the general `pad_x`, because the panel
  content width is fixed (see below).
- `flush = true` drops horizontal padding entirely (vertical padding stays).
- Optional title row: `type_small`, `text_lo`, uppercase, 6 px gap.
- Consecutive cards separated by `gap`; cards never nest.

**What a card is:** one card per existing *top-level heading* — left panel:
Tool, Brushes, Color, Swatches (and the existing per-tool blocks that own a
heading); right panel: Layers, and the layer-options block. The existing
`ui.collapsing` groups (`My brushes`, `Selection Studio`, `Pressure`,
`Velocity`, `Taper`, `Texture`, `Depth`, `Dab shape`, `Smoothing`, `Page`,
`Guides`, `Animation`, `Transform layer`, `Symmetry`, `Filters`) **stay
exactly as they are, inside their card**. Collapse state and behaviour must
survive untouched.

A card is a *visual* wrapper, never a re-cut. Splitting the brush block into
new cards would be a reorganization, which §4 assigns to project A.

**Panel width constraint:** `swatch_grid` pins its own wrap width to
`8 × 27 + 8 = 224` px so the Starter palette always lays out 3 × 8
(`lib.rs:3196-3206`), and the tools panel's default size is 240 with ~8 px
panel margins — i.e. the grid already consumes the full content width.
Therefore:

- Panel `default_size` and panel ids are **not** changed, and no width
  migration is performed (saved widths are user state).
- The Swatches card uses `flush = true` so the grid keeps its full 224 px and
  the 3 × 8 invariant holds. Verification includes counting the columns.
- Layer rows in the 280 px layers panel get `flush` treatment too if the
  8 px inset forces the name column to elide earlier than today; report
  which was chosen.

### 8.6 Primitive 3 — `thick_slider`

**Inline, not stacked.** The quick bar is a single horizontal run (`Wide`
wraps, touch scrolls), and a tool has up to 7 controls. A stacked
label-over-track block would turn the options bar into 7 rows or force a
vertical scroller inside a horizontal one. So the primitive keeps today's
one-line footprint and only changes *what the rail looks like*:

```
[Label  value]  [−]  [======track======]  [+]
```

```
thick_slider(ui, tokens, value: &mut f32, range: RangeInclusive<f32>,
             step: f64, label: &str) -> Response
```

- Track width: 110 px (Wide) / 130 px (touch). Height: `slider_track`
  (6 / 10). Radius = track / 2. Unfilled `surface_3`, filled from the left
  in `accent`.
- Row height = `control_h`; the rail is vertically centred in it.
- Drag anywhere **on the track** sets the value absolutely; click also sets.
  Double-click/double-tap resets to the supplied default.
- **Hit rect is the track only** — not the whole row. This matters on touch:
  the row scrolls horizontally, and if the slider swallowed drags across its
  label and padding there would be nowhere left to scroll with 7 controls
  on screen. Same footprint as today's stock slider, so scrolling is no
  worse than it is now.
- **`−` / `+` steppers stay**, at every class (24 px on Wide, `control_h` on
  touch). They are the only way to nudge a value on a phone without opening
  the left panel, and removing them would violate "no functional
  regressions". `step` is the existing per-control step.
- Value formatting keeps today's rule: integers when the step is integral,
  two decimals otherwise.
- Emits `WidgetInfo::slider` via `response.widget_info(...)` so accesskit
  still announces it.
- Adopted in: `quick_slider` only, which every quick control already funnels
  through (Size, Opacity, Softness, Spacing, Stabilizer, Strength,
  Tolerance, Grow, Feather, …). Typed entry stays in the left panel via the
  existing `stepped_slider`/`DragValue`.
- **Not** adopted for stock sliders elsewhere (layer opacity, adjustments)
  in this pass — they inherit the token style and look consistent, and
  converting them is a later follow-up if wanted.

### 8.7 Primitive 4 — `theme_toggle`

- A single pill button, always in the top bar's right cluster, at every
  width including `Narrow` (this is the discoverability fix).
- Shows the **current** state: `☾` dark, `☀` light, `🖹` paper, with the
  tooltip naming the next theme. The pill is icon-only at every width — no
  `Dark`/`Light`/`Paper` text variant.
- Tapping cycles **Dark → Light → Paper → Dark**.
- The toggle is the **only** theme control. The old theme combo — inline at
  `Wide`, inside the View menu on touch — is removed, so the bar spends one
  square pill on the theme instead of a pill plus a named dropdown, and no
  second control repeats the same job. Every theme stays reachable through
  the cycle.
- The toggle is the only control explicitly kept out of the
  `Narrow`/`Tablet` View-menu fold, because it is one pill and is the whole
  point of this item.

### 8.8 Touch geometry

- `quick_hit_height(class)` becomes `tokens(Theme::Dark, class).control_h`
  — i.e. 26 / 44 / 44 (was 20 / 34 / 40).
- The five hard-coded `interact_size.y = 40.0` sites are replaced with
  `tokens.control_h` for the current class:
  - `lib.rs:2221` theme popup rows → `control_h`
  - `lib.rs:2454` quick-bar touch override → removed (the global style now
    carries it, and `tool_options_bar` only needs it for `Narrow`)
  - `lib.rs:5396, 5423` top-bar menu/row heights → `control_h`
  - `lib.rs:5661` → `control_h`
- Collapsed-panel FABs move from 44 to `control_h` (44 on touch, 40 on
  desktop so they don't grow visually on mouse platforms).

### 8.9 Default theme

Dark becomes the default for **new installs**: `Settings::default().theme`
becomes `"dark"` and `crates/engine/src/settings.rs:68` updates from
`"paper"` to `"dark"`. Existing users keep whatever is in their
`settings.json` — this changes first-run only. Paper remains one tap away
and is still a first-class theme.

This is my call, stated here so it can be rejected. The reason: a
Procreate-like product reads as dark-first, and Paper's cream chrome next to
a white canvas makes the canvas edges ambiguous.

### 8.10 Chrome budget and device verification

Touch targets must grow, so total chrome height on touch will grow. What is
not allowed is **gaining rows**. Budget, measured after the restyle:

| Class | Top bar | Tool-options bar |
|---|---|---|
| `Wide` | 1 row | 1 row |
| `Tablet` | 1 row | 1 row |
| `Narrow` | ≤ 2 rows | 1 row, horizontally scrollable |

If the theme pill pushes `Narrow` to 3 rows, the fallback order is: (1) make
`About` icon-only, (2) drop `Fit`/`Rotate` to glyph-only, (3) move
`Fit`/`Rotate` into the `Narrow` View menu. Each step is a label change, not
a reorganization. If step 3 is needed, say so in the commit and report it.

**Seeing Tablet on Windows:** `BarLayout::Tablet` requires
`cfg!(ios/android)`, so a desktop build can only reach `Wide` and `Narrow`.
Add a dev-only override:

- `fn forced_layout() -> Option<BarLayout>` reads `PROCRAFT_FORCE_LAYOUT`
  (`wide` | `tablet` | `narrow`).
- `fn effective_layout(width, mobile) -> BarLayout` = `forced_layout()
  .unwrap_or_else(|| bar_layout(width, mobile))`, and is what the UI calls.
- `bar_layout` itself stays pure and unchanged, so the existing
  `bar_layout_*` tests keep their exact expectations; a new test covers
  `effective_layout` precedence without touching the environment (pass the
  override in as a parameter, or keep the env read in one thin wrapper).

This makes all three classes screenshot-able on this machine. Final
device confirmation still needs the iPad and the Pixel.

## 9. File plan

- **New**: `apps/procraft/src/design.rs` — tokens, palette, `apply`,
  the four primitives, pure geometry helpers, host tests (contrast, metrics,
  hit-rect arithmetic).
- `apps/procraft/src/lib.rs` — add `mod design;`; make `Theme` and
  `BarLayout` `pub(crate)`; replace `Theme::visuals` with
  `design::tokens(...).apply(...)`; swap style-cache key to
  `(Theme, BarLayout)`; adopt primitives at the call sites in §8.4-8.7;
  delete the five hard-coded heights.
- `crates/engine/src/settings.rs` — default theme + its test.
- Tests in `lib.rs` that assert numbers: `quick_bar_touch_geometry_is_finger_sized`
  (20/34/40 → 26/44/44). `bar_layout_*` tests unchanged.

## 10. Verification

1. `cargo test -p procraft --lib` — 77 existing + new design tests, zero
   failures.
2. `cargo test -p procraft-engine` — settings default test updated, rest
   green.
3. `cargo build --release -p procraft` — Windows exe, launched and visually
   checked at ≥600 px and at ~380 px width (resize across the boundary must
   re-apply metrics without restart), plus once with each of
   `PROCRAFT_FORCE_LAYOUT=wide|tablet|narrow`.
4. Confirm the Swatches card still lays out 3 × 8 and that no chrome row was
   added at any class (§8.10).
5. Refresh the sandbox `%APPDATA%` run to confirm a fresh profile now writes
   `"theme": "dark"` and that an existing `"paper"` profile is untouched.
6. iOS IPA + Android APK rebuilt for tablet/phone testing; artifact paths
   reported. If a toolchain is unavailable locally, use the existing GitHub
   Actions workflows and report the platform that could not be produced.
7. Screenshots of all three themes at all three classes for the user's
   review before the work is called done.

## 11. What's left to the builder

- Exact wording/placement of tooltips and section titles (use the existing
  labels, shortened only where §8.10 forces it).
- Which layer rows need `flush` cards in the 280 px panel (§8.5) — decide by
  fitting and report.
- Ordering of the implementation slices (suggested: tokens + style →
  pill buttons → cards → thick slider → theme toggle → touch unification →
  artifacts).
- Any token value that a contrast or fit test forces to move, and why.

## 12. Decisions made without the user (review these)

1. **Dark is the default for new installs** (§8.9).
2. **44 px touch targets** on both touch classes; desktop stays 26 px for
   controls and 26 px for quick-bar rows (was 20 / 34 / 40). This makes
   touch bars taller by design; §8.10 forbids gaining rows instead.
3. **The toggle cycles all three themes**, showing the current one as a
   glyph (☾ / ☀ / 🖹) and never as text, with the tooltip naming the next.
   It is the only theme control: the old combo (inline at `Wide`, in the
   View menu on touch) is removed, so two controls never do the same job
   and the bar keeps a single square pill for theme at every width.
4. **`thick_slider` keeps its `−` / `+` steppers and hit-tests only the
   track**, so horizontal scrolling in the quick bar still works and no
   existing way to nudge a value disappears.
5. **Menu triggers stay stock `MenuButton`** (token-styled, not pills) — a
   custom popup trigger would mean building a custom menu widget.
6. **A card is one per existing top-level heading**, with the existing
   `ui.collapsing` groups left intact inside it. No re-cutting of the brush
   or layer blocks.
7. **Panel widths and panel ids do not change**; the Swatches card is flush
   so the 3 × 8 palette invariant holds.
8. **Canvas overlay colours are untouched** (selection outlines, guides,
   masks) — they are artwork semantics, not chrome.
9. **No icon font.** Glyphs stay limited to what `has_symbols` already
   detects.
10. **A `PROCRAFT_FORCE_LAYOUT` dev override** is added so Tablet can be
    screenshot on this machine; `bar_layout` itself stays pure.
11. **iOS/Android artifacts are in scope** for the final slice, via the
    existing CI workflows if the local toolchains are absent.
