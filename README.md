# ProCraft

By [surfrpt1](https://github.com/surfrpt1).

An open-source, clean-room raster painting studio in pure Rust: layered
painting, a soft-round brush engine, and PNG import/export. Inspired by
professional painting workflows; every line of code and every asset here is
original.

> **Trademark note:** "Procreate" is a registered trademark of Savage
> Interactive. ProCraft is an independent project with its own name, branding
> and code, and is not affiliated with or endorsed by any other vendor.

## Status: milestone 18

- [x] Layered document model (add / hide / reorder / opacity / blend)
- [x] Soft-round brush + eraser with spacing, dynamics and undo
- [x] egui desktop app: paint, layers panel, color/size sliders, PNG export
- [x] Project files (`.procraft` v6 with masks, clipping + 10 blends and layer offsets; v1-v5 still open), zoom, themes, page colors with active-preset highlight
- [x] Rectangular + freehand lasso selections constraining paint
- [x] Flood fill bucket with tolerance (selection-aware, symmetry-mirrored)
- [x] Move tool (drag active layer), shape tools (line/rect/ellipse/triangle/diamond/star/arrow/polygon + fill)
- [x] Layer transforms (scale, rotate, flip H/V with bilinear sampling)
- [x] Mirror symmetry painting (horizontal / vertical / both + axis overlay)
- [x] Layer filters: brightness/contrast, box blur, unsharp sharpen (selection-masked)
- [x] Kaleidoscope symmetry (2-16 segments), layer rename/duplicate/merge, eyedropper, honest brush previews
- [x] Non-destructive layers: reveal masks (paint luminance, eraser reveals, mask viewer), clipping masks, 10 blend modes (masks travel with transforms, merge bakes them)
- [x] Layered PSD interchange: export keeps names/opacity/blends/clipping/transparency/masks for Photoshop/Procreate/Krita/GIMP; import restores structure (masks and exotic blends documented gaps)
- [x] Compressed project files (PCRAFT04 zlib per layer; verified 12MB → 107KB on a 3-layer painting, corrupt payloads rejected)
- [x] Brush library: named custom brushes with honest clickable previews + active highlight, `.pcbrush` set import/export (validated, max 64)
- [x] Pro brush interop: import Photoshop `.abr` (v6-v10 sampled tips + names/spacing via UUID linkage, verified on real Photoshop 2026 files), Procreate `.brush`/`.brushset` (Shape tips), Krita `.kpp`/`.bundle` (verified on Krita repo presets: MaskGenerator diameter, Opacity x Flow, embedded tips), GIMP `.gbr`, tip images; export grayscale tip PNGs for other software
- [x] Shape tools wave 2 (triangle/diamond/star/arrow/3-12-sided polygon, outline + fill), collapsible sidebars + wrapped top bar for portrait/iPad, off-page-stroke crash fix, touch-sized layer rename (Save/Cancel)
- [x] Native iOS build (target-gated eframe/rfd-shim, CI-built IPA sideload, v4 verified): document-picker imports, Documents saves + share sheet, async opens, touch-force pressure, post-open repaint fix
- [x] About dialog (auto version, builder, repo + check-for-updates links, MIT line)
- [x] Brush dynamics: pressure/velocity size + opacity curves, grain + scale, roundness, angle-follow rotation, scatter, stabilizer smoothing (old `.pcbrush` files load identically)
- [x] Palettes: named sets, `.pcpalette` save/open, GIMP `.gpl` import, starter set, HSL + complementary/triadic/analogous harmonies; drawing guides (off/square/iso/1pt/2pt) with canvas overlay + snap (iso-lattice and vanishing-point rays, Snap-range magnet)
- [x] Animation Assist: layers-as-frames, play/stop + FPS stepping, onion skins (red/blue ghosts + amount), GIF export (1-60fps, infinite loop)
- [x] Text tool: click-to-anchor, multiline 8-128px dialog, bundled Inter (OFL, latin subset, fontdue raster), bakes to layer in one undo step
- [x] Lossless layer moves: O(1) offsets (off-canvas content preserved, undoable), offset-aware paint/flood/text/merge/flip/affine/masks/animation/PSD

## Icon

Original layer-stack mark drawn natively in VectorCraft (headless batch;
`assets/icon-generator.py` replays the whole construction, `assets/icon.svg`
is the editable source). Shipped as the window icon and the embedded exe icon.

## Build

Needs Rust 1.85+ ([rustup](https://rustup.rs)):

```sh
git clone https://github.com/surfrpt1/procraft.git
cd procraft
cargo run --release -p procraft
```

## Layout

- `crates/engine` — document, layers, brush dabs, undo, PNG I/O (no GUI).
- `apps/procraft` — egui desktop app.

## License

MIT — see [LICENSE-MIT](LICENSE-MIT).
