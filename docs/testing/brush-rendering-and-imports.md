# Brush Rendering and Import Test Guide

Use this guide to verify the tapered-stroke fix and the brush importer hardening.

## 0. Confirm the automated baseline

From the repository root:

```text
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --workspace
```

Expected:

- Formatting check prints nothing.
- Clippy finishes without warnings.
- App: 77 passed, 1 ignored.
- Engine: 317 passed.
- `real_brush_corpus_imports` is ignored unless a corpus is configured.
- Doctests: 0 failures.

## 1. Run the app

```text
cargo run --release -p procraft
```

Create a new document at least 800 x 400 px, add a blank layer, and select the Brush tool with black paint.

## 2. Reproduce the original taper case

Use exactly these values:

| Setting | Value |
|---|---:|
| Brush Size | 40 |
| Brush Opacity | 1.0 |
| Softness | 0 |
| Spacing | 0.10 |
| Stabilizer | 0 |
| Start taper | 0.04 |
| End taper | 0.05 |
| Taper size | 0.46 |
| Tip shape | 1 |
| Taper opacity | 0 |
| Taper pressure | 0 |

Draw each stroke slowly enough to see the live result, then release:

1. Horizontal stroke, left to right.
2. Vertical stroke, top to bottom.
3. 45-degree stroke.
4. A smooth curve, roughly a half circle.

Inspect both ends at 300-400% zoom.

Pass criteria:

- The stroke narrows smoothly at both ends instead of starting with a blunt full-size dab.
- Both endpoints are painted. There is no detached dot beyond the tip and no unpainted centerline gap.
- The width profile rises toward the middle and falls toward the end without a sudden step.
- Releasing the stroke does not visibly cut off, double-stamp, or leave a seam at either end.
- The curved stroke is continuous through the curve, including after release.

Repeat once with Spacing 0.25 and once with Spacing 0.5 to confirm the walker still closes short segments and tapered tails.

## 3. Check the app regression directly

```text
cargo test -p procraft preview_stroke_renders_taper
cargo test -p procraft screenshot_taper_replay_closes_both_tips
cargo test -p procraft-engine tapered_tail_has_no_gaps
cargo test -p procraft-engine short_segment_stamps_its_endpoint
cargo test -p procraft-engine taper_width_profile_is_monotonic
cargo test -p procraft-engine subpixel_dab_fades_instead_of_painting_full_alpha
```

All six must pass.

## 4. Run the real brush corpus

Set `PROCRAFT_BRUSH_CORPUS` to a directory containing brush files you are licensed to test. The harness walks subdirectories and tries every supported file; it does not copy or commit them.

PowerShell:

```text
$env:PROCRAFT_BRUSH_CORPUS = "D:\path\to\brush-corpus"
cargo test -p procraft-engine --test brush_corpus -- --ignored --nocapture
Remove-Item Env:PROCRAFT_BRUSH_CORPUS
```

Expected:

- Each file prints an OK or FAIL line with brush count, skipped count, and warnings.
- Every external-format brush reports a size at or below 64.0 px; `.pcbrush` and `.json` may exceed it because authored sizes are preserved. The corpus fails if an external brush is above 64 px.
- Each brush line includes `pressure_curve=N`, `opacity_curve=N`, `velocity_size_curve=N`, and `velocity_opacity_curve=N`. For the six local CSP brushes after the tilt/velocity correction: Chalk 4/0/0/0, Dry ink 3/0/0/0, Gouache 0/0/0/0, Pointillism 3/0/0/0, Rough wash 3/4/0/0, Wet blotting 5/2/0/0. `bctc.pcbrush` stays 0/0/0/0. CSP's second effector curve is tilt, so CSP brushes use the scalar velocity fallback and report no velocity curve.
- The test fails if any file errors or if no brushes were imported.
- A file with no readable tips or presets may legitimately produce a clean error; record the file and reason.

Do not add downloaded proprietary brush files to the repository.

## 5. Import smoke tests

Use files you own. Import through the app's normal brush-import control and check the status line after each import.

### Imported default sizes

- Every third-party batch is scaled uniformly so its largest brush starts at 64 px, matching ProCraft's largest built-in default. Relative sizes inside the pack are preserved.
- `.pcbrush`/`.json` libraries keep their authored size exactly; exporting and re-importing a brush must not resize it.
- The size slider, the panel size slider, and size-drag all span `1..=400`; a 300 px `.pcbrush` brush must remain 300 after import and display.
- Karoline's set should start near 14.6 / 64 / 22 / 46.9 / 14.6 px. FT should start near 48.9 / 37 px.

Automated checks:

```text
cargo test -p procraft-engine imported_size_normalization_preserves_ratios
cargo test -p procraft-engine external_imports_are_capped_but_pcbrush_keeps_its_size
cargo test -p procraft drag_size_clamps_to_the_slider_range
cargo test -p procraft drag_size_scales_from_base_and_clamps
```

### Krita `.kpp` and `.bundle`

- Import a `.kpp` with an embedded tip: the brush should appear with that tip.
- Import a bundle whose `.kpp` references an external PNG tip: the brush should use the referenced tip, not a plain round fallback.
- A corrupt or oversized preset should be skipped by name with a warning; valid presets in the same bundle should still import.

Automated checks:

```text
cargo test -p procraft-engine krita_bundle_resolves_external_tip_files
cargo test -p procraft-engine krita_bundle_reports_skipped_presets
```

### Procreate `.brush` and `.brushset`

- Import a brushset containing `__MACOSX/`, `._*`, or `.DS_Store`: it should import without junk brushes.
- A duplicated `Shape.png` entry should produce one brush, not duplicates.
- A brush folder with `Brush.archive` but no `Shape.png` should import one brush using the bundled shape its settings name (medium-hard disc, acrylic square, artery burst, or bleed blob), apply its size/opacity/curve settings, and skip nested `Reset/` archives. A brush that names no bundled shape falls back to a smooth round tip.
- `paintSize` is a fraction of the 256 px normalized Shape canvas: imported size is `256 * paintSize`, clamped to the brush limits. `maxSize` is only the slider UI limit, not the current size. `shapeInverted=true` means a black-on-white `Shape.png` is ink; without it, white is ink.
- Brushes that name `bundledGrainPath` (e.g. `Charcoal-6B`, `Artery-Noise`) get a generated grain texture even though the set ships no `Grain.png`; `grainDepth` then controls its strength. If the set does include the named file, that file is decoded instead.
- The five Karoline brushes must show a broken, grainy stroke rather than a solid blob: square/burst tips for Line Sketch and Grain Sketch, charcoal grain on Rough Sketch and Sketch, and the Bonobo blotch on Rough Grain Sketch.

Automated checks:

```text
cargo test -p procraft-engine procreate_ignores_mac_metadata_and_duplicate_shapes
cargo test -p procraft-engine procreate_shape_uses_archive_size_and_inversion
cargo test -p procraft-engine procreate_archive_without_shape_uses_round_tip
cargo test -p procraft-engine procreate_grain_depth_requires_grain_texture
cargo test -p procraft-engine parses_string_point_curves_and_separates_size_opacity
```

### Photoshop `.abr`

- Import an ABR with sampled tips: tips should import normally.
- Import an ABR with only parametric presets: the app should import descriptors as round brushes and show the fallback warning.
- The status line should show imported and skipped counts, then the first warning.

Automated check:

```text
cargo test -p procraft-engine abr_without_tips_reports_parametric_fallback
```

### CSP `.sut`

- Import a real `.sut` that uses a `BrushPattern` texture.
- Expected: the brush uses the pattern material, not the preset preview image or a paper texture.
- If no real fixture is available, record CSP visual parity as unverified. Do not treat a synthetic fixture as proof.
- Imported pressure curves affect size and opacity separately. Paint the same brush at light, medium, and firm pressure; size and opacity should follow the two curves independently.
- CSP's second effector curve is tilt, not velocity. Imported CSP brushes therefore keep the generic speed weights; draw the same straight stroke slowly, at medium speed, and quickly while holding pressure steady. Medium and fast strokes must remain continuous, not paint only at the start and end points, and must visibly thin/fade as speed rises. Speed is sampled from the raw pointer rather than the stabilized trail, so the Stabilizer setting does not flatten the response.
- These six local CSP brushes have `BrushUseIn=0` and `BrushUseOut=0`, so the importer does not add a taper. Blunt ends on an imported brush are expected unless the source brush enables in/out taper or the user enables ProCraft's Taper.

Automated checks:

```text
cargo test -p procraft-engine sut_prefers_pattern_material_over_paper_texture
cargo test -p procraft-engine sut_reads_black_alpha_pattern_tip
cargo test -p procraft-engine sut_imports_size_pressure_curve
cargo test -p procraft-engine sut_imports_opacity_pressure_curve
cargo test -p procraft-engine sut_does_not_import_tilt_as_velocity
cargo test -p procraft-engine effector_
cargo test -p procraft speed_follows_raw_pointer_not_stabilized_trail
```

## 6. Final pass/fail record

Record for each manual case:

- File used and its source/license.
- Imported brush count and any warning text.
- Taper angle tested and zoom level.
- Pass/fail against the criteria in section 2.
- Screenshot or short recording for any failure.

Do not modify or commit `dist/`.
