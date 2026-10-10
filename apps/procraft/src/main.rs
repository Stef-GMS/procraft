//! ProCraft milestone 1: paint with a soft brush on layers, export PNG.
//! Original code; egui desktop app over `procraft-engine`.

use eframe::egui::{self, Color32, ColorImage, Sense, TextureHandle, TextureOptions, Vec2};
use procraft_engine::{Brush, Color, Document};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    Brush,
    Eraser,
}

struct ProCraftApp {
    doc: Document,
    brush: Brush,
    color: [u8; 3],
    tool: Tool,
    texture: Option<TextureHandle>,
    painting: bool,
    last: Option<(f32, f32)>,
    leftover: f32,
    status: String,
}

impl ProCraftApp {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            doc: Document::new(1280, 800),
            brush: Brush::default(),
            color: [30, 144, 255],
            tool: Tool::Brush,
            texture: None,
            painting: false,
            last: None,
            leftover: 0.0,
            status: "Drag on the canvas to paint. Ctrl+Z undoes.".to_string(),
        }
    }

    fn current_brush(&self) -> Brush {
        Brush { eraser: self.tool == Tool::Eraser, ..self.brush }
    }

    fn current_color(&self) -> Color {
        Color::rgb(self.color[0], self.color[1], self.color[2])
    }

    fn refresh_texture(&mut self, ctx: &egui::Context) {
        let flat = self.doc.flatten();
        let img = ColorImage::from_rgba_unmultiplied([self.doc.w as usize, self.doc.h as usize], &flat);
        match &mut self.texture {
            Some(handle) => handle.set(img, TextureOptions::LINEAR),
            None => {
                self.texture = Some(ctx.load_texture("canvas", img, TextureOptions::LINEAR));
            }
        }
    }
}

impl eframe::App for ProCraftApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Ctrl+Z undo.
        if ui.ctx().input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::Z)) && self.doc.undo()
        {
            self.status = "Undone.".to_string();
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
                if ui.button("Clear layer").clicked() {
                    self.doc.checkpoint();
                    if let Some(layer) = self.doc.active_layer_mut() {
                        layer.clear();
                    }
                }
                if ui.button("Undo").clicked() && self.doc.undo() {
                    self.status = "Undone.".to_string();
                }
            });
        });

        egui::Panel::left("tools").show(ui, |ui| {
            ui.heading("Tool");
            ui.radio_value(&mut self.tool, Tool::Brush, "Brush");
            ui.radio_value(&mut self.tool, Tool::Eraser, "Eraser");
            ui.separator();
            ui.heading("Color");
            egui::color_picker::color_edit_button_srgb(ui, &mut self.color);
            ui.separator();
            ui.add(egui::Slider::new(&mut self.brush.size, 1.0..=200.0).text("Size"));
            ui.add(egui::Slider::new(&mut self.brush.opacity, 0.05..=1.0).text("Opacity"));
            ui.add(egui::Slider::new(&mut self.brush.softness, 0.0..=1.0).text("Softness"));
            ui.add(egui::Slider::new(&mut self.brush.spacing, 0.05..=1.0).text("Spacing"));
        });

        egui::Panel::right("layers").show(ui, |ui| {
            ui.heading("Layers");
            if ui.button("Add layer").clicked() {
                self.doc.add_layer(format!("Layer {}", self.doc.layers.len() + 1));
            }
            let mut remove = None;
            let mut select = None;
            for (i, layer) in self.doc.layers.iter_mut().enumerate().rev() {
                ui.horizontal(|ui| {
                    let mut vis = layer.visible;
                    if ui.checkbox(&mut vis, "").changed() {
                        layer.visible = vis;
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
            if let Some(i) = remove {
                self.doc.remove_layer(i);
            }
            ui.separator();
            if let Some(layer) = self.doc.layers.get_mut(self.doc.active) {
                ui.label(format!("Opacity: {}", layer.name));
                ui.add(egui::Slider::new(&mut layer.opacity, 0.0..=1.0).text(""));
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
            egui::ScrollArea::both().show(ui, |ui| {
                let (rect, resp) =
                    ui.allocate_exact_size(Vec2::new(w, h), Sense::click_and_drag());
                ui.painter().image(
                    tex,
                    rect,
                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::Pos2::new(1.0, 1.0)),
                    Color32::WHITE,
                );
                let to_canvas = |pos: egui::Pos2| (pos.x - rect.min.x, pos.y - rect.min.y);
                if resp.drag_started() {
                    self.doc.checkpoint();
                    self.painting = true;
                    self.last = None;
                    self.leftover = 0.0;
                }
                if self.painting && resp.dragged() {
                    if let Some(pos) = resp.hover_pos() {
                        let (x, y) = to_canvas(pos);
                        let brush = self.current_brush();
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
