# ProCraft

An open-source, clean-room raster painting studio in pure Rust: layered
painting, a soft-round brush engine, and PNG import/export. Inspired by
professional painting workflows; every line of code and every asset here is
original.

> **Trademark note:** "Procreate" is a registered trademark of Savage
> Interactive. ProCraft is an independent project with its own name, branding
> and code, and is not affiliated with or endorsed by any other vendor.

## Status: milestone 1

- [x] Layered document model (add / hide / reorder / opacity)
- [x] Soft-round brush + eraser with spacing and undo
- [x] egui desktop app: paint, layers panel, color/size sliders, PNG export
- [ ] Brush library + pressure simulation
- [ ] Selections, transform, filters
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
