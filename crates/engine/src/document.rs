//! Layers, documents, compositing and undo.

/// How a layer's pixels combine with what's beneath.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BlendMode {
    #[default]
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    Difference,
    Exclusion,
    SoftLight,
    HardLight,
}

impl BlendMode {
    pub const ALL: [BlendMode; 10] = [
        BlendMode::Normal,
        BlendMode::Multiply,
        BlendMode::Screen,
        BlendMode::Overlay,
        BlendMode::Darken,
        BlendMode::Lighten,
        BlendMode::Difference,
        BlendMode::Exclusion,
        BlendMode::SoftLight,
        BlendMode::HardLight,
    ];

    pub fn name(self) -> &'static str {
        match self {
            BlendMode::Normal => "Normal",
            BlendMode::Multiply => "Multiply",
            BlendMode::Screen => "Screen",
            BlendMode::Overlay => "Overlay",
            BlendMode::Darken => "Darken",
            BlendMode::Lighten => "Lighten",
            BlendMode::Difference => "Difference",
            BlendMode::Exclusion => "Exclusion",
            BlendMode::SoftLight => "Soft light",
            BlendMode::HardLight => "Hard light",
        }
    }

    fn apply(self, src: f32, dst: f32) -> f32 {
        match self {
            BlendMode::Normal => src,
            BlendMode::Multiply => src * dst / 255.0,
            BlendMode::Screen => 255.0 - (255.0 - src) * (255.0 - dst) / 255.0,
            BlendMode::Overlay => {
                if dst < 128.0 {
                    2.0 * src * dst / 255.0
                } else {
                    255.0 - 2.0 * (255.0 - src) * (255.0 - dst) / 255.0
                }
            }
            BlendMode::Darken => src.min(dst),
            BlendMode::Lighten => src.max(dst),
            BlendMode::Difference => (src - dst).abs(),
            BlendMode::Exclusion => src + dst - 2.0 * src * dst / 255.0,
            BlendMode::SoftLight => {
                // Pegtop approximation: gentle contrast around mid-gray.
                let (s, d) = (src / 255.0, dst / 255.0);
                let out = if s <= 0.5 {
                    d - (1.0 - 2.0 * s) * d * (1.0 - d)
                } else {
                    d + (2.0 * s - 1.0) * (d.sqrt() - d)
                };
                out.clamp(0.0, 1.0) * 255.0
            }
            BlendMode::HardLight => {
                // Overlay with source and destination swapped.
                if src < 128.0 {
                    2.0 * src * dst / 255.0
                } else {
                    255.0 - 2.0 * (255.0 - src) * (255.0 - dst) / 255.0
                }
            }
        }
    }
}

/// Mirror painting mode: strokes are stamped on both sides of the canvas
/// center line(s). Horizontal mirrors left-right, Vertical mirrors top-bottom,
/// Both mirrors on both axes, Kaleidoscope reflects around N axes through
/// the canvas center (Segments slider, 2..=16).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SymmetryMode {
    #[default]
    Off,
    Horizontal,
    Vertical,
    Both,
    Kaleidoscope,
}

impl SymmetryMode {
    pub const ALL: [SymmetryMode; 5] = [
        SymmetryMode::Off,
        SymmetryMode::Horizontal,
        SymmetryMode::Vertical,
        SymmetryMode::Both,
        SymmetryMode::Kaleidoscope,
    ];

    pub fn name(self) -> &'static str {
        match self {
            SymmetryMode::Off => "Off",
            SymmetryMode::Horizontal => "Horizontal",
            SymmetryMode::Vertical => "Vertical",
            SymmetryMode::Both => "Both",
            SymmetryMode::Kaleidoscope => "Kaleidoscope",
        }
    }

    pub(crate) fn mirrored_points(
        self,
        w: u32,
        h: u32,
        segments: u8,
        x: f32,
        y: f32,
    ) -> Vec<(f32, f32)> {
        let mx = w as f32 - 1.0 - x;
        let my = h as f32 - 1.0 - y;
        match self {
            SymmetryMode::Off => vec![(x, y)],
            SymmetryMode::Horizontal => vec![(x, y), (mx, y)],
            SymmetryMode::Vertical => vec![(x, y), (x, my)],
            SymmetryMode::Both => vec![(x, y), (mx, y), (x, my), (mx, my)],
            SymmetryMode::Kaleidoscope => {
                // Dihedral symmetry: rotate by k*step, mirroring on odd k.
                let n = segments.clamp(2, 16) as usize;
                let (cx, cy) = ((w as f32 - 1.0) * 0.5, (h as f32 - 1.0) * 0.5);
                let (dx, dy) = (x - cx, y - cy);
                let step = std::f32::consts::TAU / n as f32;
                (0..n)
                    .map(|k| {
                        let (px, mut py) = (dx, dy);
                        if k % 2 == 1 {
                            py = -py;
                        }
                        let (s, c) = (k as f32 * step).sin_cos();
                        (cx + px * c - py * s, cy + px * s + py * c)
                    })
                    .collect()
            }
        }
    }

    pub(crate) fn mirrored_pixels(
        self,
        w: u32,
        h: u32,
        segments: u8,
        px: u32,
        py: u32,
    ) -> Vec<(u32, u32)> {
        match self {
            SymmetryMode::Kaleidoscope => {
                let n = segments.clamp(2, 16) as usize;
                let (cx, cy) = ((w as f32 - 1.0) * 0.5, (h as f32 - 1.0) * 0.5);
                let (dx, dy) = (px as f32 - cx, py as f32 - cy);
                let step = std::f32::consts::TAU / n as f32;
                (0..n)
                    .map(|k| {
                        let (qx, mut qy) = (dx, dy);
                        if k % 2 == 1 {
                            qy = -qy;
                        }
                        let (s, c) = (k as f32 * step).sin_cos();
                        let fx = cx + qx * c - qy * s;
                        let fy = cy + qx * s + qy * c;
                        (
                            fx.round().clamp(0.0, w as f32 - 1.0) as u32,
                            fy.round().clamp(0.0, h as f32 - 1.0) as u32,
                        )
                    })
                    .collect()
            }
            _ => {
                let mx = w.saturating_sub(1).saturating_sub(px);
                let my = h.saturating_sub(1).saturating_sub(py);
                match self {
                    SymmetryMode::Off => vec![(px, py)],
                    SymmetryMode::Horizontal => vec![(px, py), (mx, py)],
                    SymmetryMode::Vertical => vec![(px, py), (px, my)],
                    SymmetryMode::Both => vec![(px, py), (mx, py), (px, my), (mx, my)],
                    SymmetryMode::Kaleidoscope => unreachable!(),
                }
            }
        }
    }
}

/// One raster layer: row-major RGBA bytes plus presentation state.
#[derive(Clone, Debug)]
pub struct Layer {
    pub name: String,
    pub visible: bool,
    /// 0.0 (transparent) ..= 1.0 (opaque).
    pub opacity: f32,
    pub blend: BlendMode,
    pub pixels: Vec<u8>,
    /// Optional grayscale reveal mask (row-major, 255 = fully shown).
    /// Multiplies the layer's alpha at composite time. Session-painted;
    /// saved to v5 project files.
    pub mask: Option<Vec<u8>>,
    /// Clipping mask: confine this layer to the opaque content beneath it.
    pub clip: bool,
}

impl Layer {
    pub fn blank(name: String, w: u32, h: u32) -> Self {
        Self {
            name,
            visible: true,
            opacity: 1.0,
            blend: BlendMode::Normal,
            pixels: vec![0; (w * h * 4) as usize],
            mask: None,
            clip: false,
        }
    }

    pub fn clear(&mut self) {
        self.pixels.fill(0);
    }
}

/// Freehand painting target: layer color or its reveal mask. On masks the
/// brush color's luminance is the mask value (dark conceals, white reveals);
/// the eraser always reveals.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PaintTarget {
    #[default]
    Color,
    Mask,
}

impl PaintTarget {
    pub fn name(self) -> &'static str {
        match self {
            PaintTarget::Color => "Color",
            PaintTarget::Mask => "Mask",
        }
    }
}

/// A layered document. Layer 0 is the bottom of the stack.
#[derive(Clone, Debug)]
pub struct Document {
    pub w: u32,
    pub h: u32,
    pub layers: Vec<Layer>,
    pub active: usize,
    /// Backdrop shown under transparent pixels (the page color).
    pub background: [u8; 4],
    /// Optional rectangular selection (inclusive canvas pixels): painting
    /// outside it is ignored.
    pub selection: Option<(u32, u32, u32, u32)>,
    /// Optional freehand lasso mask (row-major, 1 = inside): painting outside
    /// it is ignored. Session-only, never saved to project files.
    pub selection_mask: Option<Vec<u8>>,
    /// Mirror painting mode (not all versions of the file format store it).
    pub symmetry: SymmetryMode,
    /// Kaleidoscope reflection axes, 2..=16 (only used in that mode).
    pub symmetry_segments: u8,
    /// Freehand painting target (layer color or its reveal mask).
    pub paint_target: PaintTarget,
    dirty: Option<(u32, u32, u32, u32)>,
    undo: Vec<UndoEntry>,
    redo: Vec<UndoEntry>,
}

#[derive(Clone, Debug)]
struct UndoEntry {
    layer: usize,
    pixels: Vec<u8>,
    mask: Option<Vec<u8>>,
}

impl Document {
    pub fn new(w: u32, h: u32) -> Self {
        Self {
            w,
            h,
            layers: vec![Layer::blank("Layer 1".to_string(), w, h)],
            active: 0,
            background: [255, 255, 255, 255],
            selection: None,
            selection_mask: None,
            symmetry: SymmetryMode::Off,
            symmetry_segments: 6,
            paint_target: PaintTarget::Color,
            dirty: None,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    /// Assemble a document from imported layers (white page, given active
    /// layer, no selection or history). For format importers in this crate.
    pub(crate) fn from_layers(w: u32, h: u32, layers: Vec<Layer>, active: usize) -> Self {
        let active = active.min(layers.len().saturating_sub(1).max(0));
        Self {
            w,
            h,
            layers,
            active,
            background: [255, 255, 255, 255],
            selection: None,
            selection_mask: None,
            symmetry: SymmetryMode::Off,
            symmetry_segments: 6,
            paint_target: PaintTarget::Color,
            dirty: None,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }
    /// Take and clear the dirty region (inclusive bounds) for partial
    /// texture uploads. `None` means nothing changed: skip the upload.
    pub fn take_dirty(&mut self) -> Option<(u32, u32, u32, u32)> {
        self.dirty.take()
    }

    /// Mark the whole canvas dirty (structural changes, undo, background).
    pub fn mark_full_dirty(&mut self) {
        if self.w > 0 && self.h > 0 {
            self.dirty = Some((0, 0, self.w - 1, self.h - 1));
        }
    }

    pub(crate) fn mark_dirty(&mut self, x0: u32, y0: u32, x1: u32, y1: u32) {
        // Dabs fully outside the canvas invert the rect (x0 > x1): they
        // paint nothing, so record nothing instead of poisoning consumers
        // with an underflowing region.
        if x0 > x1 || y0 > y1 {
            return;
        }
        let r = (
            x0.min(self.w.saturating_sub(1)),
            y0.min(self.h.saturating_sub(1)),
            x1.min(self.w.saturating_sub(1)),
            y1.min(self.h.saturating_sub(1)),
        );
        self.dirty = Some(match self.dirty {
            Some((a, b, c, d)) => (a.min(r.0), b.min(r.1), c.max(r.2), d.max(r.3)),
            None => r,
        });
    }

    pub fn active_layer_mut(&mut self) -> Option<&mut Layer> {
        self.layers.get_mut(self.active)
    }

    pub fn add_layer(&mut self, name: String) {
        self.layers.push(Layer::blank(name, self.w, self.h));
        self.active = self.layers.len() - 1;
        self.mark_full_dirty();
    }

    pub fn remove_layer(&mut self, index: usize) {
        if self.layers.len() <= 1 || index >= self.layers.len() {
            return;
        }
        self.layers.remove(index);
        self.active = self.active.min(self.layers.len() - 1);
        self.mark_full_dirty();
    }

    pub fn move_layer(&mut self, from: usize, to: usize) {
        if from >= self.layers.len() || to >= self.layers.len() || from == to {
            return;
        }
        let layer = self.layers.remove(from);
        self.layers.insert(to, layer);
        self.active = to;
        self.mark_full_dirty();
    }

    /// Snapshot the active layer for one undo step (call when a stroke starts).
    /// A new checkpoint discards the redo history.
    pub fn checkpoint(&mut self) {
        if let Some(entry) = self.snapshot() {
            if self.undo.len() >= 32 {
                self.undo.remove(0);
            }
            self.undo.push(entry);
            self.redo.clear();
        }
    }

    /// Copy of the active layer for the history stacks, if there is one.
    fn snapshot(&self) -> Option<UndoEntry> {
        self.layers.get(self.active).map(|layer| UndoEntry {
            layer: self.active,
            pixels: layer.pixels.clone(),
            mask: layer.mask.clone(),
        })
    }

    /// Restore the most recent snapshot. Returns false when history is empty.
    pub fn undo(&mut self) -> bool {
        if let Some(entry) = self.undo.pop() {
            if let Some(cur) = self.snapshot() {
                if self.redo.len() >= 32 {
                    self.redo.remove(0);
                }
                self.redo.push(cur);
                if let Some(layer) = self.layers.get_mut(entry.layer) {
                    layer.pixels = entry.pixels;
                    layer.mask = entry.mask;
                    self.active = entry.layer.min(self.layers.len().saturating_sub(1));
                    self.mark_full_dirty();
                    return true;
                }
            }
        }
        false
    }

    /// Re-apply the most recently undone snapshot. Returns false when empty.
    pub fn redo(&mut self) -> bool {
        if let Some(entry) = self.redo.pop() {
            if let Some(cur) = self.snapshot() {
                if self.undo.len() >= 32 {
                    self.undo.remove(0);
                }
                self.undo.push(cur);
                if let Some(layer) = self.layers.get_mut(entry.layer) {
                    layer.pixels = entry.pixels;
                    layer.mask = entry.mask;
                    self.active = entry.layer.min(self.layers.len().saturating_sub(1));
                    self.mark_full_dirty();
                    return true;
                }
            }
        }
        false
    }

    fn composite_pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let (mut r, mut g, mut b) =
            (self.background[0] as f32, self.background[1] as f32, self.background[2] as f32);
        let o = ((y * self.w + x) * 4) as usize;
        // Opaque content composited so far (layers only, not the page):
        // clipping masks intersect against it.
        let mut below_a = 0.0f32;
        for layer in &self.layers {
            if !layer.visible || layer.opacity <= 0.0 || o + 3 >= layer.pixels.len() {
                continue;
            }
            let mut a = layer.pixels[o + 3] as f32 / 255.0 * layer.opacity.clamp(0.0, 1.0);
            if let Some(m) = layer.mask.as_deref() {
                a *= m.get((y * self.w + x) as usize).copied().unwrap_or(255) as f32 / 255.0;
            }
            if layer.clip {
                a *= below_a;
            }
            if a <= 0.0 {
                continue;
            }
            let blend = layer.blend;
            r = blend.apply(layer.pixels[o] as f32, r) * a + r * (1.0 - a);
            g = blend.apply(layer.pixels[o + 1] as f32, g) * a + g * (1.0 - a);
            b = blend.apply(layer.pixels[o + 2] as f32, b) * a + b * (1.0 - a);
            below_a = a + below_a * (1.0 - a);
        }
        [
            r.round().clamp(0.0, 255.0) as u8,
            g.round().clamp(0.0, 255.0) as u8,
            b.round().clamp(0.0, 255.0) as u8,
            255,
        ]
    }

    /// Composite bottom-to-top over the background into RGBA bytes.
    pub fn flatten(&self) -> Vec<u8> {
        let n = (self.w * self.h) as usize;
        let mut out = vec![0u8; n * 4];
        for y in 0..self.h {
            for x in 0..self.w {
                let i = ((y * self.w + x) * 4) as usize;
                out[i..i + 4].copy_from_slice(&self.composite_pixel(x, y));
            }
        }
        out
    }

    /// Composite only the inclusive region `(x0, y0)..=(x1, y1)` (clamped),
    /// row-major RGBA, for partial texture uploads.
    pub fn flatten_region(&self, x0: u32, y0: u32, x1: u32, y1: u32) -> Vec<u8> {
        let x1 = x1.min(self.w.saturating_sub(1));
        let y1 = y1.min(self.h.saturating_sub(1));
        let (x0, y0) = (x0.min(x1), y0.min(y1));
        let mut out = Vec::with_capacity(((x1 - x0 + 1) * (y1 - y0 + 1) * 4) as usize);
        for y in y0..=y1 {
            for x in x0..=x1 {
                out.extend_from_slice(&self.composite_pixel(x, y));
            }
        }
        out
    }

    /// Shift a layer by whole pixels (clipped, transparent fill). Masks
    /// travel with their layer (shifted-in mask reveals, like new canvas).
    /// Caller checkpoints for undo first.
    pub fn translate_layer(&mut self, index: usize, dx: i32, dy: i32) {
        if (dx == 0 && dy == 0) || index >= self.layers.len() {
            return;
        }
        let (w, h) = (self.w as i32, self.h as i32);
        let mut next = vec![0u8; (self.w * self.h * 4) as usize];
        let src = self.layers[index].pixels.clone();
        for y in 0..h {
            for x in 0..w {
                let (sx, sy) = (x - dx, y - dy);
                if sx >= 0 && sx < w && sy >= 0 && sy < h {
                    let s = ((sy * w + sx) * 4) as usize;
                    let d = ((y * w + x) * 4) as usize;
                    next[d..d + 4].copy_from_slice(&src[s..s + 4]);
                }
            }
        }
        self.layers[index].pixels = next;
        if let Some(m) = self.layers[index].mask.clone() {
            let mut nm = vec![255u8; (self.w * self.h) as usize];
            for y in 0..h {
                for x in 0..w {
                    let (sx, sy) = (x - dx, y - dy);
                    if sx >= 0 && sx < w && sy >= 0 && sy < h {
                        nm[(y * w + x) as usize] = m[(sy * w + sx) as usize];
                    }
                }
            }
            self.layers[index].mask = Some(nm);
        }
        self.mark_full_dirty();
    }

    fn sample_bilinear(pixels: &[u8], w: u32, h: u32, x: f32, y: f32) -> [u8; 4] {
        let x0 = x.floor().clamp(0.0, w as f32 - 1.0) as u32;
        let y0 = y.floor().clamp(0.0, h as f32 - 1.0) as u32;
        let x1 = (x0 + 1).min(w - 1);
        let y1 = (y0 + 1).min(h - 1);
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let at = |x: u32, y: u32| -> [f32; 4] {
            let o = ((y * w + x) * 4) as usize;
            [
                pixels[o] as f32,
                pixels[o + 1] as f32,
                pixels[o + 2] as f32,
                pixels[o + 3] as f32,
            ]
        };
        let (a, b, c, d) = (at(x0, y0), at(x1, y0), at(x0, y1), at(x1, y1));
        let mut out = [0u8; 4];
        for i in 0..4 {
            let v = a[i] * (1.0 - fx) * (1.0 - fy) + b[i] * fx * (1.0 - fy) + c[i] * (1.0 - fx) * fy + d[i] * fx * fy;
            out[i] = v.round().clamp(0.0, 255.0) as u8;
        }
        out
    }

    /// Scale (about the canvas center) then rotate (degrees clockwise) a
    /// layer with bilinear sampling. Masks travel with their layer.
    /// Caller checkpoints for undo first.
    pub fn affine_layer(&mut self, index: usize, scale: f32, degrees: f32) {
        if index >= self.layers.len() || (scale - 1.0).abs() < 1e-6 && degrees.abs() < 1e-6 {
            return;
        }
        let (w, h) = (self.w, self.h);
        let src = self.layers[index].pixels.clone();
        self.layers[index].pixels = Self::affine_buffer(&src, w, h, scale, degrees);
        if let Some(m) = self.layers[index].mask.clone() {
            let rgba: Vec<u8> = m.iter().flat_map(|&v| [v, 0, 0, 255]).collect();
            let out = Self::affine_buffer(&rgba, w, h, scale, degrees);
            self.layers[index].mask = Some(out.chunks_exact(4).map(|p| p[0]).collect());
        }
        self.mark_full_dirty();
    }

    /// Shared scale-then-rotate resampler over RGBA bytes.
    fn affine_buffer(src: &[u8], w: u32, h: u32, scale: f32, degrees: f32) -> Vec<u8> {
        let (cx, cy) = (w as f32 * 0.5, h as f32 * 0.5);
        let theta = degrees.to_radians();
        let (sin, cos) = theta.sin_cos();
        let s = scale.max(0.01);
        let mut next = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                // Inverse map: destination -> source.
                let px = x as f32 - cx;
                let py = y as f32 - cy;
                let sx = (px * cos + py * sin) / s + cx;
                let sy = (-px * sin + py * cos) / s + cy;
                let o = ((y * w + x) * 4) as usize;
                if sx >= 0.0 && sx < w as f32 && sy >= 0.0 && sy < h as f32 {
                    next[o..o + 4].copy_from_slice(&Self::sample_bilinear(src, w, h, sx, sy));
                }
            }
        }
        next
    }

    /// Mirror a layer left-right in place. Masks travel with their layer.
    /// Caller checkpoints for undo first.
    pub fn flip_layer_horizontal(&mut self, index: usize) {
        if index >= self.layers.len() {
            return;
        }
        let (w, h) = (self.w, self.h);
        let src = self.layers[index].pixels.clone();
        let mut next = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let s = ((y * w + (w - 1 - x)) * 4) as usize;
                let d = ((y * w + x) * 4) as usize;
                next[d..d + 4].copy_from_slice(&src[s..s + 4]);
            }
        }
        self.layers[index].pixels = next;
        if let Some(m) = self.layers[index].mask.clone() {
            let mut nm = vec![0u8; (w * h) as usize];
            for y in 0..h {
                for x in 0..w {
                    nm[(y * w + x) as usize] = m[(y * w + (w - 1 - x)) as usize];
                }
            }
            self.layers[index].mask = Some(nm);
        }
        self.mark_full_dirty();
    }

    /// Flip a layer top-bottom in place. Masks travel with their layer.
    /// Caller checkpoints for undo first.
    pub fn flip_layer_vertical(&mut self, index: usize) {
        if index >= self.layers.len() {
            return;
        }
        let (w, h) = (self.w, self.h);
        let src = self.layers[index].pixels.clone();
        let mut next = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let s = (((h - 1 - y) * w + x) * 4) as usize;
                let d = ((y * w + x) * 4) as usize;
                next[d..d + 4].copy_from_slice(&src[s..s + 4]);
            }
        }
        self.layers[index].pixels = next;
        if let Some(m) = self.layers[index].mask.clone() {
            let mut nm = vec![0u8; (w * h) as usize];
            for y in 0..h {
                for x in 0..w {
                    nm[(y * w + x) as usize] = m[((h - 1 - y) * w + x) as usize];
                }
            }
            self.layers[index].mask = Some(nm);
        }
        self.mark_full_dirty();
    }

    /// Copy the active layer's sibling: insert a clone right above `index`
    /// and make it active. Caller checkpoints for undo first.
    pub fn duplicate_layer(&mut self, index: usize) {
        if index >= self.layers.len() {
            return;
        }
        let mut copy = self.layers[index].clone();
        copy.name = format!("{} copy", copy.name);
        self.layers.insert(index + 1, copy);
        self.active = index + 1;
        self.mark_full_dirty();
    }

    /// Composite layer `index` onto the layer below with its blend and
    /// opacity (invisible layers contribute nothing), then remove it.
    /// The source mask is baked in; a clipped source is intersected with
    /// the destination layer's own content (its mask included).
    /// Returns false on the bottom layer or a bad index.
    /// Caller checkpoints for undo first.
    pub fn merge_down(&mut self, index: usize) -> bool {
        if index == 0 || index >= self.layers.len() {
            return false;
        }
        let src = self.layers[index].clone();
        let sa_base = if src.visible { src.opacity.clamp(0.0, 1.0) } else { 0.0 };
        let dst = &mut self.layers[index - 1];
        for o in (0..dst.pixels.len()).step_by(4) {
            if o + 3 >= src.pixels.len() {
                break;
            }
            let mut sa = sa_base * src.pixels[o + 3] as f32 / 255.0;
            if let Some(m) = src.mask.as_deref() {
                sa *= m.get(o / 4).copied().unwrap_or(255) as f32 / 255.0;
            }
            if src.clip {
                let dst_a = dst.pixels[o + 3] as f32 / 255.0;
                let dst_m = dst.mask.as_deref().and_then(|m| m.get(o / 4).copied()).unwrap_or(255) as f32 / 255.0;
                sa *= dst_a * dst_m;
            }
            let da = dst.pixels[o + 3] as f32 / 255.0;
            let out_a = sa + da * (1.0 - sa);
            if out_a > 0.0 {
                for ch in 0..3 {
                    let blended = src.blend.apply(src.pixels[o + ch] as f32, dst.pixels[o + ch] as f32);
                    dst.pixels[o + ch] =
                        ((blended * sa + dst.pixels[o + ch] as f32 * da * (1.0 - sa)) / out_a).round() as u8;
                }
                dst.pixels[o + 3] = (out_a * 255.0).round() as u8;
            }
        }
        self.layers.remove(index);
        self.active = index - 1;
        self.mark_full_dirty();
        true
    }

    /// Composited RGB at `(x, y)` for the eyedropper, if inside the canvas.
    pub fn sample(&self, x: u32, y: u32) -> Option<[u8; 3]> {
        if x >= self.w || y >= self.h {
            return None;
        }
        let px = self.composite_pixel(x, y);
        Some([px[0], px[1], px[2]])
    }

    /// Clear both the rectangular selection and the lasso mask.
    pub fn clear_selection(&mut self) {
        self.selection = None;
        self.selection_mask = None;
    }

    /// Attach a fully-revealing (white) mask to a layer. Structural, like
    /// add/remove layer: not undoable itself, but every mask stroke after
    /// it is (snapshots carry the mask).
    pub fn add_mask(&mut self, index: usize) {
        if index >= self.layers.len() {
            return;
        }
        let n = (self.w * self.h) as usize;
        let slot = &mut self.layers[index].mask;
        if slot.as_ref().map(|m| m.len() != n).unwrap_or(true) {
            *slot = Some(vec![255u8; n]);
        }
        self.mark_full_dirty();
    }

    /// Remove a layer's mask, revealing the whole layer again.
    pub fn clear_mask(&mut self, index: usize) {
        if index >= self.layers.len() {
            return;
        }
        self.layers[index].mask = None;
        self.mark_full_dirty();
    }

    /// The active layer's mask as opaque grayscale RGBA for the mask viewer.
    /// `None` when there is no (valid) mask.
    pub fn mask_view(&self) -> Option<Vec<u8>> {
        let m = self.layers.get(self.active)?.mask.as_deref()?;
        if m.len() != (self.w * self.h) as usize {
            return None;
        }
        let mut out = Vec::with_capacity(m.len() * 4);
        for &v in m {
            out.extend_from_slice(&[v, v, v, 255]);
        }
        Some(out)
    }

    /// True when `(px, py)` is inside the active selection (rectangular
    /// and/or lasso). With no selection active every pixel counts.
    pub fn pixel_selected(&self, px: u32, py: u32) -> bool {
        if px >= self.w || py >= self.h {
            return false;
        }
        if let Some((sx0, sy0, sx1, sy1)) = self.selection {
            if px < sx0 || px > sx1 || py < sy0 || py > sy1 {
                return false;
            }
        }
        if let Some(mask) = self.selection_mask.as_deref() {
            if mask.get((py * self.w + px) as usize).copied().unwrap_or(0) == 0 {
                return false;
            }
        }
        true
    }

    /// Build a lasso mask from a freehand loop (canvas float coordinates).
    /// Sets the rectangular selection to the loop's bounding box and returns
    /// it, or clears the mask and returns `None` for degenerate loops.
    pub fn set_lasso(&mut self, points: &[(f32, f32)]) -> Option<(u32, u32, u32, u32)> {
        if points.len() < 3 || self.w == 0 || self.h == 0 {
            self.selection_mask = None;
            return None;
        }
        let (w, h) = (self.w, self.h);
        let mut fx0 = f32::INFINITY;
        let mut fy0 = f32::INFINITY;
        let mut fx1 = f32::NEG_INFINITY;
        let mut fy1 = f32::NEG_INFINITY;
        for &(x, y) in points {
            fx0 = fx0.min(x);
            fy0 = fy0.min(y);
            fx1 = fx1.max(x);
            fy1 = fy1.max(y);
        }
        let (x0, y0) = (fx0.floor().max(0.0) as u32, fy0.floor().max(0.0) as u32);
        let (x1, y1) = (fx1.ceil().min(w as f32 - 1.0).max(0.0) as u32, fy1.ceil().min(h as f32 - 1.0).max(0.0) as u32);
        if x1 <= x0 || y1 <= y0 {
            self.selection_mask = None;
            return None;
        }
        // Even-odd scanline fill of the polygon.
        let n = points.len();
        let mut mask = vec![0u8; (w * h) as usize];
        for y in y0..=y1 {
            let yc = y as f32 + 0.5;
            let mut xs = Vec::new();
            for i in 0..n {
                let (ax, ay) = points[i];
                let (bx, by) = points[(i + 1) % n];
                if (ay > yc) != (by > yc) {
                    xs.push(ax + (yc - ay) / (by - ay) * (bx - ax));
                }
            }
            xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            for pair in xs.chunks_exact(2) {
                let xa = (pair[0] - 0.5).ceil() as i32;
                let xb = (pair[1] - 0.5 - 1e-6).floor() as i32;
                for x in xa.max(x0 as i32)..=xb.min(x1 as i32) {
                    if x >= 0 {
                        mask[(y * w + x as u32) as usize] = 1;
                    }
                }
            }
        }
        if mask.iter().all(|&b| b == 0) {
            self.selection_mask = None;
            return None;
        }
        self.selection = Some((x0, y0, x1, y1));
        self.selection_mask = Some(mask);
        Some((x0, y0, x1, y1))
    }

    /// Pixels covered by the active selection (lasso pixel count, else the
    /// rectangle area, else 0 when nothing is selected).
    pub fn selection_pixel_count(&self) -> usize {        if let Some(mask) = self.selection_mask.as_deref() {
            mask.iter().filter(|&&b| b != 0).count()
        } else if let Some((x0, y0, x1, y1)) = self.selection {
            ((x1 - x0 + 1) * (y1 - y0 + 1)) as usize
        } else {
            0
        }
    }

    /// Separable sliding-window box blur (all channels), O(w*h).
    fn box_blur(src: &[u8], w: u32, h: u32, r: i32) -> Vec<u8> {
        let (w, h) = (w as i32, h as i32);
        let wu = w as u32;
        let window = (2 * r + 1) as u32;
        let px = |buf: &[u8], x: i32, y: i32, ch: usize| -> u32 {
            let x = x.clamp(0, w - 1) as u32;
            let y = y.clamp(0, h - 1) as u32;
            buf[((y * wu + x) * 4) as usize + ch] as u32
        };
        // Horizontal pass: src -> tmp.
        let mut tmp = vec![0u8; src.len()];
        for y in 0..h {
            let mut sum = [0u32; 4];
            for dx in -r..=r {
                for ch in 0..4 {
                    sum[ch] += px(src, dx, y, ch);
                }
            }
            for x in 0..w {
                for ch in 0..4 {
                    tmp[((y as u32 * wu + x as u32) * 4) as usize + ch] = (sum[ch] / window) as u8;
                }
                for ch in 0..4 {
                    sum[ch] += px(src, x + r + 1, y, ch);
                    sum[ch] -= px(src, x - r, y, ch);
                }
            }
        }
        // Vertical pass: tmp -> out.
        let mut out = vec![0u8; src.len()];
        for x in 0..w {
            let mut sum = [0u32; 4];
            for dy in -r..=r {
                for ch in 0..4 {
                    sum[ch] += px(&tmp, x, dy, ch);
                }
            }
            for y in 0..h {
                for ch in 0..4 {
                    out[((y as u32 * wu + x as u32) * 4) as usize + ch] = (sum[ch] / window) as u8;
                }
                for ch in 0..4 {
                    sum[ch] += px(&tmp, x, y + r + 1, ch);
                    sum[ch] -= px(&tmp, x, y - r, ch);
                }
            }
        }
        out
    }

    /// Brightness/contrast on a layer: `brightness` -255..=255 is added,
    /// `contrast` 0.0..=3.0 scales around mid-gray (1.0 keeps contrast).
    /// RGB only; alpha is untouched. Honors the active selection.
    /// Caller checkpoints for undo first.
    pub fn adjust_tone(&mut self, index: usize, brightness: i32, contrast: f32) {
        if index >= self.layers.len()
            || (brightness == 0 && (contrast - 1.0).abs() < f32::EPSILON)
        {
            return;
        }
        let (w, h) = (self.w, self.h);
        let sel = self.selection;
        let mask = self.selection_mask.as_deref();
        let pixels = &mut self.layers[index].pixels;
        for y in 0..h {
            for x in 0..w {
                if let Some((a, b, c, d)) = sel {
                    if x < a || x > c || y < b || y > d {
                        continue;
                    }
                }
                if let Some(m) = mask {
                    if m[(y * w + x) as usize] == 0 {
                        continue;
                    }
                }
                let o = ((y * w + x) * 4) as usize;
                if o + 2 >= pixels.len() {
                    continue;
                }
                for ch in 0..3 {
                    let v = (pixels[o + ch] as f32 - 128.0) * contrast + 128.0 + brightness as f32;
                    pixels[o + ch] = v.round().clamp(0.0, 255.0) as u8;
                }
            }
        }
        self.mark_full_dirty();
    }

    /// Box blur on a layer, `radius` 1..=25 (0 is a no-op). Honors the
    /// active selection for writes. Caller checkpoints for undo first.
    pub fn blur_layer(&mut self, index: usize, radius: u32) {
        if index >= self.layers.len() || radius == 0 {
            return;
        }
        let (w, h) = (self.w, self.h);
        let blurred = Self::box_blur(
            &self.layers[index].pixels.clone(),
            w,
            h,
            radius.min(25) as i32,
        );
        let sel = self.selection;
        let mask = self.selection_mask.as_deref();
        let pixels = &mut self.layers[index].pixels;
        for y in 0..h {
            for x in 0..w {
                let selected = match sel {
                    Some((a, b, c, d)) if x < a || x > c || y < b || y > d => false,
                    _ => true,
                } && match mask {
                    Some(m) => m[(y * w + x) as usize] != 0,
                    None => true,
                };
                if selected {
                    let o = ((y * w + x) * 4) as usize;
                    pixels[o..o + 4].copy_from_slice(&blurred[o..o + 4]);
                }
            }
        }
        self.mark_full_dirty();
    }

    /// Unsharp mask on a layer: `amount` 0.0..=2.0 mixes the high-frequency
    /// detail back in (0 is a no-op). Honors the active selection.
    /// Caller checkpoints for undo first.
    pub fn sharpen_layer(&mut self, index: usize, amount: f32) {
        if index >= self.layers.len() || amount <= 0.0 {
            return;
        }
        let amount = amount.min(2.0);
        let (w, h) = (self.w, self.h);
        let src = self.layers[index].pixels.clone();
        let blurred = Self::box_blur(&src, w, h, 2);
        let sel = self.selection;
        let mask = self.selection_mask.as_deref();
        let pixels = &mut self.layers[index].pixels;
        for y in 0..h {
            for x in 0..w {
                let selected = match sel {
                    Some((a, b, c, d)) if x < a || x > c || y < b || y > d => false,
                    _ => true,
                } && match mask {
                    Some(m) => m[(y * w + x) as usize] != 0,
                    None => true,
                };
                if !selected {
                    continue;
                }
                let o = ((y * w + x) * 4) as usize;
                for ch in 0..3 {
                    let v = pixels[o + ch] as f32
                        + (pixels[o + ch] as f32 - blurred[o + ch] as f32) * amount;
                    pixels[o + ch] = v.round().clamp(0.0, 255.0) as u8;
                }
            }
        }
        self.mark_full_dirty();
    }

    /// Save the composite as PNG.
    pub fn save_png(&self, path: &std::path::Path) -> Result<(), String> {
        image::save_buffer(path, &self.flatten(), self.w, self.h, image::ColorType::Rgba8)
            .map_err(|e| e.to_string())
    }

    /// Load an image file flattened onto a fresh layer.
    pub fn load_background(&mut self, path: &std::path::Path) -> Result<(), String> {
        let img = image::open(path).map_err(|e| e.to_string())?.to_rgba8();
        let (w, h) = (img.width(), img.height());
        self.w = w;
        self.h = h;
        let mut layer = Layer::blank("Imported".to_string(), w, h);
        layer.pixels.copy_from_slice(img.as_raw());
        self.layers = vec![layer];
        self.active = 0;
        self.selection = None;
        self.selection_mask = None;
        self.undo.clear();
        self.redo.clear();
        Ok(())
    }

    /// Save the full project (layers, opacity, blend, background, symmetry)
    /// to a `.procraft` file: magic + little-endian records, zlib-compressed
    /// pixels. Version 5 adds per-layer masks, clipping flags and six more
    /// blend modes (v1-v4 files still open; old app versions cleanly refuse
    /// v5 files at the magic).
    pub fn save_project(&self, path: &std::path::Path) -> Result<(), String> {
        use std::io::Write;
        let mut f = std::fs::File::create(path).map_err(|e| e.to_string())?;
        f.write_all(b"PCRAFT05").map_err(|e| e.to_string())?;
        f.write_all(&self.w.to_le_bytes()).map_err(|e| e.to_string())?;
        f.write_all(&self.h.to_le_bytes()).map_err(|e| e.to_string())?;
        f.write_all(&self.background).map_err(|e| e.to_string())?;
        f.write_all(&(self.layers.len() as u32).to_le_bytes()).map_err(|e| e.to_string())?;
        f.write_all(&(self.active as u32).to_le_bytes()).map_err(|e| e.to_string())?;
        let symmetry = match self.symmetry {
            SymmetryMode::Off => 0u8,
            SymmetryMode::Horizontal => 1u8,
            SymmetryMode::Vertical => 2u8,
            SymmetryMode::Both => 3u8,
            SymmetryMode::Kaleidoscope => 4u8,
        };
        f.write_all(&[symmetry]).map_err(|e| e.to_string())?;
        f.write_all(&[self.symmetry_segments.clamp(2, 16)]).map_err(|e| e.to_string())?;
        for layer in &self.layers {
            let name = layer.name.as_bytes();
            f.write_all(&(name.len() as u32).to_le_bytes()).map_err(|e| e.to_string())?;
            f.write_all(name).map_err(|e| e.to_string())?;
            f.write_all(&[u8::from(layer.visible)]).map_err(|e| e.to_string())?;
            f.write_all(&layer.opacity.to_le_bytes()).map_err(|e| e.to_string())?;
            let blend = match layer.blend {
                BlendMode::Normal => 0u8,
                BlendMode::Multiply => 1u8,
                BlendMode::Screen => 2u8,
                BlendMode::Overlay => 3u8,
                BlendMode::Darken => 4u8,
                BlendMode::Lighten => 5u8,
                BlendMode::Difference => 6u8,
                BlendMode::Exclusion => 7u8,
                BlendMode::SoftLight => 8u8,
                BlendMode::HardLight => 9u8,
            };
            f.write_all(&[blend]).map_err(|e| e.to_string())?;
            f.write_all(&[u8::from(layer.clip)]).map_err(|e| e.to_string())?;
            let expect = (self.w * self.h * 4) as usize;
            if layer.pixels.len() != expect {
                return Err("layer has wrong pixel size".to_string());
            }
            let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
            enc.write_all(&layer.pixels).map_err(|e| e.to_string())?;
            let packed = enc.finish().map_err(|e| e.to_string())?;
            f.write_all(&(packed.len() as u32).to_le_bytes()).map_err(|e| e.to_string())?;
            f.write_all(&packed).map_err(|e| e.to_string())?;
            // Optional reveal mask, zlib-compressed like the pixels.
            match layer.mask.as_deref() {
                Some(m) if m.len() == (self.w * self.h) as usize => {
                    f.write_all(&[1u8]).map_err(|e| e.to_string())?;
                    let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
                    enc.write_all(m).map_err(|e| e.to_string())?;
                    let packed = enc.finish().map_err(|e| e.to_string())?;
                    f.write_all(&(packed.len() as u32).to_le_bytes()).map_err(|e| e.to_string())?;
                    f.write_all(&packed).map_err(|e| e.to_string())?;
                }
                _ => {
                    f.write_all(&[0u8]).map_err(|e| e.to_string())?;
                }
            }
        }
        Ok(())
    }

    /// Load a `.procraft` file, replacing the current document.
    pub fn open_project(path: &std::path::Path) -> Result<Document, String> {
        use std::io::Read;
        let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).map_err(|e| e.to_string())?;
        let mut cur = &buf[..];
        let mut take = |n: usize| -> Result<&[u8], String> {
            if cur.len() < n {
                return Err("project file is truncated".to_string());
            }
            let (head, tail) = cur.split_at(n);
            cur = tail;
            Ok(head)
        };
        let magic = take(8)?.to_vec();
        let v5 = magic == b"PCRAFT05";
        let v4 = v5 || magic == b"PCRAFT04";
        let v3 = v4 || magic == b"PCRAFT03";
        let v2 = v3 || magic == b"PCRAFT02";
        if magic != b"PCRAFT01" && !v2 {
            return Err("not a ProCraft project file".to_string());
        }
        let w = u32::from_le_bytes(take(4)?[..4].try_into().map_err(|_| "bad width".to_string())?);
        let h = u32::from_le_bytes(take(4)?[..4].try_into().map_err(|_| "bad height".to_string())?);
        if w == 0 || h == 0 || w > 16384 || h > 16384 {
            return Err("project has bad dimensions".to_string());
        }
        let bg: [u8; 4] = take(4)?[..4].try_into().map_err(|_| "bad background".to_string())?;
        let count = u32::from_le_bytes(take(4)?[..4].try_into().map_err(|_| "bad layer count".to_string())?);
        if count == 0 || count > 256 {
            return Err("project has bad layer count".to_string());
        }
        let active = u32::from_le_bytes(take(4)?[..4].try_into().map_err(|_| "bad active layer".to_string())?) as usize;
        let symmetry = if v2 {
            match take(1)?[0] {
                1 => SymmetryMode::Horizontal,
                2 => SymmetryMode::Vertical,
                3 => SymmetryMode::Both,
                4 if v3 => SymmetryMode::Kaleidoscope,
                _ => SymmetryMode::Off,
            }
        } else {
            SymmetryMode::Off
        };
        let symmetry_segments = if v3 {
            let s = take(1)?[0];
            if (2..=16).contains(&s) { s } else { 6 }
        } else {
            6
        };
        let mut layers = Vec::new();
        for _ in 0..count {
            let name_len = u32::from_le_bytes(take(4)?[..4].try_into().map_err(|_| "bad name".to_string())?) as usize;
            if name_len > 512 {
                return Err("project has bad layer name".to_string());
            }
            let name = String::from_utf8(take(name_len)?.to_vec()).map_err(|_| "bad layer name".to_string())?;
            let visible = take(1)?[0] != 0;
            let opacity = f32::from_le_bytes(take(4)?[..4].try_into().map_err(|_| "bad opacity".to_string())?).clamp(0.0, 1.0);
            let blend = match take(1)?[0] {
                1 => BlendMode::Multiply,
                2 => BlendMode::Screen,
                3 => BlendMode::Overlay,
                4 if v5 => BlendMode::Darken,
                5 if v5 => BlendMode::Lighten,
                6 if v5 => BlendMode::Difference,
                7 if v5 => BlendMode::Exclusion,
                8 if v5 => BlendMode::SoftLight,
                9 if v5 => BlendMode::HardLight,
                _ => BlendMode::Normal,
            };
            // v5 stores the clipping flag right after the blend byte.
            let clip = if v5 { take(1)?[0] != 0 } else { false };
            let expect = (w * h * 4) as usize;
            let pixels = if v4 {
                let comp_len = u32::from_le_bytes(
                    take(4)?[..4].try_into().map_err(|_| "bad compressed size".to_string())?,
                ) as usize;
                if comp_len == 0 || comp_len > expect.saturating_add(10 * 1024 * 1024) {
                    return Err("project has bad compressed size".to_string());
                }
                let packed = take(comp_len)?.to_vec();
                let mut dec = flate2::read::ZlibDecoder::new(&packed[..]);
                let mut raw = Vec::with_capacity(expect);
                use std::io::Read;
                dec.read_to_end(&mut raw).map_err(|_| "project has corrupt pixels".to_string())?;
                if raw.len() != expect {
                    return Err("project has corrupt pixels".to_string());
                }
                raw
            } else {
                take(expect)?.to_vec()
            };
            // v5 appends an optional zlib-compressed reveal mask per layer.
            let mask = if v5 {
                let flag = take(1)?[0];
                if flag == 0 {
                    None
                } else {
                    let want = (w * h) as usize;
                    let comp_len = u32::from_le_bytes(
                        take(4)?[..4].try_into().map_err(|_| "bad mask size".to_string())?,
                    ) as usize;
                    if comp_len == 0 || comp_len > want.saturating_add(10 * 1024 * 1024) {
                        return Err("project has bad mask size".to_string());
                    }
                    let packed = take(comp_len)?.to_vec();
                    let mut dec = flate2::read::ZlibDecoder::new(&packed[..]);
                    let mut raw = Vec::with_capacity(want);
                    use std::io::Read;
                    dec.read_to_end(&mut raw).map_err(|_| "project has corrupt mask".to_string())?;
                    if raw.len() != want {
                        return Err("project has corrupt mask".to_string());
                    }
                    Some(raw)
                }
            } else {
                None
            };
            layers.push(Layer { name, visible, opacity, blend, pixels, mask, clip });
        }
        if !cur.is_empty() {
            return Err("project has trailing data".to_string());
        }
        Ok(Document {
            w,
            h,
            layers,
            active: active.min(count as usize - 1),
            background: bg,
            selection: None,
            selection_mask: None,
            symmetry,
            symmetry_segments,
            paint_target: PaintTarget::Color,
            dirty: None,
            undo: Vec::new(),
            redo: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opaque_top_layer_wins() {
        let mut doc = Document::new(2, 1);
        // Bottom layer: solid red.
        {
            let l = &mut doc.layers[0];
            l.pixels = vec![255, 0, 0, 255, 255, 0, 0, 255];
        }
        doc.add_layer("top".to_string());
        // Top layer, first pixel green, second transparent.
        {
            let l = &mut doc.layers[1];
            l.pixels = vec![0, 255, 0, 255, 0, 0, 0, 0];
        }
        let flat = doc.flatten();
        assert_eq!(&flat[0..4], &[0, 255, 0, 255]);
        assert_eq!(&flat[4..8], &[255, 0, 0, 255]);
    }

    #[test]
    fn undo_restores_pixels() {
        let mut doc = Document::new(1, 1);
        doc.checkpoint();
        doc.layers[0].pixels = vec![9, 9, 9, 9];
        assert!(doc.undo());
        assert_eq!(doc.layers[0].pixels, vec![0, 0, 0, 0]);
        assert!(!doc.undo());
    }

    #[test]
    fn redo_restores_undone_work() {
        let mut doc = Document::new(1, 1);
        assert!(!doc.redo());
        doc.checkpoint();
        doc.layers[0].pixels = vec![9, 9, 9, 9];
        assert!(doc.undo());
        assert_eq!(doc.layers[0].pixels, vec![0, 0, 0, 0]);
        assert!(doc.redo());
        assert_eq!(doc.layers[0].pixels, vec![9, 9, 9, 9]);
        assert!(!doc.redo());
        // Undo still works after a redo.
        assert!(doc.undo());
        assert_eq!(doc.layers[0].pixels, vec![0, 0, 0, 0]);
    }

    #[test]
    fn new_blend_modes_do_the_math() {
        use BlendMode::*;
        let b = BlendMode::Normal;
        assert_eq!(b.apply(10.0, 20.0), 10.0);
        assert_eq!(Darken.apply(10.0, 20.0), 10.0);
        assert_eq!(Lighten.apply(10.0, 20.0), 20.0);
        assert_eq!(Difference.apply(10.0, 20.0), 10.0);
        assert_eq!(Difference.apply(200.0, 50.0), 150.0);
        // Exclusion of identical mid-grays dips below either input.
        let e = Exclusion.apply(128.0, 128.0);
        assert!(e < 128.0 && e > 100.0, "{e}");
        // Soft light keeps mid-gray near itself, hard light pushes contrast.
        let s = SoftLight.apply(128.0, 100.0);
        assert!((s - 100.0).abs() < 12.0, "{s}");
        assert_eq!(HardLight.apply(0.0, 200.0), 0.0);
        assert_eq!(HardLight.apply(255.0, 200.0), 255.0);
        assert_eq!(BlendMode::ALL.len(), 10);
    }

    #[test]
    fn mask_multiplies_layer_alpha() {
        let mut doc = Document::new(4, 1);
        doc.background = [255, 255, 255, 255];
        doc.layers[0].pixels = vec![255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255];
        doc.add_mask(0);
        // Conceal the left half.
        if let Some(m) = doc.layers[0].mask.as_mut() {
            m[0] = 0;
            m[1] = 0;
        }
        let flat = doc.flatten();
        assert_eq!(&flat[0..4], &[255, 255, 255, 255]);
        assert_eq!(&flat[8..12], &[255, 0, 0, 255]);
        // Clearing the mask reveals everything again.
        doc.clear_mask(0);
        assert!(doc.layers[0].mask.is_none());
        let flat = doc.flatten();
        assert_eq!(&flat[0..4], &[255, 0, 0, 255]);
    }

    #[test]
    fn clip_confines_layer_to_content_below() {
        let mut doc = Document::new(4, 1);
        doc.background = [255, 255, 255, 255];
        // Base: single red dot at x=1.
        doc.layers[0].pixels[((1 * 4) + 3) as usize] = 255;
        doc.layers[0].pixels[((1 * 4)) as usize] = 255;
        doc.add_layer("top".to_string());
        doc.layers[1].pixels = vec![0, 255, 0, 255].repeat(4);
        doc.layers[1].clip = true;
        let flat = doc.flatten();
        assert_eq!(&flat[0..4], &[255, 255, 255, 255]);
        assert_eq!(&flat[4..8], &[0, 255, 0, 255]);
        assert_eq!(&flat[8..12], &[255, 255, 255, 255]);
        // Unclipped, the green covers everything.
        doc.layers[1].clip = false;
        let flat = doc.flatten();
        assert_eq!(&flat[0..4], &[0, 255, 0, 255]);
    }

    #[test]
    fn mask_painting_uses_luminance_and_undoes() {
        use crate::brush::{Brush, Color};
        let mut doc = Document::new(16, 16);
        doc.paint_target = PaintTarget::Mask;
        let brush = Brush { size: 8.0, softness: 0.0, ..Default::default() };
        assert!(doc.layers[0].mask.is_none());
        doc.checkpoint();
        doc.dab(&brush, Color::rgb(0, 0, 0), 8.0, 8.0);
        // Black paint concealed the middle; mask auto-materialized.
        let m = doc.layers[0].mask.as_ref().expect("mask created");
        assert!(m.iter().any(|&b| b < 255));
        // Eraser reveals back toward white.
        let before: Vec<u8> = m.clone();
        let eraser = Brush { eraser: true, ..brush };
        doc.dab(&eraser, Color::rgb(0, 0, 0), 8.0, 8.0);
        let after = doc.layers[0].mask.as_ref().unwrap();
        assert!(after.iter().zip(before.iter()).all(|(&a, &b)| a >= b));
        assert!(after.iter().any(|&b| b > 0));
        // Undo restores the pre-stroke state (no mask at all).
        assert!(doc.undo());
        assert!(doc.layers[0].mask.is_none());
        assert!(doc.redo());
        assert!(doc.layers[0].mask.is_some());
    }

    #[test]
    fn v5_roundtrip_keeps_masks_clips_and_blends() {
        let mut doc = Document::new(4, 2);
        doc.layers[0].blend = BlendMode::Difference;
        doc.layers[0].pixels = vec![200; 4 * 2 * 4];
        doc.add_mask(0);
        doc.layers[0].mask.as_mut().unwrap()[3] = 0;
        doc.add_layer("clipped".to_string());
        doc.layers[1].blend = BlendMode::SoftLight;
        doc.layers[1].clip = true;
        doc.layers[1].pixels = vec![7; 4 * 2 * 4];
        let dir = std::env::temp_dir();
        let path = dir.join("procraft-v5.pcraft");
        doc.save_project(&path).unwrap();
        let back = Document::open_project(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(back.layers[0].blend, BlendMode::Difference);
        assert_eq!(back.layers[0].mask.as_ref().unwrap()[3], 0);
        assert_eq!(back.layers[1].blend, BlendMode::SoftLight);
        assert!(back.layers[1].clip);
        assert_eq!(back.layers[1].pixels, vec![7; 4 * 2 * 4]);
    }

    #[test]
    fn v4_files_open_without_masks_or_clips() {
        // Minimal v4 file: zlib pixels, blend byte, no clip/mask fields.
        let dir = std::env::temp_dir();
        let path = dir.join("procraft-v4-nomask.pcraft");
        let mut buf = Vec::new();
        buf.extend_from_slice(b"PCRAFT04");
        buf.extend_from_slice(&2u32.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&[255, 255, 255, 255]);
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&[0]);
        buf.extend_from_slice(&[6]);
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(b"L");
        buf.extend_from_slice(&[1]);
        buf.extend_from_slice(&1.0f32.to_le_bytes());
        buf.extend_from_slice(&[0]);
        let raw = vec![9u8; 2 * 1 * 4];
        let packed = {
            use std::io::Write;
            let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
            enc.write_all(&raw).unwrap();
            enc.finish().unwrap()
        };
        buf.extend_from_slice(&(packed.len() as u32).to_le_bytes());
        buf.extend_from_slice(&packed);
        std::fs::write(&path, &buf).unwrap();
        let back = Document::open_project(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(back.layers[0].pixels, raw);
        assert!(back.layers[0].mask.is_none());
        assert!(!back.layers[0].clip);
    }

    #[test]
    fn merge_bakes_mask_and_clip() {
        let mut doc = Document::new(2, 1);
        // Below: blue, left pixel only.
        doc.layers[0].pixels = vec![0, 0, 255, 255, 0, 0, 0, 0];
        doc.add_layer("top".to_string());
        doc.layers[1].pixels = vec![255, 0, 0, 255, 255, 0, 0, 255];
        doc.add_mask(1);
        // Mask hides the right pixel of the top layer.
        doc.layers[1].mask.as_mut().unwrap()[1] = 0;
        doc.layers[1].clip = true;
        assert!(doc.merge_down(1));
        // Left: red over blue = red. Right: top hidden by mask AND outside
        // the blue content, so it stays transparent.
        assert_eq!(&doc.layers[0].pixels[0..4], &[255, 0, 0, 255]);
        assert_eq!(&doc.layers[0].pixels[4..8], &[0, 0, 0, 0]);
    }

    #[test]
    fn transforms_carry_the_mask() {
        let mut doc = Document::new(4, 1);
        doc.add_mask(0);
        doc.layers[0].mask.as_mut().unwrap()[0] = 0;
        doc.translate_layer(0, 1, 0);
        assert_eq!(doc.layers[0].mask.as_ref().unwrap()[1], 0);
        doc.flip_layer_horizontal(0);
        assert_eq!(doc.layers[0].mask.as_ref().unwrap()[2], 0);
        // Affine keeps dimensions and mask presence.
        doc.affine_layer(0, 1.0, 90.0);
        assert!(doc.layers[0].mask.is_some());
        assert_eq!(doc.layers[0].mask.as_ref().unwrap().len(), 4);
    }

    #[test]
    fn new_stroke_clears_redo() {
        let mut doc = Document::new(1, 1);
        doc.checkpoint();
        doc.layers[0].pixels = vec![1, 1, 1, 1];
        assert!(doc.undo());
        doc.checkpoint();
        doc.layers[0].pixels = vec![2, 2, 2, 2];
        assert!(!doc.redo());
    }

    #[test]
    fn multiply_blends_channels() {
        let mut doc = Document::new(1, 1);
        doc.background = [255, 255, 255, 255];
        doc.layers[0].pixels = vec![128, 200, 64, 255];
        doc.layers[0].blend = BlendMode::Multiply;
        let flat = doc.flatten();
        assert_eq!(&flat[0..4], &[128, 200, 64, 255]);
        // Screen over black returns the source unchanged.
        doc.background = [0, 0, 0, 255];
        doc.layers[0].blend = BlendMode::Screen;
        let flat = doc.flatten();
        assert_eq!(&flat[0..4], &[128, 200, 64, 255]);
        // Screen over mid-gray lightens past the source.
        doc.background = [128, 128, 128, 255];
        let flat = doc.flatten();
        assert!(flat[0] > 128 && flat[1] > 200 && flat[2] > 64, "{flat:?}");
    }

    #[test]
    fn project_roundtrip_keeps_everything() {
        let mut doc = Document::new(4, 3);
        doc.background = [10, 20, 30, 255];
        doc.layers[0].pixels = vec![7; 4 * 3 * 4];
        doc.layers[0].opacity = 0.5;
        doc.layers[0].blend = BlendMode::Overlay;
        doc.symmetry = SymmetryMode::Both;
        doc.add_layer("second".to_string());
        doc.layers[1].visible = false;
        let dir = std::env::temp_dir();
        let path = dir.join("procraft-roundtrip.pcraft");
        doc.save_project(&path).unwrap();
        let back = Document::open_project(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!((back.w, back.h), (4, 3));
        assert_eq!(back.background, [10, 20, 30, 255]);
        assert_eq!(back.layers.len(), 2);
        assert_eq!(back.layers[0].opacity, 0.5);
        assert_eq!(back.layers[0].blend, BlendMode::Overlay);
        assert_eq!(back.layers[0].pixels, vec![7; 4 * 3 * 4]);
        assert_eq!(back.layers[1].name, "second");
        assert!(!back.layers[1].visible);
        assert_eq!(back.active, 1);
        assert_eq!(back.symmetry, SymmetryMode::Both);
        assert!(Document::open_project(&dir.join("missing.pcraft")).is_err());
    }

    #[test]
    fn v1_project_still_opens_with_symmetry_off() {
        // Hand-build a minimal version-1 file (no symmetry byte).
        let dir = std::env::temp_dir();
        let path = dir.join("procraft-v1-compat.pcraft");
        let mut buf = Vec::new();
        buf.extend_from_slice(b"PCRAFT01");
        buf.extend_from_slice(&2u32.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&[255, 255, 255, 255]);
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(b"L");
        buf.extend_from_slice(&[1]);
        buf.extend_from_slice(&1.0f32.to_le_bytes());
        buf.extend_from_slice(&[0]);
        buf.extend_from_slice(&vec![9u8; 2 * 1 * 4]);
        std::fs::write(&path, &buf).unwrap();
        let back = Document::open_project(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!((back.w, back.h), (2, 1));
        assert_eq!(back.symmetry, SymmetryMode::Off);
        assert_eq!(back.layers[0].pixels, vec![9u8; 8]);
    }

    #[test]
    fn flip_horizontal_mirrors_pixels() {
        let mut doc = Document::new(4, 1);
        doc.layers[0].pixels[0..4].copy_from_slice(&[255, 0, 0, 255]);
        doc.flip_layer_horizontal(0);
        assert_eq!(&doc.layers[0].pixels[12..16], &[255, 0, 0, 255]);
        assert_eq!(&doc.layers[0].pixels[0..4], &[0, 0, 0, 0]);
        // Double flip restores.
        doc.flip_layer_horizontal(0);
        assert_eq!(&doc.layers[0].pixels[0..4], &[255, 0, 0, 255]);
    }

    #[test]
    fn flip_vertical_mirrors_pixels() {
        let mut doc = Document::new(1, 4);
        doc.layers[0].pixels[0..4].copy_from_slice(&[0, 255, 0, 255]);
        doc.flip_layer_vertical(0);
        assert_eq!(&doc.layers[0].pixels[12..16], &[0, 255, 0, 255]);
        doc.flip_layer_vertical(0);
        assert_eq!(&doc.layers[0].pixels[0..4], &[0, 255, 0, 255]);
    }

    #[test]
    fn translate_shifts_pixels() {
        let mut doc = Document::new(4, 1);
        doc.layers[0].pixels[0..4].copy_from_slice(&[255, 0, 0, 255]);
        doc.translate_layer(0, 2, 0);
        assert_eq!(&doc.layers[0].pixels[8..12], &[255, 0, 0, 255]);
        assert_eq!(&doc.layers[0].pixels[0..4], &[0, 0, 0, 0]);
    }

    #[test]
    fn affine_identity_keeps_pixels() {
        let mut doc = Document::new(8, 8);
        for b in doc.layers[0].pixels.iter_mut() {
            *b = 100;
        }
        let before = doc.layers[0].pixels.clone();
        doc.affine_layer(0, 1.0, 0.0);
        assert_eq!(doc.layers[0].pixels, before);
        // 2x zoom of solid color stays solid.
        doc.affine_layer(0, 2.0, 0.0);
        assert!(doc.layers[0].pixels.chunks_exact(4).all(|p| p[0] == 100 && p[3] == 100));
    }

    #[test]
    fn tone_brightness_contrast_and_alpha() {
        let mut doc = Document::new(2, 1);
        doc.layers[0].pixels = vec![100, 100, 100, 200, 100, 100, 100, 200];
        doc.adjust_tone(0, 10, 1.0);
        assert_eq!(&doc.layers[0].pixels[0..4], &[110, 110, 110, 200]);
        // Zero contrast flattens to mid-gray; alpha still untouched.
        doc.adjust_tone(0, 0, 0.0);
        assert_eq!(&doc.layers[0].pixels[0..4], &[128, 128, 128, 200]);
        // Identity is a no-op.
        let before = doc.layers[0].pixels.clone();
        doc.adjust_tone(0, 0, 1.0);
        assert_eq!(doc.layers[0].pixels, before);
    }

    #[test]
    fn blur_spreads_dot_keeps_flat() {
        let mut doc = Document::new(8, 8);
        for b in doc.layers[0].pixels.iter_mut() {
            *b = 77;
        }
        let before = doc.layers[0].pixels.clone();
        doc.blur_layer(0, 3);
        assert_eq!(doc.layers[0].pixels, before);
        // Single opaque dot on transparency spreads out.
        let mut doc = Document::new(9, 9);
        doc.layers[0].pixels[((4 * 9 + 4) * 4 + 3) as usize] = 255;
        doc.layers[0].pixels[((4 * 9 + 4) * 4) as usize] = 255;
        doc.blur_layer(0, 2);
        let at = |d: &Document, x: u32, y: u32| d.layers[0].pixels[((y * 9 + x) * 4 + 3) as usize];
        assert!(at(&doc, 4, 4) < 255);
        assert!(at(&doc, 5, 4) > 0);
        assert_eq!(at(&doc, 0, 0), 0);
    }

    #[test]
    fn sharpen_boosts_edge_contrast() {
        // Left half 100, right half 200.
        let mut doc = Document::new(8, 8);
        for y in 0..8 {
            for x in 0..8 {
                let v = if x < 4 { 100u8 } else { 200u8 };
                let o = ((y * 8 + x) * 4) as usize;
                doc.layers[0].pixels[o] = v;
                doc.layers[0].pixels[o + 1] = v;
                doc.layers[0].pixels[o + 2] = v;
                doc.layers[0].pixels[o + 3] = 255;
            }
        }
        doc.sharpen_layer(0, 1.0);
        let at = |d: &Document, x: u32, y: u32| d.layers[0].pixels[((y * 8 + x) * 4) as usize];
        assert!(at(&doc, 3, 4) < 100, "dark edge pixel dips");
        assert!(at(&doc, 4, 4) > 200 || at(&doc, 4, 4) == 255, "bright edge pixel rises");
        // Flat area is untouched by sharpening.
        assert_eq!(at(&doc, 0, 0), 100);
        assert_eq!(at(&doc, 7, 7), 200);
    }

    #[test]
    fn filters_respect_selection() {
        let mut doc = Document::new(6, 6);
        for b in doc.layers[0].pixels.chunks_exact_mut(4) {
            b.copy_from_slice(&[255, 255, 255, 255]);
        }
        doc.selection = Some((0, 0, 2, 2));
        doc.adjust_tone(0, -50, 1.0);
        assert_eq!(doc.layers[0].pixels[0], 205);
        let o = ((5 * 6 + 5) * 4) as usize;
        assert_eq!(doc.layers[0].pixels[o], 255);
        doc.blur_layer(0, 2);
        assert_eq!(doc.layers[0].pixels[o], 255);
        doc.clear_selection();
        assert!(doc.selection.is_none() && doc.selection_mask.is_none());
    }

    #[test]
    fn kaleidoscope_segments_clamp() {
        let at = |m: SymmetryMode, seg: u8| m.mirrored_pixels(21, 21, seg, 5, 5).len();
        assert_eq!(at(SymmetryMode::Kaleidoscope, 6), 6);
        assert_eq!(at(SymmetryMode::Kaleidoscope, 1), 2);
        assert_eq!(at(SymmetryMode::Kaleidoscope, 99), 16);
        assert_eq!(at(SymmetryMode::Both, 9), 4);
    }

    #[test]
    fn kaleidoscope_paints_n_copies() {
        use crate::brush::{Brush, Color};
        let mut doc = Document::new(21, 21);
        doc.symmetry = SymmetryMode::Kaleidoscope;
        doc.symmetry_segments = 4;
        let brush = Brush { size: 3.0, softness: 0.0, opacity: 1.0, ..Default::default() };
        doc.dab(&brush, Color::rgb(255, 0, 0), 14.5, 10.5);
        let at = |d: &Document, x: u32, y: u32| d.layers[0].pixels[((y * 21 + x) * 4 + 3) as usize];
        for (x, y) in [(14, 10), (10, 14), (6, 10), (10, 6)] {
            assert!(at(&doc, x, y) > 0, "missing dab at {x},{y}");
        }
        assert_eq!(at(&doc, 0, 0), 0);
    }

    #[test]
    fn roundtrip_keeps_kaleidoscope() {
        let mut doc = Document::new(4, 3);
        doc.symmetry = SymmetryMode::Kaleidoscope;
        doc.symmetry_segments = 9;
        let dir = std::env::temp_dir();
        let path = dir.join("procraft-kaleido.pcraft");
        doc.save_project(&path).unwrap();
        let back = Document::open_project(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(back.symmetry, SymmetryMode::Kaleidoscope);
        assert_eq!(back.symmetry_segments, 9);
    }

    #[test]
    fn v2_project_opens_with_default_segments() {
        // Minimal version-2 file: one symmetry byte, no segments byte.
        let dir = std::env::temp_dir();
        let path = dir.join("procraft-v2-compat.pcraft");
        let mut buf = Vec::new();
        buf.extend_from_slice(b"PCRAFT02");
        buf.extend_from_slice(&2u32.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&[255, 255, 255, 255]);
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&[1]); // Horizontal.
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(b"L");
        buf.extend_from_slice(&[1]);
        buf.extend_from_slice(&1.0f32.to_le_bytes());
        buf.extend_from_slice(&[0]);
        buf.extend_from_slice(&vec![5u8; 2 * 1 * 4]);
        std::fs::write(&path, &buf).unwrap();
        let back = Document::open_project(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(back.symmetry, SymmetryMode::Horizontal);
        assert_eq!(back.symmetry_segments, 6);
    }

    #[test]
    fn merge_down_composites_and_removes() {
        let mut doc = Document::new(2, 1);
        doc.layers[0].pixels = vec![0, 0, 255, 255, 0, 0, 255, 255];
        doc.add_layer("top".to_string());
        doc.layers[1].pixels = vec![255, 0, 0, 255, 0, 0, 0, 0];
        assert!(doc.merge_down(1));
        assert_eq!(doc.layers.len(), 1);
        assert_eq!(doc.active, 0);
        assert_eq!(&doc.layers[0].pixels[0..4], &[255, 0, 0, 255]);
        assert_eq!(&doc.layers[0].pixels[4..8], &[0, 0, 255, 255]);
        // Bottom layer refuses.
        assert!(!doc.merge_down(0));
        // Invisible top contributes nothing but still merges away.
        doc.add_layer("ghost".to_string());
        doc.layers[1].visible = false;
        doc.layers[1].pixels = vec![0, 255, 0, 255, 0, 255, 0, 255];
        assert!(doc.merge_down(1));
        assert_eq!(&doc.layers[0].pixels[0..4], &[255, 0, 0, 255]);
    }

    #[test]
    fn duplicate_copies_layer() {
        let mut doc = Document::new(2, 1);
        doc.layers[0].pixels = vec![7; 8];
        doc.duplicate_layer(0);
        assert_eq!(doc.layers.len(), 2);
        assert_eq!(doc.active, 1);
        assert_eq!(doc.layers[1].name, "Layer 1 copy");
        assert_eq!(doc.layers[1].pixels, vec![7; 8]);
        doc.duplicate_layer(9);
        assert_eq!(doc.layers.len(), 2);
    }

    #[test]
    fn sample_reads_composite() {
        let mut doc = Document::new(2, 1);
        doc.layers[0].pixels = vec![10, 20, 30, 255, 0, 0, 0, 0];
        assert_eq!(doc.sample(0, 0), Some([10, 20, 30]));
        assert_eq!(doc.sample(9, 9), None);
    }

    #[test]
    fn lasso_selects_interior_only() {
        use crate::brush::{Brush, Color};
        let mut doc = Document::new(20, 20);
        let pts = vec![(0.0, 0.0), (12.0, 0.0), (0.0, 12.0), (0.0, 0.0)];
        let bbox = doc.set_lasso(&pts).expect("lasso builds");
        assert!(doc.pixel_selected(2, 2));
        assert!(!doc.pixel_selected(15, 15));
        assert_eq!(doc.selection, Some(bbox));
        assert!(doc.selection_pixel_count() > 0);
        // Painting is constrained to the lasso.
        let brush = Brush { opacity: 1.0, ..Default::default() };
        doc.fill_rect(&brush, Color::rgb(9, 9, 9), 0, 0, 19, 19);
        let inside = ((2 * 20 + 2) * 4) as usize;
        assert_eq!(doc.layers[0].pixels[inside + 3], 255);
        let outside = ((15 * 20 + 15) * 4) as usize;
        assert_eq!(doc.layers[0].pixels[outside + 3], 0);
        // Degenerate loops clear.
        assert!(doc.set_lasso(&[]).is_none());
        assert!(doc.selection_mask.is_none());
        doc.clear_selection();
        assert!(doc.selection.is_none() && doc.selection_mask.is_none());
        assert_eq!(doc.selection_pixel_count(), 0);
    }

    #[test]
    fn shape_fill_respects_selection() {
        use crate::brush::{Brush, Color};
        let mut doc = Document::new(10, 10);
        let brush = Brush { opacity: 1.0, ..Default::default() };
        doc.selection = Some((2, 2, 5, 5));
        doc.fill_rect(&brush, Color::rgb(1, 2, 3), 0, 0, 9, 9);
        assert_eq!(doc.layers[0].pixels[0..4], vec![0, 0, 0, 0]);
        let o = ((3 * 10 + 3) * 4) as usize;
        assert_eq!(doc.layers[0].pixels[o + 3], 255);
    }

    #[test]
    fn v3_project_still_opens_raw() {
        // Hand-build a minimal version-3 file (raw pixels, symmetry + segments).
        let dir = std::env::temp_dir();
        let path = dir.join("procraft-v3-compat.pcraft");
        let mut buf = Vec::new();
        buf.extend_from_slice(b"PCRAFT03");
        buf.extend_from_slice(&2u32.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&[1, 2, 3, 255]);
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&[3]); // Both.
        buf.extend_from_slice(&[9]); // Segments.
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(b"L");
        buf.extend_from_slice(&[1]);
        buf.extend_from_slice(&1.0f32.to_le_bytes());
        buf.extend_from_slice(&[0]);
        buf.extend_from_slice(&vec![6u8; 2 * 1 * 4]);
        std::fs::write(&path, &buf).unwrap();
        let back = Document::open_project(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(back.symmetry, SymmetryMode::Both);
        assert_eq!(back.symmetry_segments, 9);
        assert_eq!(back.layers[0].pixels, vec![6u8; 8]);
    }

    #[test]
    fn v4_roundtrip_compresses_sparse_canvas() {
        // Mostly-transparent canvas must shrink far below raw size.
        let doc = Document::new(128, 128);
        let dir = std::env::temp_dir();
        let path = dir.join("procraft-v4-size.pcraft");
        doc.save_project(&path).unwrap();
        let meta = std::fs::metadata(&path).unwrap();
        let raw = 128usize * 128 * 4;
        assert!(meta.len() as usize * 4 < raw, "compressed {} vs raw {raw}", meta.len());
        let head = std::fs::read(&path).unwrap();
        assert_eq!(&head[..8], b"PCRAFT05");
        let back = Document::open_project(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(back.layers[0].pixels, doc.layers[0].pixels);
    }

    #[test]
    fn v4_noisy_pixels_still_roundtrip() {
        // Incompressible noise must survive the zlib round trip exactly.
        let mut doc = Document::new(32, 32);
        let mut x: u32 = 0x12345678;
        for b in doc.layers[0].pixels.iter_mut() {
            x = x.wrapping_mul(1664525).wrapping_add(1013904223);
            *b = (x >> 24) as u8;
        }
        let want = doc.layers[0].pixels.clone();
        let dir = std::env::temp_dir();
        let path = dir.join("procraft-v4-noise.pcraft");
        doc.save_project(&path).unwrap();
        let back = Document::open_project(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(back.layers[0].pixels, want);
    }

    #[test]
    fn v4_corrupt_pixels_rejected() {
        let doc = Document::new(8, 8);
        let dir = std::env::temp_dir();
        let path = dir.join("procraft-v4-corrupt.pcraft");
        doc.save_project(&path).unwrap();
        let mut buf = std::fs::read(&path).unwrap();
        // Flip bytes inside the compressed payload (last 8 bytes).
        let n = buf.len();
        for b in buf[n - 8..].iter_mut() {
            *b ^= 0xFF;
        }
        std::fs::write(&path, &buf).unwrap();
        assert!(Document::open_project(&path).is_err());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn strokes_fully_outside_canvas_are_safe() {
        use crate::brush::{Brush, BrushTip, Color};
        // Regression: a dab fully right/below the canvas used to record an
        // inverted dirty rect, and the texture upload's `x1 - x0 + 1` then
        // underflowed and quit the app.
        let mut doc = Document::new(32, 32);
        let brush = Brush { size: 8.0, ..Default::default() };
        let clean = doc.layers[0].pixels.clone();
        for (from, to) in [
            (Some((100.0, 100.0)), (400.0, 400.0)), // right-bottom
            (Some((-400.0, 100.0)), (-100.0, 100.0)), // left
            (Some((100.0, -400.0)), (100.0, -100.0)), // above
            (None, (1000.0, 1000.0)), // single dab far outside
        ] {
            doc.stroke_to(&brush, Color::rgb(255, 0, 0), from, to, 0.0);
        }
        assert_eq!(doc.layers[0].pixels, clean);
        // No poisoned region may survive: consumers subtract safely.
        if let Some((x0, y0, x1, y1)) = doc.take_dirty() {
            assert!(x0 <= x1 && y0 <= y1);
            let bytes = doc.flatten_region(x0, y0, x1, y1);
            let w = (x1 - x0 + 1) as usize;
            let h = (y1 - y0 + 1) as usize;
            assert_eq!(bytes.len(), w * h * 4);
        }
        // Same for stamp tips and mask painting.
        let tip = BrushTip::from_mask(2, 2, vec![255; 4]).unwrap();
        let stamp = Brush { size: 16.0, tip: Some(tip), ..Brush::default() };
        doc.stroke_to(&stamp, Color::rgb(1, 2, 3), Some((200.0, 200.0)), (500.0, 500.0), 0.0);
        assert_eq!(doc.layers[0].pixels, clean);
        doc.paint_target = PaintTarget::Mask;
        doc.stroke_to(&brush, Color::rgb(0, 0, 0), Some((200.0, 200.0)), (500.0, 500.0), 0.0);
        assert!(doc.layers[0].mask.is_none());
        doc.paint_target = PaintTarget::Color;
    }
}
