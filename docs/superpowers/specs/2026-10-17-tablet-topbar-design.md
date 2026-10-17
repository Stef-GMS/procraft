# Tablet & Mobile Top Bar — Design

**Date:** 2026-10-17
**Status:** Approved by user (mockups reviewed in visual companion; "looks right build them now")

## Problem

- iPad (portrait 820pt and landscape 1180pt+) hits the ≥600px **Wide** bar: Gallery, Reference, File ▾, Undo, Redo, Clear layer, zoom −/%/+, Fit, rotate slider, Reset rotation, theme combo, About, Shortcuts — all inline. On the user's iPad the row overflowed and "Reset rotation" was cut off.
- The iOS bar draws **under the status bar**: only Android has a top inset (`rfd::android_status_bar_height()` + `clamp_status_inset` ≤ 200 px; `android_inset_top` cached in the app). iOS has none.
- The bottom status line sits in the home-indicator zone (nothing lifts it).
- Android **tablets** hit the same crammed Wide row.
- Side panels keep every section open; the user wants the existing fold pattern ("Selection Studio" / brush sections use `ui.collapsing`, collapsed by default) applied to the less-used sections only. Panel collapse/expand itself (▶ button in each panel + edge fabs) already exists and is NOT being redesigned.

## Approved design

1. **Tablet bar (iOS/Android, width ≥ 600):** same button row as today's phone bar — `Gallery · Reference · File ▾ · Undo · Redo · Clear layer · View ▾` — i.e. the view cluster moves into **View ▾** (exactly what `view_controls(ui, true)` already renders: zoom −/%/+, Fit, Rotate° slider, Reset rotation, Theme combo, About, Shortcuts). Touch-sized: `interact_size.y = 40.0`, `item_spacing = (8, 8)`. `horizontal_wrapped` stays as the graceful-wrap fallback. Desktop ≥600 keeps the full inline Wide bar. Narrow (<600) unchanged everywhere.
2. **Safe areas (iOS):** top — pad the top bar by the window's `safeAreaInsets.top` (points, clamped ≤200 like the Android path); bottom — pad the bottom status panel so its text clears the home indicator. Read **per frame** (a cheap property read; correct across rotation/split-screen). Android keeps its existing cached JNI top inset byte-identical; Android bottom unchanged.
3. **Panel folds:** default-collapsed `ui.collapsing` (identical to the existing "Selection Studio" pattern) for — left panel: **My brushes, Page, Guides**; right panel: **Animation, Transform layer, Symmetry, Filters**. Tool / Brushes / Color / Swatches / Layers stay open as today. Panels do **not** become drawers or overlays (the slide-over part of mockup direction B was explicitly dropped by the user).

## Non-goals

- No drawer/overlay panel rework.
- No phone bar content change on Android; iPhone only gains the safe areas (it already renders the Narrow bar).
- No desktop changes (`bar_layout(≥600, mobile=false) == Wide`).
- No glyph/icon rework: bar buttons are text labels; on mobile `has_symbols == false` already picks plain "About"/"Shortcuts".
- No gestures, no brush popup, no release/versioning changes (release prep is a separate later step).

## Acceptance

- iPad portrait + landscape: one tidy bar row, below the status bar, nothing cut off; View ▾ holds the view cluster; buttons comfortably touchable.
- Bottom status text clears the home indicator on iPad/iPhone.
- Split-screen / Slide Over (<600): phone bar as today.
- Desktop: unchanged layout; full suite green.
- Panels: the seven listed sections start collapsed, expand on tap; everything else unchanged.

## Verification

- Unit: `bar_layout` test table gains a `mobile` column; full workspace suite (216 engine + 41 app + others) green; `cargo fmt`; `cargo clippy --workspace --all-targets -- -D warnings`; Android `cargo check` for aarch64-linux-android + x86_64-linux-android.
- iOS compile: CI `ios-ipa.yml` (workflow_dispatch) after push — Windows cannot compile iOS locally.
- Device: user tests the IPA on iPad (bar row, insets, folds); optionally an Android APK for a tablet if one is available.
