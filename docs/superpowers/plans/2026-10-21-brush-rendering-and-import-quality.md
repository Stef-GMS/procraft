# Brush Rendering and Import Quality

## Goal

Fix tapered brush strokes that render blunt, gappy, or closed incorrectly, then make the advertised brush importers produce predictable tips and report partial failures instead of silently dropping data.

## Constraints

- No commits or pushes unless the human partner asks.
- Do not modify or commit `dist/`.
- CSP `.bfps` is a settings reference only in this pass; do not add a `.bfps` importer.
- CSP `.sut` fixes are limited to defects proven with a real fixture. Synthetic fixtures alone are not proof of CSP compatibility.
- Tests must assert rendered pixels, imported mask dimensions/values, or explicit warnings; no tests that only assert `is_ok`.
- Real third-party brush files are not committed.

## Task 1: Lock taper behavior with failing tests

**Files:** `crates/engine/src/brush.rs`, `apps/procraft/src/lib.rs`

Add pixel-level tests:

- `short_segment_stamps_its_endpoint`: size 24, spacing 1.0, 8 px segment; endpoint alpha must be nonzero.
- `tapered_tail_has_no_gaps`: size 40, spacing 0.5, end taper 1.0, size taper 0.8, 200 px stroke; centerline tail and final endpoint must be painted.
- `screenshot_taper_replay_closes_both_tips`: start 0.04, end 0.05, size 0.46, shape 1, spacing 0.10; record/replay horizontal and curved strokes; both tips painted, no centerline gaps.
- `taper_width_profile_is_monotonic`: width rises to the middle and falls toward the end.

**Expected RED:** endpoint/gap tests fail because the walker skips the final dab and uses a fixed step.

## Task 2: Fix the taper walker

**File:** `crates/engine/src/brush.rs`

- In `Document::stroke_to_dyn`, compute each next step from the current tapered dab size: `step = (size * brush.spacing).max(1.0)`.
- Track the last stamped distance and explicitly stamp the endpoint when it is more than 0.5 px past the last dab.
- Preserve `from=None` start behavior.
- Update any existing taper tests whose assumptions change.

**Expected GREEN:** all taper tests and the full engine suite pass.

## Task 3: Normalize imported tips

**File:** `crates/engine/src/probrush.rs`, `apps/procraft/src/lib.rs`

- Add `trim_mask(w, h, mask)` returning the nonzero bounding box or `tip image is empty`.
- Apply trimming in `import_image_tip` before `stamp_brush`/`fit_tip` (keeping CSP pattern tips untrimmed).
- `import_image_tip`: use alpha when the image has meaningful transparency; otherwise infer polarity from the border (light border means dark ink).
- Add `gif/tif/tiff` to the main brush picker and complete the dual-tip extension list.
- Tests: black-on-white, white-on-transparent, white-on-black, padded tip, all-transparent error.

## Task 4: Report partial imports

**Files:** `crates/engine/src/probrush.rs`, `apps/procraft/src/lib.rs`

- Add `ImportOutcome { brushes, warnings, skipped }` and `import_brush_file_report`.
- Keep `import_brush_file` as a compatibility wrapper.
- Krita bundle and ABR paths must report skipped presets/tips by name and reason.
- App status must show imported and skipped counts.
- Test that a partially unreadable bundle returns brushes plus warnings.

## Task 5: Real brush corpus

**Files:** `crates/engine/tests/brush_corpus.rs`, `crates/engine/testdata/real/README.md`

- Add an ignored corpus test reading `PROCRAFT_BRUSH_CORPUS`.
- Import every file and print a per-file report; failures must be visible, not skipped.
- Document source URLs and licenses. Do not commit proprietary brush files.

## Task 6: Format-specific fixes from the corpus

- Procreate: ignore `__MACOSX/`, `._*`, and `.DS_Store`; dedupe duplicate `Shape.png` entries.
- Krita bundle: resolve `.kpp` external tip filenames against a bounded ZIP resource map.
- ABR: when no sampled tips exist, create parametric presets from descriptor names/diameter/spacing and report the fallback.
- CSP `.sut`: obtain a real fixture first; fix only observed defects, especially accidental selection of a preview image instead of a `BrushPattern` tip.

## Task 7: Verification

Run:

```text
cargo test --workspace
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
```

Manually test the screenshot taper settings at 0, 45, and 90 degrees and on a curve. Run the corpus and record results. Do not commit `dist/` or downloaded proprietary files.

## Progress

Automated work is green; manual app verification remains.

- Taper walker, imported-tip normalization, partial-import reporting, corpus harness, Krita external tips, ABR parametric fallback, and CSP pattern/curve fixes are implemented and covered by tests.
- Procreate `.brushset` archives without `Shape.png` now import from `Brush.archive`; `Reset/` archives are skipped and sibling `Grain.png` is attached when present.
- Procreate builds that reference app-bundled textures (`bundledShapePath` / `bundledGrainPath`, e.g. `Brush-Artery-Burst.jpg`, `Charcoal-6B`) do not ship those files in the set. `crates/engine/src/probrush/bundled.rs` now generates deterministic, original stand-ins: a medium-hard disc, an acrylic square with bristle streaks, an artery burst, and a bleed blob for shapes; charcoal, paper, bonobo, and noise lattices for grain. If a set does include the named file, `read_bundled_asset` decodes it and uses it instead of the approximation. All five Karoline brushes previously imported with a plain round tip and no grain, which is what produced the solid blobs on screen.
- `grainDepth` is applied after the bundled grain is attached, so `grainDepth = 1.0` now actually drives the grain.
- Fixed the medium-hard disc generator: the edge ramp divided the whole radius by `0.06`, shrinking the tip to a ~3 %-radius dot. It now ramps the outer 18 % of the radius so the disc reads soft-edged like the source thumbnails. In the corpus smoke render this moved Sketch from 62 painted pixels to 5395 and Rough Sketch from 383 to 17086.
- `shapeRandomise` no longer forces full per-dab rotation. It only applies when the brush carries several shapes (`shapeCount > 1`); Procreate stores `shapeCount = 0` for all five Karoline brushes, so square tips keep their orientation.
- Procreate settings map `paintSize` to `256 * paintSize` (the normalized Shape canvas), clamped to `1..=400`; `maxSize` is only the slider UI limit. `shapeInverted` maps to shape polarity, pressure size/opacity to separate curves, and speed dynamics to velocity scalars. Names strip leading `/`.
- Third-party imports are scaled uniformly so the largest brush in a batch starts at 64 px, the same scale as ProCraft's largest built-in default; relative sizes inside a pack survive. `.pcbrush`/`.json` bypass this and keep authored sizes, so re-export is lossless. The size slider and drag gesture now span `1..=400` to match the engine.
- Local corpus (11 files, 2026-10-23): all 11 import with no failures. Karoline 5 brushes / 0 warnings start at 14.6/64/22/46.9/14.6 px, each with a 256×256 generated tip and grain; FT 48.9/37; every external brush is at or below the 64 px default ceiling; `bctc.pcbrush` keeps 24. Ognjen 17 brushes / 25 skipped, all `no sampled tip` parametric presets.
- Engine tests 323 (including 6 new `bundled::` mask tests that assert coverage, a full 0..=255 grain span, determinism, and per-name texture differences), app tests 77 passed + 1 ignored corpus preview, clippy clean, fmt clean, release rebuilt.
- Size-oracle notes: the Procreate QuickLook thumbnails (1060×324) give stroke medians GrainSketch 115 / RoughSketch 39 / Sketch 48 / LineSketch 16 / RoughGrainSketch 113, and the promo sheet medians are Rough 25 / Sketch 13 / Line 7 / Grain 19 / RoughGrain 27. The promo stamp column is normalized to ~100 px for every brush, so neither is an absolute size oracle; the corpus ratios are the best available evidence.
- Grain fineness was calibrated against the QuickLook thumbnails: round-trip grain is canvas-anchored and only true zeros survive dense overlap, so the first pass looked like black/white pepper. Cell counts are now charcoal 150, paper 90, bonobo 44, noise 200, default 110, with lower drop-out thresholds, which leaves a soft body with sparse fine tooth.
- Offscreen previews at imported defaults for all five Karoline brushes confirmed a textured stroke for each (two pressures per brush). The release binary `target/release/procraft.exe` was rebuilt after the tuning.
- Remaining manual checks: real Import dialog for all 11 files, paint a stroke with the Karoline/FT/Ognjen imports and compare texture and default widths against `photo_2026-08-07_22-10-45.jpg` and the QuickLook thumbnails, and verify taper at 0/45/90 degrees and on a curve. `photo_2026-08-07_22-10-45.jpg` remains in the local corpus with undecided retention.
