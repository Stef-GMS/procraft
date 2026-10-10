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
    /// Stamp one dab centered at canvas pixel `(x, y)` on the active layer.
    pub fn dab(&mut self, brush: &Brush, color: Color, x: f32, y: f32) {
        let Some(layer) = self.active_layer_mut() else { return };
        let (w, h) = (self.w as f32, self.h as f32);
        let radius = (brush.size * 0.5).max(0.5);
        let strength = brush.opacity.clamp(0.0, 1.0);
        if strength <= 0.0 {
            return;
        }
        let x0 = (x - radius).floor().max(0.0) as u32;
        let y0 = (y - radius).floor().max(0.0) as u32;
        let x1 = (x + radius).ceil().min(w - 1.0).max(0.0) as u32;
        let y1 = (y + radius).ceil().min(h - 1.0).max(0.0) as u32;
        let w = self.w;
        for py in y0..=y1 {
            for px in x0..=x1 {
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

/// One in-progress stroke (convenience wrapper; the GUI drives
/// [`Document::stroke_to`] directly so paint state survives across frames).
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
    fn stroke_paints_and_eraser_clears() {
        let mut doc = Document::new(32, 32);
        let brush = Brush { size: 8.0, ..Default::default() };
        let mut s = Stroke::begin(&mut doc, brush, Color::rgb(255, 0, 0));
        s.push(16.0, 16.0);
        drop(s);
        assert!(doc.layers[0].pixels.iter().any(|&b| b != 0));
        let mut e = Stroke::begin(
            &mut doc,
            Brush { eraser: true, size: 32.0, ..Default::default() },
            Color::rgb(0, 0, 0),
        );
        e.push(16.0, 16.0);
        drop(e);
        assert!(doc.layers[0].pixels.iter().all(|&b| b == 0));
    }
}
