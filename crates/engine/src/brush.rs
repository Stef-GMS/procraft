//! Soft-round brush engine: colors, dab stamping and stroke interpolation.

use super::document::Document;

/// RGBA color, 0..=255 per channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Color(pub [u8; 4]);

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self([r, g, b, 255])
    }
}

/// Brush shape settings. All original math: a radial falloff stamp laid down
/// along the pointer path at `spacing` intervals.
#[derive(Clone, Copy, Debug)]
pub struct Brush {
    /// Dab diameter in canvas pixels.
    pub size: f32,
    /// Per-dab alpha multiplier, 0.0..=1.0.
    pub opacity: f32,
    /// Edge softness, 0.0 (hard disc) ..= 1.0 (fully soft).
    pub softness: f32,
    /// Dab spacing as a fraction of `size` (0.1 = smooth, 1.0 = dotted).
    pub spacing: f32,
    /// Eraser mode: remove alpha instead of painting color.
    pub eraser: bool,
}

impl Default for Brush {
    fn default() -> Self {
        Self { size: 24.0, opacity: 1.0, softness: 0.6, spacing: 0.15, eraser: false }
    }
}

impl Document {
    /// Stamp one dab centered at canvas pixel `(x, y)` on the active layer,
    /// mirrored per [`Document::symmetry`].
    /// Pixels outside [`Document::selection`] (when set) are skipped.
    pub fn dab(&mut self, brush: &Brush, color: Color, x: f32, y: f32) {
        let pts = self.symmetry.mirrored_points(self.w, self.h, self.symmetry_segments, x, y);
        let mut done: Vec<(f32, f32)> = Vec::with_capacity(4);
        for (mx, my) in pts {
            if done.iter().any(|&(dx, dy)| (dx - mx).abs() < 0.001 && (dy - my).abs() < 0.001) {
                continue;
            }
            done.push((mx, my));
            self.dab_one(brush, color, mx, my);
        }
    }

    fn dab_one(&mut self, brush: &Brush, color: Color, x: f32, y: f32) {
        let (w_u32, h_u32) = (self.w, self.h);
        let (w, h) = (w_u32 as f32, h_u32 as f32);
        let radius = (brush.size * 0.5).max(0.5);
        let strength = brush.opacity.clamp(0.0, 1.0);
        if strength <= 0.0 || self.layers.get(self.active).is_none() {
            return;
        }
        let x0 = (x - radius).floor().max(0.0) as u32;
        let y0 = (y - radius).floor().max(0.0) as u32;
        let x1 = (x + radius).ceil().min(w - 1.0).max(0.0) as u32;
        let y1 = (y + radius).ceil().min(h - 1.0).max(0.0) as u32;
        self.mark_dirty(x0, y0, x1, y1);
        let selection = self.selection;
        let active = self.active;
        if active >= self.layers.len() {
            return;
        }
        let mask = self.selection_mask.as_deref();
        let layer = &mut self.layers[active];
        let w = w_u32;
        for py in y0..=y1 {
            for px in x0..=x1 {
                if let Some((sx0, sy0, sx1, sy1)) = selection {
                    if px < sx0 || px > sx1 || py < sy0 || py > sy1 {
                        continue;
                    }
                }
                if let Some(m) = mask {
                    if m.get((py * w + px) as usize).copied().unwrap_or(0) == 0 {
                        continue;
                    }
                }
                let d = ((px as f32 + 0.5 - x).hypot(py as f32 + 0.5 - y)) / radius;
                if d > 1.0 {
                    continue;
                }
                // 1.0 inside the hard core, linear fade across the soft edge.
                let edge = brush.softness.clamp(0.0, 1.0);
                let falloff = if d <= 1.0 - edge { 1.0 } else { 1.0 - (d - (1.0 - edge)) / edge.max(0.001) };
                let a = (falloff.clamp(0.0, 1.0) * strength * 255.0).round() as u8;
                if a == 0 {
                    continue;
                }
                let o = ((py * w + px) * 4) as usize;
                if o + 3 >= layer.pixels.len() {
                    continue;
                }
                if brush.eraser {
                    // Destination-out: scale existing alpha down.
                    let keep = 255u16.saturating_sub(a as u16);
                    layer.pixels[o + 3] = ((layer.pixels[o + 3] as u16 * keep) / 255) as u8;
                } else {
                    let src_a = a as f32 / 255.0;
                    let dst_a = layer.pixels[o + 3] as f32 / 255.0;
                    let out_a = src_a + dst_a * (1.0 - src_a);
                    if out_a > 0.0 {
                        let c = color.0;
                        layer.pixels[o] = ((c[0] as f32 * src_a + layer.pixels[o] as f32 * dst_a * (1.0 - src_a)) / out_a).round() as u8;
                        layer.pixels[o + 1] = ((c[1] as f32 * src_a + layer.pixels[o + 1] as f32 * dst_a * (1.0 - src_a)) / out_a).round() as u8;
                        layer.pixels[o + 2] = ((c[2] as f32 * src_a + layer.pixels[o + 2] as f32 * dst_a * (1.0 - src_a)) / out_a).round() as u8;
                        layer.pixels[o + 3] = (out_a * 255.0).round() as u8;
                    }
                }
            }
        }
    }

    /// Advance a stroke from `from` to `to`, laying dabs every
    /// `spacing * size` pixels. Returns the leftover distance for chaining.
    /// Call [`Document::checkpoint`] once when the stroke starts.
    pub fn stroke_to(
        &mut self,
        brush: &Brush,
        color: Color,
        from: Option<(f32, f32)>,
        to: (f32, f32),
        leftover: f32,
    ) -> (Option<(f32, f32)>, f32) {
        let step = (brush.size * brush.spacing).max(1.0);
        let Some((sx, sy)) = from else {
            self.dab(brush, color, to.0, to.1);
            return (Some(to), 0.0);
        };
        let (dx, dy) = (to.0 - sx, to.1 - sy);
        let dist = (dx * dx + dy * dy).sqrt();
        if dist <= 0.0 {
            return (from, leftover);
        }
        let mut travelled = leftover;
        while travelled <= dist {
            let t = travelled / dist;
            self.dab(brush, color, sx + dx * t, sy + dy * t);
            travelled += step;
        }
        (Some(to), travelled - dist)
    }
}

/// Built-in brush presets: name plus settings. Original set, roughly grouped
/// from hard sketching to soft painting.
pub fn presets() -> Vec<(&'static str, Brush)> {
    vec![
        ("Pencil", Brush { size: 3.0, opacity: 1.0, softness: 0.15, spacing: 0.1, eraser: false }),
        ("Ink Pen", Brush { size: 6.0, opacity: 1.0, softness: 0.3, spacing: 0.1, eraser: false }),
        ("Round", Brush { size: 24.0, opacity: 1.0, softness: 0.6, spacing: 0.15, eraser: false }),
        ("Marker", Brush { size: 28.0, opacity: 0.85, softness: 0.5, spacing: 0.12, eraser: false }),
        ("Airbrush", Brush { size: 64.0, opacity: 0.25, softness: 1.0, spacing: 0.08, eraser: false }),
        ("Chalk", Brush { size: 18.0, opacity: 0.8, softness: 0.9, spacing: 0.25, eraser: false }),
    ]
}
impl Document {
    /// Paint one pixel with source-over at brush opacity, honoring selection
    /// and mirroring per [`Document::symmetry`].
    fn put(&mut self, brush: &Brush, color: Color, px: u32, py: u32) {
        let pts = self.symmetry.mirrored_pixels(self.w, self.h, self.symmetry_segments, px, py);
        let mut done: Vec<(u32, u32)> = Vec::with_capacity(4);
        for (mx, my) in pts {
            if done.contains(&(mx, my)) {
                continue;
            }
            done.push((mx, my));
            self.put_one(brush, color, mx, my);
        }
    }

    fn put_one(&mut self, brush: &Brush, color: Color, px: u32, py: u32) {
        if px >= self.w || py >= self.h {
            return;
        }
        if let Some((sx0, sy0, sx1, sy1)) = self.selection {
            if px < sx0 || px > sx1 || py < sy0 || py > sy1 {
                return;
            }
        }
        if let Some(mask) = self.selection_mask.as_deref() {
            if mask.get((py * self.w + px) as usize).copied().unwrap_or(0) == 0 {
                return;
            }
        }
        let o = ((py * self.w + px) * 4) as usize;
        let Some(layer) = self.active_layer_mut() else { return };
        if o + 3 >= layer.pixels.len() {
            return;
        }
        let src_a = brush.opacity.clamp(0.0, 1.0);
        let dst_a = layer.pixels[o + 3] as f32 / 255.0;
        let out_a = src_a + dst_a * (1.0 - src_a);
        if out_a > 0.0 {
            let c = color.0;
            layer.pixels[o] = ((c[0] as f32 * src_a + layer.pixels[o] as f32 * dst_a * (1.0 - src_a)) / out_a).round() as u8;
            layer.pixels[o + 1] = ((c[1] as f32 * src_a + layer.pixels[o + 1] as f32 * dst_a * (1.0 - src_a)) / out_a).round() as u8;
            layer.pixels[o + 2] = ((c[2] as f32 * src_a + layer.pixels[o + 2] as f32 * dst_a * (1.0 - src_a)) / out_a).round() as u8;
            layer.pixels[o + 3] = (out_a * 255.0).round() as u8;
        }
        let w = self.w;
        let h = self.h;
        self.mark_dirty(px.min(w - 1), py.min(h - 1), px.min(w - 1), py.min(h - 1));
    }

    /// Straight segment between two canvas points. Caller checkpoints first.
    /// Returns leftover spacing for chaining segments.
    pub fn shape_line(
        &mut self,
        brush: &Brush,
        color: Color,
        from: Option<(f32, f32)>,
        to: (f32, f32),
        leftover: f32,
    ) -> (Option<(f32, f32)>, f32) {
        self.stroke_to(brush, color, from, to, leftover)
    }

    /// Rectangle outline through dab segments. Caller checkpoints first.
    pub fn shape_rect(&mut self, brush: &Brush, color: Color, x0: f32, y0: f32, x1: f32, y1: f32) {
        let corners = [(x0, y0), (x1, y0), (x1, y1), (x0, y1), (x0, y0)];
        let mut last = None;
        let mut left = 0.0;
        for &p in &corners {
            let r = self.stroke_to(brush, color, last, p, left);
            last = r.0;
            left = r.1;
        }
    }

    /// Ellipse outline inside the box. Caller checkpoints first.
    pub fn shape_ellipse(&mut self, brush: &Brush, color: Color, x0: f32, y0: f32, x1: f32, y1: f32) {
        let (cx, cy) = ((x0 + x1) * 0.5, (y0 + y1) * 0.5);
        let (rx, ry) = ((x1 - x0).abs() * 0.5, (y1 - y0).abs() * 0.5);
        if rx < 1.0 || ry < 1.0 {
            return;
        }
        let n = (((rx + ry) * std::f32::consts::PI * 0.5).round() as usize).clamp(24, 720);
        let mut last = None;
        let mut left = 0.0;
        for i in 0..=n {
            let t = i as f32 / n as f32 * std::f32::consts::TAU;
            let p = (cx + rx * t.cos(), cy + ry * t.sin());
            let r = self.stroke_to(brush, color, last, p, left);
            last = r.0;
            left = r.1;
        }
    }

    /// Solid fill of a rectangle. Caller checkpoints first.
    pub fn fill_rect(&mut self, brush: &Brush, color: Color, x0: u32, y0: u32, x1: u32, y1: u32) {
        let (x0, x1) = (x0.min(x1).min(self.w.saturating_sub(1)), x0.max(x1).min(self.w.saturating_sub(1)));
        let (y0, y1) = (y0.min(y1).min(self.h.saturating_sub(1)), y0.max(y1).min(self.h.saturating_sub(1)));
        for y in y0..=y1 {
            for x in x0..=x1 {
                self.put(brush, color, x, y);
            }
        }
    }

    /// Solid fill of an ellipse inside the box. Caller checkpoints first.
    pub fn fill_ellipse(&mut self, brush: &Brush, color: Color, x0: f32, y0: f32, x1: f32, y1: f32) {
        let (cx, cy) = ((x0 + x1) * 0.5, (y0 + y1) * 0.5);
        let (rx, ry) = ((x1 - x0).abs() * 0.5, (y1 - y0).abs() * 0.5);
        if rx < 0.5 || ry < 0.5 {
            return;
        }
        let (ix0, iy0) = (x0.min(x1).floor().max(0.0) as u32, y0.min(y1).floor().max(0.0) as u32);
        let (ix1, iy1) = (x0.max(x1).ceil() as u32, y0.max(y1).ceil() as u32);
        for y in iy0..=iy1.min(self.h.saturating_sub(1)) {
            for x in ix0..=ix1.min(self.w.saturating_sub(1)) {
                let dx = (x as f32 + 0.5 - cx) / rx;
                let dy = (y as f32 + 0.5 - cy) / ry;
                if dx * dx + dy * dy <= 1.0 {
                    self.put(brush, color, x, y);
                }
            }
        }
    }

    /// Flood-fill the contiguous region containing `(x, y)` on the active
    /// layer with `color` at brush opacity. A pixel belongs to the region
    /// when every channel is within `tolerance` of the seed pixel. Honors
    /// the active selection and mirrors across symmetry axes.
    /// Caller checkpoints first.
    pub fn flood_fill(&mut self, brush: &Brush, color: Color, x: u32, y: u32, tolerance: u8) {
        if self.active >= self.layers.len() || !self.pixel_selected(x, y) {
            return;
        }
        let (w, h) = (self.w, self.h);
        let snap = self.layers[self.active].pixels.clone();
        let seed_off = ((y * w + x) * 4) as usize;
        if seed_off + 3 >= snap.len() {
            return;
        }
        let target = [snap[seed_off], snap[seed_off + 1], snap[seed_off + 2], snap[seed_off + 3]];
        let opacity = brush.opacity.clamp(0.0, 1.0);
        if blend_over(target, color, opacity) == target {
            return;
        }
        let matches = |px: &[u8]| {
            px[0].abs_diff(target[0]) <= tolerance
                && px[1].abs_diff(target[1]) <= tolerance
                && px[2].abs_diff(target[2]) <= tolerance
                && px[3].abs_diff(target[3]) <= tolerance
        };
        // Mirror the seed across symmetry axes (deduped).
        let mut seeds = Vec::with_capacity(4);
        for (sx, sy) in self.symmetry.mirrored_pixels(w, h, self.symmetry_segments, x, y) {
            if sx < w && sy < h && self.pixel_selected(sx, sy) && !seeds.contains(&(sx, sy)) {
                seeds.push((sx, sy));
            }
        }
        // Selection consulted per pixel without borrowing the layer.
        let sel = self.selection;
        let mask = self.selection_mask.clone();
        let selected = |px: u32, py: u32| {
            if px >= w || py >= h {
                return false;
            }
            if let Some((a, b, c, d)) = sel {
                if px < a || px > c || py < b || py > d {
                    return false;
                }
            }
            if let Some(m) = mask.as_deref() {
                if m[(py * w + px) as usize] == 0 {
                    return false;
                }
            }
            true
        };
        let layer = &mut self.layers[self.active];
        let mut visited = vec![0u8; (w * h) as usize];
        let mut stack = seeds;
        let mut any = false;
        while let Some((cx, cy)) = stack.pop() {
            let idx = (cy * w + cx) as usize;
            if visited[idx] == 1 {
                continue;
            }
            visited[idx] = 1;
            let o = idx * 4;
            if o + 3 >= layer.pixels.len() || !matches(&layer.pixels[o..o + 4]) {
                continue;
            }
            let dst = [layer.pixels[o], layer.pixels[o + 1], layer.pixels[o + 2], layer.pixels[o + 3]];
            layer.pixels[o..o + 4].copy_from_slice(&blend_over(dst, color, opacity));
            any = true;
            if cx > 0 && selected(cx - 1, cy) {
                stack.push((cx - 1, cy));
            }
            if cx + 1 < w && selected(cx + 1, cy) {
                stack.push((cx + 1, cy));
            }
            if cy > 0 && selected(cx, cy - 1) {
                stack.push((cx, cy - 1));
            }
            if cy + 1 < h && selected(cx, cy + 1) {
                stack.push((cx, cy + 1));
            }
        }
        if any {
            self.mark_full_dirty();
        }
    }
}

/// Source-over blend of `color` at `opacity` onto `dst`. Matches the math in
/// [`Document::dab`] so fills look like brush paint.
fn blend_over(dst: [u8; 4], color: Color, opacity: f32) -> [u8; 4] {
    let src_a = opacity.clamp(0.0, 1.0);
    let dst_a = dst[3] as f32 / 255.0;
    let out_a = src_a + dst_a * (1.0 - src_a);
    if out_a <= 0.0 {
        return [0, 0, 0, 0];
    }
    let c = color.0;
    [
        ((c[0] as f32 * src_a + dst[0] as f32 * dst_a * (1.0 - src_a)) / out_a).round() as u8,
        ((c[1] as f32 * src_a + dst[1] as f32 * dst_a * (1.0 - src_a)) / out_a).round() as u8,
        ((c[2] as f32 * src_a + dst[2] as f32 * dst_a * (1.0 - src_a)) / out_a).round() as u8,
        (out_a * 255.0).round() as u8,
    ]
}
pub struct Stroke<'a> {
    doc: &'a mut Document,
    brush: Brush,
    color: Color,
    last: Option<(f32, f32)>,
    leftover: f32,
}

impl<'a> Stroke<'a> {
    /// Checkpoint the active layer for undo, then start painting.
    pub fn begin(doc: &'a mut Document, brush: Brush, color: Color) -> Self {
        doc.checkpoint();
        Self { doc, brush, color, last: None, leftover: 0.0 }
    }

    /// Feed a pointer position (canvas pixels).
    pub fn push(&mut self, x: f32, y: f32) {
        let (last, leftover) = self
            .doc
            .stroke_to(&self.brush, self.color, self.last, (x, y), self.leftover);
        self.last = last;
        self.leftover = leftover;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flood_fill_stays_inside_barrier() {
        let mut doc = Document::new(10, 10);
        let brush = Brush { opacity: 1.0, ..Default::default() };
        // Opaque red wall splitting the canvas.
        doc.fill_rect(&brush, Color::rgb(255, 0, 0), 5, 0, 5, 9);
        doc.flood_fill(&brush, Color::rgb(0, 0, 255), 1, 5, 0);
        let at = |d: &Document, x: u32, y: u32| {
            let o = ((y * 10 + x) * 4) as usize;
            [d.layers[0].pixels[o], d.layers[0].pixels[o + 1], d.layers[0].pixels[o + 2], d.layers[0].pixels[o + 3]]
        };
        assert_eq!(at(&doc, 0, 5), [0, 0, 255, 255]);
        assert_eq!(at(&doc, 5, 5), [255, 0, 0, 255]);
        assert_eq!(at(&doc, 9, 5)[3], 0);
    }

    #[test]
    fn flood_fill_mirrors_with_symmetry() {
        use crate::document::SymmetryMode;
        let mut doc = Document::new(11, 11);
        doc.symmetry = SymmetryMode::Horizontal;
        let brush = Brush { opacity: 1.0, ..Default::default() };
        doc.fill_rect(&brush, Color::rgb(255, 0, 0), 5, 0, 5, 10);
        doc.flood_fill(&brush, Color::rgb(0, 255, 0), 1, 5, 0);
        let at = |d: &Document, x: u32, y: u32| {
            let o = ((y * 11 + x) * 4) as usize;
            [d.layers[0].pixels[o], d.layers[0].pixels[o + 1], d.layers[0].pixels[o + 2], d.layers[0].pixels[o + 3]]
        };
        assert_eq!(at(&doc, 1, 5), [0, 255, 0, 255]);
        assert_eq!(at(&doc, 9, 5), [0, 255, 0, 255]);
        assert_eq!(at(&doc, 5, 5), [255, 0, 0, 255]);
    }

    #[test]
    fn symmetry_mirrors_dabs() {
        use crate::document::SymmetryMode;
        // Off: one-sided.
        let mut doc = Document::new(21, 21);
        let brush = Brush { size: 3.0, softness: 0.0, opacity: 1.0, ..Default::default() };
        doc.dab(&brush, Color::rgb(255, 0, 0), 2.5, 10.5);
        let at = |d: &Document, x: u32, y: u32| d.layers[0].pixels[((y * 21 + x) * 4) as usize + 3];
        assert!(at(&doc, 2, 10) > 0);
        assert_eq!(at(&doc, 18, 10), 0);
        // Horizontal: mirrored left-right.
        let mut doc = Document::new(21, 21);
        doc.symmetry = SymmetryMode::Horizontal;
        doc.dab(&brush, Color::rgb(255, 0, 0), 2.5, 10.5);
        assert!(at(&doc, 2, 10) > 0);
        assert!(at(&doc, 18, 10) > 0);
        // Both: four quadrants.
        let mut doc = Document::new(21, 21);
        doc.symmetry = SymmetryMode::Both;
        doc.dab(&brush, Color::rgb(0, 255, 0), 2.5, 3.5);
        assert!(at(&doc, 2, 3) > 0);
        assert!(at(&doc, 18, 3) > 0);
        assert!(at(&doc, 2, 17) > 0);
        assert!(at(&doc, 18, 17) > 0);
    }

    #[test]
    fn shapes_paint_pixels() {
        let mut doc = Document::new(32, 32);
        let brush = Brush { size: 4.0, ..Default::default() };
        doc.shape_rect(&brush, Color::rgb(255, 0, 0), 4.0, 4.0, 20.0, 20.0);
        assert!(doc.layers[0].pixels.iter().any(|&b| b != 0));
        let mut doc2 = Document::new(32, 32);
        doc2.shape_ellipse(&brush, Color::rgb(0, 255, 0), 4.0, 4.0, 28.0, 28.0);
        assert!(doc2.layers[0].pixels.iter().any(|&b| b != 0));
        let mut doc3 = Document::new(32, 32);
        doc3.fill_rect(&brush, Color::rgb(0, 0, 255), 2, 2, 6, 6);
        let o = ((4 * 32 + 4) * 4) as usize;
        assert_eq!(doc3.layers[0].pixels[o + 3], 255);
        let mut doc4 = Document::new(32, 32);
        doc4.fill_ellipse(&brush, Color::rgb(1, 2, 3), 2.0, 2.0, 10.0, 10.0);
        assert!(doc4.layers[0].pixels.iter().any(|&b| b != 0));
    }

    #[test]
    fn stroke_paints_and_eraser_clears() {
        let mut doc = Document::new(32, 32);
        let brush = Brush { size: 8.0, ..Default::default() };
        let mut s = Stroke::begin(&mut doc, brush, Color::rgb(255, 0, 0));
        s.push(16.0, 16.0);
        drop(s);
        assert!(doc.layers[0].pixels.iter().any(|&b| b != 0));
        let mut e = Stroke::begin(
            &mut doc,
            Brush { eraser: true, size: 32.0, softness: 0.0, ..Default::default() },
            Color::rgb(0, 0, 0),
        );
        e.push(16.0, 16.0);
        drop(e);
        // Eraser removes alpha (destination-out); color bytes of fully
        // transparent pixels are intentionally left alone.
        assert!(doc.layers[0].pixels.iter().skip(3).step_by(4).all(|&b| b == 0));
    }
}
