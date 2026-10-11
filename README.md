# ProCraft

An open-source, clean-room raster painting studio in pure Rust: layered
painting, a soft-round brush engine, and PNG import/export. Inspired by
professional painting workflows; every line of code and every asset here is
original.

> **Trademark note:** "Procreate" is a registered trademark of Savage
> Interactive. ProCraft is an independent project with its own name, branding
> and code, and is not affiliated with or endorsed by any other vendor.

## Status: milestone 4

- [x] Layered document model (add / hide / reorder / opacity / blend)
- [x] Soft-round brush + eraser with spacing, dynamics and undo
- [x] egui desktop app: paint, layers panel, color/size sliders, PNG export
- [x] Project files (`.procraft` keeps every layer), zoom, themes, page colors
- [x] Rectangular selections constraining paint
- [x] Move tool (drag active layer), shape tools (line/rect/ellipse + fill)
- [x] Layer transforms (scale, rotate with bilinear sampling)
- [ ] Brush library file format (import/export sets)
- [ ] Selections: ellipse/lasso, fill, transform
- [ ] Native iOS build (same playbook as our PdfCraft iOS spike)

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
