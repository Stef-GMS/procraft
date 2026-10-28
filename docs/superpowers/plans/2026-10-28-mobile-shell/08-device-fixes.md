# Plan 08 — Device-verdict fixes (iterative)

**Goal:** close the gaps only a real iPhone/iPad finds.

**Inputs:** the user's device notes on the iOS IPA test build (`aed004a`,
ad-hoc signed, `dist/ios-aed004a/`).

**Steps**

- [x] Record each verdict item as a checkbox with the device, orientation and
  build SHA.
- [x] Fix one item at a time with a host test when the defect has a pure seam
  (icon geometry, readout width); otherwise screenshot before/after.
- [x] Keep desktop screenshots unchanged apart from the two deliberate
  differences noted below (painted theme icon, reserved readout slot).
- [x] Update this file's checklist and append a session per fix wave.
- [x] Commit each wave: `fix(ui): <device verdict item>`.

**Exit:** the user signs off the mobile shell on both devices.

**Log:** append a session to `chunk-26.json`.

## Verdict 1 — 2026-10-28, iPhone/iPad, build `aed004a`

- [x] **D1 — Undo, redo and theme icons showed as small squares.** Fixed.
- [x] **D2 — Brush-size readout flickered when 90–99 became 100+.** Fixed.
- [ ] **D3 — Painting lags as the brush gets bigger.** Root cause confirmed
  and measured; the fix belongs to the rendering milestone (user's call).

### D1 — tofu boxes in the floating clusters

**Root cause.** The cluster buttons rendered `↶` (U+21B6), `↷` (U+21B7) and
the theme toggle `☾`/`☀` (U+263E/U+2600) as *text*. The host only gets glyph
coverage from `C:\Windows\Fonts\seguisym.ttf`; the iOS/Android builds ship
egui's bundled fonts only (Ubuntu-Light, NotoEmoji, emoji-icon-font), and
`Fonts::has_glyph` says those three are **not** covered — hence tofu on
device and arrows on Windows.

**Fix.**

- `design.rs` grew a painter-drawn icon set (`Icon::{Undo, Redo, Moon, Sun,
  Paper}`) with `paint_icon` / `icon_button`; the clusters, the phone strip
  and the theme pill now draw shapes, never text glyphs. The moon crescent is
  cut with the pill's own fill, so it survives the hover state.
- The same probe found unguarded glyphs on the fallback path: `✏ ⌫ 💧`
  (tool radios) and `↔ Move`. Those now fall back to plain words, like the
  rest of the `has_symbols == false` strings.
- Panel arrows that were tofu on device became ASCII: palette `<` / `>`,
  drawer close `<` / `>`, layer `Up` / `Down`, canvas-anchor `·`.
- New test `bundled_fonts_cover_every_glyph_the_fallback_paths_print` builds
  an `egui::Context` with the same default fonts the phone ships and asserts
  every string the no-symbol paths print is covered — so this cannot come
  back silently.

**Evidence.** `dist/mobile-shell/2026-10-28/plan08-tablet-{dark,light,paper}.png`
(moon, sun, page), `plan08-phone-mobile-dark-scrolled.png` (both arrows in the
strip), `plan08-phone-icons-base.png` (docked shell: `<` on the Tool card) and
`plan08-desktop-dark.png` (same painted icon on desktop).

### D2 — the size readout shifts the rail at 99 → 100

**Root cause.** `thick_slider` printed `"Size 90"` / `"Size 100"` as one
label, so the extra digit pushed the `−`, the rail and the `+` sideways — and
the capsule is a centred `Area`, so the whole row re-centred. The phone strip
printed the bare number between pills, moving Tools/Layers/undo by a digit.

**Fix.** `design::readout_label` paints the value right-aligned in a slot
sized for the widest value the range can show (`widest_value_text`: "400" for
brush size, "-32" for grow, "1.00" for the 0.05-step sliders). The strip
readout reserves `{:.0}` of `BRUSH_SIZE_MAX`. The rail, the steppers and every
strip pill keep their x position across 9 → 99 → 400.

**Evidence.** `plan08-phone-mobile-dark.png` (24) vs
`plan08-phone-size-3digit.png` (384): identical x positions for `−`, the rail
and Tools/Layers. Test `readout_slot_is_one_width_across_the_whole_range`.

### D3 — lag while painting with a large brush

**Root cause (confirmed, not fixed).** Dabs are stamped on the CPU:
`Document::dab_one_resolved` walks the brush bounding box (≈ `size²` pixels)
with float falloff, selection, grain and dual-tip math per pixel, and
`refresh_texture` then rebuilds and uploads the dirty rect. At least one dab
lands per pointer event, so once the spacing step exceeds the movement the
cost per event is ≈ `size²`.

**Measured** (release build, 1280×800 page, 8 px pointer steps, host CPU):

| size | ms / event | ms / dirty flatten |
| ---- | ---------- | ------------------ |
| 10   | 0.07       | 0.02               |
| 50   | 1.56       | 1.15               |
| 100  | 2.43       | 1.38               |
| 200  | 8.08       | 2.67               |
| 400  | 22.57      | 5.56               |

An iPhone core is slower than this host, so 200+ px brushes cannot hold
60 fps. The fix is a rendering change (GPU dab splatting, or an incremental
clipped CPU path with a cheaper falloff), not a shell tweak — deferred by the
user to a later milestone. Recorded here so that milestone starts from these
numbers.

## Packs

- **iOS IPA for verdict 2** — `ui-b2-restyle` pushed at `406b8a6`; run
  [40702350473](https://github.com/surfrpt1/procraft/actions/runs/40702350473)
  succeeded (check, build, package, upload). Staged as
  `dist/ios-406b8a6/ProCraft-ios-v1.ipa`: 23,309,377 bytes, SHA-256
  `3dd63046b6382f790c01ba480d4ccae999887622bcdfa427c8ef129ee1c03f9f`,
  version 0.9.0, `com.surfrpt1.procraft`, iOS 17+, iPhone + iPad. Ad-hoc
  signed — re-sign with a development profile (Sideloadly, AltStore or Xcode)
  before installing.
- **Android APK** — not requested.
