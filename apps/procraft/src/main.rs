//! ProCraft by surfrpt1 (https://github.com/surfrpt1).
//! Original clean-room code, MIT licensed.
//!
//! ProCraft milestone 9: brush library (custom brushes + .pcbrush import/export),
//! plus all earlier features. Original code.

use eframe::egui::{self, Color32, ColorImage, Sense, TextureHandle, TextureOptions, Vec2};
// iOS has no rfd backend: same call shape, every dialog declined.
#[cfg(target_os = "ios")]
use rfd_shim as rfd;
use procraft_engine::{
    BlendMode, Brush, BrushPreset, Color, DabDynamics, Document, GuideKind, Guides, PaintTarget, Palette,
    ShapePoly, SymmetryMode, MAX_ANIM_FPS, MAX_LIBRARY_BRUSHES, MAX_PALETTE_COLORS, MIN_ANIM_FPS, analogous,
    clamp_frame, complementary, export_gif, export_tip_png, import_brush_file, import_gpl_file, next_frame,
    onion_ghost, open_brush_library, open_palette, open_psd, prev_frame, rasterize_frame,
    rasterize_frame_region, save_brush_library, save_palette, save_psd, starter_palette, triadic, MIN_TEXT_PX,
};

/// Latest Apple Pencil / touch pressure (0.0..=1.0) seen in this frame's
/// input events, if the backend forwarded any. egui 0.36 exposes pressure
/// only on `Event::Touch.force` — desktop mice always yield `None`, and
/// tilt is not exposed at all (documented M14 limitation).
fn latest_touch_force(ctx: &egui::Context) -> Option<f32> {
    ctx.input(|i| {
        let mut force = None;
        for e in &i.events {
            if let egui::Event::Touch { force: f, phase, .. } = e {
                if *phase == egui::TouchPhase::End || *phase == egui::TouchPhase::Cancel {
                    continue;
                }
                if let Some(v) = f {
                    force = Some(v.clamp(0.0, 1.0));
                }
            }
        }
        force
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    Brush,
    Eraser,
    Select,
    Lasso,
    Fill,
    Move,
    Shapes,
    Text,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ShapeKind {
    Line,
    Rect,
    Ellipse,
    Triangle,
    Diamond,
    Star,
    Arrow,
    Polygon,
}

impl ShapeKind {
    const ALL: [ShapeKind; 8] = [
        ShapeKind::Line,
        ShapeKind::Rect,
        ShapeKind::Ellipse,
        ShapeKind::Triangle,
        ShapeKind::Diamond,
        ShapeKind::Star,
        ShapeKind::Arrow,
        ShapeKind::Polygon,
    ];

    fn name(self) -> &'static str {
        match self {
            ShapeKind::Line => "Line",
            ShapeKind::Rect => "Rect",
            ShapeKind::Ellipse => "Ellipse",
            ShapeKind::Triangle => "Triangle",
            ShapeKind::Diamond => "Diamond",
            ShapeKind::Star => "Star",
            ShapeKind::Arrow => "Arrow",
            ShapeKind::Polygon => "Polygon",
        }
    }

    /// Closed shapes honor the Fill toggle (arrow/line are strokes only).
    fn fillable(self) -> bool {
        !matches!(self, ShapeKind::Line | ShapeKind::Arrow)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Theme {
    Dark,
    Light,
    Paper,
}

/// iOS asynchronous open: what to load once the picker resolves.
#[cfg(target_os = "ios")]
#[derive(Clone, Copy, PartialEq, Eq)]
enum PendingOpen {
    Image,
    Project,
    Psd,
    Brushes,
    Palette,
}

impl Theme {
    fn visuals(self) -> egui::Visuals {
        match self {
            Theme::Dark => egui::Visuals::dark(),
            Theme::Light => egui::Visuals::light(),
            Theme::Paper => {
                let mut v = egui::Visuals::light();
                v.window_fill = Color32::from_rgb(250, 246, 238);
                v.panel_fill = Color32::from_rgb(244, 238, 228);
                v.faint_bg_color = Color32::from_rgb(238, 230, 218);
                v
            }
        }
    }
}

/// Append `.ext` when the chosen path has none (rfd doesn't force it).
fn ensure_extension(mut path: std::path::PathBuf, ext: &str) -> std::path::PathBuf {
    if path.extension().is_none() {
        path.set_extension(ext);
    }
    path
}

fn page_presets() -> Vec<(&'static str, [u8; 3])> {    vec![
        ("White", [255, 255, 255]),
        ("Black", [0, 0, 0]),
        ("Gray", [128, 128, 128]),
        ("Warm", [240, 230, 210]),
        ("Blue", [52, 120, 200]),
    ]
}

struct ProCraftApp {
    doc: Document,
    brush: Brush,
    color: [u8; 3],
    tool: Tool,
    theme: Theme,
    theme_applied: Option<Theme>,
    page_custom: [u8; 3],
    picker_size: f32,
    texture: Option<TextureHandle>,
    painting: bool,
    last: Option<(f32, f32)>,
    last_dyn: DabDynamics,
    last_t: f64,
    leftover: f32,
    // Stabilizer state: exponentially smoothed pointer + pressure. Reset
    // on every stroke start so strokes never inherit the last line's tail.
    smooth_pt: Option<(f32, f32)>,
    smooth_pressure: f32,
    sel_anchor: Option<(f32, f32)>,
    sel_current: Option<(f32, f32)>,
    lasso_points: Vec<(f32, f32)>,
    lasso_poly: Vec<(f32, f32)>,
    fill_tolerance: u8,
    filt_brightness: i32,
    filt_contrast: f32,
    filt_blur: i32,
    filt_sharpen: f32,
    renaming: Option<usize>,
    renaming_text: String,
    preset_textures: Vec<TextureHandle>,
    view_mask: bool,
    show_tools: bool,
    show_layers: bool,
    narrow_setup_done: bool,
    show_about: bool,
    #[cfg(target_os = "ios")]
    pending_open: Option<PendingOpen>,
    preview_theme: Option<Theme>,
    custom_brushes: Vec<BrushPreset>,
    new_brush_name: String,
    custom_colors: Vec<[u8; 3]>,
    guides: Guides,
    // Animation Assist: layers are frames (bottom-to-top), the active
    // layer is the current frame. Ghost handles are half-res overlays,
    // rebuilt when the neighbor pair changes or `ghost_dirty` is set by
    // undo/redo and layer edits (painting only touches the current frame,
    // so strokes never invalidate ghosts).
    anim_assist: bool,
    anim_playing: bool,
    anim_fps: u32,
    anim_last_t: f64,
    anim_shown: Option<usize>,
    onion: bool,
    onion_amount: f32,
    ghost_prev: Option<TextureHandle>,
    ghost_next: Option<TextureHandle>,
    ghost_key: (Option<usize>, Option<usize>, u32),
    ghost_dirty: bool,
    move_from: Option<(f32, f32)>,
    move_acc: (f32, f32),
    shape_anchor: Option<(f32, f32)>,
    shape_current: Option<(f32, f32)>,
    shape_kind: ShapeKind,
    shape_fill: bool,
    shape_sides: u8,
    text_anchor: Option<(f32, f32)>,
    text_open: bool,
    text_body: String,
    text_size: f32,
    zoom: f32,
    status: String,
}

impl ProCraftApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Extra glyph coverage (tool icons) from a system symbol font; the
        // app works fine without it.
        let mut fonts = egui::FontDefinitions::default();
        if let Ok(bytes) = std::fs::read("C:\\Windows\\Fonts\\seguisym.ttf") {
            fonts.font_data.insert(
                "segui-symbol".to_owned(),
                std::sync::Arc::new(egui::FontData::from_owned(bytes)),
            );
            if let Some(list) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
                list.push("segui-symbol".to_owned());
            }
        }
        cc.egui_ctx.set_fonts(fonts);
        Self {
            doc: Document::new(1280, 800),
            brush: Brush::default(),
            color: [30, 144, 255],
            tool: Tool::Brush,
            theme: Theme::Paper,
            theme_applied: None,
            page_custom: [255, 255, 255],
            picker_size: 165.0,
            texture: None,
            painting: false,
            last: None,
            last_dyn: DabDynamics::default(),
            last_t: 0.0,
            leftover: 0.0,
            smooth_pt: None,
            smooth_pressure: 1.0,
            sel_anchor: None,
            sel_current: None,
            lasso_points: Vec::new(),
            lasso_poly: Vec::new(),
            fill_tolerance: 32,
            filt_brightness: 0,
            filt_contrast: 100.0,
            filt_blur: 0,
            filt_sharpen: 0.0,
            renaming: None,
            renaming_text: String::new(),
            preset_textures: Vec::new(),
            view_mask: false,
            show_tools: true,
            show_layers: true,
            narrow_setup_done: false,
            show_about: false,
            #[cfg(target_os = "ios")]
            pending_open: None,
            preview_theme: None,
            custom_brushes: Vec::new(),
            new_brush_name: String::new(),
            custom_colors: Vec::new(),
            guides: Guides::default(),
            anim_assist: false,
            anim_playing: false,
            anim_fps: 12,
            anim_last_t: 0.0,
            anim_shown: None,
            onion: true,
            onion_amount: 0.3,
            ghost_prev: None,
            ghost_next: None,
            ghost_key: (None, None, 0),
            ghost_dirty: true,
            move_from: None,
            move_acc: (0.0, 0.0),
            shape_anchor: None,
            shape_current: None,
            shape_kind: ShapeKind::Rect,
            shape_fill: false,
            shape_sides: 6,
            text_anchor: None,
            text_open: false,
            text_body: String::new(),
            text_size: 48.0,
            zoom: 1.0,
            status: "Drag on the canvas to paint. Ctrl+Z undoes, Ctrl+Y redoes, Esc clears selection.".to_string(),
        }
    }

    fn base_brush(&self) -> Brush {
        Brush { eraser: self.tool == Tool::Eraser, ..self.brush.clone() }
    }

    fn current_color(&self) -> Color {
        Color::rgb(self.color[0], self.color[1], self.color[2])
    }

    /// Upload only what changed since the last frame (no-op when clean).
    fn refresh_texture(&mut self, ctx: &egui::Context) {
        // Mask viewer: grayscale of the active layer's mask, full upload.
        if self.view_mask {
            if let Some(mv) = self.doc.mask_view() {
                let dirty = self.texture.is_none() || self.doc.take_dirty().is_some();
                if dirty {
                    let img = ColorImage::from_rgba_unmultiplied(
                        [self.doc.w as usize, self.doc.h as usize],
                        &mv,
                    );
                    self.texture = Some(ctx.load_texture("canvas", img, TextureOptions::LINEAR));
                }
                return;
            }
        }
        // Animation Assist: the canvas shows the current frame only
        // (active layer over the page), not the full composite.
        if self.anim_assist && !self.doc.layers.is_empty() {
            let active = self.doc.active.min(self.doc.layers.len() - 1);
            if self.texture.is_none() || self.anim_shown != Some(active) {
                let bytes = rasterize_frame(&self.doc, active);
                let img = ColorImage::from_rgba_unmultiplied(
                    [self.doc.w as usize, self.doc.h as usize],
                    &bytes,
                );
                self.texture = Some(ctx.load_texture("canvas", img, TextureOptions::LINEAR));
                self.anim_shown = Some(active);
                return;
            }
            if let Some((x0, y0, x1, y1)) = self.doc.take_dirty() {
                let bytes = rasterize_frame_region(&self.doc, active, x0, y0, x1, y1);
                let w = (x1 - x0 + 1) as usize;
                let h = ((y1 - y0 + 1) as usize).max(bytes.len() / 4 / w.max(1));
                let img = ColorImage::from_rgba_unmultiplied([w, h], &bytes);
                if let Some(handle) = &mut self.texture {
                    handle.set_partial([x0 as usize, y0 as usize], img, TextureOptions::LINEAR);
                }
            }
            return;
        }
        self.anim_shown = None;
        if self.texture.is_none() {
            let flat = self.doc.flatten();
            let img = ColorImage::from_rgba_unmultiplied(
                [self.doc.w as usize, self.doc.h as usize],
                &flat,
            );
            self.texture = Some(ctx.load_texture("canvas", img, TextureOptions::LINEAR));
            return;
        }
        if let Some((x0, y0, x1, y1)) = self.doc.take_dirty() {
            let bytes = self.doc.flatten_region(x0, y0, x1, y1);
            let w = (x1 - x0 + 1) as usize;
            let h = ((y1 - y0 + 1) as usize).max(bytes.len() / 4 / w.max(1));
            let img = ColorImage::from_rgba_unmultiplied([w, h], &bytes);
            if let Some(handle) = &mut self.texture {
                handle.set_partial([x0 as usize, y0 as usize], img, TextureOptions::LINEAR);
            }
        }
    }

    /// Rebuild half-res onion ghost textures when the neighbor pair, the
    /// amount, or layer content (`ghost_dirty`) changed. No-op otherwise.
    fn refresh_ghosts(&mut self, ctx: &egui::Context) {
        let active = clamp_frame(&self.doc, self.doc.active).unwrap_or(0);
        let bits = (self.onion_amount.clamp(0.05, 0.8) * 1000.0).round() as u32;
        let pair = if self.anim_assist && self.onion && !self.doc.layers.is_empty() {
            (prev_frame(&self.doc, active), next_frame(&self.doc, active), bits)
        } else {
            (None, None, bits)
        };
        if !self.ghost_dirty && pair == self.ghost_key {
            return;
        }
        self.ghost_dirty = false;
        self.ghost_key = pair;
        let (w, h) = (self.doc.w as usize, self.doc.h as usize);
        let amount = self.onion_amount;
        let load = |ctx: &egui::Context, name: &str, idx: Option<usize>| -> Option<TextureHandle> {
            let full = onion_ghost(&self.doc, idx?, amount);
            Some(ctx.load_texture(name, downsample_half(&full, w, h), TextureOptions::LINEAR))
        };
        self.ghost_prev = load(ctx, "ghost-prev", pair.0);
        self.ghost_next = load(ctx, "ghost-next", pair.1);
    }

    fn set_page_color(&mut self, rgb: [u8; 3]) {
        self.doc.background = [rgb[0], rgb[1], rgb[2], 255];
        self.doc.mark_full_dirty();
    }

    /// iOS: load a file the document picker resolved. Mirrors the desktop
    /// open arms (kept inline there so desktop behavior never changes).
    #[cfg(target_os = "ios")]
    fn open_picked(&mut self, kind: PendingOpen, path: std::path::PathBuf) {
        match kind {
            PendingOpen::Image => match self.doc.load_background(&path) {
                Ok(()) => {
                    self.texture = None;
                    self.lasso_points.clear();
                    self.lasso_poly.clear();
                    self.status = format!("Opened {}", path.display());
                }
                Err(e) => self.status = format!("Open failed: {e}"),
            },
            PendingOpen::Project => match Document::open_project(&path) {
                Ok(doc) => {
                    self.doc = doc;
                    self.texture = None;
                    self.lasso_points.clear();
                    self.lasso_poly.clear();
                    self.status = format!("Project opened {}", path.display());
                }
                Err(e) => self.status = format!("Project open failed: {e}"),
            },
            PendingOpen::Psd => match open_psd(&path) {
                Ok(doc) => {
                    self.doc = doc;
                    self.texture = None;
                    self.lasso_points.clear();
                    self.lasso_poly.clear();
                    self.status = format!("PSD opened {} (masks do not import)", path.display());
                }
                Err(e) => self.status = format!("PSD open failed: {e}"),
            },
            PendingOpen::Brushes => {
                match load_brush_file(&path) {
                    Ok(libs) => self.status = self.absorb_brushes(libs, &path),
                    Err(e) => self.status = format!("Brush import failed: {e}"),
                }
            }
            PendingOpen::Palette => match load_palette_file(&path) {
                Ok(pal) => self.status = self.absorb_colors(pal, &path),
                Err(e) => self.status = format!("Palette import failed: {e}"),
            },
        }
    }

    /// Honest brush previews: each preset paints a wavy sample stroke through
    /// the real engine into a thumbnail texture (dotted chalk reads dotted).
    /// Ink follows the theme so cards stay crisp on dark and paper alike.
    /// Built-ins come first, then the user's custom brushes in order.
    fn build_preset_previews(&mut self, ctx: &egui::Context, ink: Color) {
        self.preset_textures.clear();
        let brushes: Vec<Brush> = procraft_engine::presets()
            .into_iter()
            .map(|(_, b)| b)
            .chain(self.custom_brushes.iter().map(|p| p.brush.clone()))
            .collect();
        for (idx, preset) in brushes.into_iter().enumerate() {
            let mut doc = Document::new(144, 36);
            let brush = Brush { eraser: false, ..preset };
            let mut last = None;
            let mut left = 0.0;
            let mut x = 8.0;
            while x <= 136.0 {
                let y = 18.0
                    + 8.0 * ((x - 8.0) / 128.0 * std::f32::consts::PI * 2.0).sin();
                let (l, lo) = doc.stroke_to(&brush, ink, last, (x, y), left);
                last = l;
                left = lo;
                x += 2.0;
            }
            let img = ColorImage::from_rgba_unmultiplied(
                [144, 36],
                &doc.layers[0].pixels.clone(),
            );
            self.preset_textures.push(ctx.load_texture(
                format!("brush-{idx}"),
                img,
                TextureOptions::LINEAR,
            ));
        }
    }

    /// Total preview cards currently needed (built-ins + customs).
    fn preview_count(&self) -> usize {
        procraft_engine::presets().len() + self.custom_brushes.len()
    }
}

/// Dispatch a palette file by extension (shared by the desktop Import arm
/// and the iOS picker resolve path): `.pcpalette`/`.json` or GIMP `.gpl`.
fn load_palette_file(path: &std::path::Path) -> Result<Palette, String> {
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
    if ext == "gpl" {
        import_gpl_file(path)
    } else {
        open_palette(path)
    }
}
/// Dispatch a brush file by extension into presets (shared by the desktop
/// Import arm and the iOS picker resolve path).
fn load_brush_file(path: &std::path::Path) -> Result<Vec<BrushPreset>, String> {
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
    if ext == "pcbrush" || ext == "json" {
        open_brush_library(path).map(|libs| {
            libs.into_iter().map(|p| BrushPreset { name: p.name, brush: p.brush }).collect()
        })
    } else {
        import_brush_file(path).map(|libs| {
            libs.into_iter().map(|b| BrushPreset { name: b.name, brush: b.brush }).collect()
        })
    }
}

impl ProCraftApp {
    /// Unique display name for a new custom brush: "Foo", "Foo (2)", ...
    fn unique_brush_name(&self, want: &str) -> String {
        let want = want.trim();
        if self.custom_brushes.iter().all(|p| p.name != want) {
            return want.to_string();
        }
        for n in 2..=MAX_LIBRARY_BRUSHES + 2 {
            let cand = format!("{want} ({n})");
            if self.custom_brushes.iter().all(|p| p.name != cand) {
                return cand;
            }
        }
        want.to_string()
    }

    /// Append imported presets with dedup names (shared by desktop Import
    /// and the iOS picker resolve path). Returns the status message.
    fn absorb_brushes(&mut self, libs: Vec<BrushPreset>, path: &std::path::Path) -> String {
        let mut added = 0;
        for p in libs {
            if self.custom_brushes.len() >= MAX_LIBRARY_BRUSHES {
                break;
            }
            let name = self.unique_brush_name(&p.name);
            self.custom_brushes.push(BrushPreset { name, brush: p.brush });
            added += 1;
        }
        format!("Imported {added} brushes from {}.", path.display())
    }

    /// Append an imported palette's swatches, skipping exact duplicates
    /// (shared by desktop Import and the iOS picker resolve path).
    fn absorb_colors(&mut self, pal: Palette, path: &std::path::Path) -> String {
        let mut added = 0;
        for c in pal.colors {
            if self.custom_colors.len() >= MAX_PALETTE_COLORS {
                break;
            }
            if !self.custom_colors.contains(&c) {
                self.custom_colors.push(c);
                added += 1;
            }
        }
        format!("Imported {added} swatches from {}.", path.display())
    }
}

/// One clickable swatch: applies the color. Returns the response for
/// right-click-to-remove handling by the caller.
fn swatch_button(ui: &mut egui::Ui, rgb: [u8; 3], selected: bool) -> egui::Response {
    let (r, g, b) = (rgb[0], rgb[1], rgb[2]);
    ui.add_sized(
        [24.0, 24.0],
        egui::Button::new("")
            .fill(Color32::from_rgb(r, g, b))
            .selected(selected),
    )
}

/// Box-downsample straight RGBA to half resolution for cheap onion-ghost
/// overlays (the canvas rect stretches them back; ghosts are soft anyway).
fn downsample_half(full: &[u8], w: usize, h: usize) -> ColorImage {
    let (hw, hh) = ((w / 2).max(1), (h / 2).max(1));
    let mut px = vec![0u8; hw * hh * 4];
    for y in 0..hh {
        for x in 0..hw {
            let mut acc = [0u32; 4];
            let mut n = 0u32;
            for dy in 0..2 {
                for dx in 0..2 {
                    let (sx, sy) = (x * 2 + dx, y * 2 + dy);
                    if sx < w && sy < h {
                        let o = (sy * w + sx) * 4;
                        if o + 3 < full.len() {
                            for c in 0..4 {
                                acc[c] += full[o + c] as u32;
                            }
                            n += 1;
                        }
                    }
                }
            }
            let o = (y * hw + x) * 4;
            for c in 0..4 {
                px[o + c] = if n > 0 { (acc[c] / n) as u8 } else { 0 };
            }
        }
    }
    ColorImage::from_rgba_unmultiplied([hw, hh], &px)
}

/// Marching-ants rectangle: dark base line plus moving light dashes.
fn paint_selection(painter: &egui::Painter, rect: egui::Rect, time: f64) {
    let dark = Color32::BLACK;
    let light = Color32::WHITE;
    painter.rect_stroke(rect, 0.0, (1.5, dark), egui::StrokeKind::Outside);
    let dash = 7.0;
    let gap = 5.0;
    let period = dash + gap;
    let mut offset = (time * 28.0) as f32 % period;
    let mut draw_dashed = |a: egui::Pos2, b: egui::Pos2| {
        let len = (b - a).length();
        let mut t = -offset;
        while t < len {
            let t0 = t.max(0.0);
            let t1 = (t + dash).min(len);
            if t1 > t0 {
                let dir = (b - a) / len.max(0.001);
                painter.line_segment([a + dir * t0, a + dir * t1], (1.5, light));
            }
            t += period;
        }
        offset = (offset + len) % period;
    };
    draw_dashed(rect.left_top(), rect.right_top());
    draw_dashed(rect.right_top(), rect.right_bottom());
    draw_dashed(rect.right_bottom(), rect.left_bottom());
    draw_dashed(rect.left_bottom(), rect.left_top());
}

/// Freehand path overlay: dark base plus light top, visible on any art.
fn paint_path(painter: &egui::Painter, pts: &[egui::Pos2], close: bool) {
    if pts.len() < 2 {
        return;
    }
    let mut segs: Vec<[egui::Pos2; 2]> = pts.windows(2).map(|w| [w[0], w[1]]).collect();
    if close {
        segs.push([pts[pts.len() - 1], pts[0]]);
    }
    for [a, b] in &segs {
        painter.line_segment([*a, *b], (3.0, Color32::BLACK));
    }
    for [a, b] in &segs {
        painter.line_segment([*a, *b], (1.5, Color32::WHITE));
    }
}

impl eframe::App for ProCraftApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.theme_applied != Some(self.theme) {
            ui.ctx().set_visuals(self.theme.visuals());
            self.theme_applied = Some(self.theme);
        }
        // Ctrl+Z undo, Ctrl+Y / Ctrl+Shift+Z redo, Esc clears selection.
        if ui.ctx().input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::Z)) && self.doc.undo()
        {
            self.ghost_dirty = true;
            self.status = "Undone.".to_string();
        }
        if ui.ctx().input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::Y)) && self.doc.redo()
        {
            self.ghost_dirty = true;
            self.status = "Redone.".to_string();
        }
        if ui.ctx().input_mut(|i| {
            i.consume_key(egui::Modifiers::CTRL.plus(egui::Modifiers::SHIFT), egui::Key::Z)
        }) && self.doc.redo()
        {
            self.ghost_dirty = true;
            self.status = "Redone.".to_string();
        }
        if ui.ctx().input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            self.doc.clear_selection();
            self.sel_anchor = None;
            self.sel_current = None;
            self.lasso_points.clear();
            self.lasso_poly.clear();
        }
        // Keep frames coming while painting, selecting, lassoing, moving,
        // shaping, or marching ants.
        if self.painting
            || self.sel_anchor.is_some()
            || !self.lasso_points.is_empty()
            || self.shape_anchor.is_some()
            || self.move_from.is_some()
            || self.doc.selection.is_some()
            || self.doc.selection_mask.is_some()
        {
            ui.ctx().request_repaint();
        }
        // Animation playback: advance the current frame on an fps clock.
        if self.anim_assist && self.anim_playing && !self.doc.layers.is_empty() {
            ui.ctx().request_repaint();
            let now = ui.ctx().input(|i| i.time);
            let fps = self.anim_fps.clamp(MIN_ANIM_FPS, MAX_ANIM_FPS);
            if now - self.anim_last_t >= 1.0 / fps as f64 {
                self.anim_last_t = now;
                self.doc.active = (self.doc.active + 1) % self.doc.layers.len();
            }
        }

        // One-time narrow-screen setup: portrait phones/tablets start
        // with both sidebars hidden so the canvas owns the screen.
        if !self.narrow_setup_done {
            self.narrow_setup_done = true;
            if ui.ctx().viewport_rect().width() < 900.0 {
                self.show_tools = false;
                self.show_layers = false;
            }
        }

        // iOS asynchronous opens: the picker was presented at tap time;
        // each frame checks whether it resolved.
        #[cfg(target_os = "ios")]
        if let Some(kind) = self.pending_open.take() {
            match rfd::poll_pick() {
                rfd::PickPoll::NoRequest | rfd::PickPoll::Waiting => {
                    self.pending_open = Some(kind);
                }
                rfd::PickPoll::Picked(path) => {
                    self.open_picked(kind, path);
                    // Deferred v4 bug: the foreground frame after the
                    // picker dismisses can drop before the rebuilt texture
                    // shows (content invisible until first tap). Force one
                    // more frame so opens appear at once.
                    ui.ctx().request_repaint();
                }
                rfd::PickPoll::Cancelled => {
                    self.status = "Pick cancelled.".to_string();
                }
            }
        }

        egui::Panel::top("menu").show(ui, |ui| {
            // Wrapped rows: one row on desktop, two on narrow screens.
            ui.horizontal_wrapped(|ui| {
                if ui.button("Save PNG").clicked() {
                    if let Some(path) = rfd::FileDialog::new().add_filter("PNG", &["png"]).save_file() {
                        // The dialog may return an extensionless path (always
                        // on iOS); the encoder needs the suffix to pick PNG.
                        let path = ensure_extension(path, "png");
                        match self.doc.save_png(&path) {
                            Ok(()) => {
                                self.status = format!("Saved {}", path.display());
                                #[cfg(target_os = "ios")]
                                rfd::ios_share_file(&path);
                            }
                            Err(e) => self.status = format!("Save failed: {e}"),
                        }
                    }
                }
                if ui.button("Open image").clicked() {
                    #[cfg(not(target_os = "ios"))]
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Images", &["png", "jpg", "jpeg", "webp", "bmp", "gif", "tif", "tiff"])
                        .pick_file()
                    {
                        match self.doc.load_background(&path) {
                            Ok(()) => {
                                self.texture = None;
                                self.lasso_points.clear();
                                self.lasso_poly.clear();
                                self.status = format!("Opened {}", path.display());
                            }
                            Err(e) => self.status = format!("Open failed: {e}"),
                        }
                    }
                    // iOS: presents the picker; open_picked() loads on resolve.
                    #[cfg(target_os = "ios")]
                    {
                        let _ = rfd::FileDialog::new()
                            .add_filter("Images", &["png", "jpg", "jpeg", "webp", "bmp", "gif", "tif", "tiff"])
                            .pick_file();
                        self.pending_open = Some(PendingOpen::Image);
                        self.status = "Pick an image…".to_string();
                    }
                }
                if ui.button("Save project").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("ProCraft project", &["procraft"])
                        .save_file()
                    {
                        let path = ensure_extension(path, "procraft");
                        match self.doc.save_project(&path) {
                            Ok(()) => {
                                self.status = format!("Project saved {}", path.display());
                                #[cfg(target_os = "ios")]
                                rfd::ios_share_file(&path);
                            }
                            Err(e) => self.status = format!("Project save failed: {e}"),
                        }
                    }
                }
                if ui.button("Open project").clicked() {
                    #[cfg(not(target_os = "ios"))]
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("ProCraft project", &["procraft"])
                        .pick_file()
                    {
                        match Document::open_project(&path) {
                            Ok(doc) => {
                                self.doc = doc;
                                self.texture = None;
                                self.lasso_points.clear();
                                self.lasso_poly.clear();
                                self.status = format!("Project opened {}", path.display());
                            }
                            Err(e) => self.status = format!("Project open failed: {e}"),
                        }
                    }
                    #[cfg(target_os = "ios")]
                    {
                        let _ = rfd::FileDialog::new()
                            .add_filter("ProCraft project", &["procraft"])
                            .pick_file();
                        self.pending_open = Some(PendingOpen::Project);
                        self.status = "Pick a project…".to_string();
                    }
                }
                if ui.button("PSD export").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Photoshop document", &["psd"])
                        .save_file()
                    {
                        let path = ensure_extension(path, "psd");
                        match save_psd(&path, &self.doc) {
                            Ok(()) => {
                                self.status = format!("PSD saved {} (masks do not round-trip)", path.display());
                                #[cfg(target_os = "ios")]
                                rfd::ios_share_file(&path);
                            }
                            Err(e) => self.status = format!("PSD save failed: {e}"),
                        }
                    }
                }
                if ui.button("Open PSD").clicked() {
                    #[cfg(not(target_os = "ios"))]
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Photoshop document", &["psd"])
                        .pick_file()
                    {
                        match open_psd(&path) {
                            Ok(doc) => {
                                self.doc = doc;
                                self.texture = None;
                                self.lasso_points.clear();
                                self.lasso_poly.clear();
                                self.status = format!("PSD opened {} (masks do not import)", path.display());
                            }
                            Err(e) => self.status = format!("PSD open failed: {e}"),
                        }
                    }
                    #[cfg(target_os = "ios")]
                    {
                        let _ = rfd::FileDialog::new()
                            .add_filter("Photoshop document", &["psd"])
                            .pick_file();
                        self.pending_open = Some(PendingOpen::Psd);
                        self.status = "Pick a PSD…".to_string();
                    }
                }
                if ui.button("Clear layer").clicked() {
                    self.doc.checkpoint();
                    if let Some(layer) = self.doc.active_layer_mut() {
                        layer.clear();
                    }
                    self.doc.mark_full_dirty();
                }
                if ui.button("Undo").clicked() && self.doc.undo() {
                    self.status = "Undone.".to_string();
                }
                if ui.button("Redo").clicked() && self.doc.redo() {
                    self.status = "Redone.".to_string();
                }
                ui.separator();
                ui.label("Theme:");
                ui.radio_value(&mut self.theme, Theme::Dark, "Dark");
                ui.radio_value(&mut self.theme, Theme::Light, "Light");
                ui.radio_value(&mut self.theme, Theme::Paper, "Paper");
                ui.separator();
                if ui.button("−").clicked() {
                    self.zoom = (self.zoom / 1.25).clamp(0.1, 8.0);
                }
                ui.label(format!("{}%", (self.zoom * 100.0).round() as u32));
                if ui.button("+").clicked() {
                    self.zoom = (self.zoom * 1.25).clamp(0.1, 8.0);
                }
                if ui.button("Fit").clicked() {
                    self.zoom = 0.0; // recomputed in the canvas view
                }
                if ui.button("ⓘ About").clicked() {
                    self.show_about = !self.show_about;
                }
            });
        });

        // About box: version, builder, home page, updates. Version comes
        // from the workspace Cargo.toml at compile time, so release bumps
        // show up here with no code changes.
        if self.show_about {
            egui::Window::new("About ProCraft")
                .open(&mut self.show_about)
                .resizable(false)
                .collapsible(false)
                .show(ui.ctx(), |ui| {
                    ui.heading("ProCraft");
                    ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                    ui.label("Painting studio by surfrpt1.");
                    ui.hyperlink_to(
                        "github.com/surfrpt1/procraft",
                        "https://github.com/surfrpt1/procraft",
                    );
                    ui.separator();
                    ui.hyperlink_to(
                        "Check for updates",
                        "https://github.com/surfrpt1/procraft/releases",
                    );
                    ui.label("Original code, MIT licensed.");
                });
        }

        // Text dialog: type, size, then Render bakes the block to the
        // active layer at the canvas anchor (one undo step).
        if self.text_open {
            let mut open = self.text_open;
            let ink = Color::rgb(self.color[0], self.color[1], self.color[2]);
            egui::Window::new("Text")
                .open(&mut open)
                .collapsible(false)
                .show(ui.ctx(), |ui| {
                    ui.label("Click the canvas to move the anchor, then Render.");
                    ui.add(
                        egui::TextEdit::multiline(&mut self.text_body)
                            .desired_rows(3)
                            .hint_text("Type here (Enter = new line)"),
                    );
                    ui.add(
                        egui::Slider::new(&mut self.text_size, MIN_TEXT_PX..=128.0).text("Size px"),
                    );
                    ui.horizontal(|ui| {
                        if ui.button("Render").clicked() {
                            match self.text_anchor {
                                Some((ax, ay)) => {
                                    self.doc.checkpoint();
                                    match self.doc.draw_text(ink, ax, ay, self.text_size, &self.text_body) {
                                        Ok((w, h)) => {
                                            self.status =
                                                format!("Text placed ({w}x{h}px).");
                                            self.text_open = false;
                                        }
                                        Err(e) => self.status = format!("Text failed: {e}"),
                                    }
                                }
                                None => {
                                    self.status =
                                        "Click the canvas to anchor the text first.".to_string()
                                }
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            self.text_open = false;
                        }
                    });
                });
            self.text_open = open;
        }

        // Fresh id (not "tools"): eframe persists panel widths to disk, and the
        // old narrow width would otherwise override `default_size` forever.
        if self.show_tools {
        egui::Panel::left("tools-wide").default_size(300.0).show(ui, |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Tool");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add_sized([40.0, 24.0], egui::Button::new("◀")).clicked() {
                        self.show_tools = false;
                    }
                });
            });
            ui.radio_value(&mut self.tool, Tool::Brush, "✏ Brush");
            ui.radio_value(&mut self.tool, Tool::Eraser, "⌫ Eraser");
            ui.radio_value(&mut self.tool, Tool::Select, "⬚ Select");
            ui.radio_value(&mut self.tool, Tool::Lasso, "◌ Lasso");
            ui.radio_value(&mut self.tool, Tool::Fill, "◧ Fill");
            ui.radio_value(&mut self.tool, Tool::Move, "↔ Move");
            ui.radio_value(&mut self.tool, Tool::Shapes, "○ Shapes");
            ui.radio_value(&mut self.tool, Tool::Text, "T Text");
            if self.tool == Tool::Fill {
                ui.add(egui::Slider::new(&mut self.fill_tolerance, 0..=255).text("Tolerance"));
            }
            if self.tool == Tool::Shapes {
                ui.horizontal_wrapped(|ui| {
                    for k in ShapeKind::ALL {
                        ui.radio_value(&mut self.shape_kind, k, k.name());
                    }
                });
                if self.shape_kind.fillable() {
                    ui.checkbox(&mut self.shape_fill, "Fill");
                }
                if self.shape_kind == ShapeKind::Polygon {
                    ui.add(egui::Slider::new(&mut self.shape_sides, 3..=12).text("Sides"));
                }
            }
            if ui.button("Clear selection").clicked() {
                self.doc.clear_selection();
                self.lasso_poly.clear();
                self.status = "Selection cleared.".to_string();
            }
            ui.separator();
            ui.heading("Brushes");
            // Theme ink: light strokes on dark theme, dark strokes otherwise.
            let ink = match self.theme {
                Theme::Dark => Color::rgb(215, 215, 215),
                _ => Color::rgb(70, 70, 70),
            };
            if self.preview_theme != Some(self.theme)
                || self.preset_textures.len() != self.preview_count()
            {
                let ctx = ui.ctx().clone();
                self.build_preset_previews(&ctx, ink);
                self.preview_theme = Some(self.theme);
            }
            let builtin = procraft_engine::presets();
            ui.scope(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(6.0, 3.0);
                for (idx, (name, preset)) in builtin.into_iter().enumerate() {
                    // Name and thumbnail both apply the brush; the active
                    // brush reads highlighted.
                    let active = self.brush == preset;
                    ui.horizontal(|ui| {
                        if ui.add_sized([64.0, 22.0], egui::Button::new(name).selected(active)).clicked() {
                            self.brush = preset.clone();
                            self.tool = Tool::Brush;
                        }
                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            if let Some(tex) = self.preset_textures.get(idx) {
                                let thumb = ui.add(
                                    egui::Image::new((tex.id(), Vec2::new(96.0, 24.0)))
                                        .sense(Sense::click()),
                                );
                                if thumb.clicked() {
                                    self.brush = preset.clone();
                                    self.tool = Tool::Brush;
                                }
                            }
                        });
                    });
                }
            });
            ui.separator();
            ui.heading("My brushes");
            ui.horizontal(|ui| {
                // Fixed widths: a free-size text edit would keep pushing the
                // whole left panel wider every frame.
                ui.add_sized(
                    [110.0, 22.0],
                    egui::TextEdit::singleline(&mut self.new_brush_name).hint_text("Name"),
                );
                if ui.add_sized([90.0, 22.0], egui::Button::new("Add current")).clicked() {
                    let want = self.new_brush_name.trim().to_string();
                    if want.is_empty() {
                        self.status = "Name the brush first.".to_string();
                    } else if self.custom_brushes.len() >= MAX_LIBRARY_BRUSHES {
                        self.status = format!("Brush library is full ({MAX_LIBRARY_BRUSHES}).");
                    } else {
                        let name = self.unique_brush_name(&want);
                        let brush = Brush { eraser: false, ..self.brush.clone() };
                        self.custom_brushes.push(BrushPreset { name: name.clone(), brush });
                        self.new_brush_name.clear();
                        self.status = format!("Brush '{name}' added.");
                    }
                }
            });
            ui.horizontal(|ui| {
                if ui.button("Import").clicked() {
                    #[cfg(not(target_os = "ios"))]
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Brushes", &[
                            "pcbrush", "json", "abr", "brush", "brushset",
                            "kpp", "bundle", "gbr", "png", "jpg", "jpeg", "webp", "bmp",
                        ])
                        .pick_file()
                    {
                        match load_brush_file(&path) {
                            Ok(libs) => self.status = self.absorb_brushes(libs, &path),
                            Err(e) => self.status = format!("Brush import failed: {e}"),
                        }
                    }
                    #[cfg(target_os = "ios")]
                    {
                        let _ = rfd::FileDialog::new()
                            .add_filter("Brushes", &[
                                "pcbrush", "json", "abr", "brush", "brushset",
                                "kpp", "bundle", "gbr", "png", "jpg", "jpeg", "webp", "bmp",
                            ])
                            .pick_file();
                        self.pending_open = Some(PendingOpen::Brushes);
                        self.status = "Pick brushes…".to_string();
                    }
                }
                if ui.button("Export set").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("ProCraft brushes", &["pcbrush"])
                        .save_file()
                    {
                        let path = ensure_extension(path, "pcbrush");
                        match save_brush_library(&path, &self.custom_brushes) {
                            Ok(()) => {
                                self.status = format!("Brushes saved {}", path.display());
                                #[cfg(target_os = "ios")]
                                rfd::ios_share_file(&path);
                            }
                            Err(e) => self.status = format!("Brush save failed: {e}"),
                        }
                    }
                }
                if ui.button("Tip PNG").clicked() {
                    // Grayscale tip of the current brush for Photoshop,
                    // Procreate, Krita and GIMP.
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("PNG image", &["png"])
                        .save_file()
                    {
                        let path = ensure_extension(path, "png");
                        let brush = Brush { eraser: false, ..self.brush.clone() };
                        match export_tip_png(&path, &brush) {
                            Ok(()) => {
                                self.status = format!("Tip saved {}", path.display());
                                #[cfg(target_os = "ios")]
                                rfd::ios_share_file(&path);
                            }
                            Err(e) => self.status = format!("Tip save failed: {e}"),
                        }
                    }
                }
            });
            let custom_count = self.custom_brushes.len();
            if custom_count > 0 {
                let base = procraft_engine::presets().len();
                let mut remove: Option<usize> = None;
                ui.scope(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(6.0, 3.0);
                    for idx in 0..custom_count {
                        let (name, brush) = {
                            let p = &self.custom_brushes[idx];
                            (p.name.clone(), p.brush.clone())
                        };
                        let active = self.brush == brush;
                        ui.horizontal(|ui| {
                            if ui.add_sized([64.0, 22.0], egui::Button::new(&name).selected(active)).clicked() {
                                self.brush = brush.clone();
                                self.tool = Tool::Brush;
                            }
                            egui::Frame::group(ui.style()).show(ui, |ui| {
                                if let Some(tex) = self.preset_textures.get(base + idx) {
                                    let thumb = ui.add(
                                        egui::Image::new((tex.id(), Vec2::new(96.0, 24.0)))
                                            .sense(Sense::click()),
                                    );
                                    if thumb.clicked() {
                                        self.brush = brush.clone();
                                        self.tool = Tool::Brush;
                                    }
                                }
                            });
                            if ui.small_button("✕").clicked() {
                                remove = Some(idx);
                            }
                        });
                    }
                });
                if let Some(idx) = remove {
                    let name = self.custom_brushes[idx].name.clone();
                    self.custom_brushes.remove(idx);
                    self.status = format!("Brush '{name}' removed.");
                }
            }
            ui.separator();
            ui.heading("Color");
            ui.horizontal(|ui| {
                ui.label("Picker size:");
                ui.add(egui::Slider::new(&mut self.picker_size, 140.0..=420.0).text("px"));
            });
            let mut c = Color32::from_rgb(self.color[0], self.color[1], self.color[2]);
            // The SV square + hue strip size themselves from `slider_width`,
            // not the panel width: drive it from the slider above.
            let picker_width = self.picker_size;
            ui.scope(|ui| {
                ui.spacing_mut().slider_width = picker_width;
                if egui::color_picker::color_picker_color32(
                    ui,
                    &mut c,
                    egui::color_picker::Alpha::Opaque,
                ) {
                    self.color = [c.r(), c.g(), c.b()];
                }
            });
            ui.separator();
            ui.heading("Swatches");
            ui.label("Starter (click applies):");
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(3.0, 3.0);
                for rgb in starter_palette().colors {
                    let resp = swatch_button(ui, rgb, self.color == rgb);
                    if resp.clicked() {
                        self.color = rgb;
                    }
                }
            });
            ui.label("My colors (click applies, right-click removes):");
            if self.custom_colors.is_empty() {
                ui.label("Empty — store the current color with +.");
            } else {
                let mut remove = None;
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(3.0, 3.0);
                    for (idx, rgb) in self.custom_colors.iter().enumerate() {
                        let resp = swatch_button(ui, *rgb, self.color == *rgb);
                        if resp.clicked() {
                            self.color = *rgb;
                        }
                        if resp.secondary_clicked() {
                            remove = Some(idx);
                        }
                    }
                });
                if let Some(idx) = remove {
                    self.custom_colors.remove(idx);
                }
            }
            ui.horizontal(|ui| {
                if ui.button("+ Current").clicked() {
                    if self.custom_colors.contains(&self.color) {
                        self.status = "That color is already stored.".to_string();
                    } else if self.custom_colors.len() >= MAX_PALETTE_COLORS {
                        self.status = format!("Swatch set is full ({MAX_PALETTE_COLORS}).");
                    } else {
                        self.custom_colors.push(self.color);
                        self.status = "Color stored.".to_string();
                    }
                }
                if ui.button("Import").clicked() {
                    #[cfg(not(target_os = "ios"))]
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Palettes", &["pcpalette", "json", "gpl"])
                        .pick_file()
                    {
                        match load_palette_file(&path) {
                            Ok(pal) => self.status = self.absorb_colors(pal, &path),
                            Err(e) => self.status = format!("Palette import failed: {e}"),
                        }
                    }
                    #[cfg(target_os = "ios")]
                    {
                        let _ = rfd::FileDialog::new()
                            .add_filter("Palettes", &["pcpalette", "json", "gpl"])
                            .pick_file();
                        self.pending_open = Some(PendingOpen::Palette);
                        self.status = "Pick palette…".to_string();
                    }
                }
                if ui.button("Export").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("ProCraft palette", &["pcpalette"])
                        .save_file()
                    {
                        let path = ensure_extension(path, "pcpalette");
                        let pal = Palette { name: "My colors".to_string(), colors: self.custom_colors.clone() };
                        match save_palette(&path, &pal) {
                            Ok(()) => {
                                self.status = format!("Palette saved {}", path.display());
                                #[cfg(target_os = "ios")]
                                rfd::ios_share_file(&path);
                            }
                            Err(e) => self.status = format!("Palette save failed: {e}"),
                        }
                    }
                }
                if ui.button("Clear").clicked() {
                    self.custom_colors.clear();
                    self.status = "Swatch set cleared.".to_string();
                }
            });
            ui.label("Harmony of the current color (click applies):");
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(3.0, 3.0);
                let cur = self.color;
                let mut harmonies = vec![complementary(cur)];
                harmonies.extend(triadic(cur));
                harmonies.extend(analogous(cur));
                for rgb in harmonies {
                    if swatch_button(ui, rgb, self.color == rgb).clicked() {
                        self.color = rgb;
                    }
                }
            });
            ui.separator();
            ui.add(egui::Slider::new(&mut self.brush.size, 1.0..=200.0).text("Size"));
            ui.add(egui::Slider::new(&mut self.brush.opacity, 0.05..=1.0).text("Opacity"));
            ui.add(egui::Slider::new(&mut self.brush.softness, 0.0..=1.0).text("Softness"));
            ui.add(egui::Slider::new(&mut self.brush.spacing, 0.05..=1.0).text("Spacing"));
            ui.collapsing("Pressure (touch / Pencil)", |ui| {
                ui.add(egui::Slider::new(&mut self.brush.pressure_size, 0.0..=1.0).text("Size"));
                ui.add(egui::Slider::new(&mut self.brush.pressure_opacity, 0.0..=1.0).text("Opacity"));
                ui.label("Mouse has no pressure sensor: it paints at full pressure.");
            });
            ui.collapsing("Velocity", |ui| {
                ui.add(egui::Slider::new(&mut self.brush.velocity_size, 0.0..=1.0).text("Thin"));
                ui.add(egui::Slider::new(&mut self.brush.velocity_opacity, 0.0..=1.0).text("Fade"));
                ui.label("Fast moves thin and fade (this is the old Dynamics, split).");
            });
            ui.collapsing("Texture", |ui| {
                ui.add(egui::Slider::new(&mut self.brush.grain, 0.0..=1.0).text("Grain"));
                ui.add(egui::Slider::new(&mut self.brush.grain_scale, 0.25..=4.0).text("Grain size"));
            });
            ui.collapsing("Dab shape", |ui| {
                ui.add(egui::Slider::new(&mut self.brush.roundness, 0.1..=1.0).text("Roundness"));
                ui.checkbox(&mut self.brush.angle_follow, "Follow direction");
                ui.add(egui::Slider::new(&mut self.brush.scatter, 0.0..=1.0).text("Scatter"));
            });
            ui.collapsing("Smoothing", |ui| {
                ui.add(egui::Slider::new(&mut self.brush.stabilizer, 0.0..=1.0).text("Stabilizer"));
            });
            ui.separator();
            ui.heading("Page");
            ui.horizontal(|ui| {
                for (name, rgb) in page_presets() {
                    let active = self.doc.background[..3] == rgb;
                    if ui.add(egui::Button::new(name).selected(active)).clicked() {
                        self.set_page_color(rgb);
                    }
                }
            });
            if egui::color_picker::color_edit_button_srgb(ui, &mut self.page_custom).changed() {
                self.set_page_color(self.page_custom);
            }
            ui.separator();
            ui.heading("Guides");
            ui.horizontal_wrapped(|ui| {
                for k in GuideKind::ALL {
                    ui.radio_value(&mut self.guides.kind, k, k.name());
                }
            });
            if self.guides.kind != GuideKind::Off {
                match self.guides.kind {
                    GuideKind::Square | GuideKind::Isometric => {
                        ui.add(egui::Slider::new(&mut self.guides.spacing, 8.0..=256.0).text("Spacing"));
                    }
                    GuideKind::OnePoint | GuideKind::TwoPoint => {
                        ui.label("Vanishing points are fixed in this version.");
                    }
                    GuideKind::Off => {}
                }
                ui.checkbox(&mut self.guides.snap, "Snap while painting");
                if self.guides.snap {
                    ui.add(egui::Slider::new(&mut self.guides.snap_px, 2.0..=32.0).text("Snap range (px)"));
                }
                ui.add(egui::Slider::new(&mut self.guides.opacity, 0.05..=1.0).text("Opacity"));
            }
            if self.doc.selection_mask.is_some() {
                ui.separator();
                ui.label(format!("Lasso: {} px", self.doc.selection_pixel_count()));
            } else if let Some((x0, y0, x1, y1)) = self.doc.selection {
                ui.separator();
                ui.label(format!("Selected {}x{} px", x1 - x0 + 1, y1 - y0 + 1));
            }
            });
        });
        } // end show_tools

        // Fresh id (not "layers"): eframe persists panel widths to disk, and the
        // old narrow width would otherwise override `default_size` forever.
        if self.show_layers {
        egui::Panel::right("layers-wide").default_size(280.0).show(ui, |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Layers");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add_sized([40.0, 24.0], egui::Button::new("▶")).clicked() {
                        self.show_layers = false;
                    }
                });
            });
            ui.horizontal(|ui| {
                if ui.button("Add layer").clicked() {
                    self.ghost_dirty = true;
                    self.doc.add_layer(format!("Layer {}", self.doc.layers.len() + 1));
                }
                if ui.button("Duplicate").clicked() {
                    let active = self.doc.active;
                    self.ghost_dirty = true;
                    self.doc.duplicate_layer(active);
                    self.status = "Layer duplicated.".to_string();
                }
                let can_merge = self.doc.active > 0 && self.doc.layers.len() > 1;
                if ui.add_enabled(can_merge, egui::Button::new("Merge down")).clicked() {
                    let active = self.doc.active;
                    if self.doc.merge_down(active) {
                        self.ghost_dirty = true;
                        self.status = "Merged down.".to_string();
                    }
                }
            });
            let mut remove = None;
            let mut select = None;
            let mut touched = false;
            for (i, layer) in self.doc.layers.iter_mut().enumerate().rev() {
                ui.horizontal(|ui| {
                    let mut vis = layer.visible;
                    if ui.checkbox(&mut vis, "").changed() {
                        layer.visible = vis;
                        touched = true;
                    }
                    let selected = self.doc.active == i;
                    if Some(i) == self.renaming {
                        // Double-clicked: edit the name in a staged buffer so
                        // Cancel can discard it. Commits via Save, Enter, or
                        // focus loss (touch keyboards often send neither, so
                        // the buttons are the reliable path on iPad).
                        let resp = ui.text_edit_singleline(&mut self.renaming_text);
                        if !resp.has_focus() {
                            resp.request_focus();
                        }
                        let enter = ui.ctx().input(|inp| inp.key_pressed(egui::Key::Enter));
                        let mut commit = resp.lost_focus() || enter;
                        let mut cancel = false;
                        if ui.add_sized([36.0, 22.0], egui::Button::new("✓")).clicked() {
                            commit = true;
                        }
                        if ui.add_sized([36.0, 22.0], egui::Button::new("✕")).clicked() {
                            cancel = true;
                        }
                        if commit && !cancel {
                            if self.renaming_text.trim().is_empty() {
                                self.renaming_text = format!("Layer {}", i + 1);
                            }
                            layer.name = std::mem::take(&mut self.renaming_text);
                        }
                        if commit || cancel {
                            self.renaming = None;
                            self.renaming_text.clear();
                        }
                    } else {
                        let resp = ui.selectable_label(selected, &layer.name);
                        if resp.clicked() {
                            select = Some(i);
                        }
                        if resp.double_clicked() {
                            select = Some(i);
                            self.renaming = Some(i);
                            self.renaming_text = layer.name.clone();
                        }
                    }
                    if ui.small_button("del").clicked() {
                        remove = Some(i);
                    }
                    if layer.mask.is_some() {
                        ui.label("M");
                    }
                    if layer.clip {
                        ui.label("C");
                    }
                });
            }
            if let Some(i) = select {
                self.doc.active = i;
                // A fresh full upload (matters for the mask viewer).
                self.doc.mark_full_dirty();
            }
            if touched {
                self.ghost_dirty = true;
                self.doc.mark_full_dirty();
            }
            if let Some(i) = remove {
                self.ghost_dirty = true;
                self.doc.remove_layer(i);
            }
            ui.separator();
            if let Some(layer) = self.doc.layers.get_mut(self.doc.active) {
                ui.label(format!("Opacity: {}", layer.name));
                let before = layer.opacity;
                ui.add(egui::Slider::new(&mut layer.opacity, 0.0..=1.0).text(""));
                let mut blend = layer.blend;
                let mut moved = 0;
                egui::ComboBox::from_label("Blend")
                    .selected_text(blend.name())
                    .show_ui(ui, |ui| {
                        for m in BlendMode::ALL {
                            ui.selectable_value(&mut blend, m, m.name());
                        }
                    });
                ui.horizontal(|ui| {
                    if ui.button("▲ Up").clicked() {
                        moved = 1;
                    }
                    if ui.button("▼ Down").clicked() {
                        moved = -1;
                    }
                });
                let active = self.doc.active;
                let mut dirty = (self.doc.layers[active].opacity - before).abs() > f32::EPSILON;
                if self.doc.layers[active].blend != blend {
                    self.doc.layers[active].blend = blend;
                    dirty = true;
                }
                if moved > 0 && active + 1 < self.doc.layers.len() {
                    self.doc.move_layer(active, active + 1);
                } else if moved < 0 && active > 0 {
                    self.doc.move_layer(active, active - 1);
                }
                if dirty {
                    self.ghost_dirty = true;
                    self.doc.mark_full_dirty();
                }
                // Masks, clipping and paint target for the active layer.
                let has_mask = self.doc.layers[active].mask.is_some();
                ui.horizontal(|ui| {
                    if ui.button(if has_mask { "Clear mask" } else { "Add mask" }).clicked() {
                        self.ghost_dirty = true;
                        if has_mask {
                            self.doc.clear_mask(active);
                            self.view_mask = false;
                            self.status = "Mask cleared.".to_string();
                        } else {
                            self.doc.add_mask(active);
                            self.status = "Mask added (white reveals). Paint on Mask to edit.".to_string();
                        }
                    }
                    if ui.checkbox(&mut self.view_mask, "View").changed() {
                        self.doc.mark_full_dirty();
                        self.status = if self.view_mask {
                            "Mask view: white reveals, black conceals.".to_string()
                        } else {
                            "Canvas view.".to_string()
                        };
                    }
                });
                ui.horizontal(|ui| {
                    let mut clip = self.doc.layers[active].clip;
                    if ui.checkbox(&mut clip, "Clip to below").changed() {
                        self.ghost_dirty = true;
                        self.doc.layers[active].clip = clip;
                        self.doc.mark_full_dirty();
                        self.status = if clip { "Layer clips to below.".to_string() } else { "Clip off.".to_string() };
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Paint:");
                    ui.radio_value(&mut self.doc.paint_target, PaintTarget::Color, "Color");
                    ui.radio_value(&mut self.doc.paint_target, PaintTarget::Mask, "Mask");
                });
            }
            ui.separator();
            ui.label(format!(
                "{} layer(s), active: {}",
                self.doc.layers.len(),
                self.doc.layers.get(self.doc.active).map(|l| l.name.as_str()).unwrap_or("-")
            ));
            ui.separator();
            ui.heading("Animation");
            ui.label("Layers are frames, bottom first.");
            ui.horizontal(|ui| {
                if ui.checkbox(&mut self.anim_assist, "Assist").changed() {
                    self.anim_playing = false;
                    self.texture = None; // rebuild the view (frame vs composite)
                    self.ghost_dirty = true;
                    self.status = if self.anim_assist {
                        "Animation Assist on: one frame at a time.".to_string()
                    } else {
                        "Animation Assist off.".to_string()
                    };
                }
                let playing = self.anim_playing;
                if ui.button(if playing { "Stop" } else { "Play" }).clicked() {
                    if playing {
                        self.anim_playing = false;
                        self.status = "Playback stopped.".to_string();
                    } else if self.doc.layers.len() < 2 {
                        self.status = "Need at least 2 layers to play.".to_string();
                    } else {
                        self.anim_assist = true;
                        self.anim_playing = true;
                        self.anim_last_t = ui.ctx().input(|i| i.time);
                        self.texture = None;
                        self.ghost_dirty = true;
                        self.status = "Playing.".to_string();
                    }
                }
            });
            if self.anim_assist && !self.doc.layers.is_empty() {
                ui.horizontal(|ui| {
                    if ui.button("|<").clicked() {
                        let n = self.doc.layers.len();
                        self.doc.active = (self.doc.active + n - 1) % n;
                        self.doc.mark_full_dirty();
                    }
                    ui.label(format!("Frame {}/{}", self.doc.active + 1, self.doc.layers.len()));
                    if ui.button(">|").clicked() {
                        let n = self.doc.layers.len();
                        self.doc.active = (self.doc.active + 1) % n;
                        self.doc.mark_full_dirty();
                    }
                });
                ui.add(egui::Slider::new(&mut self.anim_fps, MIN_ANIM_FPS..=MAX_ANIM_FPS).text("FPS"));
                if ui.checkbox(&mut self.onion, "Onion skin").changed() {
                    self.ghost_dirty = true;
                }
                if self.onion {
                    if ui
                        .add(egui::Slider::new(&mut self.onion_amount, 0.05..=0.8).text("Amount"))
                        .changed()
                    {
                        self.ghost_dirty = true;
                    }
                }
                if ui.button("Export GIF").clicked() {
                    if let Some(path) = rfd::FileDialog::new().add_filter("GIF image", &["gif"]).save_file()
                    {
                        let path = ensure_extension(path, "gif");
                        match export_gif(&self.doc, self.anim_fps) {
                            Ok(bytes) => match std::fs::write(&path, &bytes) {
                                Ok(()) => {
                                    let shown =
                                        self.doc.layers.iter().filter(|l| l.visible).count();
                                    self.status = format!(
                                        "GIF saved {} ({} frames, hidden skipped).",
                                        path.display(),
                                        shown
                                    );
                                    #[cfg(target_os = "ios")]
                                    rfd::ios_share_file(&path);
                                }
                                Err(e) => self.status = format!("GIF save failed: {e}"),
                            },
                            Err(e) => self.status = format!("GIF export failed: {e}"),
                        }
                    }
                }
            }
            ui.separator();
            ui.heading("Transform layer");
            ui.horizontal(|ui| {
                if ui.button("Scale +").clicked() {
                    self.doc.checkpoint();
                    self.doc.affine_layer(self.doc.active, 1.25, 0.0);
                }
                if ui.button("Scale −").clicked() {
                    self.doc.checkpoint();
                    self.doc.affine_layer(self.doc.active, 1.0 / 1.25, 0.0);
                }
            });
            ui.horizontal(|ui| {
                if ui.button("⟲ −15°").clicked() {
                    self.doc.checkpoint();
                    self.doc.affine_layer(self.doc.active, 1.0, -15.0);
                }
                if ui.button("⟳ +15°").clicked() {
                    self.doc.checkpoint();
                    self.doc.affine_layer(self.doc.active, 1.0, 15.0);
                }
            });
            ui.horizontal(|ui| {
                if ui.button("⇄ Flip H").clicked() {
                    self.doc.checkpoint();
                    self.doc.flip_layer_horizontal(self.doc.active);
                    self.status = "Layer flipped horizontally.".to_string();
                }
                if ui.button("⇅ Flip V").clicked() {
                    self.doc.checkpoint();
                    self.doc.flip_layer_vertical(self.doc.active);
                    self.status = "Layer flipped vertically.".to_string();
                }
            });
            ui.separator();
            ui.heading("Symmetry");
            for m in SymmetryMode::ALL {
                if ui.radio_value(&mut self.doc.symmetry, m, m.name()).changed() {
                    self.status = format!("Symmetry: {}.", m.name());
                }
            }
            if self.doc.symmetry == SymmetryMode::Kaleidoscope {
                ui.add(
                    egui::Slider::new(&mut self.doc.symmetry_segments, 2..=16).text("Segments"),
                );
            }
            ui.separator();
            ui.heading("Filters");
            ui.label("Active layer. Selections mask filters.");
            ui.add(egui::Slider::new(&mut self.filt_brightness, -100..=100).text("Brightness"));
            ui.add(egui::Slider::new(&mut self.filt_contrast, 0.0..=200.0).text("Contrast %"));
            if ui.button("Apply tone").clicked() {
                self.doc.checkpoint();
                self.doc.adjust_tone(
                    self.doc.active,
                    (self.filt_brightness as f32 * 2.55).round() as i32,
                    self.filt_contrast / 100.0,
                );
                self.status = "Tone applied.".to_string();
            }
            ui.add(egui::Slider::new(&mut self.filt_blur, 0..=25).text("Blur"));
            if ui.button("Apply blur").clicked() {
                self.doc.checkpoint();
                self.doc.blur_layer(self.doc.active, self.filt_blur.max(0) as u32);
                self.status = "Blur applied.".to_string();
            }
            ui.add(egui::Slider::new(&mut self.filt_sharpen, 0.0..=200.0).text("Sharpen %"));
            if ui.button("Apply sharpen").clicked() {
                self.doc.checkpoint();
                self.doc.sharpen_layer(self.doc.active, self.filt_sharpen / 100.0);
                self.status = "Sharpen applied.".to_string();
            }
        });
        });
        } // end show_layers

        // Floating edge buttons reopen hidden sidebars (touch-sized).
        if !self.show_tools {
            egui::Area::new(egui::Id::new("tools-fab"))
                .anchor(egui::Align2::LEFT_CENTER, [8.0, 0.0])
                .show(ui.ctx(), |ui| {
                    if ui.add_sized([44.0, 44.0], egui::Button::new("▶")).clicked() {
                        self.show_tools = true;
                    }
                });
        }
        if !self.show_layers {
            egui::Area::new(egui::Id::new("layers-fab"))
                .anchor(egui::Align2::RIGHT_CENTER, [-8.0, 0.0])
                .show(ui.ctx(), |ui| {
                    if ui.add_sized([44.0, 44.0], egui::Button::new("◀")).clicked() {
                        self.show_layers = true;
                    }
                });
        }

        egui::Panel::bottom("status").show(ui, |ui| {
            ui.label(&self.status);
        });

        egui::CentralPanel::default().show(ui, |ui| {
            let ctx = ui.ctx().clone();
            self.refresh_texture(&ctx);
            let tex = self.texture.as_ref().unwrap().id();
            let (w, h) = (self.doc.w as f32, self.doc.h as f32);
            let avail = ui.available_size();
            if self.zoom <= 0.0 {
                self.zoom = (avail.x / w).min(avail.y / h).clamp(0.1, 8.0);
            }
            let z = self.zoom;
            egui::ScrollArea::both().show(ui, |ui| {
                let (rect, resp) =
                    ui.allocate_exact_size(Vec2::new(w * z, h * z), Sense::click_and_drag());
                ui.painter().image(
                    tex,
                    rect,
                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::Pos2::new(1.0, 1.0)),
                    Color32::WHITE,
                );
                let to_canvas =
                    |pos: egui::Pos2| ((pos.x - rect.min.x) / z, (pos.y - rect.min.y) / z);
                let clamp_pt = |x: f32, y: f32| {
                    (x.clamp(0.0, w - 1.0), y.clamp(0.0, h - 1.0))
                };
                // Mirror axis overlay so the symmetry center is visible.
                let axis_color = Color32::from_rgba_unmultiplied(220, 40, 40, 180);
                let axis = |a: egui::Pos2, b: egui::Pos2| {
                    ui.painter().line_segment([a, b], (1.0, axis_color));
                };
                let center = egui::Pos2::new(rect.min.x + w * z * 0.5, rect.min.y + h * z * 0.5);
                let vline = || {
                    axis(
                        egui::Pos2::new(center.x, rect.min.y),
                        egui::Pos2::new(center.x, rect.max.y),
                    )
                };
                let hline = || {
                    axis(
                        egui::Pos2::new(rect.min.x, center.y),
                        egui::Pos2::new(rect.max.x, center.y),
                    )
                };
                match self.doc.symmetry {
                    SymmetryMode::Off => {}
                    SymmetryMode::Horizontal => vline(),
                    SymmetryMode::Vertical => hline(),
                    SymmetryMode::Both => {
                        vline();
                        hline();
                    }
                    SymmetryMode::Kaleidoscope => {
                        let n = self.doc.symmetry_segments.clamp(2, 16) as usize;
                        let reach = (rect.width().powi(2) + rect.height().powi(2)).sqrt() * 0.5;
                        for k in 0..n {
                            let (s, c) = (k as f32 * std::f32::consts::PI / n as f32).sin_cos();
                            let dir = egui::Vec2::new(c * reach, s * reach);
                            axis(center - dir, center + dir);
                        }
                    }
                }
                // Drawing guides overlay: view-assist only, never baked in.
                if self.guides.kind != GuideKind::Off {
                    let alpha = (self.guides.opacity.clamp(0.05, 1.0) * 255.0).round() as u8;
                    let gc = Color32::from_rgba_unmultiplied(70, 130, 200, alpha);
                    for [x0, y0, x1, y1] in self.guides.overlay_lines(w, h) {
                        ui.painter().line_segment(
                            [
                                egui::Pos2::new(rect.min.x + x0 * z, rect.min.y + y0 * z),
                                egui::Pos2::new(rect.min.x + x1 * z, rect.min.y + y1 * z),
                            ],
                            (1.0, gc),
                        );
                    }
                }
                // Onion skins over the current frame (assist only, not in
                // mask view): previous frame red, next frame blue.
                if self.anim_assist && self.onion && !self.view_mask {
                    self.refresh_ghosts(&ctx);
                    let full = egui::Rect::from_min_max(
                        egui::Pos2::ZERO,
                        egui::Pos2::new(1.0, 1.0),
                    );
                    if let Some(tex) = &self.ghost_prev {
                        ui.painter().image(tex.id(), rect, full, Color32::from_rgb(255, 130, 130));
                    }
                    if let Some(tex) = &self.ghost_next {
                        ui.painter().image(tex.id(), rect, full, Color32::from_rgb(130, 165, 255));
                    }
                }
                // Text anchor crosshair while placing type.
                if self.tool == Tool::Text {
                    if let Some((ax, ay)) = self.text_anchor {
                        let p = egui::Pos2::new(rect.min.x + ax * z, rect.min.y + ay * z);
                        for (color, width) in [(Color32::BLACK, 3.0), (Color32::WHITE, 1.5)] {
                            ui.painter().line_segment(
                                [p + egui::Vec2::new(-8.0, 0.0), p + egui::Vec2::new(8.0, 0.0)],
                                (width, color),
                            );
                            ui.painter().line_segment(
                                [p + egui::Vec2::new(0.0, -8.0), p + egui::Vec2::new(0.0, 8.0)],
                                (width, color),
                            );
                        }
                    }
                }
                // Committed lasso outline stays visible on every tool (it is
                // skipped while a new loop is being dragged: the Lasso branch
                // below draws the live path instead).
                if !self.lasso_poly.is_empty() && self.lasso_points.is_empty() {
                    let pts: Vec<egui::Pos2> = self
                        .lasso_poly
                        .iter()
                        .map(|&(x, y)| egui::Pos2::new(rect.min.x + x * z, rect.min.y + y * z))
                        .collect();
                    paint_path(ui.painter(), &pts, true);
                }

                // Eyedropper: Alt-click samples the composited canvas color
                // into the brush on any tool (Alt held blocks painting).
                if ctx.input(|i| i.modifiers.alt) {
                    if resp.drag_started() || resp.clicked() {
                        if let Some(pos) = resp.hover_pos() {
                            let (x, y) = to_canvas(pos);
                            let (cx, cy) = clamp_pt(x, y);
                            if let Some(rgb) = self.doc.sample(cx as u32, cy as u32) {
                                self.color = rgb;
                                self.status = format!(
                                    "Picked #{:02X}{:02X}{:02X}.",
                                    rgb[0], rgb[1], rgb[2]
                                );
                            }
                        }
                    }
                    self.painting = false;
                    self.last = None;
                    return;
                }

                if self.tool == Tool::Select {
                    // Marquee selection.
                    if resp.drag_started() {
                        if let Some(pos) = resp.hover_pos() {
                            let (x, y) = to_canvas(pos);
                            self.sel_anchor = Some(clamp_pt(x, y));
                            self.sel_current = self.sel_anchor;
                        }
                    }
                    if self.sel_anchor.is_some() && resp.dragged() {
                        if let Some(pos) = resp.hover_pos() {
                            let (x, y) = to_canvas(pos);
                            self.sel_current = Some(clamp_pt(x, y));
                        }
                    }
                    if resp.drag_stopped() {
                        if let (Some((ax, ay)), Some((bx, by))) = (self.sel_anchor, self.sel_current) {
                            let (x0, x1) = if ax <= bx { (ax as u32, bx as u32) } else { (bx as u32, ax as u32) };
                            let (y0, y1) = if ay <= by { (ay as u32, by as u32) } else { (by as u32, ay as u32) };
                            if x1.saturating_sub(x0) >= 2 && y1.saturating_sub(y0) >= 2 {
                                self.doc.selection = Some((x0, y0, x1, y1));
                                self.doc.selection_mask = None;
                                self.lasso_poly.clear();
                                self.status = format!("Selected {}x{} px.", x1 - x0 + 1, y1 - y0 + 1);
                            } else {
                                self.doc.clear_selection();
                            }
                        }
                        self.sel_anchor = None;
                        self.sel_current = None;
                    }
                    // Live rubber band + committed marching ants.
                    let band = match (self.sel_anchor, self.sel_current) {
                        (Some(a), Some(b)) => Some((a, b)),
                        _ => self.doc.selection.map(|(x0, y0, x1, y1)| {
                            ((x0 as f32, y0 as f32), ((x1 + 1) as f32, (y1 + 1) as f32))
                        }),
                    };
                    if let Some(((ax, ay), (bx, by))) = band {
                        let (x0, x1) = if ax <= bx { (ax, bx) } else { (bx, ax) };
                        let (y0, y1) = if ay <= by { (ay, by) } else { (by, ay) };
                        let r = egui::Rect::from_min_max(
                            egui::Pos2::new(rect.min.x + x0 * z, rect.min.y + y0 * z),
                            egui::Pos2::new(rect.min.x + x1 * z, rect.min.y + y1 * z),
                        );
                        paint_selection(ui.painter(), r, ctx.input(|i| i.time));
                    }
                    self.painting = false;
                    self.last = None;
                    return;
                }

                if self.tool == Tool::Lasso {
                    // Freehand loop: collect points, build the mask on release.
                    if resp.drag_started() {
                        if let Some(pos) = resp.hover_pos() {
                            let (x, y) = to_canvas(pos);
                            self.lasso_points = vec![clamp_pt(x, y)];
                        }
                    }
                    if resp.dragged() {
                        if let Some(pos) = resp.hover_pos() {
                            let (x, y) = to_canvas(pos);
                            let p = clamp_pt(x, y);
                            let push = match self.lasso_points.last() {
                                Some(&(lx, ly)) => {
                                    (p.0 - lx) * (p.0 - lx) + (p.1 - ly) * (p.1 - ly) >= 4.0
                                }
                                None => true,
                            };
                            if push {
                                self.lasso_points.push(p);
                            }
                        }
                    }
                    if !self.lasso_points.is_empty() {
                        let pts: Vec<egui::Pos2> = self
                            .lasso_points
                            .iter()
                            .map(|&(x, y)| egui::Pos2::new(rect.min.x + x * z, rect.min.y + y * z))
                            .collect();
                        paint_path(ui.painter(), &pts, true);
                    }
                    if resp.drag_stopped() {
                        if self.lasso_points.len() >= 3 {
                            let pts = self.lasso_points.clone();
                            match self.doc.set_lasso(&pts) {
                                Some(_) => {
                                    self.lasso_poly = pts;
                                    self.status = format!(
                                        "Lasso: {} px selected.",
                                        self.doc.selection_pixel_count()
                                    );
                                }
                                None => {
                                    self.doc.clear_selection();
                                    self.lasso_poly.clear();
                                    self.status = "Lasso too small.".to_string();
                                }
                            }
                        } else {
                            self.doc.clear_selection();
                            self.lasso_poly.clear();
                        }
                        self.lasso_points.clear();
                    }
                    self.painting = false;
                    self.last = None;
                    return;
                }

                if self.tool == Tool::Fill {
                    // Bucket fill at the press point (one undo step).
                    if resp.drag_started() {
                        if let Some(pos) = resp.hover_pos() {
                            let (x, y) = to_canvas(pos);
                            let (cx, cy) = clamp_pt(x, y);
                            self.doc.checkpoint();
                            let brush = self.base_brush();
                            let color = self.current_color();
                            self.doc.flood_fill(
                                &brush,
                                color,
                                cx as u32,
                                cy as u32,
                                self.fill_tolerance,
                            );
                            self.status = "Filled.".to_string();
                        }
                    }
                    self.painting = false;
                    self.last = None;
                    return;
                }

                if self.tool == Tool::Move {
                    // Drag the active layer around (integer steps).
                    if resp.drag_started() {
                        if let Some(pos) = resp.hover_pos() {
                            self.doc.checkpoint();
                            self.move_from = Some(to_canvas(pos));
                            self.move_acc = (0.0, 0.0);
                        }
                    }
                    if self.move_from.is_some() && resp.dragged() {
                        if let Some(pos) = resp.hover_pos() {
                            let (x, y) = to_canvas(pos);
                            let (fx, fy) = self.move_from.unwrap();
                            let ax = self.move_acc.0 + x - fx;
                            let ay = self.move_acc.1 + y - fy;
                            let (dx, dy) = (ax.trunc() as i32, ay.trunc() as i32);
                            if dx != 0 || dy != 0 {
                                self.doc.translate_layer(self.doc.active, dx, dy);
                            }
                            self.move_acc = (ax - dx as f32, ay - dy as f32);
                            self.move_from = Some((x, y));
                        }
                    }
                    if resp.drag_stopped() {
                        self.move_from = None;
                        self.status = "Layer moved.".to_string();
                    }
                    self.painting = false;
                    self.last = None;
                    return;
                }

                if self.tool == Tool::Shapes {
                    // Drag a preview, rasterize on release.
                    if resp.drag_started() {
                        if let Some(pos) = resp.hover_pos() {
                            let (x, y) = to_canvas(pos);
                            self.shape_anchor = Some(clamp_pt(x, y));
                            self.shape_current = self.shape_anchor;
                        }
                    }
                    if self.shape_anchor.is_some() && resp.dragged() {
                        if let Some(pos) = resp.hover_pos() {
                            let (x, y) = to_canvas(pos);
                            self.shape_current = Some(clamp_pt(x, y));
                        }
                    }
                    if let (Some((ax, ay)), Some((bx, by))) = (self.shape_anchor, self.shape_current) {
                        let r = egui::Rect::from_min_max(
                            egui::Pos2::new(rect.min.x + ax.min(bx) * z, rect.min.y + ay.min(by) * z),
                            egui::Pos2::new(rect.min.x + ax.max(bx) * z, rect.min.y + ay.max(by) * z),
                        );
                        paint_selection(ui.painter(), r, ctx.input(|i| i.time));
                    }
                    if resp.drag_stopped() {
                        if let (Some((ax, ay)), Some((bx, by))) = (self.shape_anchor, self.shape_current) {
                            self.doc.checkpoint();
                            let brush = self.base_brush();
                            let color = self.current_color();
                            match self.shape_kind {
                                ShapeKind::Line => {
                                    self.doc.shape_line(&brush, color, Some((ax, ay)), (bx, by), 0.0);
                                }
                                ShapeKind::Rect => {
                                    if self.shape_fill {
                                        self.doc.fill_rect(&brush, color, ax as u32, ay as u32, bx as u32, by as u32);
                                    } else {
                                        self.doc.shape_rect(&brush, color, ax, ay, bx, by);
                                    }
                                }
                                ShapeKind::Ellipse => {
                                    if self.shape_fill {
                                        self.doc.fill_ellipse(&brush, color, ax, ay, bx, by);
                                    } else {
                                        self.doc.shape_ellipse(&brush, color, ax, ay, bx, by);
                                    }
                                }
                                ShapeKind::Triangle => {
                                    if self.shape_fill {
                                        self.doc.fill_poly(&brush, color, ShapePoly::Triangle, ax, ay, bx, by, self.shape_sides);
                                    } else {
                                        self.doc.shape_poly(&brush, color, ShapePoly::Triangle, ax, ay, bx, by, self.shape_sides);
                                    }
                                }
                                ShapeKind::Diamond => {
                                    if self.shape_fill {
                                        self.doc.fill_poly(&brush, color, ShapePoly::Diamond, ax, ay, bx, by, self.shape_sides);
                                    } else {
                                        self.doc.shape_poly(&brush, color, ShapePoly::Diamond, ax, ay, bx, by, self.shape_sides);
                                    }
                                }
                                ShapeKind::Star => {
                                    if self.shape_fill {
                                        self.doc.fill_poly(&brush, color, ShapePoly::Star, ax, ay, bx, by, self.shape_sides);
                                    } else {
                                        self.doc.shape_poly(&brush, color, ShapePoly::Star, ax, ay, bx, by, self.shape_sides);
                                    }
                                }
                                ShapeKind::Arrow => {
                                    self.doc.shape_arrow(&brush, color, ax, ay, bx, by);
                                }
                                ShapeKind::Polygon => {
                                    if self.shape_fill {
                                        self.doc.fill_poly(&brush, color, ShapePoly::Polygon, ax, ay, bx, by, self.shape_sides);
                                    } else {
                                        self.doc.shape_poly(&brush, color, ShapePoly::Polygon, ax, ay, bx, by, self.shape_sides);
                                    }
                                }
                            }
                            self.status = format!("{} drawn.", self.shape_kind.name());
                        }
                        self.shape_anchor = None;
                        self.shape_current = None;
                    }
                    self.painting = false;
                    self.last = None;
                    return;
                }

                if self.tool == Tool::Text {
                    // Click anchors the text block and opens the dialog;
                    // Render bakes it to the active layer (undoable).
                    if resp.clicked() {
                        if let Some(pos) = resp.hover_pos() {
                            let (x, y) = to_canvas(pos);
                            self.text_anchor = Some(clamp_pt(x, y));
                            self.text_open = true;
                        }
                    }
                    self.painting = false;
                    self.last = None;
                    return;
                }

                if resp.drag_started() {
                    self.doc.checkpoint();
                    self.painting = true;
                    // Painting edits the current frame: pause playback so
                    // the viewed frame never goes stale mid-stroke.
                    if self.anim_playing {
                        self.anim_playing = false;
                        self.status = "Paused for painting.".to_string();
                    }
                    self.last = None;
                    self.last_dyn = DabDynamics::default();
                    self.leftover = 0.0;
                    self.smooth_pt = None;
                    self.smooth_pressure = 1.0;
                    self.last_t = ctx.input(|i| i.time);
                }
                if self.painting && resp.dragged() {
                    if let Some(pos) = resp.hover_pos() {
                        let (x, y) = to_canvas(pos);
                        // Clamp: off-page drags paint nothing instead of
                        // streaking the canvas edge (and poisoning the
                        // dirty rect, which used to quit the app).
                        let (x, y) = clamp_pt(x, y);
                        let now = ctx.input(|i| i.time);
                        let dt = (now - self.last_t).max(1.0 / 240.0) as f32;
                        self.last_t = now;
                        let brush = self.base_brush();
                        // Pressure: Pencil/touch force when the backend
                        // forwards it, else full pressure (mice have no
                        // sensor; velocity taper below still applies).
                        let pressure = latest_touch_force(&ctx).unwrap_or(1.0);
                        // Stabilizer: exponential smoothing toward the raw
                        // pointer; 0 keeps the raw point bit-for-bit.
                        let a = brush.stabilizer.clamp(0.0, 1.0) * 0.85;
                        let (sx, sy) = match self.smooth_pt {
                            Some((px, py)) => (px + (x - px) * (1.0 - a), py + (y - py) * (1.0 - a)),
                            None => (x, y),
                        };
                        self.smooth_pt = Some((sx, sy));
                        // Drawing assist: snap the smoothed point to the
                        // guides before speed + stroke (shapes stay free).
                        let (sx, sy) = self.guides.snap_point(w, h, sx, sy);
                        self.smooth_pressure += (pressure - self.smooth_pressure) * (1.0 - a);
                        // Speed + direction from the smoothed trail.
                        let (speed_norm, angle) = match self.last {
                            Some((lx, ly)) => {
                                let dist = ((sx - lx).powi(2) + (sy - ly).powi(2)).sqrt();
                                let n = (dist / dt / 3500.0).clamp(0.0, 1.0);
                                let ang = if dist > 0.001 {
                                    (sy - ly).atan2(sx - lx)
                                } else {
                                    self.last_dyn.angle
                                };
                                (n, ang)
                            }
                            None => (0.0, self.last_dyn.angle),
                        };
                        let from_dyn = if self.last.is_some() { self.last_dyn } else { DabDynamics::default() };
                        let to_dyn = DabDynamics {
                            pressure: self.smooth_pressure,
                            speed_norm,
                            angle,
                        };
                        let color = self.current_color();
                        let (last, leftover) = self.doc.stroke_to_dyn(
                            &brush,
                            color,
                            self.last,
                            from_dyn,
                            (sx, sy),
                            to_dyn,
                            self.leftover,
                        );
                        self.last = last;
                        self.last_dyn = to_dyn;
                        self.leftover = leftover;
                    }
                }
                if resp.drag_stopped() {
                    self.painting = false;
                    self.last = None;
                    self.smooth_pt = None;
                }
            });
        });
    }
}

/// Window icon: original layer-stack mark drawn in VectorCraft
/// (see `assets/icon-generator.py`, rendered to `assets/icon-48.png`).
fn app_icon() -> Option<egui::IconData> {
    let bytes = include_bytes!("../../../assets/icon-48.png");
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    let (w, h) = (img.width(), img.height());
    Some(egui::IconData { rgba: img.into_raw(), width: w, height: h })
}

fn main() {
    let mut viewport = egui::ViewportBuilder::default().with_inner_size([1400.0, 900.0]);
    if let Some(icon) = app_icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    let _ = eframe::run_native(
        "ProCraft",
        options,
        Box::new(|cc| Ok(Box::new(ProCraftApp::new(cc)))),
    );
}
