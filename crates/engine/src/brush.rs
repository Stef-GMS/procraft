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

/// Grayscale brush tip (stamp) from a third-party brush file: alpha mask,
/// row-major, white (255) = full ink. Rendered at `Brush::size` wide.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BrushTip {
    pub w: u32,
    pub h: u32,
    pub mask: Vec<u8>,
}

/// Maximum tip dimension in pixels (larger tips are downscaled on import).
pub const MAX_TIP_SIDE: u32 = 256;

impl BrushTip {
    /// Build a tip from a raw mask, validating dimensions and length.
    pub fn from_mask(w: u32, h: u32, mask: Vec<u8>) -> Result<Self, String> {
        if w == 0 || h == 0 || w > MAX_TIP_SIDE || h > MAX_TIP_SIDE {
            return Err("brush tip has bad dimensions".to_string());
        }
        if mask.len() != (w * h) as usize {
            return Err("brush tip has corrupt pixels".to_string());
        }
        if mask.iter().all(|&b| b == 0) {
            return Err("brush tip is fully transparent".to_string());
        }
        Ok(Self { w, h, mask })
    }
}

/// Polygonal shape families for the shape tools (outline + fill share
/// the same vertices).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShapePoly {
    Triangle,
    Diamond,
    Star,
    Polygon,
}

/// Brush shape settings. All original math: a radial falloff stamp laid down
/// along the pointer path at `spacing` intervals. When `tip` is present the
/// parametric falloff is replaced by the grayscale stamp (imported brushes).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Brush {
    /// Dab diameter in canvas pixels (stamp width when `tip` is present).
    pub size: f32,
    /// Per-dab alpha multiplier, 0.0..=1.0.
    pub opacity: f32,
    /// Edge softness, 0.0 (hard disc) ..= 1.0 (fully soft). Unused by stamps.
    pub softness: f32,
    /// Dab spacing as a fraction of `size` (0.1 = smooth, 1.0 = dotted).
    pub spacing: f32,
    /// Eraser mode: remove alpha instead of painting color.
    pub eraser: bool,
    /// Optional imported stamp tip (absent in `.pcbrush` files from v9).
    #[serde(default)]
    pub tip: Option<BrushTip>,
}

impl Default for Brush {
    fn default() -> Self {
        Self { size: 24.0, opacity: 1.0, softness: 0.6, spacing: 0.15, eraser: false, tip: None }
    }
}

impl Document {
    /// Stamp one dab centered at canvas pixel `(x, y)` on the active layer,
    /// mirrored per [`Document::symmetry`].
    /// Pixels outside [`Document::selection`] (when set) are skipped.
    /// With [`PaintTarget::Mask`] the brush paints the layer's reveal mask
    /// instead (auto-created white on first stroke; color luminance is the
    /// mask value, the eraser reveals). Shapes, fills and flood fill always
    /// paint color.
    pub fn dab(&mut self, brush: &Brush, color: Color, x: f32, y: f32) {
        let pts = self.symmetry.mirrored_points(self.w, self.h, self.symmetry_segments, x, y);
        let mut done: Vec<(f32, f32)> = Vec::with_capacity(4);
        let target = self.paint_target;
        for (mx, my) in pts {
            if done.iter().any(|&(dx, dy)| (dx - mx).abs() < 0.001 && (dy - my).abs() < 0.001) {
                continue;
            }
            done.push((mx, my));
            match target {
                super::document::PaintTarget::Color => self.dab_one(brush, color, mx, my),
                super::document::PaintTarget::Mask => self.dab_mask(brush, color, mx, my),
            }
        }
    }

    /// Mask dab through the parametric falloff or the stamp tip; shared
    /// selection/symmetry gating comes from [`Document::dab`].
    fn dab_mask(&mut self, brush: &Brush, color: Color, x: f32, y: f32) {
        if brush.tip.is_some() {
            return self.dab_mask_stamp(brush, color, x, y);
        }
        let (w_u32, h_u32) = (self.w, self.h);
        let radius = (brush.size * 0.5).max(0.5);
        let strength = brush.opacity.clamp(0.0, 1.0);
        if strength <= 0.0 || self.layers.get(self.active).is_none() {
            return;
        }
        // Fully outside: a complete no-op (no mask, no dirty rect).
        if x + radius < 0.0 || y + radius < 0.0 || x - radius >= w_u32 as f32 || y - radius >= h_u32 as f32 {
            return;
        }
        // First mask stroke materializes a white mask (covered by the
        // pre-stroke checkpoint, so undo removes it again).
        if self.layers[self.active].mask.is_none() {
            self.add_mask(self.active);
        }
        let x0 = (x - radius).floor().max(0.0) as u32;
        let y0 = (y - radius).floor().max(0.0) as u32;
        let x1 = (x + radius).ceil().min(w_u32 as f32 - 1.0).max(0.0) as u32;
        let y1 = (y + radius).ceil().min(h_u32 as f32 - 1.0).max(0.0) as u32;
        self.mark_dirty(x0, y0, x1, y1);
        let selection = self.selection;
        let active = self.active;
        if active >= self.layers.len() {
            return;
        }
        let sel_mask = self.selection_mask.as_deref();
        let w = w_u32;
        let value = if brush.eraser {
            255.0
        } else {
            let c = color.0;
            (299.0 * c[0] as f32 + 587.0 * c[1] as f32 + 114.0 * c[2] as f32) / 1000.0
        };
        let edge = brush.softness.clamp(0.0, 1.0);
        let Some(layer_mask) = self.layers[active].mask.as_mut() else { return };
        for py in y0..=y1 {
            for px in x0..=x1 {
                if let Some((sx0, sy0, sx1, sy1)) = selection {
                    if px < sx0 || px > sx1 || py < sy0 || py > sy1 {
                        continue;
                    }
                }
                if let Some(m) = sel_mask {
                    if m.get((py * w + px) as usize).copied().unwrap_or(0) == 0 {
                        continue;
                    }
                }
                let d = ((px as f32 + 0.5 - x).hypot(py as f32 + 0.5 - y)) / radius;
                if d > 1.0 {
                    continue;
                }
                let falloff = if d <= 1.0 - edge { 1.0 } else { 1.0 - (d - (1.0 - edge)) / edge.max(0.001) };
                let a = (falloff.clamp(0.0, 1.0) * strength).clamp(0.0, 1.0);
                if a <= 0.0 {
                    continue;
                }
                let o = (py * w + px) as usize;
                if o >= layer_mask.len() {
                    continue;
                }
                layer_mask[o] = (value * a + layer_mask[o] as f32 * (1.0 - a)).round() as u8;
            }
        }
    }

    /// Mask dab through an imported stamp tip (nearest-neighbor at brush
    /// size, like [`Document::dab_stamp`] but writing luminance).
    fn dab_mask_stamp(&mut self, brush: &Brush, color: Color, x: f32, y: f32) {
        let Some(tip) = brush.tip.as_ref() else { return };
        if tip.w == 0 || tip.h == 0 || tip.mask.len() != (tip.w * tip.h) as usize {
            return;
        }
        let (w_u32, h_u32) = (self.w, self.h);
        let strength = brush.opacity.clamp(0.0, 1.0);
        if strength <= 0.0 || brush.size <= 0.0 || self.layers.get(self.active).is_none() {
            return;
        }
        let scale = brush.size / tip.w as f32;
        let th = (tip.h as f32 * scale).max(1.0);
        // Fully outside: a complete no-op (no mask, no dirty rect).
        if x + brush.size * 0.5 < 0.0
            || y + th * 0.5 < 0.0
            || x - brush.size * 0.5 >= w_u32 as f32
            || y - th * 0.5 >= h_u32 as f32
        {
            return;
        }
        if self.layers[self.active].mask.is_none() {
            self.add_mask(self.active);
        }
        let x0 = (x - brush.size * 0.5).floor().max(0.0) as u32;
        let y0 = (y - th * 0.5).floor().max(0.0) as u32;
        let x1 = (x + brush.size * 0.5).ceil().min(w_u32 as f32 - 1.0).max(0.0) as u32;
        let y1 = (y + th * 0.5).ceil().min(h_u32 as f32 - 1.0).max(0.0) as u32;
        self.mark_dirty(x0, y0, x1, y1);
        let selection = self.selection;
        let active = self.active;
        if active >= self.layers.len() {
            return;
        }
        let sel_mask = self.selection_mask.as_deref();
        let w = w_u32;
        let value = if brush.eraser {
            255.0
        } else {
            let c = color.0;
            (299.0 * c[0] as f32 + 587.0 * c[1] as f32 + 114.0 * c[2] as f32) / 1000.0
        };
        let Some(layer_mask) = self.layers[active].mask.as_mut() else { return };
        for py in y0..=y1 {
            for px in x0..=x1 {
                if let Some((sx0, sy0, sx1, sy1)) = selection {
                    if px < sx0 || px > sx1 || py < sy0 || py > sy1 {
                        continue;
                    }
                }
                if let Some(m) = sel_mask {
                    if m.get((py * w + px) as usize).copied().unwrap_or(0) == 0 {
                        continue;
                    }
                }
                let tx = (((px as f32 + 0.5 - (x - brush.size * 0.5)) / scale) as u32).min(tip.w - 1);
                let ty = (((py as f32 + 0.5 - (y - th * 0.5)) / scale) as u32).min(tip.h - 1);
                let a = (tip.mask[(ty * tip.w + tx) as usize] as f32 / 255.0 * strength).clamp(0.0, 1.0);
                if a <= 0.0 {
                    continue;
                }
                let o = (py * w + px) as usize;
                if o >= layer_mask.len() {
                    continue;
                }
                layer_mask[o] = (value * a + layer_mask[o] as f32 * (1.0 - a)).round() as u8;
            }
        }
    }

    fn dab_one(&mut self, brush: &Brush, color: Color, x: f32, y: f32) {
        if brush.tip.is_some() {
            return self.dab_stamp(brush, color, x, y);
        }
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

    /// Stamp a grayscale-tip dab centered at `(x, y)`: the tip is scaled so
    /// its width spans `brush.size` canvas pixels (nearest-neighbor), and
    /// each texel contributes `mask * opacity` exactly like falloff alpha.
    /// Selection, symmetry and eraser handling match [`Document::dab`].
    fn dab_stamp(&mut self, brush: &Brush, color: Color, x: f32, y: f32) {
        let Some(tip) = brush.tip.as_ref() else { return };
        if tip.w == 0 || tip.h == 0 || tip.mask.len() != (tip.w * tip.h) as usize {
            return;
        }
        let (w_u32, h_u32) = (self.w, self.h);
        let (w, h) = (w_u32 as f32, h_u32 as f32);
        let strength = brush.opacity.clamp(0.0, 1.0);
        if strength <= 0.0 || brush.size <= 0.0 || self.layers.get(self.active).is_none() {
            return;
        }
        let scale = brush.size / tip.w as f32;
        let th = (tip.h as f32 * scale).max(1.0);
        let x0 = (x - brush.size * 0.5).floor().max(0.0) as u32;
        let y0 = (y - th * 0.5).floor().max(0.0) as u32;
        let x1 = (x + brush.size * 0.5).ceil().min(w - 1.0).max(0.0) as u32;
        let y1 = (y + th * 0.5).ceil().min(h - 1.0).max(0.0) as u32;
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
                let tx = (((px as f32 + 0.5 - (x - brush.size * 0.5)) / scale) as u32).min(tip.w - 1);
                let ty = (((py as f32 + 0.5 - (y - th * 0.5)) / scale) as u32).min(tip.h - 1);
                let a = (tip.mask[(ty * tip.w + tx) as usize] as f32 / 255.0 * strength * 255.0).round() as u8;
                if a == 0 {
                    continue;
                }
                let o = ((py * w + px) * 4) as usize;
                if o + 3 >= layer.pixels.len() {
                    continue;
                }
                if brush.eraser {
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

/// A named brush for a `.pcbrush` library file: display name plus settings.
/// Library brushes never erase; the Eraser tool covers that.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BrushPreset {
    pub name: String,
    pub brush: Brush,
}

/// Maximum brushes in one library file (keeps imports cheap and bounded).
pub const MAX_LIBRARY_BRUSHES: usize = 64;

/// Shared validation for one imported brush. Names are trimmed; numeric
/// ranges match what the desktop sliders can produce and more.
fn check_preset(name: &str, brush: &Brush) -> Result<String, String> {
    let clean = name.trim().to_string();
    if clean.is_empty() {
        return Err("brush has an empty name".to_string());
    }
    if clean.chars().count() > 64 {
        return Err(format!("brush name too long: {clean}"));
    }
    if !(0.0 < brush.size && brush.size <= 400.0) {
        return Err(format!("brush '{clean}' has a bad size"));
    }
    if !(0.0 <= brush.opacity && brush.opacity <= 1.0) {
        return Err(format!("brush '{clean}' has a bad opacity"));
    }
    if !(0.0 <= brush.softness && brush.softness <= 1.0) {
        return Err(format!("brush '{clean}' has a bad softness"));
    }
    if !(0.0 < brush.spacing && brush.spacing <= 2.0) {
        return Err(format!("brush '{clean}' has a bad spacing"));
    }
    if brush.eraser {
        return Err(format!("brush '{clean}' must not be an eraser"));
    }
    Ok(clean)
}

/// Save brush presets to a `.pcbrush` file (pretty JSON array of
/// `{name, brush}` records). Eraser flags are forced off on export.
pub fn save_brush_library(path: &std::path::Path, presets: &[BrushPreset]) -> Result<(), String> {
    if presets.is_empty() {
        return Err("nothing to export: the brush library is empty".to_string());
    }
    if presets.len() > MAX_LIBRARY_BRUSHES {
        return Err(format!("too many brushes (max {MAX_LIBRARY_BRUSHES})"));
    }
    let mut clean = Vec::with_capacity(presets.len());
    for p in presets {
        let forced = Brush { eraser: false, ..p.brush.clone() };
        let name = check_preset(&p.name, &forced)?;
        clean.push(BrushPreset { name, brush: forced });
    }
    let text = serde_json::to_string_pretty(&clean).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| e.to_string())
}

/// Load a `.pcbrush` file, validating every record. Malformed JSON,
/// out-of-range numbers and eraser brushes are all rejected.
pub fn open_brush_library(path: &std::path::Path) -> Result<Vec<BrushPreset>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let raw: Vec<BrushPreset> = serde_json::from_str(&text).map_err(|_| "not a brush library file".to_string())?;
    if raw.is_empty() {
        return Err("brush library is empty".to_string());
    }
    if raw.len() > MAX_LIBRARY_BRUSHES {
        return Err(format!("too many brushes (max {MAX_LIBRARY_BRUSHES})"));
    }
    let mut clean = Vec::with_capacity(raw.len());
    for p in &raw {
        let name = check_preset(&p.name, &p.brush)?;
        clean.push(BrushPreset { name, brush: p.brush.clone() });
    }
    Ok(clean)
}

/// Built-in brush presets: name plus settings. Original set, roughly grouped
/// from hard sketching to soft painting.
pub fn presets() -> Vec<(&'static str, Brush)> {
    vec![
        ("Pencil", Brush { size: 3.0, opacity: 1.0, softness: 0.15, spacing: 0.1, eraser: false, tip: None }),
        ("Ink Pen", Brush { size: 6.0, opacity: 1.0, softness: 0.3, spacing: 0.1, eraser: false, tip: None }),
        ("Round", Brush { size: 24.0, opacity: 1.0, softness: 0.6, spacing: 0.15, eraser: false, tip: None }),
        ("Marker", Brush { size: 28.0, opacity: 0.85, softness: 0.5, spacing: 0.12, eraser: false, tip: None }),
        ("Airbrush", Brush { size: 64.0, opacity: 0.25, softness: 1.0, spacing: 0.08, eraser: false, tip: None }),
        ("Chalk", Brush { size: 18.0, opacity: 0.8, softness: 0.9, spacing: 0.25, eraser: false, tip: None }),
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

    /// Closed vertex loop (last == first) for a polygonal shape in the box.
    /// Triangle/diamond/star are fixed; `polygon` is the regular N-gon
    /// (`sides` clamped 3..=12, first vertex on top).
    fn poly_vertices(kind: ShapePoly, x0: f32, y0: f32, x1: f32, y1: f32, sides: u8) -> Vec<(f32, f32)> {
        let (lo_x, hi_x) = (x0.min(x1), x0.max(x1));
        let (lo_y, hi_y) = (y0.min(y1), y0.max(y1));
        let (cx, cy) = ((lo_x + hi_x) * 0.5, (lo_y + hi_y) * 0.5);
        let close = |mut v: Vec<(f32, f32)>| {
            if let Some(&first) = v.first() {
                v.push(first);
            }
            v
        };
        match kind {
            ShapePoly::Triangle => close(vec![(cx, lo_y), (hi_x, hi_y), (lo_x, hi_y)]),
            ShapePoly::Diamond => close(vec![(cx, lo_y), (hi_x, cy), (cx, hi_y), (lo_x, cy)]),
            ShapePoly::Star => {
                let r = ((hi_x - lo_x).min(hi_y - lo_y)) * 0.5;
                let mut v = Vec::with_capacity(11);
                for i in 0..10 {
                    let rr = if i % 2 == 0 { r } else { r * 0.382 };
                    let t = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
                    v.push((cx + rr * t.cos(), cy + rr * t.sin()));
                }
                close(v)
            }
            ShapePoly::Polygon => {
                let n = sides.clamp(3, 12) as usize;
                let (rx, ry) = ((hi_x - lo_x) * 0.5, (hi_y - lo_y) * 0.5);
                let mut v = Vec::with_capacity(n + 1);
                for i in 0..n {
                    let t = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::TAU / n as f32;
                    v.push((cx + rx * t.cos(), cy + ry * t.sin()));
                }
                close(v)
            }
        }
    }

    /// Open stroke path for an arrow along the drag direction: shaft plus
    /// two head wings (backtracked, like a hand stroke). Caller checkpoints.
    fn arrow_path(x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<(f32, f32)> {
        let (dx, dy) = (x1 - x0, y1 - y0);
        let len = (dx * dx + dy * dy).sqrt();
        if len < 2.0 {
            return vec![(x0, y0), (x1, y1)];
        }
        let (ux, uy) = (dx / len, dy / len);
        let (tip_x, tip_y) = (x1, y1);
        let head_len = (len * 0.28).min(48.0).max(6.0);
        let head_w = (len * 0.16).min(32.0).max(4.0);
        // Perpendicular for the wing spread.
        let (px, py) = (-uy, ux);
        let b = (tip_x - ux * head_len, tip_y - uy * head_len);
        vec![
            (x0, y0),
            (tip_x, tip_y),
            (b.0 + px * head_w, b.1 + py * head_w),
            (tip_x, tip_y),
            (b.0 - px * head_w, b.1 - py * head_w),
        ]
    }

    /// Outline of a polygonal shape through dab segments. Caller checkpoints.
    pub fn shape_poly(
        &mut self,
        brush: &Brush,
        color: Color,
        kind: ShapePoly,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        sides: u8,
    ) {
        let pts = Self::poly_vertices(kind, x0, y0, x1, y1, sides);
        let mut last = None;
        let mut left = 0.0;
        for &p in &pts {
            let r = self.stroke_to(brush, color, last, p, left);
            last = r.0;
            left = r.1;
        }
    }

    /// Arrow outline (shaft + head). Caller checkpoints first.
    pub fn shape_arrow(&mut self, brush: &Brush, color: Color, x0: f32, y0: f32, x1: f32, y1: f32) {
        let pts = Self::arrow_path(x0, y0, x1, y1);
        let mut last = None;
        let mut left = 0.0;
        for &p in &pts {
            let r = self.stroke_to(brush, color, last, p, left);
            last = r.0;
            left = r.1;
        }
    }

    /// Solid even-odd fill of a polygonal shape. Caller checkpoints first.
    pub fn fill_poly(
        &mut self,
        brush: &Brush,
        color: Color,
        kind: ShapePoly,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        sides: u8,
    ) {
        let pts = Self::poly_vertices(kind, x0, y0, x1, y1, sides);
        if pts.len() < 4 {
            return;
        }
        let (ix0, iy0) = (x0.min(x1).floor().max(0.0) as u32, y0.min(y1).floor().max(0.0) as u32);
        let (ix1, iy1) = (x0.max(x1).ceil() as u32, y0.max(y1).ceil() as u32);
        for y in iy0..=iy1.min(self.h.saturating_sub(1)) {
            for x in ix0..=ix1.min(self.w.saturating_sub(1)) {
                // Even-odd ray cast from the pixel center.
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                let mut inside = false;
                let mut j = pts.len() - 1;
                for i in 0..pts.len() {
                    let (xi, yi) = pts[i];
                    let (xj, yj) = pts[j];
                    if (yi > py) != (yj > py) {
                        let at = xi + (py - yi) / (yj - yi) * (xj - xi);
                        if px < at {
                            inside = !inside;
                        }
                    }
                    j = i;
                }
                if inside {
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
    fn poly_shapes_outline_and_fill() {
        use ShapePoly::*;
        let brush = Brush { size: 3.0, ..Default::default() };
        for kind in [Triangle, Diamond, Star, Polygon] {
            // Outline paints.
            let mut doc = Document::new(48, 48);
            doc.shape_poly(&brush, Color::rgb(255, 0, 0), kind, 6.0, 6.0, 42.0, 42.0, 6);
            assert!(doc.layers[0].pixels.iter().any(|&b| b != 0), "{kind:?} outline");
            // Fill covers the middle but not the far corner.
            let mut doc = Document::new(48, 48);
            doc.fill_poly(&brush, Color::rgb(0, 255, 0), kind, 6.0, 6.0, 42.0, 42.0, 6);
            let mid = ((24 * 48 + 24) * 4) as usize;
            assert_eq!(doc.layers[0].pixels[mid + 3], 255, "{kind:?} fill middle");
            let corner = ((2 * 48 + 2) * 4) as usize;
            assert_eq!(doc.layers[0].pixels[corner + 3], 0, "{kind:?} fill corner");
        }
        // Polygon sides clamp: 2 behaves like a triangle, 99 like 12.
        let mut doc = Document::new(48, 48);
        doc.fill_poly(&brush, Color::rgb(1, 1, 1), Polygon, 6.0, 6.0, 42.0, 42.0, 2);
        let mid = ((30 * 48 + 24) * 4) as usize;
        assert_eq!(doc.layers[0].pixels[mid + 3], 255);
        // Arrow paints along its drag direction.
        let mut doc = Document::new(48, 48);
        doc.shape_arrow(&brush, Color::rgb(0, 0, 255), 4.0, 24.0, 44.0, 24.0);
        assert!(doc.layers[0].pixels.iter().any(|&b| b != 0));
        let tip = ((24 * 48 + 43) * 4) as usize;
        assert!(doc.layers[0].pixels[tip + 3] > 0, "arrowhead lands at the tip");
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

    fn sample_library() -> Vec<BrushPreset> {
        presets()
            .into_iter()
            .map(|(name, brush)| BrushPreset { name: name.to_string(), brush })
            .collect()
    }

    #[test]
    fn brush_library_roundtrips_builtins() {
        let libs = sample_library();
        let dir = std::env::temp_dir();
        let path = dir.join("procraft-lib-roundtrip.pcbrush");
        save_brush_library(&path, &libs).unwrap();
        let back = open_brush_library(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(back, libs);
    }

    #[test]
    fn brush_library_export_forces_eraser_off() {
        let mut libs = sample_library();
        libs[0].brush.eraser = true;
        let dir = std::env::temp_dir();
        let path = dir.join("procraft-lib-eraser.pcbrush");
        save_brush_library(&path, &libs).unwrap();
        let back = open_brush_library(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert!(!back[0].brush.eraser);
    }

    #[test]
    fn brush_library_rejects_garbage() {
        let dir = std::env::temp_dir();
        // Empty export.
        let path = dir.join("procraft-lib-empty.pcbrush");
        assert!(save_brush_library(&path, &[]).is_err());
        // Not JSON at all.
        std::fs::write(&path, "this is not json").unwrap();
        assert!(open_brush_library(&path).is_err());
        // Empty JSON array.
        std::fs::write(&path, "[]").unwrap();
        assert!(open_brush_library(&path).is_err());
        // Empty name.
        let bad = vec![BrushPreset { name: "   ".to_string(), brush: Brush::default() }];
        std::fs::write(&path, serde_json::to_string(&bad).unwrap()).unwrap();
        assert!(open_brush_library(&path).is_err());
        // Out-of-range size.
        let bad = vec![BrushPreset {
            name: "Big".to_string(),
            brush: Brush { size: 9999.0, ..Brush::default() },
        }];
        std::fs::write(&path, serde_json::to_string(&bad).unwrap()).unwrap();
        assert!(open_brush_library(&path).is_err());
        // Eraser brush.
        let bad = vec![BrushPreset {
            name: "Eraser".to_string(),
            brush: Brush { eraser: true, ..Brush::default() },
        }];
        std::fs::write(&path, serde_json::to_string(&bad).unwrap()).unwrap();
        assert!(open_brush_library(&path).is_err());
        // Too many brushes.
        let many: Vec<BrushPreset> = (0..MAX_LIBRARY_BRUSHES + 1)
            .map(|i| BrushPreset { name: format!("b{i}"), brush: Brush::default() })
            .collect();
        assert!(save_brush_library(&path, &many).is_err());
        std::fs::write(&path, serde_json::to_string(&many).unwrap()).unwrap();
        assert!(open_brush_library(&path).is_err());
        let _ = std::fs::remove_file(&path);
    }
}
