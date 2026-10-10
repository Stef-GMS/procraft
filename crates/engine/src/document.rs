//! Layers, documents, compositing and undo.

/// One raster layer: row-major RGBA bytes plus presentation state.
#[derive(Clone, Debug)]
pub struct Layer {
    pub name: String,
    pub visible: bool,
    /// 0.0 (transparent) ..= 1.0 (opaque).
    pub opacity: f32,
    pub pixels: Vec<u8>,
}

impl Layer {
    pub fn blank(name: String, w: u32, h: u32) -> Self {
        Self { name, visible: true, opacity: 1.0, pixels: vec![0; (w * h * 4) as usize] }
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
    /// Backdrop shown under transparent pixels (checkerboard-ish default).
    pub background: [u8; 4],
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
            background: [128, 128, 128, 255],
            undo: Vec::new(),
        }
    }

    pub fn active_layer_mut(&mut self) -> Option<&mut Layer> {
        self.layers.get_mut(self.active)
    }

    pub fn add_layer(&mut self, name: String) {
        self.layers.push(Layer::blank(name, self.w, self.h));
        self.active = self.layers.len() - 1;
    }

    pub fn remove_layer(&mut self, index: usize) {
        if self.layers.len() <= 1 || index >= self.layers.len() {
            return;
        }
        self.layers.remove(index);
        self.active = self.active.min(self.layers.len() - 1);
    }

    pub fn move_layer(&mut self, from: usize, to: usize) {
        if from >= self.layers.len() || to >= self.layers.len() || from == to {
            return;
        }
        let layer = self.layers.remove(from);
        self.layers.insert(to, layer);
        self.active = to;
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
                return true;
            }
        }
        false
    }

    /// Composite bottom-to-top over the background into RGBA bytes.
    pub fn flatten(&self) -> Vec<u8> {
        let n = (self.w * self.h) as usize;
        let mut out = vec![0u8; n * 4];
        for i in 0..n {
            let (mut r, mut g, mut b) =
                (self.background[0] as f32, self.background[1] as f32, self.background[2] as f32);
            for layer in &self.layers {
                if !layer.visible || layer.opacity <= 0.0 {
                    continue;
                }
                let o = (i * 4).min(layer.pixels.len().saturating_sub(4));
                let a = layer.pixels[o + 3] as f32 / 255.0 * layer.opacity.clamp(0.0, 1.0);
                if a <= 0.0 {
                    continue;
                }
                r = layer.pixels[o] as f32 * a + r * (1.0 - a);
                g = layer.pixels[o + 1] as f32 * a + g * (1.0 - a);
                b = layer.pixels[o + 2] as f32 * a + b * (1.0 - a);
            }
            out[i * 4] = r.round().clamp(0.0, 255.0) as u8;
            out[i * 4 + 1] = g.round().clamp(0.0, 255.0) as u8;
            out[i * 4 + 2] = b.round().clamp(0.0, 255.0) as u8;
            out[i * 4 + 3] = 255;
        }
        out
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
}
