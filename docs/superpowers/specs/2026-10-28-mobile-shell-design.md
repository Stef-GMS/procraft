# ProCraft Mobile Shell (Procreate-style) — Design

**Scope:** iOS `.ipa` and Android `.apk` builds only. The desktop/Windows shell is
unchanged: every new arrangement lives behind a native-mobile gate, with a
dev-only override so the same code path can be screenshotted on Windows.

**Why:** on phones and tablets the docked top bars plus left/right panels spend
a large share of the screen on chrome, which is the opposite of Procreate's
canvas-first model. Phones also inherit the tablet arrangement, which wastes the
little width they have.

## Shell selection

- `Shell::Desktop` — current behaviour, untouched.
- `Shell::Mobile` — canvas-first shell described below.
- Native: `cfg!(any(target_os = "ios", target_os = "android"))` selects Mobile.
- Dev override: `PROCRAFT_FORCE_SHELL=mobile|desktop` (host builds only, same
  spirit as `PROCRAFT_FORCE_LAYOUT`). `PROCRAFT_FORCE_LAYOUT=narrow|tablet`
  continues to choose the phone/tablet sub-layout inside Mobile.
- Resolution is a pure function (`resolve_shell(forced, native)`) with host
  tests. `bar_layout` and `effective_layout` keep their current contracts.

## Mobile arrangement

### Canvas

- The canvas owns the full viewport. No top menu bar, no tool-options panel,
  no left/right dock, no bottom status panel.
- Chrome floats above the canvas; opening a drawer or cluster never resizes or
  re-fits the canvas.
- Pinch/rotate/pan keep working exactly as today; overlays capture their own
  pointer input so a drag that starts on chrome never paints.

### Tablet (width >= 600 pt)

- **Top-left floating cluster:** Gallery, File (existing File menu contents),
  Reference. Rounded translucent capsules, safe-area inset applied.
- **Top-right floating cluster:** Undo, Redo, Layers (drawer), Color (opens the
  Tools drawer scrolled to Color), theme toggle.
- **Bottom-centre options capsule:** the existing per-tool quick controls
  (`quick_controls(tool)`), horizontally scrollable, only when non-empty.
- **Slide-over drawers:** Tools from the left, Layers from the right. Width
  `min(360, 88% of viewport)`, full height below the top safe inset, dim scrim,
  one drawer at a time, ~180 ms slide, tap scrim or the card's close button to
  dismiss. The drawer reuses the exact docked panel bodies.
- **Status toast:** the last status message in a capsule above the options bar,
  fading after ~2.5 s; no persistent status row.

### Phone (width < 600 pt)

- No top-left/top-right clusters. A **bottom strip** replaces them:
  Gallery, File, Undo, Redo, Tools, Layers, Color, Reference. The strip is
  horizontally scrollable, 44 pt targets, padded by the bottom safe inset.
- The per-tool options capsule sits directly above the strip.
- Drawers slide in from left/right over the canvas, stopping above the strip
  (width `min(360, 88%)`).
- Status toast sits above the options capsule.

### Shared overlay rules

- Drawers use `egui::Area` at `Order::Foreground`, a full-screen scrim at
  `Order::Middle`, and a solid `Frame` using the existing design tokens.
- The panel bodies keep their internal close buttons; closing sets
  `show_tools`/`show_layers` false, and the shell closes the matching drawer.
- Drawers are session-only; `show_tools`/`show_layers` persistence still works
  for the desktop dock and seeds the first drawer state on mobile.
- Gallery, Reference, About, Shortcuts and Text stay `egui::Window`s but are
  clamped to 92% of the viewport and made scrollable on Mobile.

## Reuse, not duplication

- Extract the left panel body, the right panel body, and the File menu body
  into methods. Both the desktop dock and the mobile drawer call the same
  method, so tools, brushes, color, swatches, dynamics and every layer action
  keep working with one implementation.
- Quick controls reuse `tool_options_bar`'s control row.
- No new dependencies; egui 0.36.2 only.

## Per-plan device verdicts

The user reviews builds on real devices. Expected checkpoints:

1. after the tablet clusters + drawers exist (Plan 04),
2. after the phone strip exists (Plan 05),
3. after overlay polish (Plan 06).

Building `.ipa`/`.apk` requires a push and the existing `ios-ipa` /
`android-apk` workflows. The branch stays unpushed until the user approves.

## Non-goals

- Desktop layout changes.
- New tools, panels, or artwork behaviour.
- Removing any existing control; the File menu, View controls, theme toggle,
  selection studio, brush library, swatches, dynamics and layer actions all
  remain reachable in the mobile shell.
