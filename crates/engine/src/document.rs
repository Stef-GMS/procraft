//! Layers, documents, compositing and undo.

/// How a layer's pixels combine with what's beneath.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BlendMode {
    #[default]
    Normal,
    Multiply,
    Screen,
    Overlay,
}

impl BlendMode {
    pub const ALL: [BlendMode; 4] = [BlendMode::Normal, BlendMode::Multiply, BlendMode::Screen, BlendMode::Overlay];

    pub fn name(self) -> &'static str {
        match self {
            BlendMode::Normal => "Normal",
            BlendMode::Multiply => "Multiply",
            BlendMode::Screen => "Screen",
            BlendMode::Overlay => "Overlay",
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
}

impl Layer {
    pub fn blank(name: String, w: u32, h: u32) -> Self {
        Self { name, visible: true, opacity: 1.0, blend: BlendMode::Normal, pixels: vec![0; (w * h * 4) as usize] }
    }

    pub fn clear(&mut self) {
        self.pixels.fill(0);
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
    dirty: Option<(u32, u32, u32, u32)>,
    undo: Vec<UndoEntry>,
}

#[derive(Clone, Debug)]
struct UndoEntry {
    layer: usize,
    pixels: Vec<u8>,
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
            dirty: None,
            undo: Vec::new(),
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
        let r = (x0, y0, x1, y1);
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
    pub fn checkpoint(&mut self) {
        if let Some(layer) = self.layers.get(self.active) {
            if self.undo.len() >= 32 {
                self.undo.remove(0);
            }
            self.undo.push(UndoEntry { layer: self.active, pixels: layer.pixels.clone() });
        }
    }

    /// Restore the most recent snapshot. Returns false when history is empty.
    pub fn undo(&mut self) -> bool {
        if let Some(entry) = self.undo.pop() {
            if let Some(layer) = self.layers.get_mut(entry.layer) {
                layer.pixels = entry.pixels;
                self.active = entry.layer.min(self.layers.len().saturating_sub(1));
                self.mark_full_dirty();
                return true;
            }
        }
        false
    }

    fn composite_pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let (mut r, mut g, mut b) =
            (self.background[0] as f32, self.background[1] as f32, self.background[2] as f32);
        let o = ((y * self.w + x) * 4) as usize;
        for layer in &self.layers {
            if !layer.visible || layer.opacity <= 0.0 || o + 3 >= layer.pixels.len() {
                continue;
            }
            let a = layer.pixels[o + 3] as f32 / 255.0 * layer.opacity.clamp(0.0, 1.0);
            if a <= 0.0 {
                continue;
            }
            let blend = layer.blend;
            r = blend.apply(layer.pixels[o] as f32, r) * a + r * (1.0 - a);
            g = blend.apply(layer.pixels[o + 1] as f32, g) * a + g * (1.0 - a);
            b = blend.apply(layer.pixels[o + 2] as f32, b) * a + b * (1.0 - a);
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

    /// Shift a layer by whole pixels (clipped, transparent fill).
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
    /// layer with bilinear sampling. Caller checkpoints for undo first.
    pub fn affine_layer(&mut self, index: usize, scale: f32, degrees: f32) {
        if index >= self.layers.len() || (scale - 1.0).abs() < 1e-6 && degrees.abs() < 1e-6 {
            return;
        }
        let (w, h) = (self.w, self.h);
        let (cx, cy) = (w as f32 * 0.5, h as f32 * 0.5);
        let theta = degrees.to_radians();
        let (sin, cos) = theta.sin_cos();
        let s = scale.max(0.01);
        let src = self.layers[index].pixels.clone();
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
                    next[o..o + 4].copy_from_slice(&Self::sample_bilinear(&src, w, h, sx, sy));
                }
            }
        }
        self.layers[index].pixels = next;
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
        self.undo.clear();
        Ok(())
    }

    /// Save the full project (layers, opacity, blend, background) to a
    /// `.procraft` file: magic + little-endian records, raw pixels.
    pub fn save_project(&self, path: &std::path::Path) -> Result<(), String> {
        use std::io::Write;
        let mut f = std::fs::File::create(path).map_err(|e| e.to_string())?;
        f.write_all(b"PCRAFT01").map_err(|e| e.to_string())?;
        f.write_all(&self.w.to_le_bytes()).map_err(|e| e.to_string())?;
        f.write_all(&self.h.to_le_bytes()).map_err(|e| e.to_string())?;
        f.write_all(&self.background).map_err(|e| e.to_string())?;
        f.write_all(&(self.layers.len() as u32).to_le_bytes()).map_err(|e| e.to_string())?;
        f.write_all(&(self.active as u32).to_le_bytes()).map_err(|e| e.to_string())?;
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
            };
            f.write_all(&[blend]).map_err(|e| e.to_string())?;
            let expect = (self.w * self.h * 4) as usize;
            if layer.pixels.len() != expect {
                return Err("layer has wrong pixel size".to_string());
            }
            f.write_all(&layer.pixels).map_err(|e| e.to_string())?;
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
        if take(8)? != b"PCRAFT01" {
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
                _ => BlendMode::Normal,
            };
            let expect = (w * h * 4) as usize;
            let pixels = take(expect)?.to_vec();
            layers.push(Layer { name, visible, opacity, blend, pixels });
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
            dirty: None,
            undo: Vec::new(),
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
    fn project_roundtrip_keeps_everything() {        let mut doc = Document::new(4, 3);
        doc.background = [10, 20, 30, 255];
        doc.layers[0].pixels = vec![7; 4 * 3 * 4];
        doc.layers[0].opacity = 0.5;
        doc.layers[0].blend = BlendMode::Overlay;
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
        assert!(Document::open_project(&dir.join("missing.pcraft")).is_err());
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
}
