# ProCraft

An open-source, clean-room raster painting studio in pure Rust: layered
painting, a soft-round brush engine, and PNG import/export. Inspired by
professional painting workflows; every line of code and every asset here is
original.

> **Trademark note:** "Procreate" is a registered trademark of Savage
> Interactive. ProCraft is an independent project with its own name, branding
> and code, and is not affiliated with or endorsed by any other vendor.

## Status: milestone 7

- [x] Layered document model (add / hide / reorder / opacity / blend)
- [x] Soft-round brush + eraser with spacing, dynamics and undo
- [x] egui desktop app: paint, layers panel, color/size sliders, PNG export
- [x] Project files (`.procraft` v2 keeps every layer + symmetry; v1 still opens), zoom, themes, page colors
- [x] Rectangular + freehand lasso selections constraining paint
- [x] Flood fill bucket with tolerance (selection-aware, symmetry-mirrored)
- [x] Move tool (drag active layer), shape tools (line/rect/ellipse + fill)
- [x] Layer transforms (scale, rotate, flip H/V with bilinear sampling)
- [x] Mirror symmetry painting (horizontal / vertical / both + axis overlay)
- [x] Layer filters: brightness/contrast, box blur, unsharp sharpen (selection-masked)
- [x] Kaleidoscope symmetry (2-16 segments), layer rename/duplicate/merge, eyedropper, honest brush previews
- [ ] Brush library file format (import/export sets)
- [ ] Native iOS build (same playbook as our PdfCraft iOS spike)

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
