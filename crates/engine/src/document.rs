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
        segments: u8,
        cx: f32,
        cy: f32,
        x: f32,
        y: f32,
    ) -> Vec<(f32, f32)> {
        let mx = 2.0 * cx - x;
        let my = 2.0 * cy - y;
        match self {
            SymmetryMode::Off => vec![(x, y)],
            SymmetryMode::Horizontal => vec![(x, y), (mx, y)],
            SymmetryMode::Vertical => vec![(x, y), (x, my)],
            SymmetryMode::Both => vec![(x, y), (mx, y), (x, my), (mx, my)],
            SymmetryMode::Kaleidoscope => {
                // Dihedral symmetry: rotate by k*step, mirroring on odd k.
                let n = segments.clamp(2, 16) as usize;
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

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn mirrored_pixels(
        self,
        w: u32,
        h: u32,
        segments: u8,
        cx: f32,
        cy: f32,
        px: u32,
        py: u32,
    ) -> Vec<(u32, u32)> {
        // Mirror across the center, clamped into the canvas.
        let mirror = |v: f32, max: u32| {
            if max == 0 {
                0
            } else {
                v.round().clamp(0.0, max as f32 - 1.0) as u32
            }
        };
        match self {
            SymmetryMode::Kaleidoscope => {
                let n = segments.clamp(2, 16) as usize;
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
                let mx = mirror(2.0 * cx - px as f32, w);
                let my = mirror(2.0 * cy - py as f32, h);
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
    /// Canvas offset of the buffer's top-left in pixels. The Move tool only
    /// ever changes these (lossless, O(1)): content pushed off-canvas stays
    /// in the buffer and returns when moved back. Paint paths map canvas
    /// coordinates through the offset; compositing, merge and export bake
    /// it. Saved to v6 project files (older versions read 0).
    pub ox: i32,
    pub oy: i32,
    /// Owning layer group (id into [`Document::groups`); `None` lives at
    /// the top level. Saved to v8 project files (older versions read none).
    pub group: Option<u32>,
}

/// A named layer container: visibility (and collapse) apply to the whole
/// subtree. Ids are monotonic per document and never reused while a group
/// with that id exists; removal leaves gaps, which open/ops tolerate.
#[derive(Clone, Debug)]
pub struct LayerGroup {
    pub id: u32,
    pub name: String,
    pub visible: bool,
    pub collapsed: bool,
    /// Parent group id for nesting; `None` lives at the top level.
    pub parent: Option<u32>,
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
            ox: 0,
            oy: 0,
            group: None,
        }
    }

    /// Map a canvas pixel to this layer's buffer index, if it holds content
    /// there. `w`/`h` are the (shared) buffer dimensions.
    fn local_offset(&self, px: u32, py: u32, w: u32, h: u32) -> Option<usize> {
        let lx = px as i32 - self.ox;
        let ly = py as i32 - self.oy;
        if lx < 0 || ly < 0 || lx >= w as i32 || ly >= h as i32 {
            return None;
        }
        Some(((ly as u32 * w + lx as u32) * 4) as usize)
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
    /// Mirror center in canvas pixels (`None` = canvas center). Draggable
    /// on-canvas like guide vanishing points; saved to v7 project files.
    pub symmetry_center: Option<(f32, f32)>,
    /// Freehand painting target (layer color or its reveal mask).
    pub paint_target: PaintTarget,
    dirty: Option<(u32, u32, u32, u32)>,
    undo: Vec<UndoEntry>,
    redo: Vec<UndoEntry>,
    undo_bytes: usize,
    redo_bytes: usize,
    /// Layer groups (presentation only: paint/masks still address layers).
    /// Saved to v8 project files.
    pub groups: Vec<LayerGroup>,
    next_group_id: u32,
    /// Running pickup color for the smudge stroke (straight RGBA f32s);
    /// reset on every stroke start, never saved.
    pub(crate) smudge_finger: Option<[f32; 4]>,
}

/// History cap: at most 32 steps per stack (backstop)…
const MAX_UNDO_ENTRIES: usize = 32;
/// …and at most 256 MiB per stack. A 4K masked layer is ~40 MiB per step,
/// so the byte budget binds first on big canvases (6 steps) while the count
/// cap binds on small ones (32 steps x ~5 MiB at 1280x800).
const MAX_UNDO_BYTES: usize = 256 * 1024 * 1024;

#[derive(Clone, Debug)]
struct UndoEntry {
    layer: usize,
    pixels: Vec<u8>,
    mask: Option<Vec<u8>>,
    ox: i32,
    oy: i32,
}

impl UndoEntry {
    fn byte_size(&self) -> usize {
        self.pixels.len() + self.mask.as_ref().map_or(0, Vec::len)
    }
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
            symmetry_center: None,
            paint_target: PaintTarget::Color,
            dirty: None,
            undo: Vec::new(),
            redo: Vec::new(),
            undo_bytes: 0,
            redo_bytes: 0,
            groups: Vec::new(),
            next_group_id: 1,
            smudge_finger: None,
        }
    }

    /// Assemble a document from imported layers (white page, given active
    /// layer, no selection or history). For format importers in this crate.
    pub(crate) fn from_layers(w: u32, h: u32, layers: Vec<Layer>, active: usize) -> Self {
        let active = active.min(layers.len().saturating_sub(1));
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
            symmetry_center: None,
            paint_target: PaintTarget::Color,
            dirty: None,
            undo: Vec::new(),
            redo: Vec::new(),
            undo_bytes: 0,
            redo_bytes: 0,
            groups: Vec::new(),
            next_group_id: 1,
            smudge_finger: None,
        }
    }
    /// Mirror center in canvas pixels (canvas middle when never moved).
    pub fn symmetry_center_px(&self) -> (f32, f32) {
        self.symmetry_center
            .unwrap_or(((self.w as f32 - 1.0) * 0.5, (self.h as f32 - 1.0) * 0.5))
    }

    /// Move the mirror center (clamped, like layer offsets and VPs).
    pub fn set_symmetry_center(&mut self, x: f32, y: f32) {
        self.symmetry_center = Some((x.clamp(-16384.0, 16384.0), y.clamp(-16384.0, 16384.0)));
    }

    /// Forget a moved mirror center (back to canvas middle).
    pub fn reset_symmetry_center(&mut self) {
        self.symmetry_center = None;
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

    /// Look up a group by id.
    pub fn group(&self, id: u32) -> Option<&LayerGroup> {
        self.groups.iter().find(|g| g.id == id)
    }

    fn group_mut(&mut self, id: u32) -> Option<&mut LayerGroup> {
        self.groups.iter_mut().find(|g| g.id == id)
    }

    /// True when the layer and every ancestor group are visible (what
    /// compositing, playback, export and onion paths must honor instead of
    /// the bare layer flag).
    pub fn is_layer_shown(&self, index: usize) -> bool {
        let Some(layer) = self.layers.get(index) else {
            return false;
        };
        if !layer.visible {
            return false;
        }
        let mut parent = layer.group;
        // Bounded walk: a corrupt parent cycle can never hang compositing.
        let mut guard = self.groups.len().saturating_add(1);
        while let Some(id) = parent {
            if guard == 0 {
                break;
            }
            guard -= 1;
            let Some(g) = self.group(id) else {
                break;
            };
            if !g.visible {
                return false;
            }
            parent = g.parent;
        }
        true
    }

    /// Create an empty top-level group and return its id. Structural like
    /// add/remove layer: not undoable, marks the view dirty.
    pub fn new_group(&mut self, name: String) -> u32 {
        // Ids start at 1; 0 always reads as "no group".
        let id = self.next_group_id.max(1);
        self.next_group_id = id.saturating_add(1).max(1);
        self.groups.push(LayerGroup {
            id,
            name,
            visible: true,
            collapsed: false,
            parent: None,
        });
        self.mark_full_dirty();
        id
    }

    pub fn rename_group(&mut self, id: u32, name: String) {
        if let Some(g) = self.group_mut(id) {
            g.name = name;
        }
    }

    pub fn set_group_visible(&mut self, id: u32, visible: bool) {
        if let Some(g) = self.group_mut(id) {
            g.visible = visible;
        }
        self.mark_full_dirty();
    }

    pub fn set_group_collapsed(&mut self, id: u32, collapsed: bool) {
        if let Some(g) = self.group_mut(id) {
            g.collapsed = collapsed;
        }
    }

    /// Move a layer into a group (or to the top level with `None`),
    /// keeping group members contiguous: the layer lands next to the
    /// group's current members (topmost side).
    pub fn move_layer_into_group(&mut self, index: usize, group: Option<u32>) {
        if index >= self.layers.len() {
            return;
        }
        if let Some(id) = group {
            if self.group(id).is_none() {
                return;
            }
        }
        let mut layer = self.layers.remove(index);
        layer.group = group;
        let at = match group {
            None => self.layers.len(),
            Some(id) => self
                .layers
                .iter()
                .rposition(|l| l.group == Some(id))
                .map_or(self.layers.len(), |p| p + 1),
        };
        // `active` tracked the removed layer: follow it to its new home.
        self.layers.insert(at, layer);
        self.active = at;
        self.mark_full_dirty();
    }

    /// Disband a group: member layers (and subgroups) move up one level,
    /// the group row disappears. Contents always survive (no destructive
    /// group delete in v1).
    pub fn ungroup(&mut self, id: u32) {
        let Some(pos) = self.groups.iter().position(|g| g.id == id) else {
            return;
        };
        let parent = self.groups[pos].parent;
        self.groups.remove(pos);
        for layer in self.layers.iter_mut() {
            if layer.group == Some(id) {
                layer.group = parent;
            }
        }
        for g in self.groups.iter_mut() {
            if g.parent == Some(id) {
                g.parent = parent;
            }
        }
        self.active = self.active.min(self.layers.len().saturating_sub(1));
        self.mark_full_dirty();
    }

    /// Snapshot the active layer for one undo step (call when a stroke starts).
    /// A new checkpoint discards the redo history.
    pub fn checkpoint(&mut self) {
        if let Some(entry) = self.snapshot() {
            self.push_undo(entry);
            self.redo.clear();
            self.redo_bytes = 0;
        }
    }

    /// Push onto the undo stack, evicting oldest-first while over either cap.
    /// The newest step is always kept, even if it alone exceeds the budget.
    fn push_undo(&mut self, entry: UndoEntry) {
        self.undo_bytes += entry.byte_size();
        self.undo.push(entry);
        while self.undo.len() > 1
            && (self.undo.len() > MAX_UNDO_ENTRIES || self.undo_bytes > MAX_UNDO_BYTES)
        {
            self.undo_bytes -= self.undo[0].byte_size();
            self.undo.remove(0);
        }
    }

    /// Push onto the redo stack, same dual-cap eviction as [`Self::push_undo`].
    fn push_redo(&mut self, entry: UndoEntry) {
        self.redo_bytes += entry.byte_size();
        self.redo.push(entry);
        while self.redo.len() > 1
            && (self.redo.len() > MAX_UNDO_ENTRIES || self.redo_bytes > MAX_UNDO_BYTES)
        {
            self.redo_bytes -= self.redo[0].byte_size();
            self.redo.remove(0);
        }
    }

    /// Copy of the active layer for the history stacks, if there is one.
    fn snapshot(&self) -> Option<UndoEntry> {
        self.layers.get(self.active).map(|layer| UndoEntry {
            layer: self.active,
            pixels: layer.pixels.clone(),
            mask: layer.mask.clone(),
            ox: layer.ox,
            oy: layer.oy,
        })
    }

    /// Restore the most recent snapshot. Returns false when history is empty.
    pub fn undo(&mut self) -> bool {
        if let Some(entry) = self.undo.pop() {
            self.undo_bytes -= entry.byte_size();
            if let Some(cur) = self.snapshot() {
                self.push_redo(cur);
                if let Some(layer) = self.layers.get_mut(entry.layer) {
                    layer.pixels = entry.pixels;
                    layer.mask = entry.mask;
                    layer.ox = entry.ox;
                    layer.oy = entry.oy;
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
            self.redo_bytes -= entry.byte_size();
            if let Some(cur) = self.snapshot() {
                self.push_undo(cur);
                if let Some(layer) = self.layers.get_mut(entry.layer) {
                    layer.pixels = entry.pixels;
                    layer.mask = entry.mask;
                    layer.ox = entry.ox;
                    layer.oy = entry.oy;
                    self.active = entry.layer.min(self.layers.len().saturating_sub(1));
                    self.mark_full_dirty();
                    return true;
                }
            }
        }
        false
    }

    fn composite_pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let (mut r, mut g, mut b) = (
            self.background[0] as f32,
            self.background[1] as f32,
            self.background[2] as f32,
        );
        // Opaque content composited so far (layers only, not the page):
        // clipping masks intersect against it.
        let mut below_a = 0.0f32;
        for (li, layer) in self.layers.iter().enumerate() {
            if !self.is_layer_shown(li) || layer.opacity <= 0.0 {
                continue;
            }
            // Buffer content sits at the layer offset; off-canvas parts of
            // the buffer simply don't cover this pixel.
            let Some(o) = layer.local_offset(x, y, self.w, self.h) else {
                continue;
            };
            if o + 3 >= layer.pixels.len() {
                continue;
            }
            let mut a = layer.pixels[o + 3] as f32 / 255.0 * layer.opacity.clamp(0.0, 1.0);
            if let Some(m) = layer.mask.as_deref() {
                a *= m.get(o / 4).copied().unwrap_or(255) as f32 / 255.0;
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

    /// Shift a layer by whole pixels. Lossless and O(1): only the layer
    /// offset changes, so content pushed off-canvas stays in the buffer
    /// and returns when moved back (this used to crop destructively).
    /// Offsets are clamped to +/-16384 (the dimension cap). Caller
    /// checkpoints for undo first (offsets ride the history entries).
    pub fn translate_layer(&mut self, index: usize, dx: i32, dy: i32) {
        if (dx == 0 && dy == 0) || index >= self.layers.len() {
            return;
        }
        const LIM: i32 = 16384;
        let layer = &mut self.layers[index];
        layer.ox = (layer.ox + dx).clamp(-LIM, LIM);
        layer.oy = (layer.oy + dy).clamp(-LIM, LIM);
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
            let v = a[i] * (1.0 - fx) * (1.0 - fy)
                + b[i] * fx * (1.0 - fy)
                + c[i] * (1.0 - fx) * fy
                + d[i] * fx * fy;
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
            self.layers[index].mask = Some(out.as_chunks::<4>().0.iter().map(|p| p[0]).collect());
        }
        // Carry the offset through the same forward map (scale, then
        // rotate about the canvas center) so transformed content lands
        // where the pixels do. Matches affine_buffer's inverse map.
        let theta = degrees.to_radians();
        let (sin, cos) = theta.sin_cos();
        let s = scale.max(0.01);
        let (ox, oy) = (self.layers[index].ox as f32, self.layers[index].oy as f32);
        self.layers[index].ox = (s * (ox * cos - oy * sin)).round() as i32;
        self.layers[index].oy = (s * (ox * sin + oy * cos)).round() as i32;
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
        // Mirror the offset too so the content flips about the canvas
        // center, exactly as before offsets existed.
        self.layers[index].ox = -self.layers[index].ox;
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
        // Mirror the offset too so the content flips about the canvas
        // center, exactly as before offsets existed.
        self.layers[index].oy = -self.layers[index].oy;
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
        let sa_base = if self.is_layer_shown(index) {
            src.opacity.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let (w, h) = (self.w, self.h);
        let dst = &mut self.layers[index - 1];
        let (dox, doy) = (dst.ox, dst.oy);
        for y in 0..h {
            for x in 0..w {
                // Destination slot -> canvas -> source slot. The merge bakes
                // both offsets; the result keeps dst's offset.
                let (dlx, dly) = (x as i32 - dox, y as i32 - doy);
                if dlx < 0 || dly < 0 || dlx >= w as i32 || dly >= h as i32 {
                    continue;
                }
                let o = ((dly as u32 * w + dlx as u32) * 4) as usize;
                if o + 3 >= dst.pixels.len() {
                    continue;
                }
                let (slx, sly) = (x as i32 - src.ox, y as i32 - src.oy);
                let (sr, sg, sb, mut sa) =
                    if slx < 0 || sly < 0 || slx >= w as i32 || sly >= h as i32 {
                        (0, 0, 0, 0.0)
                    } else {
                        let so = ((sly as u32 * w + slx as u32) * 4) as usize;
                        if so + 3 >= src.pixels.len() {
                            (0, 0, 0, 0.0)
                        } else {
                            let mut a = sa_base * src.pixels[so + 3] as f32 / 255.0;
                            if let Some(m) = src.mask.as_deref() {
                                a *= m.get(so / 4).copied().unwrap_or(255) as f32 / 255.0;
                            }
                            (src.pixels[so], src.pixels[so + 1], src.pixels[so + 2], a)
                        }
                    };
                if src.clip {
                    let dst_a = dst.pixels[o + 3] as f32 / 255.0;
                    let dst_m = dst
                        .mask
                        .as_deref()
                        .and_then(|m| m.get(o / 4).copied())
                        .unwrap_or(255) as f32
                        / 255.0;
                    sa *= dst_a * dst_m;
                }
                let da = dst.pixels[o + 3] as f32 / 255.0;
                let out_a = sa + da * (1.0 - sa);
                if out_a > 0.0 {
                    for (ch, sc) in [sr, sg, sb].iter().enumerate() {
                        let blended = src.blend.apply(*sc as f32, dst.pixels[o + ch] as f32);
                        dst.pixels[o + ch] =
                            ((blended * sa + dst.pixels[o + ch] as f32 * da * (1.0 - sa)) / out_a)
                                .round() as u8;
                    }
                    dst.pixels[o + 3] = (out_a * 255.0).round() as u8;
                }
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

    /// Selection coverage 0.0..=1.0 at a canvas pixel (1.0 with no mask):
    /// how strongly paint and filters apply through graduated feathered
    /// masks. Binary masks behave exactly as before.
    pub(crate) fn sel_k(mask: Option<&[u8]>, w: u32, x: u32, y: u32) -> f32 {
        match mask {
            Some(m) => m.get((y * w + x) as usize).copied().unwrap_or(0) as f32 / 255.0,
            None => 1.0,
        }
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
    /// Rendered at the layer offset (aligned with canvas paint); uncovered
    /// canvas reads concealed (black). `None` when there is no (valid) mask.
    pub fn mask_view(&self) -> Option<Vec<u8>> {
        let layer = self.layers.get(self.active)?;
        let m = layer.mask.as_deref()?;
        if m.len() != (self.w * self.h) as usize {
            return None;
        }
        let mut out = Vec::with_capacity(m.len() * 4);
        for y in 0..self.h {
            for x in 0..self.w {
                let v = match layer.local_offset(x, y, self.w, self.h) {
                    Some(o) => m.get(o / 4).copied().unwrap_or(0),
                    None => 0,
                };
                out.extend_from_slice(&[v, v, v, 255]);
            }
        }
        Some(out)
    }

    /// Bake one layer to canvas-size buffers with its offset applied
    /// (cropped to the canvas): what interchange formats store. Returns
    /// the shifted pixels plus the shifted mask (or `None` when the layer
    /// has no valid mask, matching the exporter's existing rule).
    pub fn baked_layer(&self, index: usize) -> Option<(Vec<u8>, Option<Vec<u8>>)> {
        let layer = self.layers.get(index)?;
        let n = (self.w * self.h) as usize;
        if layer.pixels.len() != n * 4 {
            return None;
        }
        let mut px = vec![0u8; n * 4];
        let mut mk: Option<Vec<u8>> = match layer.mask.as_deref() {
            Some(m) if m.len() == n => Some(vec![0u8; n]),
            _ => None,
        };
        for y in 0..self.h {
            for x in 0..self.w {
                if let Some(o) = layer.local_offset(x, y, self.w, self.h) {
                    if o + 3 < layer.pixels.len() {
                        let d = ((y * self.w + x) * 4) as usize;
                        px[d..d + 4].copy_from_slice(&layer.pixels[o..o + 4]);
                        if let (Some(m), Some(dst)) = (layer.mask.as_deref(), mk.as_mut()) {
                            dst[(y * self.w + x) as usize] = m.get(o / 4).copied().unwrap_or(0);
                        }
                    }
                }
            }
        }
        Some((px, mk))
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
        let (x1, y1) = (
            fx1.ceil().min(w as f32 - 1.0).max(0.0) as u32,
            fy1.ceil().min(h as f32 - 1.0).max(0.0) as u32,
        );
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
            for pair in xs.as_chunks::<2>().0 {
                let xa = (pair[0] - 0.5).ceil() as i32;
                let xb = (pair[1] - 0.5 - 1e-6).floor() as i32;
                for x in xa.max(x0 as i32)..=xb.min(x1 as i32) {
                    if x >= 0 {
                        mask[(y * w + x as u32) as usize] = 255;
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

    /// Pixels covered by the active selection (mask coverage for graduated
    /// feathered masks, rectangle area, else 0 when nothing is selected).
    pub fn selection_pixel_count(&self) -> usize {
        if let Some(mask) = self.selection_mask.as_deref() {
            ((mask.iter().map(|&b| b as u32).sum::<u32>() + 127) / 255) as usize
        } else if let Some((x0, y0, x1, y1)) = self.selection {
            ((x1 - x0 + 1) * (y1 - y0 + 1)) as usize
        } else {
            0
        }
    }

    /// Honored radius bound for feather/grow (the UI offers a subset).
    pub const MAX_SELECT_RADIUS: i32 = 64;

    /// Bounding box of nonzero mask pixels (tight marching-ants bounds).
    fn mask_bbox(mask: &[u8], w: u32, h: u32) -> Option<(u32, u32, u32, u32)> {
        let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
        for y in 0..h {
            for x in 0..w {
                if mask[(y * w + x) as usize] != 0 {
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x);
                    y1 = y1.max(y);
                }
            }
        }
        if x0 == u32::MAX {
            None
        } else {
            Some((x0, y0, x1, y1))
        }
    }

    /// Rasterize the rectangular selection into the mask (255 inside),
    /// ANDed with any existing mask. Returns false when nothing is
    /// selected (mask-only selections pass through untouched).
    fn bake_rect_to_mask(&mut self) -> bool {
        let (w, h) = (self.w, self.h);
        if w == 0 || h == 0 {
            return false;
        }
        let Some((rx0, ry0, rx1, ry1)) = self.selection else {
            return self
                .selection_mask
                .as_ref()
                .is_some_and(|m| m.iter().any(|&b| b != 0));
        };
        let n = (w * h) as usize;
        let mut mask = vec![0u8; n];
        for y in ry0..=ry1.min(h - 1) {
            for x in rx0..=rx1.min(w - 1) {
                mask[(y * w + x) as usize] = 255;
            }
        }
        if let Some(old) = self.selection_mask.as_deref() {
            if old.len() == n {
                for (d, s) in mask.iter_mut().zip(old.iter()) {
                    *d = (*d).min(*s);
                }
            }
        }
        if mask.iter().all(|&b| b == 0) {
            self.clear_selection();
            return false;
        }
        self.selection_mask = Some(mask);
        true
    }

    /// Re-tighten the bounds rect to the mask (clears both when empty).
    fn retighten_selection(&mut self) {
        match self.selection_mask.as_deref() {
            Some(m) => {
                self.selection = Self::mask_bbox(m, self.w, self.h);
                if self.selection.is_none() {
                    self.selection_mask = None;
                }
            }
            None => self.selection = None,
        }
    }

    /// Separable sliding-window blur over one gray channel (edge-replicated).
    fn blur_gray(src: &[u8], w: u32, h: u32, r: i32) -> Vec<u8> {
        if r <= 0 || w == 0 || h == 0 {
            return src.to_vec();
        }
        let (w, h, r) = (w as usize, h as usize, r as isize);
        let win = (2 * r + 1) as u32;
        let at = |buf: &[u8], i: isize, n: isize| buf[i.clamp(0, n - 1) as usize] as u32;
        let mut tmp = vec![0u8; w * h];
        for y in 0..h {
            let mut sum = 0u32;
            for x in -r..=r {
                sum += at(&src[y * w..(y + 1) * w], x, w as isize);
            }
            for x in 0..w {
                tmp[y * w + x] = ((sum + win / 2) / win) as u8;
                sum = sum - at(&src[y * w..(y + 1) * w], x as isize - r, w as isize)
                    + at(&src[y * w..(y + 1) * w], x as isize + r + 1, w as isize);
            }
        }
        let mut out = vec![0u8; w * h];
        for x in 0..w {
            let mut sum = 0u32;
            for y in -r..=r {
                let yy = y.clamp(0, h as isize - 1) as usize;
                sum += tmp[yy * w + x] as u32;
            }
            for y in 0..h {
                out[y * w + x] = ((sum + win / 2) / win) as u8;
                let yo = (y as isize - r).clamp(0, h as isize - 1) as usize;
                let yi = (y as isize + r + 1).clamp(0, h as isize - 1) as usize;
                sum = sum - tmp[yo * w + x] as u32 + tmp[yi * w + x] as u32;
            }
        }
        out
    }

    /// Sliding-window extremum over a replicated-edge line.
    fn slide_extreme(line: &[u8], r: usize, dilate: bool) -> Vec<u8> {
        use std::collections::VecDeque;
        let n = line.len();
        if n == 0 || r == 0 {
            return line.to_vec();
        }
        let mut pad = Vec::with_capacity(n + 2 * r);
        for _ in 0..r {
            pad.push(line[0]);
        }
        pad.extend_from_slice(line);
        for _ in 0..r {
            pad.push(line[n - 1]);
        }
        let better = |a: u8, b: u8| if dilate { a >= b } else { a <= b };
        let mut out = vec![0u8; n];
        let mut dq: VecDeque<usize> = VecDeque::new();
        for i in 0..pad.len() {
            while let Some(&back) = dq.back() {
                if better(pad[i], pad[back]) {
                    dq.pop_back();
                } else {
                    break;
                }
            }
            dq.push_back(i);
            if i >= 2 * r {
                let j = i - 2 * r;
                while let Some(&front) = dq.front() {
                    if front < j {
                        dq.pop_front();
                    } else {
                        break;
                    }
                }
                if j < n {
                    out[j] = pad[*dq.front().unwrap()];
                }
            }
        }
        out
    }

    /// Separable box dilate (max) / erode (min) over one gray channel.
    fn morph_gray(src: &[u8], w: u32, h: u32, r: i32, dilate: bool) -> Vec<u8> {
        if r <= 0 || w == 0 || h == 0 {
            return src.to_vec();
        }
        let (w, h, r) = (w as usize, h as usize, r as usize);
        let mut tmp = vec![0u8; w * h];
        for y in 0..h {
            tmp[y * w..(y + 1) * w].copy_from_slice(&Self::slide_extreme(
                &src[y * w..(y + 1) * w],
                r,
                dilate,
            ));
        }
        let mut out = vec![0u8; w * h];
        let mut col = vec![0u8; h];
        for x in 0..w {
            for y in 0..h {
                col[y] = tmp[y * w + x];
            }
            let done = Self::slide_extreme(&col, r, dilate);
            for y in 0..h {
                out[y * w + x] = done[y];
            }
        }
        out
    }

    /// Feather (soften) the selection edge by `radius` px: the mask ramps
    /// instead of stepping, so paint and filters fade across the edge.
    /// The bounds rect grows to contain the falloff. Returns false when
    /// there was nothing to feather. Selection-only: not undoable (like
    /// Clear selection).
    pub fn feather_selection(&mut self, radius: u32) -> bool {
        let r = radius.min(Self::MAX_SELECT_RADIUS as u32) as i32;
        if !self.bake_rect_to_mask() {
            return false;
        }
        if r > 0 {
            if let Some(m) = self.selection_mask.take() {
                self.selection_mask = Some(Self::blur_gray(&m, self.w, self.h, r));
            }
            self.retighten_selection();
        }
        self.selection_mask.is_some()
    }

    /// Grow (positive) or shrink (negative) the selection by `px`,
    /// clamped to ±[`Document::MAX_SELECT_RADIUS`]. Shrinking to nothing
    /// clears it. Returns false when nothing remains. Not undoable.
    pub fn grow_selection(&mut self, px: i32) -> bool {
        let px = px.clamp(-Self::MAX_SELECT_RADIUS, Self::MAX_SELECT_RADIUS);
        if !self.bake_rect_to_mask() {
            return false;
        }
        if px == 0 {
            return true;
        }
        if let Some(m) = self.selection_mask.take() {
            self.selection_mask = Some(Self::morph_gray(&m, self.w, self.h, px.abs(), px > 0));
        }
        self.retighten_selection();
        self.selection_mask.is_some()
    }

    /// De-jag the selection edge (blur + re-threshold, edge position kept).
    /// Returns false when there was nothing to smooth. Not undoable.
    pub fn smooth_selection(&mut self) -> bool {
        if !self.bake_rect_to_mask() {
            return false;
        }
        if let Some(m) = self.selection_mask.take() {
            let soft = Self::blur_gray(&m, self.w, self.h, 2);
            self.selection_mask = Some(
                soft.into_iter()
                    .map(|v| if v >= 128 { 255 } else { 0 })
                    .collect(),
            );
        }
        self.retighten_selection();
        self.selection_mask.is_some()
    }

    /// Flip the selection (unbounded complements become full-canvas masks).
    /// Returns false when there was nothing to invert. Not undoable.
    pub fn invert_selection(&mut self) -> bool {
        if !self.bake_rect_to_mask() {
            return false;
        }
        if let Some(m) = self.selection_mask.take() {
            self.selection_mask = Some(m.into_iter().map(|v| 255 - v).collect());
        }
        self.retighten_selection();
        self.selection_mask.is_some()
    }

    /// Select the active layer's pixels more opaque than `threshold`
    /// (offset-aware; uncovered canvas counts as transparent). Replaces any
    /// selection; empty layers clear it. Returns false when nothing is
    /// selected. Not undoable.
    pub fn select_opaque(&mut self, threshold: u8) -> bool {
        let (w, h) = (self.w, self.h);
        if w == 0 || h == 0 {
            self.clear_selection();
            return false;
        }
        let Some(layer) = self.layers.get(self.active) else {
            self.clear_selection();
            return false;
        };
        let (ox, oy) = (layer.ox, layer.oy);
        let mut mask = vec![0u8; (w * h) as usize];
        for y in 0..h {
            for x in 0..w {
                let (lx, ly) = (x as i32 - ox, y as i32 - oy);
                let a = if lx < 0 || ly < 0 || lx >= w as i32 || ly >= h as i32 {
                    0
                } else {
                    let o = ((ly as u32 * w + lx as u32) * 4) as usize;
                    if o + 3 < layer.pixels.len() {
                        layer.pixels[o + 3]
                    } else {
                        0
                    }
                };
                if a > threshold {
                    mask[(y * w + x) as usize] = 255;
                }
            }
        }
        self.selection_mask = if mask.iter().any(|&b| b != 0) {
            Some(mask)
        } else {
            None
        };
        self.retighten_selection();
        self.selection_mask.is_some()
    }

    /// Paint the selection outline `width` px wide (clamped 1..=32) with
    /// `brush` at `color` through `put` (symmetry- and target-aware). The
    /// edge comes from the selection itself. Caller checkpoints for undo.
    /// Returns false without a selection.
    pub fn stroke_selection(
        &mut self,
        brush: &super::brush::Brush,
        color: super::brush::Color,
        width: u32,
    ) -> bool {
        let (w, h) = (self.w, self.h);
        if w == 0 || h == 0 {
            return false;
        }
        let inside = |m: &[u8], x: i32, y: i32| -> bool {
            if x < 0 || y < 0 || x >= w as i32 || y >= h as i32 {
                return false;
            }
            m[(y as u32 * w + x as u32) as usize] > 127
        };
        let mut edge = vec![false; (w * h) as usize];
        let mut any = false;
        match self.selection_mask.as_deref() {
            Some(m) if m.len() == (w * h) as usize => {
                for y in 0..h as i32 {
                    for x in 0..w as i32 {
                        if !inside(m, x, y) {
                            continue;
                        }
                        if !(inside(m, x - 1, y)
                            && inside(m, x + 1, y)
                            && inside(m, x, y - 1)
                            && inside(m, x, y + 1))
                        {
                            edge[(y as u32 * w + x as u32) as usize] = true;
                            any = true;
                        }
                    }
                }
            }
            _ => {
                if let Some((x0, y0, x1, y1)) = self.selection {
                    for x in x0..=x1.min(w - 1) {
                        for &y in &[y0, y1] {
                            if y < h {
                                edge[(y * w + x) as usize] = true;
                                any = true;
                            }
                        }
                    }
                    for y in y0..=y1.min(h - 1) {
                        for &x in &[x0, x1] {
                            if x < w {
                                edge[(y * w + x) as usize] = true;
                                any = true;
                            }
                        }
                    }
                }
            }
        }
        if !any {
            return false;
        }
        let r = width.clamp(1, 32) as i32 - 1;
        if r > 0 {
            let gray: Vec<u8> = edge.iter().map(|&b| if b { 255 } else { 0 }).collect();
            let fat = Self::morph_gray(&gray, w, h, r, true);
            edge = fat.into_iter().map(|v| v != 0).collect();
        }
        for y in 0..h {
            for x in 0..w {
                if edge[(y * w + x) as usize] {
                    self.put(brush, color, x, y);
                }
            }
        }
        self.mark_full_dirty();
        true
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
                for (ch, s) in sum.iter_mut().enumerate() {
                    *s += px(src, dx, y, ch);
                }
            }
            for x in 0..w {
                for ch in 0..4 {
                    tmp[((y as u32 * wu + x as u32) * 4) as usize + ch] = (sum[ch] / window) as u8;
                }
                for (ch, s) in sum.iter_mut().enumerate() {
                    *s += px(src, x + r + 1, y, ch);
                    *s -= px(src, x - r, y, ch);
                }
            }
        }
        // Vertical pass: tmp -> out.
        let mut out = vec![0u8; src.len()];
        for x in 0..w {
            let mut sum = [0u32; 4];
            for dy in -r..=r {
                for (ch, s) in sum.iter_mut().enumerate() {
                    *s += px(&tmp, x, dy, ch);
                }
            }
            for y in 0..h {
                for ch in 0..4 {
                    out[((y as u32 * wu + x as u32) * 4) as usize + ch] = (sum[ch] / window) as u8;
                }
                for (ch, s) in sum.iter_mut().enumerate() {
                    *s += px(&tmp, x, y + r + 1, ch);
                    *s -= px(&tmp, x, y - r, ch);
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
        if index >= self.layers.len() || (brightness == 0 && (contrast - 1.0).abs() < f32::EPSILON)
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
                // Graduated masks partially apply (binary masks: full strength).
                let k = Self::sel_k(mask, w, x, y);
                for ch in 0..3 {
                    let v = (pixels[o + ch] as f32 - 128.0) * contrast + 128.0 + brightness as f32;
                    let mixed = pixels[o + ch] as f32 * (1.0 - k) + v * k;
                    pixels[o + ch] = mixed.round().clamp(0.0, 255.0) as u8;
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
                let selected = !matches!(sel, Some((a, b, c, d)) if x < a || x > c || y < b || y > d)
                    && match mask {
                        Some(m) => m[(y * w + x) as usize] != 0,
                        None => true,
                    };
                if selected {
                    let o = ((y * w + x) * 4) as usize;
                    let k = Self::sel_k(mask, w, x, y);
                    for ch in 0..4 {
                        pixels[o + ch] = (pixels[o + ch] as f32 * (1.0 - k)
                            + blurred[o + ch] as f32 * k)
                            .round() as u8;
                    }
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
                let selected = !matches!(sel, Some((a, b, c, d)) if x < a || x > c || y < b || y > d)
                    && match mask {
                        Some(m) => m[(y * w + x) as usize] != 0,
                        None => true,
                    };
                if !selected {
                    continue;
                }
                let o = ((y * w + x) * 4) as usize;
                let k = Self::sel_k(mask, w, x, y);
                for ch in 0..3 {
                    let v = pixels[o + ch] as f32
                        + (pixels[o + ch] as f32 - blurred[o + ch] as f32) * amount;
                    let mixed = pixels[o + ch] as f32 * (1.0 - k) + v * k;
                    pixels[o + ch] = mixed.round().clamp(0.0, 255.0) as u8;
                }
            }
        }
        self.mark_full_dirty();
    }

    /// Save the composite as PNG.
    pub fn save_png(&self, path: &std::path::Path) -> Result<(), String> {
        image::save_buffer(
            path,
            &self.flatten(),
            self.w,
            self.h,
            image::ColorType::Rgba8,
        )
        .map_err(|e| e.to_string())
    }

    /// Save the composite as JPEG at `quality` (1-100, higher keeps more
    /// detail). JPEG stores no alpha; the composite is already opaque
    /// over the background, so this only strips the alpha channel.
    pub fn save_jpeg(&self, path: &std::path::Path, quality: u8) -> Result<(), String> {
        if !(1..=100).contains(&quality) {
            return Err(format!("bad JPEG quality: {quality} (want 1-100)"));
        }
        let flat = self.flatten();
        let mut rgb = Vec::with_capacity((self.w * self.h) as usize * 3);
        for i in 0..flat.len() / 4 {
            let o = i * 4;
            rgb.extend_from_slice(&flat[o..o + 3]);
        }
        let mut buf = Vec::new();
        {
            let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality);
            enc.encode(&rgb, self.w, self.h, image::ExtendedColorType::Rgb8)
                .map_err(|e| e.to_string())?;
        }
        std::fs::write(path, &buf).map_err(|e| e.to_string())
    }

    /// Save the composite as a plain single-image TIFF (lossless RGBA,
    /// no layers or extras — the layered original stays in `.procraft`).
    pub fn save_tiff(&self, path: &std::path::Path) -> Result<(), String> {
        use image::ImageEncoder;
        let flat = self.flatten();
        let mut buf = Vec::new();
        {
            let enc = image::codecs::tiff::TiffEncoder::new(std::io::Cursor::new(&mut buf));
            enc.write_image(&flat, self.w, self.h, image::ExtendedColorType::Rgba8)
                .map_err(|e| e.to_string())?;
        }
        std::fs::write(path, &buf).map_err(|e| e.to_string())
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
        self.undo_bytes = 0;
        self.redo_bytes = 0;
        Ok(())
    }

    /// Save the full project (layers, opacity, blend, background, symmetry)
    /// to a `.procraft` file: magic + little-endian records, zlib-compressed
    /// pixels. Version 8 adds layer groups (v1-v7 files still open with no
    /// groups; old app versions cleanly refuse v8 files at the magic).
    pub fn save_project(&self, path: &std::path::Path) -> Result<(), String> {
        use std::io::Write;
        let mut f = std::fs::File::create(path).map_err(|e| e.to_string())?;
        f.write_all(b"PCRAFT08").map_err(|e| e.to_string())?;
        f.write_all(&self.w.to_le_bytes())
            .map_err(|e| e.to_string())?;
        f.write_all(&self.h.to_le_bytes())
            .map_err(|e| e.to_string())?;
        f.write_all(&self.background).map_err(|e| e.to_string())?;
        f.write_all(&(self.layers.len() as u32).to_le_bytes())
            .map_err(|e| e.to_string())?;
        f.write_all(&(self.active as u32).to_le_bytes())
            .map_err(|e| e.to_string())?;
        let symmetry = match self.symmetry {
            SymmetryMode::Off => 0u8,
            SymmetryMode::Horizontal => 1u8,
            SymmetryMode::Vertical => 2u8,
            SymmetryMode::Both => 3u8,
            SymmetryMode::Kaleidoscope => 4u8,
        };
        f.write_all(&[symmetry]).map_err(|e| e.to_string())?;
        f.write_all(&[self.symmetry_segments.clamp(2, 16)])
            .map_err(|e| e.to_string())?;
        // v7 stores the mirror center (flag + two f32s); older files read
        // canvas middle.
        match self.symmetry_center {
            Some((cx, cy)) => {
                f.write_all(&[1u8]).map_err(|e| e.to_string())?;
                f.write_all(&cx.to_le_bytes()).map_err(|e| e.to_string())?;
                f.write_all(&cy.to_le_bytes()).map_err(|e| e.to_string())?;
            }
            None => {
                f.write_all(&[0u8]).map_err(|e| e.to_string())?;
            }
        }
        // v8 stores the layer-group table (count + records); older files
        // read an empty table.
        f.write_all(&(self.groups.len() as u32).to_le_bytes())
            .map_err(|e| e.to_string())?;
        for g in &self.groups {
            f.write_all(&g.id.to_le_bytes())
                .map_err(|e| e.to_string())?;
            match g.parent {
                Some(p) => {
                    f.write_all(&[1u8]).map_err(|e| e.to_string())?;
                    f.write_all(&p.to_le_bytes()).map_err(|e| e.to_string())?;
                }
                None => {
                    f.write_all(&[0u8]).map_err(|e| e.to_string())?;
                }
            }
            let name = g.name.as_bytes();
            f.write_all(&(name.len() as u32).to_le_bytes())
                .map_err(|e| e.to_string())?;
            f.write_all(name).map_err(|e| e.to_string())?;
            f.write_all(&[u8::from(g.visible)])
                .map_err(|e| e.to_string())?;
            f.write_all(&[u8::from(g.collapsed)])
                .map_err(|e| e.to_string())?;
        }
        for layer in &self.layers {
            let name = layer.name.as_bytes();
            f.write_all(&(name.len() as u32).to_le_bytes())
                .map_err(|e| e.to_string())?;
            f.write_all(name).map_err(|e| e.to_string())?;
            f.write_all(&[u8::from(layer.visible)])
                .map_err(|e| e.to_string())?;
            f.write_all(&layer.opacity.to_le_bytes())
                .map_err(|e| e.to_string())?;
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
            f.write_all(&[u8::from(layer.clip)])
                .map_err(|e| e.to_string())?;
            // v6 stores the canvas offset (Move tool) as two i32s.
            f.write_all(&layer.ox.to_le_bytes())
                .map_err(|e| e.to_string())?;
            f.write_all(&layer.oy.to_le_bytes())
                .map_err(|e| e.to_string())?;
            // v8 stores the owning group (flag + id); older files read none.
            match layer.group {
                Some(id) => {
                    f.write_all(&[1u8]).map_err(|e| e.to_string())?;
                    f.write_all(&id.to_le_bytes()).map_err(|e| e.to_string())?;
                }
                None => {
                    f.write_all(&[0u8]).map_err(|e| e.to_string())?;
                }
            }
            let expect = (self.w * self.h * 4) as usize;
            if layer.pixels.len() != expect {
                return Err("layer has wrong pixel size".to_string());
            }
            let mut enc =
                flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
            enc.write_all(&layer.pixels).map_err(|e| e.to_string())?;
            let packed = enc.finish().map_err(|e| e.to_string())?;
            f.write_all(&(packed.len() as u32).to_le_bytes())
                .map_err(|e| e.to_string())?;
            f.write_all(&packed).map_err(|e| e.to_string())?;
            // Optional reveal mask, zlib-compressed like the pixels.
            match layer.mask.as_deref() {
                Some(m) if m.len() == (self.w * self.h) as usize => {
                    f.write_all(&[1u8]).map_err(|e| e.to_string())?;
                    let mut enc =
                        flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
                    enc.write_all(m).map_err(|e| e.to_string())?;
                    let packed = enc.finish().map_err(|e| e.to_string())?;
                    f.write_all(&(packed.len() as u32).to_le_bytes())
                        .map_err(|e| e.to_string())?;
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
        let v8 = magic == b"PCRAFT08";
        let v7 = v8 || magic == b"PCRAFT07";
        let v6 = v7 || magic == b"PCRAFT06";
        let v5 = v6 || magic == b"PCRAFT05";
        let v4 = v5 || magic == b"PCRAFT04";
        let v3 = v4 || magic == b"PCRAFT03";
        let v2 = v3 || magic == b"PCRAFT02";
        if magic != b"PCRAFT01" && !v2 {
            return Err("not a ProCraft project file".to_string());
        }
        let w = u32::from_le_bytes(
            take(4)?[..4]
                .try_into()
                .map_err(|_| "bad width".to_string())?,
        );
        let h = u32::from_le_bytes(
            take(4)?[..4]
                .try_into()
                .map_err(|_| "bad height".to_string())?,
        );
        if w == 0 || h == 0 || w > 16384 || h > 16384 {
            return Err("project has bad dimensions".to_string());
        }
        let bg: [u8; 4] = take(4)?[..4]
            .try_into()
            .map_err(|_| "bad background".to_string())?;
        let count = u32::from_le_bytes(
            take(4)?[..4]
                .try_into()
                .map_err(|_| "bad layer count".to_string())?,
        );
        if count == 0 || count > 256 {
            return Err("project has bad layer count".to_string());
        }
        let active = u32::from_le_bytes(
            take(4)?[..4]
                .try_into()
                .map_err(|_| "bad active layer".to_string())?,
        ) as usize;
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
            if (2..=16).contains(&s) {
                s
            } else {
                6
            }
        } else {
            6
        };
        // v7 appends the mirror center (flag + two f32s); older files read
        // canvas middle. Non-finite values fall back to the default.
        let symmetry_center = if v7 {
            let flag = take(1)?[0];
            if flag == 0 {
                None
            } else {
                let cx = f32::from_le_bytes(
                    take(4)?[..4]
                        .try_into()
                        .map_err(|_| "bad symmetry center".to_string())?,
                );
                let cy = f32::from_le_bytes(
                    take(4)?[..4]
                        .try_into()
                        .map_err(|_| "bad symmetry center".to_string())?,
                );
                if cx.is_finite() && cy.is_finite() {
                    Some((cx.clamp(-16384.0, 16384.0), cy.clamp(-16384.0, 16384.0)))
                } else {
                    None
                }
            }
        } else {
            None
        };
        // v8 appends the layer-group table after the mirror center;
        // older files read an empty table. Unknown parents sanitize to
        // top-level so corrupt ids can never orphan a group.
        let mut groups = Vec::new();
        if v8 {
            let gcount = u32::from_le_bytes(
                take(4)?[..4]
                    .try_into()
                    .map_err(|_| "bad group count".to_string())?,
            ) as usize;
            if gcount > 256 {
                return Err("project has bad group count".to_string());
            }
            let mut seen = Vec::new();
            for _ in 0..gcount {
                let id = u32::from_le_bytes(
                    take(4)?[..4]
                        .try_into()
                        .map_err(|_| "bad group id".to_string())?,
                );
                if id == 0 || seen.contains(&id) {
                    return Err("project has bad group id".to_string());
                }
                seen.push(id);
                let parent = if take(1)?[0] == 0 {
                    None
                } else {
                    Some(u32::from_le_bytes(
                        take(4)?[..4]
                            .try_into()
                            .map_err(|_| "bad group parent".to_string())?,
                    ))
                };
                let name_len = u32::from_le_bytes(
                    take(4)?[..4]
                        .try_into()
                        .map_err(|_| "bad group name".to_string())?,
                ) as usize;
                if name_len > 512 {
                    return Err("project has bad group name".to_string());
                }
                let name = String::from_utf8(take(name_len)?.to_vec())
                    .map_err(|_| "bad group name".to_string())?;
                let visible = take(1)?[0] != 0;
                let collapsed = take(1)?[0] != 0;
                groups.push(LayerGroup {
                    id,
                    name,
                    visible,
                    collapsed,
                    parent,
                });
            }
            for g in groups.iter_mut() {
                if let Some(p) = g.parent {
                    if !seen.contains(&p) {
                        g.parent = None;
                    }
                }
            }
        }
        let mut layers = Vec::new();
        for _ in 0..count {
            let name_len = u32::from_le_bytes(
                take(4)?[..4]
                    .try_into()
                    .map_err(|_| "bad name".to_string())?,
            ) as usize;
            if name_len > 512 {
                return Err("project has bad layer name".to_string());
            }
            let name = String::from_utf8(take(name_len)?.to_vec())
                .map_err(|_| "bad layer name".to_string())?;
            let visible = take(1)?[0] != 0;
            let opacity = f32::from_le_bytes(
                take(4)?[..4]
                    .try_into()
                    .map_err(|_| "bad opacity".to_string())?,
            )
            .clamp(0.0, 1.0);
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
            // v6 appends the canvas offset (two i32s); older files read 0.
            let (ox, oy) = if v6 {
                let ox = i32::from_le_bytes(
                    take(4)?[..4]
                        .try_into()
                        .map_err(|_| "bad layer offset".to_string())?,
                )
                .clamp(-16384, 16384);
                let oy = i32::from_le_bytes(
                    take(4)?[..4]
                        .try_into()
                        .map_err(|_| "bad layer offset".to_string())?,
                )
                .clamp(-16384, 16384);
                (ox, oy)
            } else {
                (0, 0)
            };
            // v8 appends the owning group (flag + id) after the offset;
            // older files read top-level. Unknown ids sanitize to none.
            let group = if v8 {
                if take(1)?[0] == 0 {
                    None
                } else {
                    Some(u32::from_le_bytes(
                        take(4)?[..4]
                            .try_into()
                            .map_err(|_| "bad layer group".to_string())?,
                    ))
                }
            } else {
                None
            };
            let expect = (w * h * 4) as usize;
            let pixels = if v4 {
                let comp_len = u32::from_le_bytes(
                    take(4)?[..4]
                        .try_into()
                        .map_err(|_| "bad compressed size".to_string())?,
                ) as usize;
                if comp_len == 0 || comp_len > expect.saturating_add(10 * 1024 * 1024) {
                    return Err("project has bad compressed size".to_string());
                }
                let packed = take(comp_len)?.to_vec();
                let mut dec = flate2::read::ZlibDecoder::new(&packed[..]);
                let mut raw = Vec::with_capacity(expect);
                use std::io::Read;
                dec.read_to_end(&mut raw)
                    .map_err(|_| "project has corrupt pixels".to_string())?;
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
                        take(4)?[..4]
                            .try_into()
                            .map_err(|_| "bad mask size".to_string())?,
                    ) as usize;
                    if comp_len == 0 || comp_len > want.saturating_add(10 * 1024 * 1024) {
                        return Err("project has bad mask size".to_string());
                    }
                    let packed = take(comp_len)?.to_vec();
                    let mut dec = flate2::read::ZlibDecoder::new(&packed[..]);
                    let mut raw = Vec::with_capacity(want);
                    use std::io::Read;
                    dec.read_to_end(&mut raw)
                        .map_err(|_| "project has corrupt mask".to_string())?;
                    if raw.len() != want {
                        return Err("project has corrupt mask".to_string());
                    }
                    Some(raw)
                }
            } else {
                None
            };
            // Unknown group ids sanitize to top-level so a corrupt byte
            // can never orphan a layer.
            let known = groups.iter().map(|g| g.id).collect::<Vec<_>>();
            let group = group.filter(|id| known.contains(id));
            layers.push(Layer {
                name,
                visible,
                opacity,
                blend,
                pixels,
                mask,
                clip,
                ox,
                oy,
                group,
            });
        }
        if !cur.is_empty() {
            return Err("project has trailing data".to_string());
        }
        let next_group_id = groups
            .iter()
            .map(|g| g.id)
            .max()
            .map_or(1, |m| m.saturating_add(1).max(1));
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
            symmetry_center,
            paint_target: PaintTarget::Color,
            dirty: None,
            undo: Vec::new(),
            redo: Vec::new(),
            undo_bytes: 0,
            redo_bytes: 0,
            groups,
            next_group_id,
            smudge_finger: None,
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
    fn group_hide_conceals_members_in_flatten() {
        let mut doc = Document::new(2, 1);
        doc.layers[0].pixels = vec![255, 0, 0, 255, 255, 0, 0, 255];
        doc.add_layer("top".to_string());
        doc.layers[1].pixels = vec![0, 0, 255, 255, 0, 0, 255, 255];
        let g = doc.new_group("G".to_string());
        doc.move_layer_into_group(1, Some(g));
        assert!(doc.is_layer_shown(1));
        doc.set_group_visible(g, false);
        assert!(!doc.is_layer_shown(1));
        assert!(doc.is_layer_shown(0));
        // Top pixel falls through to the red layer below.
        assert_eq!(&doc.flatten()[0..4], &[255, 0, 0, 255]);
        doc.set_group_visible(g, true);
        assert_eq!(&doc.flatten()[0..4], &[0, 0, 255, 255]);
    }

    #[test]
    fn nested_groups_need_every_level_visible() {
        let mut doc = Document::new(1, 1);
        let outer = doc.new_group("outer".to_string());
        let inner = doc.new_group("inner".to_string());
        doc.groups
            .iter_mut()
            .find(|g| g.id == inner)
            .unwrap()
            .parent = Some(outer);
        doc.move_layer_into_group(0, Some(inner));
        assert!(doc.is_layer_shown(0));
        doc.set_group_visible(inner, false);
        assert!(!doc.is_layer_shown(0));
        doc.set_group_visible(inner, true);
        doc.set_group_visible(outer, false);
        assert!(!doc.is_layer_shown(0));
    }

    #[test]
    fn ungroup_spills_members_up_one_level() {
        let mut doc = Document::new(1, 1);
        doc.add_layer("b".to_string());
        let outer = doc.new_group("outer".to_string());
        let inner = doc.new_group("inner".to_string());
        doc.groups
            .iter_mut()
            .find(|g| g.id == inner)
            .unwrap()
            .parent = Some(outer);
        doc.move_layer_into_group(0, Some(inner));
        doc.move_layer_into_group(0, Some(outer));
        doc.ungroup(inner);
        assert!(doc.group(inner).is_none());
        assert_eq!(doc.layers[0].group, Some(outer));
        assert_eq!(doc.layers[1].group, Some(outer));
        doc.ungroup(outer);
        assert!(doc.groups.is_empty());
        assert!(doc.layers.iter().all(|l| l.group.is_none()));
    }

    #[test]
    fn move_into_group_lands_adjacent() {
        let mut doc = Document::new(1, 1);
        doc.add_layer("b".to_string());
        doc.add_layer("c".to_string());
        let g = doc.new_group("G".to_string());
        // Bottom layer joins the group: inserted at the top end.
        doc.move_layer_into_group(0, Some(g));
        assert_eq!(doc.layers[2].name, "Layer 1");
        assert_eq!(doc.layers[2].group, Some(g));
        assert_eq!(doc.active, 2);
        // Unknown groups and indices are ignored.
        doc.move_layer_into_group(0, Some(999));
        assert!(doc.layers[0].group.is_none());
        doc.move_layer_into_group(99, Some(g));
        assert_eq!(doc.layers.len(), 3);
    }

    #[test]
    fn v8_roundtrip_keeps_groups() {
        let mut doc = Document::new(2, 1);
        let g = doc.new_group("Sketch".to_string());
        doc.set_group_visible(g, false);
        doc.move_layer_into_group(0, Some(g));
        let dir = std::env::temp_dir();
        let path = dir.join("procraft-v8.pcraft");
        doc.save_project(&path).unwrap();
        let head = std::fs::read(&path).unwrap();
        assert_eq!(&head[..8], b"PCRAFT08");
        let back = Document::open_project(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(back.groups.len(), 1);
        assert_eq!(back.groups[0].name, "Sketch");
        assert!(!back.groups[0].visible);
        assert_eq!(back.layers[0].group, Some(g));
        assert!(!back.is_layer_shown(0));
        // Ids keep increasing after a reload (no reuse).
        let mut back = back;
        let g2 = back.new_group("More".to_string());
        assert!(g2 > g);
    }

    #[test]
    fn jpeg_export_decodes_at_canvas_size() {
        let mut doc = Document::new(16, 12);
        // Solid red: lossy error stays one-sided and easy to bound.
        let px = &mut doc.layers[0].pixels;
        for i in 0..px.len() / 4 {
            let o = i * 4;
            px[o] = 255;
            px[o + 1] = 0;
            px[o + 2] = 0;
            px[o + 3] = 255;
        }
        let dir = std::env::temp_dir();
        let path = dir.join("procraft-m28.jpg");
        doc.save_jpeg(&path, 90).unwrap();
        let raw = std::fs::read(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(&raw[..2], &[0xFF, 0xD8]); // JPEG SOI marker
        let img = image::load_from_memory(&raw).unwrap().to_rgb8();
        assert_eq!((img.width(), img.height()), (16, 12));
        let p = img.get_pixel(8, 6);
        assert!(p[0] > 200 && p[1] < 60 && p[2] < 60);
    }

    #[test]
    fn jpeg_rejects_bad_quality() {
        let doc = Document::new(4, 4);
        let dir = std::env::temp_dir();
        assert!(doc.save_jpeg(&dir.join("procraft-m28-q0.jpg"), 0).is_err());
        assert!(doc
            .save_jpeg(&dir.join("procraft-m28-q101.jpg"), 101)
            .is_err());
    }

    #[test]
    fn tiff_export_roundtrips_lossless() {
        let mut doc = Document::new(8, 6);
        let px = &mut doc.layers[0].pixels;
        for i in 0..px.len() / 4 {
            let o = i * 4;
            px[o] = (i % 251) as u8;
            px[o + 1] = 17;
            px[o + 2] = 200;
            px[o + 3] = 255;
        }
        let dir = std::env::temp_dir();
        let path = dir.join("procraft-m28.tiff");
        doc.save_tiff(&path).unwrap();
        let raw = std::fs::read(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert!(raw.starts_with(b"II*\0") || raw.starts_with(b"MM\0*"));
        let img = image::load_from_memory(&raw).unwrap().to_rgba8();
        assert_eq!((img.width(), img.height()), (8, 6));
        assert_eq!(img.as_raw(), &doc.flatten());
    }

    #[test]
    fn v7_project_opens_groupless() {
        // Hand-build a minimal version-7 file (center flag, no groups).
        use std::io::Write;
        let dir = std::env::temp_dir();
        let path = dir.join("procraft-v7-compat.pcraft");
        let mut buf = Vec::new();
        buf.extend_from_slice(b"PCRAFT07");
        buf.extend_from_slice(&2u32.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&[255, 255, 255, 255]);
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&[0]); // Symmetry off.
        buf.extend_from_slice(&[6]);
        buf.extend_from_slice(&[0]); // No mirror center.
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(b"L");
        buf.extend_from_slice(&[1]);
        buf.extend_from_slice(&1.0f32.to_le_bytes());
        buf.extend_from_slice(&[0]);
        buf.extend_from_slice(&[0]);
        buf.extend_from_slice(&0i32.to_le_bytes());
        buf.extend_from_slice(&0i32.to_le_bytes());
        let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(&[9u8; 2 * 4]).unwrap();
        let packed = enc.finish().unwrap();
        buf.extend_from_slice(&(packed.len() as u32).to_le_bytes());
        buf.extend_from_slice(&packed);
        buf.extend_from_slice(&[0]); // No mask.
        std::fs::write(&path, &buf).unwrap();
        let back = Document::open_project(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert!(back.groups.is_empty());
        assert!(back.layers[0].group.is_none());
        assert!(back.is_layer_shown(0));
        assert_eq!(back.layers[0].pixels, vec![9u8; 8]);
    }

    #[test]
    fn undo_keeps_count_cap_on_small_canvas() {
        let mut doc = Document::new(16, 16);
        for _ in 0..40 {
            doc.checkpoint();
        }
        assert_eq!(doc.undo.len(), 32);
        assert!(doc.undo_bytes <= MAX_UNDO_BYTES);
    }

    #[test]
    fn undo_byte_budget_binds_on_large_canvas() {
        // 2048x2048 RGBA = 16 MiB per step: the 256 MiB budget caps at 16,
        // well under the 32-count backstop.
        let mut doc = Document::new(2048, 2048);
        for _ in 0..40 {
            doc.checkpoint();
        }
        assert_eq!(doc.undo.len(), 16);
        assert!(doc.undo_bytes <= MAX_UNDO_BYTES);
        // The newest step is still undoable after budget eviction.
        assert!(doc.undo());
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
        doc.layers[0].pixels = vec![
            255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255,
        ];
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
        doc.layers[0].pixels[7] = 255;
        doc.layers[0].pixels[4] = 255;
        doc.add_layer("top".to_string());
        doc.layers[1].pixels = [0, 255, 0, 255].repeat(4);
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
        let brush = Brush {
            size: 8.0,
            softness: 0.0,
            ..Default::default()
        };
        assert!(doc.layers[0].mask.is_none());
        doc.checkpoint();
        doc.dab(&brush, Color::rgb(0, 0, 0), 8.0, 8.0);
        // Black paint concealed the middle; mask auto-materialized.
        let m = doc.layers[0].mask.as_ref().expect("mask created");
        assert!(m.iter().any(|&b| b < 255));
        // Eraser reveals back toward white.
        let before: Vec<u8> = m.clone();
        let eraser = Brush {
            eraser: true,
            ..brush
        };
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
    fn v6_roundtrip_keeps_masks_clips_and_blends() {
        let mut doc = Document::new(4, 2);
        doc.layers[0].blend = BlendMode::Difference;
        doc.layers[0].pixels = vec![200; 4 * 2 * 4];
        doc.add_mask(0);
        doc.layers[0].mask.as_mut().unwrap()[3] = 0;
        doc.add_layer("clipped".to_string());
        doc.layers[1].blend = BlendMode::SoftLight;
        doc.layers[1].clip = true;
        doc.layers[1].pixels = vec![7; 4 * 2 * 4];
        doc.layers[1].ox = -3;
        doc.layers[1].oy = 2;
        let dir = std::env::temp_dir();
        let path = dir.join("procraft-v6.pcraft");
        doc.save_project(&path).unwrap();
        let head = std::fs::read(&path).unwrap();
        assert_eq!(&head[..8], b"PCRAFT08");
        let back = Document::open_project(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(back.layers[0].blend, BlendMode::Difference);
        assert_eq!(back.layers[0].mask.as_ref().unwrap()[3], 0);
        assert_eq!(back.layers[1].blend, BlendMode::SoftLight);
        assert!(back.layers[1].clip);
        assert_eq!(back.layers[1].pixels, vec![7; 4 * 2 * 4]);
        assert_eq!((back.layers[1].ox, back.layers[1].oy), (-3, 2));
    }

    #[test]
    fn v5_files_open_with_zero_offsets() {
        // Minimal v5 file: like v4 plus clip byte, no ox/oy fields.
        let dir = std::env::temp_dir();
        let path = dir.join("procraft-v5-compat.pcraft");
        let mut buf = Vec::new();
        buf.extend_from_slice(b"PCRAFT05");
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
        buf.extend_from_slice(&[0]); // blend: Normal
        buf.extend_from_slice(&[0]); // clip: off
        let raw = vec![9u8; 2 * 4];
        let packed = {
            use std::io::Write;
            let mut enc =
                flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
            enc.write_all(&raw).unwrap();
            enc.finish().unwrap()
        };
        buf.extend_from_slice(&(packed.len() as u32).to_le_bytes());
        buf.extend_from_slice(&packed);
        buf.extend_from_slice(&[0]); // mask flag: none
        std::fs::write(&path, &buf).unwrap();
        let back = Document::open_project(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(back.layers[0].pixels, raw);
        assert_eq!((back.layers[0].ox, back.layers[0].oy), (0, 0));
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
        let raw = vec![9u8; 2 * 4];
        let packed = {
            use std::io::Write;
            let mut enc =
                flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
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
        // With offsets, mask data doesn't move; offset changes instead.
        assert_eq!(doc.layers[0].ox, 1);
        assert_eq!(doc.layers[0].mask.as_ref().unwrap()[0], 0);
        doc.flip_layer_horizontal(0);
        // 4-pixel wide: horizontal flip mirrors mask indices 0<->3, 1<->2.
        assert_eq!(doc.layers[0].mask.as_ref().unwrap()[3], 0);
        assert_eq!(doc.layers[0].ox, -1);
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
        doc.set_symmetry_center(1.5, 1.0);
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
        assert_eq!(back.symmetry_center, Some((1.5, 1.0)));
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
        buf.extend_from_slice(&[9u8; 2 * 4]);
        std::fs::write(&path, &buf).unwrap();
        let back = Document::open_project(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!((back.w, back.h), (2, 1));
        assert_eq!(back.symmetry, SymmetryMode::Off);
        assert_eq!(back.layers[0].pixels, vec![9u8; 8]);
    }

    #[test]
    fn moved_mirror_center_shifts_paint() {
        use crate::brush::{Brush, Color};
        let mut doc = Document::new(21, 21);
        doc.symmetry = SymmetryMode::Horizontal;
        // Default middle: x=2 mirrors to x=18.
        assert!(SymmetryMode::Horizontal
            .mirrored_pixels(21, 21, 6, 10.0, 10.0, 2, 5)
            .contains(&(18, 5)));
        // Moved center x=5: x=2 mirrors to x=8 instead.
        doc.set_symmetry_center(5.0, 10.0);
        assert_eq!(doc.symmetry_center_px(), (5.0, 10.0));
        let brush = Brush {
            size: 3.0,
            softness: 0.0,
            opacity: 1.0,
            ..Default::default()
        };
        doc.dab(&brush, Color::rgb(255, 0, 0), 2.5, 5.5);
        let at = |d: &Document, x: u32, y: u32| d.layers[0].pixels[((y * 21 + x) * 4 + 3) as usize];
        assert!(at(&doc, 2, 5) > 0);
        assert!(at(&doc, 8, 5) > 0);
        assert_eq!(at(&doc, 18, 5), 0);
        // Reset restores the middle; out-of-range values clamp.
        doc.reset_symmetry_center();
        assert_eq!(doc.symmetry_center, None);
        doc.set_symmetry_center(1e9, -1e9);
        assert_eq!(doc.symmetry_center, Some((16384.0, -16384.0)));
    }

    #[test]
    fn v6_project_opens_with_centered_symmetry() {
        // Hand-build a minimal version-6 file (no center bytes).
        use std::io::Write;
        let dir = std::env::temp_dir();
        let path = dir.join("procraft-v6-compat.pcraft");
        let mut buf = Vec::new();
        buf.extend_from_slice(b"PCRAFT06");
        buf.extend_from_slice(&2u32.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&[255, 255, 255, 255]);
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&[1]); // Horizontal.
        buf.extend_from_slice(&[6]);
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(b"L");
        buf.extend_from_slice(&[1]);
        buf.extend_from_slice(&1.0f32.to_le_bytes());
        buf.extend_from_slice(&[0]);
        buf.extend_from_slice(&[0]);
        buf.extend_from_slice(&0i32.to_le_bytes());
        buf.extend_from_slice(&0i32.to_le_bytes());
        let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(&[9u8; 2 * 4]).unwrap();
        let packed = enc.finish().unwrap();
        buf.extend_from_slice(&(packed.len() as u32).to_le_bytes());
        buf.extend_from_slice(&packed);
        buf.extend_from_slice(&[0]); // No mask.
        std::fs::write(&path, &buf).unwrap();
        let back = Document::open_project(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(back.symmetry, SymmetryMode::Horizontal);
        assert_eq!(back.symmetry_center, None);
        assert_eq!(back.symmetry_center_px(), (0.5, 0.0));
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
        // With offsets, pixels don't move; offset changes instead.
        assert_eq!(doc.layers[0].ox, 2);
        assert_eq!(&doc.layers[0].pixels[0..4], &[255, 0, 0, 255]);
    }

    #[test]
    fn move_off_canvas_and_back_restores() {
        // The reported Move bug: content pushed fully off-canvas vanished
        // on return because translate cropped destructively. Offsets keep
        // every pixel, so the round trip is exact.
        let mut doc = Document::new(8, 4);
        doc.layers[0].pixels[0..4].copy_from_slice(&[255, 0, 0, 255]);
        doc.layers[0].pixels[4..8].copy_from_slice(&[0, 255, 0, 255]);
        let want = doc.layers[0].pixels.clone();
        // Paint shows at the origin.
        assert_eq!(&doc.flatten()[0..4], &[255, 0, 0, 255]);
        // Fully off-canvas: composite shows only the page, buffer intact.
        doc.translate_layer(0, 100, 0);
        assert!(doc
            .flatten()
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| p == &[255, 255, 255, 255]));
        assert_eq!(doc.layers[0].pixels, want);
        // Back home: pixel-perfect, buffer untouched throughout.
        doc.translate_layer(0, -100, 0);
        assert_eq!((doc.layers[0].ox, doc.layers[0].oy), (0, 0));
        assert_eq!(doc.layers[0].pixels, want);
        assert_eq!(&doc.flatten()[0..4], &[255, 0, 0, 255]);
        assert_eq!(&doc.flatten()[4..8], &[0, 255, 0, 255]);
        // Partially off-canvas: visible part composites, rest survives.
        doc.translate_layer(0, -1, 0);
        assert_eq!(&doc.flatten()[0..4], &[0, 255, 0, 255]);
        doc.translate_layer(0, 1, 0);
        assert_eq!(&doc.flatten()[0..4], &[255, 0, 0, 255]);
    }

    #[test]
    fn move_undo_redo_restores_offset() {
        let mut doc = Document::new(8, 8);
        doc.checkpoint();
        doc.translate_layer(0, 30, -12);
        assert_eq!((doc.layers[0].ox, doc.layers[0].oy), (30, -12));
        assert!(doc.undo());
        assert_eq!((doc.layers[0].ox, doc.layers[0].oy), (0, 0));
        assert!(doc.redo());
        assert_eq!((doc.layers[0].ox, doc.layers[0].oy), (30, -12));
    }

    #[test]
    fn paint_after_move_lands_shifted() {
        // Brush input is canvas-space; storage follows the layer offset,
        // so paint appears under the cursor on moved layers.
        use crate::brush::{Brush, Color};
        let mut doc = Document::new(32, 32);
        doc.translate_layer(0, 5, 0);
        let brush = Brush {
            size: 3.0,
            softness: 0.0,
            opacity: 1.0,
            ..Default::default()
        };
        doc.dab(&brush, Color::rgb(255, 0, 0), 10.5, 10.5);
        // Composite shows paint at the canvas dab point...
        let at = |d: &Document, x: u32, y: u32| {
            let o = ((y * 32 + x) * 4) as usize;
            [
                d.flatten()[o],
                d.flatten()[o + 1],
                d.flatten()[o + 2],
                d.flatten()[o + 3],
            ]
        };
        assert_eq!(at(&doc, 10, 10), [255, 0, 0, 255]);
        // ...stored shifted in the buffer (local 5, 10).
        let o = ((10 * 32 + 5) * 4) as usize;
        assert_eq!(&doc.layers[0].pixels[o..o + 4], &[255, 0, 0, 255]);
        // And off-canvas dabs on a shifted layer still paint nothing.
        let before = doc.layers[0].pixels.clone();
        doc.translate_layer(0, 1000, 0);
        doc.dab(&brush, Color::rgb(0, 255, 0), 10.5, 10.5);
        assert_eq!(doc.layers[0].pixels, before);
    }

    #[test]
    fn merge_with_offsets_bakes_positions() {
        // dst blue at canvas 0; src red lives at buffer 0 but ox=1.
        let mut doc = Document::new(3, 1);
        doc.layers[0].pixels[0..4].copy_from_slice(&[0, 0, 255, 255]);
        doc.add_layer("top".to_string());
        doc.layers[1].pixels[0..4].copy_from_slice(&[255, 0, 0, 255]);
        doc.layers[1].ox = 1;
        assert!(doc.merge_down(1));
        // Red baked at canvas 1 (buffer 1), blue kept at canvas 0.
        assert_eq!(&doc.layers[0].pixels[0..4], &[0, 0, 255, 255]);
        assert_eq!(&doc.layers[0].pixels[4..8], &[255, 0, 0, 255]);
        assert_eq!((doc.layers[0].ox, doc.layers[0].oy), (0, 0));
    }

    #[test]
    fn flip_mirrors_offset_about_center() {
        let mut doc = Document::new(8, 8);
        doc.layers[0].ox = 3;
        doc.layers[0].oy = -2;
        doc.flip_layer_horizontal(0);
        assert_eq!((doc.layers[0].ox, doc.layers[0].oy), (-3, -2));
        doc.flip_layer_vertical(0);
        assert_eq!((doc.layers[0].ox, doc.layers[0].oy), (-3, 2));
    }

    #[test]
    fn mask_view_follows_offset() {
        let mut doc = Document::new(4, 1);
        doc.add_mask(0);
        doc.layers[0].mask.as_mut().unwrap()[0] = 200;
        // Unshifted: viewer shows the value at canvas 0.
        assert_eq!(&doc.mask_view().unwrap()[0..4], &[200, 200, 200, 255]);
        // Shifted right: canvas 0 reads concealed, canvas 1 shows it.
        doc.translate_layer(0, 1, 0);
        let view = doc.mask_view().unwrap();
        assert_eq!(&view[0..4], &[0, 0, 0, 255]);
        assert_eq!(&view[4..8], &[200, 200, 200, 255]);
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
        assert!(doc.layers[0]
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| p[0] == 100 && p[3] == 100));
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
        assert!(
            at(&doc, 4, 4) > 200 || at(&doc, 4, 4) == 255,
            "bright edge pixel rises"
        );
        // Flat area is untouched by sharpening.
        assert_eq!(at(&doc, 0, 0), 100);
        assert_eq!(at(&doc, 7, 7), 200);
    }

    #[test]
    fn filters_respect_selection() {
        let mut doc = Document::new(6, 6);
        for b in doc.layers[0].pixels.as_chunks_mut::<4>().0 {
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
        let at = |m: SymmetryMode, seg: u8| m.mirrored_pixels(21, 21, seg, 10.0, 10.0, 5, 5).len();
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
        let brush = Brush {
            size: 3.0,
            softness: 0.0,
            opacity: 1.0,
            ..Default::default()
        };
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
        buf.extend_from_slice(&[5u8; 2 * 4]);
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
        let brush = Brush {
            opacity: 1.0,
            ..Default::default()
        };
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
        let brush = Brush {
            opacity: 1.0,
            ..Default::default()
        };
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
        buf.extend_from_slice(&[6u8; 2 * 4]);
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
        assert!(
            meta.len() as usize * 4 < raw,
            "compressed {} vs raw {raw}",
            meta.len()
        );
        let head = std::fs::read(&path).unwrap();
        assert_eq!(&head[..8], b"PCRAFT08");
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
        let brush = Brush {
            size: 8.0,
            ..Default::default()
        };
        let clean = doc.layers[0].pixels.clone();
        for (from, to) in [
            (Some((100.0, 100.0)), (400.0, 400.0)),   // right-bottom
            (Some((-400.0, 100.0)), (-100.0, 100.0)), // left
            (Some((100.0, -400.0)), (100.0, -100.0)), // above
            (None, (1000.0, 1000.0)),                 // single dab far outside
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
        let stamp = Brush {
            size: 16.0,
            tip: Some(tip),
            ..Brush::default()
        };
        doc.stroke_to(
            &stamp,
            Color::rgb(1, 2, 3),
            Some((200.0, 200.0)),
            (500.0, 500.0),
            0.0,
        );
        assert_eq!(doc.layers[0].pixels, clean);
        doc.paint_target = PaintTarget::Mask;
        doc.stroke_to(
            &brush,
            Color::rgb(0, 0, 0),
            Some((200.0, 200.0)),
            (500.0, 500.0),
            0.0,
        );
        assert!(doc.layers[0].mask.is_none());
        doc.paint_target = PaintTarget::Color;
    }

    fn selected_rect(w: u32, h: u32, r: (u32, u32, u32, u32)) -> Document {
        let mut doc = Document::new(w, h);
        doc.selection = Some(r);
        doc
    }

    #[test]
    fn lasso_writes_full_strength_masks() {
        let mut doc = Document::new(10, 10);
        doc.set_lasso(&[(2.0, 2.0), (2.0, 5.0), (5.0, 5.0), (5.0, 2.0)]);
        let mask = doc.selection_mask.clone().unwrap();
        assert!(mask.iter().all(|&b| b == 0 || b == 255));
        assert_eq!(
            doc.selection_pixel_count(),
            mask.iter().filter(|&&b| b != 0).count()
        );
    }

    #[test]
    fn feather_ramps_and_expands_bounds() {
        let mut doc = selected_rect(20, 20, (5, 5, 14, 14));
        assert!(doc.feather_selection(3));
        let m = doc.selection_mask.clone().unwrap();
        let at = |x: u32, y: u32| m[(y * 20 + x) as usize];
        // Deep interior stays full, far exterior stays empty.
        assert_eq!(at(10, 10), 255);
        assert_eq!(at(0, 0), 0);
        // The old hard edge is now a ramp (partial coverage just outside).
        let ramp = at(3, 10);
        assert!(ramp > 0 && ramp < 255, "{ramp}");
        // Bounds grew to contain the falloff; coverage is conserved by blur.
        assert_eq!(doc.selection, Some((2, 2, 17, 17)));
        let count = doc.selection_pixel_count() as i32;
        assert!((count - 100).abs() <= 2, "{count}");
        // Radius 0 only normalizes, and empty selections report false.
        let mut doc2 = selected_rect(20, 20, (5, 5, 14, 14));
        assert!(doc2.feather_selection(0));
        assert_eq!(doc2.selection_pixel_count(), 100);
        let mut doc3 = Document::new(8, 8);
        assert!(!doc3.feather_selection(4));
    }

    #[test]
    fn grow_and_shrink_track_bounds() {
        let mut doc = selected_rect(20, 20, (5, 5, 14, 14));
        assert!(doc.grow_selection(2));
        assert_eq!(doc.selection, Some((3, 3, 16, 16)));
        assert!(doc.selection_pixel_count() > 100);
        assert!(doc.grow_selection(-2));
        assert_eq!(doc.selection, Some((5, 5, 14, 14)));
        assert_eq!(doc.selection_pixel_count(), 100);
        // Shrinking to nothing clears everything.
        assert!(!doc.grow_selection(-50));
        assert!(doc.selection.is_none() && doc.selection_mask.is_none());
        // Zero on nothing stays nothing.
        assert!(!doc.grow_selection(0));
        let mut doc2 = Document::new(8, 8);
        assert!(!doc2.grow_selection(3));
    }

    #[test]
    fn smooth_fills_pinholes_keeps_solids() {
        let mut doc = Document::new(12, 12);
        doc.selection_mask = Some(vec![255u8; 144]);
        doc.selection_mask.as_mut().unwrap()[5 * 12 + 5] = 0;
        assert!(doc.smooth_selection());
        // Single-pixel hole heals (blur r2 keeps it over threshold).
        assert_eq!(doc.selection_mask.clone().unwrap()[5 * 12 + 5], 255);
        // Solid rects survive smoothing byte-identical in the interior.
        let mut doc2 = selected_rect(20, 20, (5, 5, 14, 14));
        assert!(doc2.smooth_selection());
        assert_eq!(
            doc2.selection_mask.clone().unwrap()[(10 * 20 + 10) as usize],
            255
        );
        let mut doc3 = Document::new(8, 8);
        assert!(!doc3.smooth_selection());
    }

    #[test]
    fn invert_flips_to_full_canvas_complement() {
        let mut doc = selected_rect(10, 10, (2, 2, 5, 5));
        assert!(doc.invert_selection());
        let m = doc.selection_mask.clone().unwrap();
        assert_eq!(m[(3 * 10 + 3) as usize], 0);
        assert_eq!(m[0], 255);
        assert_eq!(doc.selection, Some((0, 0, 9, 9)));
        assert_eq!(doc.selection_pixel_count(), 100 - 16);
        let mut doc2 = Document::new(8, 8);
        assert!(!doc2.invert_selection());
    }

    #[test]
    fn opaque_selects_painted_pixels() {
        use crate::brush::{Brush, Color};
        let mut doc = Document::new(10, 10);
        let brush = Brush {
            size: 3.0,
            softness: 0.0,
            opacity: 1.0,
            ..Default::default()
        };
        doc.dab(&brush, Color::rgb(255, 0, 0), 2.5, 2.5);
        assert!(doc.select_opaque(8));
        let n = doc.selection_pixel_count();
        assert!(n > 0 && n < 100, "{n}");
        // Bounds hug the dab.
        let (x0, y0, x1, y1) = doc.selection.unwrap();
        assert!(x1 - x0 <= 4 && y1 - y0 <= 4);
        // Untouched layers clear the selection.
        let mut doc2 = Document::new(8, 8);
        assert!(!doc2.select_opaque(8));
        assert!(doc2.selection.is_none());
    }

    #[test]
    fn stroke_paints_only_the_edge() {
        use crate::brush::{Brush, Color};
        let mut doc = selected_rect(12, 12, (2, 2, 7, 7));
        let brush = Brush {
            size: 1.0,
            softness: 0.0,
            opacity: 1.0,
            ..Default::default()
        };
        doc.checkpoint();
        assert!(doc.stroke_selection(&brush, Color::rgb(0, 255, 0), 1));
        let at = |d: &Document, x: u32, y: u32| d.layers[0].pixels[((y * 12 + x) * 4) as usize + 3];
        // Edge pixels painted, deep interior untouched.
        assert!(at(&doc, 2, 4) > 0);
        assert!(at(&doc, 7, 7) > 0);
        assert_eq!(at(&doc, 4, 4), 0);
        assert_eq!(at(&doc, 0, 0), 0);
        // One undo step removes the whole outline.
        assert!(doc.undo());
        assert_eq!(at(&doc, 2, 4), 0);
        // No selection, no paint.
        let mut doc2 = Document::new(8, 8);
        assert!(!doc2.stroke_selection(&brush, Color::rgb(0, 255, 0), 1));
    }

    #[test]
    fn feathered_masks_partially_apply_paint_and_tone() {
        use crate::brush::{Brush, Color};
        let mut doc = Document::new(20, 20);
        doc.selection = Some((5, 5, 14, 14));
        assert!(doc.feather_selection(3));
        // A ramp pixel just outside the old edge carries partial coverage.
        let m = doc.selection_mask.clone().unwrap();
        let ramp = m[(4 * 20 + 10) as usize];
        // Tone applies proportionally through the ramp.
        doc.checkpoint();
        doc.adjust_tone(0, 100, 1.0);
        let at = |d: &Document, x: u32, y: u32| d.layers[0].pixels[((y * 20 + x) * 4) as usize];
        assert_eq!(at(&doc, 10, 10), 100);
        let edge = at(&doc, 10, 4);
        if ramp > 0 {
            assert!(edge > 0 && edge < 100, "{edge} ramp {ramp}");
        } else {
            assert_eq!(edge, 0);
        }
        assert_eq!(at(&doc, 0, 0), 0);
        // Dabs fade through the same ramp.
        let mut doc2 = Document::new(20, 20);
        doc2.selection = Some((5, 5, 14, 14));
        assert!(doc2.feather_selection(3));
        let brush = Brush {
            size: 1.0,
            softness: 0.0,
            opacity: 1.0,
            ..Default::default()
        };
        doc2.dab(&brush, Color::rgb(255, 255, 255), 10.5, 4.5);
        let a = doc2.layers[0].pixels[((4 * 20 + 10) * 4) as usize + 3];
        let ramp2 = doc2.selection_mask.clone().unwrap()[(4 * 20 + 10) as usize];
        if ramp2 > 0 {
            assert!(a > 0 && a < 255, "{a} ramp {ramp2}");
        } else {
            assert_eq!(a, 0);
        }
    }
}
