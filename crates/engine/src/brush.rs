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

/// Per-dab dynamics sample for [`Document::stroke_to_dyn`]: pen pressure,
/// normalized pointer speed and stroke direction at one stroke endpoint.
/// The engine interpolates these per dab, so a single pointer segment can
/// taper, fade and turn across its dabs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DabDynamics {
    /// Pen pressure, 0.0..=1.0 (1.0 = full pressure / mouse).
    pub pressure: f32,
    /// Pointer speed normalized by 3500 px/s, 0.0..=1.0.
    pub speed_norm: f32,
    /// Stroke direction in radians (`atan2(dy, dx)`).
    pub angle: f32,
}

impl Default for DabDynamics {
    fn default() -> Self {
        Self { pressure: 1.0, speed_norm: 0.0, angle: 0.0 }
    }
}

fn clamp01(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

// serde defaults for the fields added in M14: old `.pcbrush` files without
// these keys load with the same values as `Brush::default()`.
fn def_pressure_size() -> f32 {
    0.7
}
fn def_pressure_opacity() -> f32 {
    0.6
}
fn def_velocity_size() -> f32 {
    0.6
}
fn def_velocity_opacity() -> f32 {
    0.6
}
fn def_grain() -> f32 {
    0.0
}
fn def_grain_scale() -> f32 {
    1.0
}
fn def_roundness() -> f32 {
    1.0
}
fn def_angle_follow() -> bool {
    false
}
fn def_scatter() -> f32 {
    0.0
}
fn def_stabilizer() -> f32 {
    0.35
}

/// Deterministic 0.0..=1.0 hash for scatter offsets and grain. Position
/// based (not counter based) so strokes replay identically across
/// undo/redo, thumbnails and file round-trips. Original integer-mix math.
fn hash01(x: i32, y: i32) -> f32 {
    let mut h = (x as u32)
        .wrapping_mul(374761393)
        .wrapping_add((y as u32).wrapping_mul(668265263));
    h ^= h >> 13;
    h = h.wrapping_mul(1274126177);
    h ^= h >> 16;
    (h >> 8) as f32 / 16_777_215.0
}

/// Procedural grain sample 0.0..=1.0: one value-noise texel per `cell`
/// pixel block (`cell` grows with `scale`). No assets, clean-room.
fn grain_at(px: u32, py: u32, scale: f32) -> f32 {
    let cell = ((3.0 * scale.clamp(0.25, 4.0)).round() as u32).max(1);
    hash01((px / cell) as i32, (py / cell) as i32)
}

/// Brush shape settings. All original math: a radial falloff stamp laid down
/// along the pointer path at `spacing` intervals. When `tip` is present the
/// parametric falloff is replaced by the grayscale stamp (imported brushes).
/// M14 adds per-dab dynamics: pressure and pointer speed scale size/opacity
/// through [`Brush::resolve_size`]/[`Brush::resolve_opacity`], `grain`
/// breaks the alpha with procedural noise, `roundness` + `angle_follow`
/// squeeze and steer dabs along the stroke, and `scatter` offsets dabs.
/// `stabilizer` is applied app-side (pointer smoothing) and stored here so
/// it travels with the brush preset.
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
    /// How much light pressure shrinks the dab, 0.0 (no-op) ..= 1.0 (full).
    /// Effective size scales as `size * (1 - pressure_size * (1 - p))`.
    #[serde(default = "def_pressure_size")]
    pub pressure_size: f32,
    /// How much light pressure fades the dab, 0.0..=1.0 (same curve).
    #[serde(default = "def_pressure_opacity")]
    pub pressure_opacity: f32,
    /// How much speed thins the dab, 0.0..=1.0 (weight 0.6, as M7 dynamics).
    #[serde(default = "def_velocity_size")]
    pub velocity_size: f32,
    /// How much speed fades the dab, 0.0..=1.0 (weight 0.4, as M7 dynamics).
    #[serde(default = "def_velocity_opacity")]
    pub velocity_opacity: f32,
    /// Procedural grain strength, 0.0 (off) ..= 1.0 (heavy chalk).
    #[serde(default = "def_grain")]
    pub grain: f32,
    /// Grain cell scale, 0.25 (fine) ..= 4.0 (coarse).
    #[serde(default = "def_grain_scale")]
    pub grain_scale: f32,
    /// Dab roundness, 0.1 (flat calligraphy) ..= 1.0 (circle).
    #[serde(default = "def_roundness")]
    pub roundness: f32,
    /// Steer the dab's long axis along the stroke direction.
    #[serde(default = "def_angle_follow")]
    pub angle_follow: bool,
    /// Random dab offset as a fraction of `size`, 0.0 (off) ..= 1.0.
    #[serde(default = "def_scatter")]
    pub scatter: f32,
    /// Pointer smoothing amount, 0.0 (raw) ..= 1.0 (heavy streamline).
    #[serde(default = "def_stabilizer")]
    pub stabilizer: f32,
}

impl Default for Brush {
    fn default() -> Self {
        Self {
            size: 24.0,
            opacity: 1.0,
            softness: 0.6,
            spacing: 0.15,
            eraser: false,
            tip: None,
            pressure_size: def_pressure_size(),
            pressure_opacity: def_pressure_opacity(),
            velocity_size: def_velocity_size(),
            velocity_opacity: def_velocity_opacity(),
            grain: def_grain(),
            grain_scale: def_grain_scale(),
            roundness: def_roundness(),
            angle_follow: def_angle_follow(),
            scatter: def_scatter(),
            stabilizer: def_stabilizer(),
        }
    }
}

impl Brush {
    /// Effective dab diameter for a pressure/speed sample. At full pressure
    /// and zero speed this returns exactly `size` (old behavior preserved).
    pub fn resolve_size(&self, pressure: f32, speed_norm: f32) -> f32 {
        let p = clamp01(self.pressure_size);
        let v = clamp01(self.velocity_size);
        let sized = self.size * (1.0 - p * (1.0 - clamp01(pressure)));
        (sized * (1.0 - 0.6 * clamp01(speed_norm) * v)).max(0.0)
    }

    /// Effective per-dab opacity for a pressure/speed sample. At full
    /// pressure and zero speed this returns exactly `opacity`.
    pub fn resolve_opacity(&self, pressure: f32, speed_norm: f32) -> f32 {
        let p = clamp01(self.pressure_opacity);
        let v = clamp01(self.velocity_opacity);
        let op = self.opacity * (1.0 - p * (1.0 - clamp01(pressure)));
        (op * (1.0 - 0.4 * clamp01(speed_norm) * v)).clamp(0.0, 1.0)
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
        // Single dabs (stroke starts, shape taps) paint at full pressure:
        // resolve(1, 0) returns exactly size/opacity, angle 0 keeps the
        // dab round unless the preset squeezes it.
        let size = brush.resolve_size(1.0, 0.0);
        let opacity = brush.resolve_opacity(1.0, 0.0);
        self.dab_dyn(brush, color, x, y, size, opacity, 0.0);
    }

    /// Stamp one dab with pre-resolved size/opacity/angle, mirrored per
    /// [`Document::symmetry`]. The [`Document::stroke_to_dyn`] walker
    /// resolves dynamics per dab and calls this; [`Document::dab`] is the
    /// full-pressure shorthand above.
    fn dab_dyn(
        &mut self,
        brush: &Brush,
        color: Color,
        x: f32,
        y: f32,
        size: f32,
        opacity: f32,
        angle: f32,
    ) {
        let (ccx, ccy) = self.symmetry_center_px();
        let pts = self.symmetry.mirrored_points(self.symmetry_segments, ccx, ccy, x, y);
        let mut done: Vec<(f32, f32)> = Vec::with_capacity(4);
        let target = self.paint_target;
        for (mx, my) in pts {
            if done.iter().any(|&(dx, dy)| (dx - mx).abs() < 0.001 && (dy - my).abs() < 0.001) {
                continue;
            }
            done.push((mx, my));
            match target {
                super::document::PaintTarget::Color => {
                    self.dab_one_resolved(brush, color, mx, my, size, opacity, angle)
                }
                super::document::PaintTarget::Mask => {
                    self.dab_mask_resolved(brush, color, mx, my, size, opacity, angle)
                }
            }
        }
    }

    /// Mask dab through the parametric falloff or the stamp tip; shared
    /// selection/symmetry gating comes from [`Document::dab`].
    fn dab_mask_resolved(
        &mut self,
        brush: &Brush,
        color: Color,
        x: f32,
        y: f32,
        size: f32,
        opacity: f32,
        angle: f32,
    ) {
        if brush.tip.is_some() {
            return self.dab_mask_stamp_resolved(brush, color, x, y, size, opacity, angle);
        }
        let (w_u32, h_u32) = (self.w, self.h);
        let radius = (size * 0.5).max(0.5);
        let strength = opacity.clamp(0.0, 1.0);
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
        // Layer offset (Move tool): canvas pixel -> buffer slot.
        let (lox, loy) = (self.layers[active].ox, self.layers[active].oy);
        let bh = h_u32 as i32;
        let round = brush.roundness.clamp(0.1, 1.0);
        let (cos_a, sin_a) = (angle.cos(), angle.sin());
        let grain = clamp01(brush.grain);
        let grain_scale = brush.grain_scale;
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
                // Dab-frame coords: u along the stroke, v across (squeezed
                // by roundness). At roundness 1.0 this is the old circle.
                let (dx, dy) = (px as f32 + 0.5 - x, py as f32 + 0.5 - y);
                let (u, v) = (dx * cos_a + dy * sin_a, (-dx * sin_a + dy * cos_a) / round);
                let d = (u * u + v * v).sqrt() / radius;
                if d > 1.0 {
                    continue;
                }
                let falloff = if d <= 1.0 - edge { 1.0 } else { 1.0 - (d - (1.0 - edge)) / edge.max(0.001) };
                let mut a = (falloff.clamp(0.0, 1.0)
                    * strength
                    * Document::sel_k(sel_mask, w, px, py))
                .clamp(0.0, 1.0);
                if grain > 0.0 {
                    a *= 1.0 - grain * (1.0 - grain_at(px, py, grain_scale));
                }
                if a <= 0.0 {
                    continue;
                }
                // Store at the layer-offset slot (paint lands shifted on
                // moved layers, exactly where the composite reads it).
                let (lx, ly) = (px as i32 - lox, py as i32 - loy);
                if lx < 0 || ly < 0 || lx >= w as i32 || ly >= bh {
                    continue;
                }
                let o = (ly as u32 * w + lx as u32) as usize;
                if o >= layer_mask.len() {
                    continue;
                }
                layer_mask[o] = (value * a + layer_mask[o] as f32 * (1.0 - a)).round() as u8;
            }
        }
    }

    /// Mask dab through an imported stamp tip (nearest-neighbor at brush
    /// size, like [`Document::dab_stamp`] but writing luminance).
    fn dab_mask_stamp_resolved(
        &mut self,
        brush: &Brush,
        color: Color,
        x: f32,
        y: f32,
        size: f32,
        opacity: f32,
        angle: f32,
    ) {
        let Some(tip) = brush.tip.as_ref() else { return };
        if tip.w == 0 || tip.h == 0 || tip.mask.len() != (tip.w * tip.h) as usize {
            return;
        }
        let (w_u32, h_u32) = (self.w, self.h);
        let strength = opacity.clamp(0.0, 1.0);
        if strength <= 0.0 || size <= 0.0 || self.layers.get(self.active).is_none() {
            return;
        }
        let scale = size / tip.w as f32;
        let th = (tip.h as f32 * scale).max(1.0);
        // A steered stamp sweeps its corners: bound the circumscribed
        // circle so rotation never clips. Unsteered keeps the old box.
        let (ex, ey) = if brush.angle_follow {
            let r = (size * size + th * th).sqrt() * 0.5;
            (r, r)
        } else {
            (size * 0.5, th * 0.5)
        };
        // Fully outside: a complete no-op (no mask, no dirty rect).
        if x + ex < 0.0 || y + ey < 0.0 || x - ex >= w_u32 as f32 || y - ey >= h_u32 as f32 {
            return;
        }
        if self.layers[self.active].mask.is_none() {
            self.add_mask(self.active);
        }
        let x0 = (x - ex).floor().max(0.0) as u32;
        let y0 = (y - ey).floor().max(0.0) as u32;
        let x1 = (x + ex).ceil().min(w_u32 as f32 - 1.0).max(0.0) as u32;
        let y1 = (y + ey).ceil().min(h_u32 as f32 - 1.0).max(0.0) as u32;
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
        let round = brush.roundness.clamp(0.1, 1.0);
        // Layer offset (Move tool): canvas pixel -> buffer slot.
        let (lox, loy) = (self.layers[active].ox, self.layers[active].oy);
        let bh = h_u32 as i32;
        let follow = brush.angle_follow;
        let (cos_a, sin_a) = (angle.cos(), angle.sin());
        let grain = clamp01(brush.grain);
        let grain_scale = brush.grain_scale;
        let half_w = size * 0.5;
        let half_h = th * 0.5;
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
                // Stamp-frame coords: un-rotate into the tip when steered,
                // squeeze across the stroke by roundness. Off-tip pixels
                // are transparent (clean rotated corners).
                let (ox, oy) = (px as f32 + 0.5 - x, py as f32 + 0.5 - y);
                let (rx, ry) = if follow {
                    (ox * cos_a + oy * sin_a, (-ox * sin_a + oy * cos_a) / round)
                } else {
                    (ox, oy / round)
                };
                let tx = ((rx + half_w) / scale) as i32;
                let ty = ((ry + half_h) / scale) as i32;
                if tx < 0 || ty < 0 || tx >= tip.w as i32 || ty >= tip.h as i32 {
                    continue;
                }
                let mut a = (tip.mask[(ty as u32 * tip.w + tx as u32) as usize] as f32 / 255.0
                    * strength
                    * Document::sel_k(sel_mask, w, px, py))
                .clamp(0.0, 1.0);
                if grain > 0.0 {
                    a *= 1.0 - grain * (1.0 - grain_at(px, py, grain_scale));
                }
                if a <= 0.0 {
                    continue;
                }
                // Store at the layer-offset slot (see parametric mask dab).
                let (lx, ly) = (px as i32 - lox, py as i32 - loy);
                if lx < 0 || ly < 0 || lx >= w as i32 || ly >= bh {
                    continue;
                }
                let o = (ly as u32 * w + lx as u32) as usize;
                if o >= layer_mask.len() {
                    continue;
                }
                layer_mask[o] = (value * a + layer_mask[o] as f32 * (1.0 - a)).round() as u8;
            }
        }
    }

    fn dab_one_resolved(
        &mut self,
        brush: &Brush,
        color: Color,
        x: f32,
        y: f32,
        size: f32,
        opacity: f32,
        angle: f32,
    ) {
        if brush.tip.is_some() {
            return self.dab_stamp_resolved(brush, color, x, y, size, opacity, angle);
        }
        let (w_u32, h_u32) = (self.w, self.h);
        let (w, h) = (w_u32 as f32, h_u32 as f32);
        let radius = (size * 0.5).max(0.5);
        let strength = opacity.clamp(0.0, 1.0);
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
        let w = w_u32;
        // Layer offset (Move tool): canvas pixel -> buffer slot.
        let (lox, loy) = (self.layers[active].ox, self.layers[active].oy);
        let bh = h_u32 as i32;
        let layer = &mut self.layers[active];
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
                // 1.0 inside the hard core, linear fade across the soft edge.
                let edge = brush.softness.clamp(0.0, 1.0);
                let round = brush.roundness.clamp(0.1, 1.0);
                let (cos_a, sin_a) = (angle.cos(), angle.sin());
                let grain = clamp01(brush.grain);
                let grain_scale = brush.grain_scale;
                // Dab-frame coords (see mask dab): roundness 1.0 keeps the
                // old circular falloff bit-for-bit.
                let (dx, dy) = (px as f32 + 0.5 - x, py as f32 + 0.5 - y);
                let (u, v) = (dx * cos_a + dy * sin_a, (-dx * sin_a + dy * cos_a) / round);
                let d = (u * u + v * v).sqrt() / radius;
                if d > 1.0 {
                    continue;
                }
                let falloff = if d <= 1.0 - edge { 1.0 } else { 1.0 - (d - (1.0 - edge)) / edge.max(0.001) };
                let mut fa = (falloff.clamp(0.0, 1.0)
                    * strength
                    * Document::sel_k(mask, w, px, py))
                .clamp(0.0, 1.0);
                if grain > 0.0 {
                    fa *= 1.0 - grain * (1.0 - grain_at(px, py, grain_scale));
                }
                let a = (fa * 255.0).round() as u8;
                if a == 0 {
                    continue;
                }
                // Store at the layer-offset slot (see mask dabs).
                let (lx, ly) = (px as i32 - lox, py as i32 - loy);
                if lx < 0 || ly < 0 || lx >= w as i32 || ly >= bh {
                    continue;
                }
                let o = ((ly as u32 * w + lx as u32) * 4) as usize;
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
    fn dab_stamp_resolved(
        &mut self,
        brush: &Brush,
        color: Color,
        x: f32,
        y: f32,
        size: f32,
        opacity: f32,
        angle: f32,
    ) {
        let Some(tip) = brush.tip.as_ref() else { return };
        if tip.w == 0 || tip.h == 0 || tip.mask.len() != (tip.w * tip.h) as usize {
            return;
        }
        let (w_u32, h_u32) = (self.w, self.h);
        let (w, h) = (w_u32 as f32, h_u32 as f32);
        let strength = opacity.clamp(0.0, 1.0);
        if strength <= 0.0 || size <= 0.0 || self.layers.get(self.active).is_none() {
            return;
        }
        let scale = size / tip.w as f32;
        let th = (tip.h as f32 * scale).max(1.0);
        // Steered stamps sweep their corners: bound the circumscribed
        // circle so rotation never clips. Unsteered keeps the old box.
        let (ex, ey) = if brush.angle_follow {
            let r = (size * size + th * th).sqrt() * 0.5;
            (r, r)
        } else {
            (size * 0.5, th * 0.5)
        };
        let x0 = (x - ex).floor().max(0.0) as u32;
        let y0 = (y - ey).floor().max(0.0) as u32;
        let x1 = (x + ex).ceil().min(w - 1.0).max(0.0) as u32;
        let y1 = (y + ey).ceil().min(h - 1.0).max(0.0) as u32;
        self.mark_dirty(x0, y0, x1, y1);
        let selection = self.selection;
        let active = self.active;
        if active >= self.layers.len() {
            return;
        }
        let mask = self.selection_mask.as_deref();
        let w = w_u32;
        let round = brush.roundness.clamp(0.1, 1.0);
        // Layer offset (Move tool): canvas pixel -> buffer slot.
        let (lox, loy) = (self.layers[active].ox, self.layers[active].oy);
        let bh = h_u32 as i32;
        let layer = &mut self.layers[active];
        let follow = brush.angle_follow;
        let (cos_a, sin_a) = (angle.cos(), angle.sin());
        let grain = clamp01(brush.grain);
        let grain_scale = brush.grain_scale;
        let half_w = size * 0.5;
        let half_h = th * 0.5;
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
                // Stamp-frame coords (see mask stamp): un-rotate when
                // steered, squeeze across the stroke by roundness. Off-tip
                // pixels are transparent (clean rotated corners).
                let (ox, oy) = (px as f32 + 0.5 - x, py as f32 + 0.5 - y);
                let (rx, ry) = if follow {
                    (ox * cos_a + oy * sin_a, (-ox * sin_a + oy * cos_a) / round)
                } else {
                    (ox, oy / round)
                };
                let tx = ((rx + half_w) / scale) as i32;
                let ty = ((ry + half_h) / scale) as i32;
                if tx < 0 || ty < 0 || tx >= tip.w as i32 || ty >= tip.h as i32 {
                    continue;
                }
                let mut fa = (tip.mask[(ty as u32 * tip.w + tx as u32) as usize] as f32 / 255.0
                    * strength
                    * Document::sel_k(mask, w, px, py))
                .clamp(0.0, 1.0);
                if grain > 0.0 {
                    fa *= 1.0 - grain * (1.0 - grain_at(px, py, grain_scale));
                }
                let a = (fa * 255.0).round() as u8;
                if a == 0 {
                    continue;
                }
                // Store at the layer-offset slot (see mask dabs).
                let (lx, ly) = (px as i32 - lox, py as i32 - loy);
                if lx < 0 || ly < 0 || lx >= w as i32 || ly >= bh {
                    continue;
                }
                let o = ((ly as u32 * w + lx as u32) * 4) as usize;
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
    /// Full-pressure shorthand over [`Document::stroke_to_dyn`]: the segment
    /// direction still steers `angle_follow` brushes (shapes, previews).
    pub fn stroke_to(
        &mut self,
        brush: &Brush,
        color: Color,
        from: Option<(f32, f32)>,
        to: (f32, f32),
        leftover: f32,
    ) -> (Option<(f32, f32)>, f32) {
        let angle = match from {
            Some((sx, sy)) => (to.1 - sy).atan2(to.0 - sx),
            None => 0.0,
        };
        let dyn_ = DabDynamics { pressure: 1.0, speed_norm: 0.0, angle };
        self.stroke_to_dyn(brush, color, from, dyn_, to, dyn_, leftover)
    }

    /// Advance a stroke with per-endpoint dynamics, interpolating pressure,
    /// speed and direction per dab so segments taper, fade and turn across
    /// their dabs. `from_dyn` is ignored when `from` is `None` (stroke start
    /// paints one full `to_dyn` dab). Returns the leftover distance plus the
    /// new endpoint for chaining. Call [`Document::checkpoint`] once when
    /// the stroke starts.
    pub fn stroke_to_dyn(
        &mut self,
        brush: &Brush,
        color: Color,
        from: Option<(f32, f32)>,
        from_dyn: DabDynamics,
        to: (f32, f32),
        to_dyn: DabDynamics,
        leftover: f32,
    ) -> (Option<(f32, f32)>, f32) {
        let Some((sx, sy)) = from else {
            let size = brush.resolve_size(to_dyn.pressure, to_dyn.speed_norm);
            let opacity = brush.resolve_opacity(to_dyn.pressure, to_dyn.speed_norm);
            let (ox, oy) = scatter_offset(brush, to.0, to.1, size);
            self.dab_dyn(brush, color, to.0 + ox, to.1 + oy, size, opacity, to_dyn.angle);
            return (Some(to), 0.0);
        };
        let (dx, dy) = (to.0 - sx, to.1 - sy);
        let dist = (dx * dx + dy * dy).sqrt();
        if dist <= 0.0 {
            return (from, leftover);
        }
        // Step from the midpoint size so tapered strokes keep even dabs.
        let mid_size = brush.resolve_size(
            (from_dyn.pressure + to_dyn.pressure) * 0.5,
            (from_dyn.speed_norm + to_dyn.speed_norm) * 0.5,
        );
        let step = (mid_size * brush.spacing).max(1.0);
        // Lerp direction as unit vectors (stable across +/-PI).
        let (fx, fy) = (from_dyn.angle.cos(), from_dyn.angle.sin());
        let (tx, ty) = (to_dyn.angle.cos(), to_dyn.angle.sin());
        let mut travelled = leftover;
        while travelled <= dist {
            let t = travelled / dist;
            let pressure = from_dyn.pressure + (to_dyn.pressure - from_dyn.pressure) * t;
            let speed = from_dyn.speed_norm + (to_dyn.speed_norm - from_dyn.speed_norm) * t;
            let vx = fx + (tx - fx) * t;
            let vy = fy + (ty - fy) * t;
            let angle = vy.atan2(vx);
            let size = brush.resolve_size(pressure, speed);
            let opacity = brush.resolve_opacity(pressure, speed);
            let (x, y) = (sx + dx * t, sy + dy * t);
            let (ox, oy) = scatter_offset(brush, x, y, size);
            self.dab_dyn(brush, color, x + ox, y + oy, size, opacity, angle);
            travelled += step;
        }
        (Some(to), travelled - dist)
    }
}

/// Deterministic scatter offset for a dab center: uniform in
/// `[-scatter*size/2, +scatter*size/2]` per axis, hashed from the dab
/// position (quantized) so strokes replay identically. Returns (0, 0)
/// when `scatter` is 0.
fn scatter_offset(brush: &Brush, x: f32, y: f32, size: f32) -> (f32, f32) {
    let sc = brush.scatter.clamp(0.0, 1.0);
    if sc <= 0.0 || size <= 0.0 {
        return (0.0, 0.0);
    }
    let qx = (x * 2.0).round() as i32;
    let qy = (y * 2.0).round() as i32;
    let ox = (hash01(qx, qy ^ 0x51ab) - 0.5) * sc * size;
    let oy = (hash01(qx ^ 0x2b7e, qy) - 0.5) * sc * size;
    (ox, oy)
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
    if !(0.0 <= brush.pressure_size && brush.pressure_size <= 1.0) {
        return Err(format!("brush '{clean}' has a bad pressure-size"));
    }
    if !(0.0 <= brush.pressure_opacity && brush.pressure_opacity <= 1.0) {
        return Err(format!("brush '{clean}' has a bad pressure-opacity"));
    }
    if !(0.0 <= brush.velocity_size && brush.velocity_size <= 1.0) {
        return Err(format!("brush '{clean}' has a bad velocity-size"));
    }
    if !(0.0 <= brush.velocity_opacity && brush.velocity_opacity <= 1.0) {
        return Err(format!("brush '{clean}' has a bad velocity-opacity"));
    }
    if !(0.0 <= brush.grain && brush.grain <= 1.0) {
        return Err(format!("brush '{clean}' has a bad grain"));
    }
    if !(0.25 <= brush.grain_scale && brush.grain_scale <= 4.0) {
        return Err(format!("brush '{clean}' has a bad grain scale"));
    }
    if !(0.1 <= brush.roundness && brush.roundness <= 1.0) {
        return Err(format!("brush '{clean}' has a bad roundness"));
    }
    if !(0.0 <= brush.scatter && brush.scatter <= 1.0) {
        return Err(format!("brush '{clean}' has a bad scatter"));
    }
    if !(0.0 <= brush.stabilizer && brush.stabilizer <= 1.0) {
        return Err(format!("brush '{clean}' has a bad stabilizer"));
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
/// from hard sketching to soft painting. M14 gives each preset its own
/// dynamics voice: Pencil bites with pressure and grain, Ink smooths and
/// follows direction, Marker is a flat chisel, Airbrush answers pressure
/// with flow rather than size, Chalk is all grain and scatter.
pub fn presets() -> Vec<(&'static str, Brush)> {
    vec![
        ("Pencil", Brush {
            size: 3.0, opacity: 1.0, softness: 0.15, spacing: 0.1, eraser: false, tip: None,
            pressure_size: 0.8, pressure_opacity: 0.7,
            velocity_size: 0.5, velocity_opacity: 0.4,
            grain: 0.35, grain_scale: 0.8, roundness: 1.0,
            angle_follow: false, scatter: 0.05, stabilizer: 0.25,
        }),
        ("Ink Pen", Brush {
            size: 6.0, opacity: 1.0, softness: 0.3, spacing: 0.1, eraser: false, tip: None,
            pressure_size: 0.9, pressure_opacity: 0.8,
            velocity_size: 0.4, velocity_opacity: 0.3,
            grain: 0.0, grain_scale: 1.0, roundness: 0.6,
            angle_follow: true, scatter: 0.0, stabilizer: 0.5,
        }),
        ("Round", Brush {
            size: 24.0, opacity: 1.0, softness: 0.6, spacing: 0.15, eraser: false, tip: None,
            pressure_size: 0.7, pressure_opacity: 0.6,
            velocity_size: 0.6, velocity_opacity: 0.6,
            grain: 0.0, grain_scale: 1.0, roundness: 1.0,
            angle_follow: false, scatter: 0.0, stabilizer: 0.35,
        }),
        ("Marker", Brush {
            size: 28.0, opacity: 0.85, softness: 0.5, spacing: 0.12, eraser: false, tip: None,
            pressure_size: 0.4, pressure_opacity: 0.5,
            velocity_size: 0.3, velocity_opacity: 0.3,
            grain: 0.0, grain_scale: 1.0, roundness: 0.55,
            angle_follow: true, scatter: 0.0, stabilizer: 0.3,
        }),
        ("Airbrush", Brush {
            size: 64.0, opacity: 0.25, softness: 1.0, spacing: 0.08, eraser: false, tip: None,
            pressure_size: 0.3, pressure_opacity: 0.9,
            velocity_size: 0.2, velocity_opacity: 0.3,
            grain: 0.15, grain_scale: 1.5, roundness: 1.0,
            angle_follow: false, scatter: 0.0, stabilizer: 0.4,
        }),
        ("Chalk", Brush {
            size: 18.0, opacity: 0.8, softness: 0.9, spacing: 0.25, eraser: false, tip: None,
            pressure_size: 0.6, pressure_opacity: 0.6,
            velocity_size: 0.5, velocity_opacity: 0.5,
            grain: 0.55, grain_scale: 0.7, roundness: 1.0,
            angle_follow: false, scatter: 0.12, stabilizer: 0.2,
        }),
    ]
}
impl Document {
    /// Paint one pixel with source-over at brush opacity, honoring selection
    /// and mirroring per [`Document::symmetry`].
    pub(crate) fn put(&mut self, brush: &Brush, color: Color, px: u32, py: u32) {
        let (ccx, ccy) = self.symmetry_center_px();
        let pts =
            self.symmetry.mirrored_pixels(self.w, self.h, self.symmetry_segments, ccx, ccy, px, py);
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
        let (w, h) = (self.w, self.h);
        // Graduated feathered masks scale the paint (binary masks: full).
        let sel_k = Document::sel_k(self.selection_mask.as_deref(), self.w, px, py);
        // Layer offset: store at the buffer slot for this canvas pixel.
        let Some(layer) = self.active_layer_mut() else { return };
        let (lox, loy) = (layer.ox, layer.oy);
        let (lx, ly) = (px as i32 - lox, py as i32 - loy);
        if lx < 0 || ly < 0 || lx >= w as i32 || ly >= h as i32 {
            return;
        }
        let o = ((ly as u32 * w + lx as u32) * 4) as usize;
        if o + 3 >= layer.pixels.len() {
            return;
        }
        let src_a = brush.opacity.clamp(0.0, 1.0) * sel_k;
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
        let (lox, loy) = (self.layers[self.active].ox, self.layers[self.active].oy);
        // Canvas cell -> buffer offset, if the layer covers it. Uncovered
        // canvas reads as transparent content (fill can flow through it
        // when it matches, but there is no slot to store into).
        let slot = |cx: u32, cy: u32| -> Option<usize> {
            let lx = cx as i32 - lox;
            let ly = cy as i32 - loy;
            if lx < 0 || ly < 0 || lx >= w as i32 || ly >= h as i32 {
                return None;
            }
            Some(((ly as u32 * w + lx as u32) * 4) as usize)
        };
        // Seed on uncovered canvas has no buffer to fill: no-op.
        let Some(seed_off) = slot(x, y) else { return };
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
        let (ccx, ccy) = self.symmetry_center_px();
        for (sx, sy) in self.symmetry.mirrored_pixels(w, h, self.symmetry_segments, ccx, ccy, x, y) {
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
            let content: [u8; 4] = match slot(cx, cy) {
                Some(o) if o + 3 < layer.pixels.len() => [
                    layer.pixels[o],
                    layer.pixels[o + 1],
                    layer.pixels[o + 2],
                    layer.pixels[o + 3],
                ],
                _ => [0, 0, 0, 0],
            };
            if !matches(&content) {
                continue;
            }
            // Store only where the buffer exists (see `slot`).
            if let Some(o) = slot(cx, cy) {
                if o + 3 < layer.pixels.len() {
                    layer.pixels[o..o + 4].copy_from_slice(&blend_over(content, color, opacity));
                    any = true;
                }
            }
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
    last_dyn: DabDynamics,
    leftover: f32,
}

impl<'a> Stroke<'a> {
    /// Checkpoint the active layer for undo, then start painting.
    pub fn begin(doc: &'a mut Document, brush: Brush, color: Color) -> Self {
        doc.checkpoint();
        Self { doc, brush, color, last: None, last_dyn: DabDynamics::default(), leftover: 0.0 }
    }

    /// Feed a pointer position (canvas pixels).
    pub fn push(&mut self, x: f32, y: f32) {
        self.push_dyn(x, y, DabDynamics::default());
    }

    /// Feed a pointer position with dynamics (pressure/speed/direction).
    pub fn push_dyn(&mut self, x: f32, y: f32, dyn_: DabDynamics) {
        let (last, leftover) = self.doc.stroke_to_dyn(
            &self.brush,
            self.color,
            self.last,
            self.last_dyn,
            (x, y),
            dyn_,
            self.leftover,
        );
        self.last = last;
        self.last_dyn = dyn_;
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
    fn flood_fill_tolerance_widens_the_region() {
        // Two near-identical reds side by side (ColorDrop-style threshold).
        let two_tone = |doc: &mut Document| {
            let brush = Brush { opacity: 1.0, ..Default::default() };
            doc.fill_rect(&brush, Color::rgb(200, 0, 0), 0, 0, 4, 9);
            doc.fill_rect(&brush, Color::rgb(206, 0, 0), 5, 0, 9, 9);
        };
        let at = |d: &Document, x: u32, y: u32| {
            let o = ((y * 10 + x) * 4) as usize;
            [d.layers[0].pixels[o], d.layers[0].pixels[o + 1], d.layers[0].pixels[o + 2]]
        };
        let brush = Brush { opacity: 1.0, ..Default::default() };
        // Strict: only the seed side fills.
        let mut tight = Document::new(10, 10);
        two_tone(&mut tight);
        tight.flood_fill(&brush, Color::rgb(0, 0, 255), 1, 5, 0);
        assert_eq!(at(&tight, 0, 5), [0, 0, 255]);
        assert_eq!(at(&tight, 9, 5), [206, 0, 0]);
        // Loose: the neighbor tone joins the region.
        let mut loose = Document::new(10, 10);
        two_tone(&mut loose);
        loose.flood_fill(&brush, Color::rgb(0, 0, 255), 1, 5, 10);
        assert_eq!(at(&loose, 0, 5), [0, 0, 255]);
        assert_eq!(at(&loose, 9, 5), [0, 0, 255]);
    }

    #[test]
    fn flood_fill_restore_and_refill_matches_direct() {
        // The ColorDrop preview loop (snapshot, restore, re-fill at a new
        // tolerance) must equal a single direct fill at that tolerance.
        let brush = Brush { opacity: 1.0, ..Default::default() };
        let mut direct = Document::new(10, 10);
        direct.fill_rect(&brush, Color::rgb(200, 0, 0), 0, 0, 4, 9);
        direct.fill_rect(&brush, Color::rgb(206, 0, 0), 5, 0, 9, 9);
        direct.flood_fill(&brush, Color::rgb(0, 0, 255), 1, 5, 10);
        let mut preview = Document::new(10, 10);
        preview.fill_rect(&brush, Color::rgb(200, 0, 0), 0, 0, 4, 9);
        preview.fill_rect(&brush, Color::rgb(206, 0, 0), 5, 0, 9, 9);
        let snap = preview.layers[0].pixels.clone();
        preview.flood_fill(&brush, Color::rgb(0, 0, 255), 1, 5, 0);
        preview.layers[0].pixels = snap;
        preview.flood_fill(&brush, Color::rgb(0, 0, 255), 1, 5, 10);
        assert_eq!(preview.layers[0].pixels, direct.layers[0].pixels);
    }

    #[test]
    fn stamp_dabs_fade_through_feathered_masks() {
        use crate::document::Document;
        let mut doc = Document::new(20, 20);
        doc.selection = Some((5, 5, 14, 14));
        assert!(doc.feather_selection(3));
        let ramp = doc.selection_mask.clone().unwrap()[(4 * 20 + 10) as usize];
        assert!(ramp > 0 && ramp < 255, "{ramp}");
        // Solid 2x2 stamp tip through the ramp paints partially.
        let tip = BrushTip::from_mask(2, 2, vec![255; 4]).unwrap();
        let stamp = Brush { size: 4.0, tip: Some(tip), ..Brush::default() };
        doc.dab(&stamp, Color::rgb(255, 255, 255), 10.5, 4.5);
        let mut peak = 0u8;
        for y in 3..=5 {
            for x in 9..=11 {
                peak = peak.max(doc.layers[0].pixels[((y * 20 + x) * 4) as usize + 3]);
            }
        }
        assert!(peak > 0 && peak < 255, "{peak}");
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
    fn dynamics_resolve_identity_at_full_pressure() {
        let brush = Brush::default();
        assert_eq!(brush.resolve_size(1.0, 0.0), brush.size);
        assert_eq!(brush.resolve_opacity(1.0, 0.0), brush.opacity);
    }

    #[test]
    fn pressure_tapers_size_and_opacity() {
        let brush = Brush {
            size: 20.0,
            opacity: 1.0,
            pressure_size: 0.8,
            pressure_opacity: 0.6,
            velocity_size: 0.0,
            velocity_opacity: 0.0,
            ..Default::default()
        };
        // p=0 keeps (1-0.8)=20% of size, (1-0.6)=40% of opacity.
        assert!((brush.resolve_size(0.0, 0.0) - 4.0).abs() < 0.001);
        assert!((brush.resolve_opacity(0.0, 0.0) - 0.4).abs() < 0.001);
        // Disabled amounts are no-ops at any pressure.
        let flat = Brush {
            pressure_size: 0.0,
            pressure_opacity: 0.0,
            ..brush.clone()
        };
        assert_eq!(flat.resolve_size(0.0, 0.0), 20.0);
        assert_eq!(flat.resolve_opacity(0.0, 0.0), 1.0);
    }

    #[test]
    fn velocity_thins_and_fades() {
        let brush = Brush::default(); // velocity 0.6 / 0.6
        let size_full = brush.resolve_size(1.0, 1.0);
        let op_full = brush.resolve_opacity(1.0, 1.0);
        assert!(size_full < brush.size);
        assert!(op_full < brush.opacity);
        // Old M7 weights preserved: size 0.6, opacity 0.4 at full speed.
        assert!((size_full - brush.size * (1.0 - 0.6 * 0.6)).abs() < 0.001);
        assert!((op_full - brush.opacity * (1.0 - 0.4 * 0.6)).abs() < 0.001);
    }

    #[test]
    fn old_brush_json_loads_with_new_defaults() {
        let old = r#"[{"name":"Old","brush":{"size":10.0,"opacity":1.0,"softness":0.5,"spacing":0.2,"eraser":false}}]"#;
        let libs: Vec<BrushPreset> = serde_json::from_str(old).unwrap();
        assert_eq!(libs.len(), 1);
        let want = Brush { size: 10.0, softness: 0.5, spacing: 0.2, ..Default::default() };
        assert_eq!(libs[0].brush, want);
    }

    fn alpha_sum(doc: &Document) -> u64 {
        doc.layers[0].pixels.iter().skip(3).step_by(4).map(|&b| b as u64).sum()
    }

    fn painted_bbox(doc: &Document) -> Option<(u32, u32, u32, u32)> {
        let (w, h) = (doc.w, doc.h);
        let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
        let mut any = false;
        for y in 0..h {
            for x in 0..w {
                if doc.layers[0].pixels[((y * w + x) * 4 + 3) as usize] > 0 {
                    any = true;
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x);
                    y1 = y1.max(y);
                }
            }
        }
        any.then_some((x0, y0, x1, y1))
    }

    #[test]
    fn light_pressure_stroke_paints_less() {
        let brush = Brush { size: 12.0, softness: 0.0, spacing: 0.1, ..Default::default() };
        let stroke = |pressure: f32| {
            let mut doc = Document::new(64, 64);
            let d = DabDynamics { pressure, speed_norm: 0.0, angle: 0.0 };
            doc.stroke_to_dyn(&brush, Color::rgb(255, 0, 0), Some((10.0, 32.0)), d, (54.0, 32.0), d, 0.0);
            doc
        };
        let full = alpha_sum(&stroke(1.0));
        let light = alpha_sum(&stroke(0.2));
        assert!(full > 0 && light > 0);
        assert!(light < full, "light {light} vs full {full}");
    }

    #[test]
    fn grain_breaks_up_the_dab() {
        let clean = Brush { size: 20.0, softness: 0.0, spacing: 0.5, ..Default::default() };
        let grained = Brush { grain: 0.8, grain_scale: 1.0, ..clean.clone() };
        let paint = |brush: &Brush| {
            let mut doc = Document::new(48, 48);
            doc.dab(brush, Color::rgb(255, 255, 255), 24.0, 24.0);
            doc
        };
        let (c, g) = (paint(&clean), paint(&grained));
        assert!(alpha_sum(&g) < alpha_sum(&c));
        // Same footprint (grain never fully erases), different values.
        assert_eq!(painted_bbox(&g), painted_bbox(&c));
        assert_ne!(c.layers[0].pixels, g.layers[0].pixels);
    }

    #[test]
    fn roundness_squeezes_across_the_stroke() {
        let paint = |roundness: f32| {
            let brush = Brush {
                size: 20.0,
                softness: 0.0,
                spacing: 0.5,
                roundness,
                ..Default::default()
            };
            let mut doc = Document::new(48, 48);
            doc.dab(&brush, Color::rgb(255, 255, 255), 24.0, 24.0);
            doc
        };
        let (x0r, y0r, x1r, y1r) = painted_bbox(&paint(1.0)).unwrap();
        let (x0f, y0f, x1f, y1f) = painted_bbox(&paint(0.3)).unwrap();
        // Angle 0: u along x stays full width, v (y) is squeezed.
        assert_eq!((x0f, x1f), (x0r, x1r));
        assert!(y1f - y0f < y1r - y0r, "flat {} vs round {}", y1f - y0f, y1r - y0r);
    }

    #[test]
    fn scatter_moves_dabs() {
        let plain = Brush { size: 12.0, softness: 0.5, spacing: 0.2, ..Default::default() };
        let scattered = Brush { scatter: 0.6, ..plain.clone() };
        let paint = |brush: &Brush| {
            let mut doc = Document::new(64, 64);
            let d = DabDynamics::default();
            doc.stroke_to_dyn(&brush, Color::rgb(255, 0, 0), Some((8.0, 32.0)), d, (56.0, 32.0), d, 0.0);
            doc
        };
        assert_ne!(paint(&plain).layers[0].pixels, paint(&scattered).layers[0].pixels);
    }

    #[test]
    fn angle_follow_steers_stamp_orientation() {
        // Wide flat tip: horizontal strokes stamp wide, vertical tall.
        let tip = BrushTip::from_mask(8, 4, vec![255u8; 32]).unwrap();
        let brush = Brush {
            size: 8.0,
            opacity: 1.0,
            spacing: 2.0,
            angle_follow: true,
            ..Default::default()
        };
        let brush = Brush { tip: Some(tip), ..brush };
        let paint = |from: (f32, f32), to: (f32, f32), angle: f32| {
            let mut doc = Document::new(48, 48);
            let d = DabDynamics { pressure: 1.0, speed_norm: 0.0, angle };
            // Short segment + wide spacing: exactly one dab at `from`.
            doc.stroke_to_dyn(&brush, Color::rgb(255, 255, 255), Some(from), d, to, d, 0.0);
            painted_bbox(&doc).unwrap()
        };
        let (hx0, hy0, hx1, hy1) = paint((24.0, 24.0), (32.0, 24.0), 0.0);
        let (vx0, vy0, vx1, vy1) = paint((24.0, 24.0), (24.0, 32.0), std::f32::consts::FRAC_PI_2);
        assert!(hx1 - hx0 > hy1 - hy0, "horizontal should be wide");
        assert!(vy1 - vy0 > vx1 - vx0, "vertical should be tall");
    }

    #[test]
    fn validation_rejects_new_ranges() {
        let dir = std::env::temp_dir();
        let path = dir.join("procraft-lib-m14-range.pcbrush");
        for brush in [
            Brush { pressure_size: 2.0, ..Default::default() },
            Brush { pressure_opacity: -0.5, ..Default::default() },
            Brush { velocity_size: 1.5, ..Default::default() },
            Brush { grain: 1.5, ..Default::default() },
            Brush { grain_scale: 9.0, ..Default::default() },
            Brush { roundness: 0.0, ..Default::default() },
            Brush { scatter: -1.0, ..Default::default() },
            Brush { stabilizer: 2.0, ..Default::default() },
        ] {
            let libs = vec![BrushPreset { name: "Bad".to_string(), brush }];
            assert!(save_brush_library(&path, &libs).is_err());
        }
        let _ = std::fs::remove_file(&path);
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
