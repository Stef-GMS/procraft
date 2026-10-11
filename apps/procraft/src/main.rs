//! ProCraft milestone 2: smooth partial-upload painting, page colors, themes,
//! brush presets, velocity dynamics and rectangular selections. Original code.

use eframe::egui::{self, Color32, ColorImage, Sense, TextureHandle, TextureOptions, Vec2};
use procraft_engine::{BlendMode, Brush, Color, Document};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    Brush,
    Eraser,
    Select,
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
        ("Blue", [30, 40, 60]),
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
    texture: Option<TextureHandle>,
    painting: bool,
    last: Option<(f32, f32)>,
    last_t: f64,
    leftover: f32,
    sel_anchor: Option<(f32, f32)>,
    sel_current: Option<(f32, f32)>,
    zoom: f32,
    status: String,
}

impl ProCraftApp {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            doc: Document::new(1280, 800),
            brush: Brush::default(),
            color: [30, 144, 255],
            tool: Tool::Brush,
            theme: Theme::Dark,
            theme_applied: None,
            page_custom: [128, 128, 128],
            dynamics: 0.6,
            texture: None,
            painting: false,
            last: None,
            last_t: 0.0,
            leftover: 0.0,
            sel_anchor: None,
            sel_current: None,
            zoom: 1.0,
            status: "Drag on the canvas to paint. Ctrl+Z undoes, Esc clears selection.".to_string(),
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

impl eframe::App for ProCraftApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.theme_applied != Some(self.theme) {
            ui.ctx().set_visuals(self.theme.visuals());
            self.theme_applied = Some(self.theme);
        }
        // Ctrl+Z undo, Esc clears selection.
        if ui.ctx().input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::Z)) && self.doc.undo()
        {
            self.status = "Undone.".to_string();
        }
        if ui.ctx().input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            self.doc.selection = None;
            self.sel_anchor = None;
            self.sel_current = None;
        }
        // Keep frames coming while painting, selecting, or marching ants.
        if self.painting || self.sel_anchor.is_some() || self.doc.selection.is_some() {
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

        egui::Panel::left("tools").show(ui, |ui| {
            ui.heading("Tool");
            ui.radio_value(&mut self.tool, Tool::Brush, "Brush");
            ui.radio_value(&mut self.tool, Tool::Eraser, "Eraser");
            ui.radio_value(&mut self.tool, Tool::Select, "Select");
            if ui.button("Clear selection").clicked() {
                self.doc.selection = None;
                self.status = "Selection cleared.".to_string();
            }
            ui.separator();
            ui.heading("Brushes");
            for (name, preset) in procraft_engine::presets() {
                if ui.button(name).clicked() {
                    self.brush = preset;
                    self.tool = Tool::Brush;
                }
            }
            ui.separator();
            ui.heading("Color");
            egui::color_picker::color_edit_button_srgb(ui, &mut self.color);
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
            if let Some((x0, y0, x1, y1)) = self.doc.selection {
                ui.separator();
                ui.label(format!("Selected {}x{} px", x1 - x0 + 1, y1 - y0 + 1));
            }
        });

        egui::Panel::right("layers").show(ui, |ui| {
            ui.heading("Layers");
            if ui.button("Add layer").clicked() {
                self.doc.add_layer(format!("Layer {}", self.doc.layers.len() + 1));
            }
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
                    if ui.selectable_label(selected, &layer.name).clicked() {
                        select = Some(i);
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
                                self.status = format!("Selected {}x{} px.", x1 - x0 + 1, y1 - y0 + 1);
                            } else {
                                self.doc.selection = None;
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

fn main() {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1400.0, 900.0]),
        ..Default::default()
    };
    let _ = eframe::run_native(
        "ProCraft",
        options,
        Box::new(|cc| Ok(Box::new(ProCraftApp::new(cc)))),
    );
}
