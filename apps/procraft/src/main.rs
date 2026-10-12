//! ProCraft milestone 7: layer filters (tone, blur, sharpen), plus all
//! earlier features. Original code.

use eframe::egui::{self, Color32, ColorImage, Sense, TextureHandle, TextureOptions, Vec2};
use procraft_engine::{BlendMode, Brush, Color, Document, SymmetryMode};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    Brush,
    Eraser,
    Select,
    Lasso,
    Fill,
    Move,
    Shapes,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ShapeKind {
    Line,
    Rect,
    Ellipse,
}

impl ShapeKind {
    const ALL: [ShapeKind; 3] = [ShapeKind::Line, ShapeKind::Rect, ShapeKind::Ellipse];

    fn name(self) -> &'static str {
        match self {
            ShapeKind::Line => "Line",
            ShapeKind::Rect => "Rect",
            ShapeKind::Ellipse => "Ellipse",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Theme {
    Dark,
    Light,
    Paper,
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
    dynamics: f32,
    picker_size: f32,
    texture: Option<TextureHandle>,
    painting: bool,
    last: Option<(f32, f32)>,
    last_t: f64,
    leftover: f32,
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
    preset_textures: Vec<TextureHandle>,
    move_from: Option<(f32, f32)>,
    move_acc: (f32, f32),
    shape_anchor: Option<(f32, f32)>,
    shape_current: Option<(f32, f32)>,
    shape_kind: ShapeKind,
    shape_fill: bool,
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
            dynamics: 0.6,
            picker_size: 280.0,
            texture: None,
            painting: false,
            last: None,
            last_t: 0.0,
            leftover: 0.0,
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
            preset_textures: Vec::new(),
            move_from: None,
            move_acc: (0.0, 0.0),
            shape_anchor: None,
            shape_current: None,
            shape_kind: ShapeKind::Rect,
            shape_fill: false,
            zoom: 1.0,
            status: "Drag on the canvas to paint. Ctrl+Z undoes, Ctrl+Y redoes, Esc clears selection.".to_string(),
        }
    }

    fn base_brush(&self) -> Brush {
        Brush { eraser: self.tool == Tool::Eraser, ..self.brush }
    }

    fn current_color(&self) -> Color {
        Color::rgb(self.color[0], self.color[1], self.color[2])
    }

    /// Upload only what changed since the last frame (no-op when clean).
    fn refresh_texture(&mut self, ctx: &egui::Context) {
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

    fn set_page_color(&mut self, rgb: [u8; 3]) {
        self.doc.background = [rgb[0], rgb[1], rgb[2], 255];
        self.doc.mark_full_dirty();
    }

    /// Honest brush previews: each preset paints a wavy sample stroke through
    /// the real engine into a thumbnail texture (dotted chalk reads dotted).
    fn build_preset_previews(&mut self, ctx: &egui::Context) {
        self.preset_textures.clear();
        for (idx, (_, preset)) in procraft_engine::presets().into_iter().enumerate() {
            let mut doc = Document::new(144, 36);
            let brush = Brush { eraser: false, ..preset };
            let color = Color::rgb(150, 150, 150);
            let mut last = None;
            let mut left = 0.0;
            let mut x = 8.0;
            while x <= 136.0 {
                let y = 18.0
                    + 8.0 * ((x - 8.0) / 128.0 * std::f32::consts::PI * 2.0).sin();
                let (l, lo) = doc.stroke_to(&brush, color, last, (x, y), left);
                last = l;
                left = lo;
                x += 2.0;
            }
            let img = ColorImage::from_rgba_unmultiplied(
                [144, 36],
                &doc.layers[0].pixels.clone(),
            );
            self.preset_textures.push(ctx.load_texture(
                format!("preset-{idx}"),
                img,
                TextureOptions::LINEAR,
            ));
        }
    }
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
            self.status = "Undone.".to_string();
        }
        if ui.ctx().input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::Y)) && self.doc.redo()
        {
            self.status = "Redone.".to_string();
        }
        if ui.ctx().input_mut(|i| {
            i.consume_key(egui::Modifiers::CTRL.plus(egui::Modifiers::SHIFT), egui::Key::Z)
        }) && self.doc.redo()
        {
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

        egui::Panel::top("menu").show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Save PNG").clicked() {
                    if let Some(path) = rfd::FileDialog::new().add_filter("PNG", &["png"]).save_file() {
                        match self.doc.save_png(&path) {
                            Ok(()) => self.status = format!("Saved {}", path.display()),
                            Err(e) => self.status = format!("Save failed: {e}"),
                        }
                    }
                }
                if ui.button("Open image").clicked() {
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
                }
                if ui.button("Save project").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("ProCraft project", &["procraft"])
                        .save_file()
                    {
                        let path = ensure_extension(path, "procraft");
                        match self.doc.save_project(&path) {
                            Ok(()) => self.status = format!("Project saved {}", path.display()),
                            Err(e) => self.status = format!("Project save failed: {e}"),
                        }
                    }
                }
                if ui.button("Open project").clicked() {
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
            });
        });

        // Fresh id (not "tools"): eframe persists panel widths to disk, and the
        // old narrow width would otherwise override `default_size` forever.
        egui::Panel::left("tools-wide").default_size(300.0).show(ui, |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.heading("Tool");
            ui.radio_value(&mut self.tool, Tool::Brush, "✏ Brush");
            ui.radio_value(&mut self.tool, Tool::Eraser, "⌫ Eraser");
            ui.radio_value(&mut self.tool, Tool::Select, "⬚ Select");
            ui.radio_value(&mut self.tool, Tool::Lasso, "◌ Lasso");
            ui.radio_value(&mut self.tool, Tool::Fill, "◧ Fill");
            ui.radio_value(&mut self.tool, Tool::Move, "↔ Move");
            ui.radio_value(&mut self.tool, Tool::Shapes, "○ Shapes");
            if self.tool == Tool::Fill {
                ui.add(egui::Slider::new(&mut self.fill_tolerance, 0..=255).text("Tolerance"));
            }
            if self.tool == Tool::Shapes {
                ui.horizontal(|ui| {
                    for k in ShapeKind::ALL {
                        ui.radio_value(&mut self.shape_kind, k, k.name());
                    }
                });
                ui.checkbox(&mut self.shape_fill, "Fill");
            }
            if ui.button("Clear selection").clicked() {
                self.doc.clear_selection();
                self.lasso_poly.clear();
                self.status = "Selection cleared.".to_string();
            }
            ui.separator();
            ui.heading("Brushes");
            if self.preset_textures.len() != procraft_engine::presets().len() {
                let ctx = ui.ctx().clone();
                self.build_preset_previews(&ctx);
            }
            for (idx, (name, preset)) in procraft_engine::presets().into_iter().enumerate() {
                ui.horizontal(|ui| {
                    if let Some(tex) = self.preset_textures.get(idx) {
                        ui.image((tex.id(), Vec2::new(120.0, 30.0)));
                    }
                    if ui.button(name).clicked() {
                        self.brush = preset;
                        self.tool = Tool::Brush;
                    }
                });
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
            ui.add(egui::Slider::new(&mut self.brush.size, 1.0..=200.0).text("Size"));
            ui.add(egui::Slider::new(&mut self.brush.opacity, 0.05..=1.0).text("Opacity"));
            ui.add(egui::Slider::new(&mut self.brush.softness, 0.0..=1.0).text("Softness"));
            ui.add(egui::Slider::new(&mut self.brush.spacing, 0.05..=1.0).text("Spacing"));
            ui.add(egui::Slider::new(&mut self.dynamics, 0.0..=1.0).text("Dynamics"));
            ui.separator();
            ui.heading("Page");
            ui.horizontal(|ui| {
                for (name, rgb) in page_presets() {
                    if ui.button(name).clicked() {
                        self.set_page_color(rgb);
                    }
                }
            });
            if egui::color_picker::color_edit_button_srgb(ui, &mut self.page_custom).changed() {
                self.set_page_color(self.page_custom);
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

        // Fresh id (not "layers"): eframe persists panel widths to disk, and the
        // old narrow width would otherwise override `default_size` forever.
        egui::Panel::right("layers-wide").default_size(280.0).show(ui, |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.heading("Layers");
            ui.horizontal(|ui| {
                if ui.button("Add layer").clicked() {
                    self.doc.add_layer(format!("Layer {}", self.doc.layers.len() + 1));
                }
                if ui.button("Duplicate").clicked() {
                    let active = self.doc.active;
                    self.doc.duplicate_layer(active);
                    self.status = "Layer duplicated.".to_string();
                }
                let can_merge = self.doc.active > 0 && self.doc.layers.len() > 1;
                if ui.add_enabled(can_merge, egui::Button::new("Merge down")).clicked() {
                    let active = self.doc.active;
                    if self.doc.merge_down(active) {
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
                        // Double-clicked: edit the name in place.
                        let resp = ui.text_edit_singleline(&mut layer.name);
                        if !resp.has_focus() {
                            resp.request_focus();
                        }
                        let enter = ui.ctx().input(|inp| inp.key_pressed(egui::Key::Enter));
                        if resp.lost_focus() || enter {
                            if layer.name.trim().is_empty() {
                                layer.name = format!("Layer {}", i + 1);
                            }
                            self.renaming = None;
                        }
                    } else {
                        let resp = ui.selectable_label(selected, &layer.name);
                        if resp.clicked() {
                            select = Some(i);
                        }
                        if resp.double_clicked() {
                            select = Some(i);
                            self.renaming = Some(i);
                        }
                    }
                    if ui.small_button("del").clicked() {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = select {
                self.doc.active = i;
            }
            if touched {
                self.doc.mark_full_dirty();
            }
            if let Some(i) = remove {
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
                    self.doc.mark_full_dirty();
                }
            }
            ui.separator();
            ui.label(format!(
                "{} layer(s), active: {}",
                self.doc.layers.len(),
                self.doc.layers.get(self.doc.active).map(|l| l.name.as_str()).unwrap_or("-")
            ));
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

                if resp.drag_started() {
                    self.doc.checkpoint();
                    self.painting = true;
                    self.last = None;
                    self.leftover = 0.0;
                    self.last_t = ctx.input(|i| i.time);
                }
                if self.painting && resp.dragged() {
                    if let Some(pos) = resp.hover_pos() {
                        let (x, y) = to_canvas(pos);
                        let now = ctx.input(|i| i.time);
                        let dt = (now - self.last_t).max(1.0 / 240.0) as f32;
                        self.last_t = now;
                        // Velocity dynamics: fast moves paint thinner/lighter.
                        let mut brush = self.base_brush();
                        if self.dynamics > 0.0 {
                            if let Some((lx, ly)) = self.last {
                                let dist = ((x - lx).powi(2) + (y - ly).powi(2)).sqrt();
                                let speed = dist / dt;
                                let k = (speed / 3500.0).clamp(0.0, 1.0) * self.dynamics;
                                brush.size *= 1.0 - 0.6 * k;
                                brush.opacity *= 1.0 - 0.4 * k;
                            }
                        }
                        let color = self.current_color();
                        let (last, leftover) =
                            self.doc.stroke_to(&brush, color, self.last, (x, y), self.leftover);
                        self.last = last;
                        self.leftover = leftover;
                    }
                }
                if resp.drag_stopped() {
                    self.painting = false;
                    self.last = None;
                }
            });
        });
    }
}

/// Window icon: original peacock-feather art drawn in VectorCraft
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
