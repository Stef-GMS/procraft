//! ProCraft by surfrpt1 (https://github.com/surfrpt1).
//! Original clean-room code, MIT licensed.
//!
//! ProCraft milestone 9: brush library (custom brushes + .pcbrush import/export),
//! plus all earlier features. Original code.

use eframe::egui::{self, Color32, ColorImage, Sense, TextureHandle, TextureOptions, Vec2};
// iOS has no rfd backend: same call shape, every dialog declined.
use procraft_engine::{
    analogous, check_font, clamp_frame, complementary, curves_lut, export_gif_ext,
    export_timelapse, export_tip_png, gallery_thumb, import_brush_file, import_gif_document,
    import_gpl_file, levels_lut, next_frame, onion_ghost, open_brush_library, open_palette,
    open_psd, palette_from_rgba, prev_frame, rasterize_frame, rasterize_frame_region,
    save_brush_library, save_palette, save_psd, starter_palette, step_playback_visible, triadic,
    AssistLine, BlendMode, Brush, BrushPreset, Color, DabDynamics, Document, GuideKind, Guides,
    LoopMode, PaintTarget, Palette, PaletteSet, ShapePoly, SymmetryMode, TextStyle, LAPSE_SIDE,
    MAX_ANIM_FPS, MAX_ANIM_HOLD, MAX_CANVAS_SIDE, MAX_IMAGE_COLORS, MAX_LAPSE_FRAMES,
    MAX_LIBRARY_BRUSHES, MAX_OUTLINE_PX, MAX_PALETTES, MAX_PALETTE_COLORS, MAX_TEXT_SPACING,
    MIN_ANIM_FPS, MIN_ANIM_HOLD, MIN_TEXT_PX,
};
#[cfg(any(target_os = "ios", target_os = "android"))]
use rfd_shim as rfd;

/// Latest Apple Pencil / touch pressure (0.0..=1.0) seen in this frame's
/// input events, if the backend forwarded any. egui 0.36 exposes pressure
/// only on `Event::Touch.force` — desktop mice always yield `None`, and
/// tilt is not exposed at all (documented M14 limitation).
fn latest_touch_force(ctx: &egui::Context) -> Option<f32> {
    ctx.input(|i| {
        let mut force = None;
        for e in &i.events {
            if let egui::Event::Touch {
                force: f, phase, ..
            } = e
            {
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
    Smudge,
    Select,
    Lasso,
    Fill,
    Picker,
    Move,
    Liquify,
    Crop,
    Shapes,
    Text,
}

/// Tool radio label: icon glyphs on symbol-font platforms, plain words
/// where the glyphs would be tofu (iPad). Shared render-safe strings
/// (brush, eraser, smudge, move, shapes) are identical either way.
fn tool_label(tool: Tool, symbols: bool) -> &'static str {
    match tool {
        Tool::Brush => "✏ Brush",
        Tool::Eraser => "⌫ Eraser",
        Tool::Smudge => "💧 Smudge",
        Tool::Select => {
            if symbols {
                "⬚ Select"
            } else {
                "Select"
            }
        }
        Tool::Lasso => {
            if symbols {
                "◌ Lasso"
            } else {
                "Lasso"
            }
        }
        Tool::Fill => {
            if symbols {
                "◧ Fill"
            } else {
                "Fill"
            }
        }
        Tool::Picker => {
            if symbols {
                "◉ Pick"
            } else {
                "Pick"
            }
        }
        Tool::Move => "↔ Move",
        Tool::Liquify => {
            if symbols {
                "➰ Liquify"
            } else {
                "Liquify"
            }
        }
        Tool::Crop => {
            if symbols {
                "◫ Crop"
            } else {
                "Crop"
            }
        }
        Tool::Shapes => "○ Shapes",
        Tool::Text => "T Text",
    }
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

/// Live ColorDrop preview: the pre-fill layer snapshot plus the drag
/// geometry, so each tolerance update restores and re-fills from the same
/// seed (one undo step for the whole gesture).
struct FillPreview {
    layer: usize,
    seed: (u32, u32),
    pixels: Vec<u8>,
    start_x: f32,
    start_tol: u8,
    applied: u8,
    live_hash: u64,
}

/// Live filter preview baseline: scrubbing a filter slider restores these
/// pixels then re-applies the slider value, so a whole preview gesture
/// costs one undo step instead of compounding per change. Re-applies are
/// throttled (see `PREVIEW_MIN_DT`): skipped ticks flag `stale`, and
/// Apply restamps the latest values so nothing is lost.
struct FilterPreview {
    /// 0 = tone, 1 = blur, 2 = sharpen, 3 = levels, 4 = curves,
    /// 5 = HSL, 6 = balance, 7 = recolor.
    kind: u8,
    layer: usize,
    pixels: Vec<u8>,
    last: std::time::Instant,
    stale: bool,
    /// Fingerprint of the layer right after our last apply (or of the
    /// snapshot on a fresh preview). A tick finding anything else means
    /// another edit touched the layer meanwhile — restoring the stale
    /// snapshot would wipe that work, so the preview restarts instead.
    live_hash: u64,
}

/// Cheap non-crypto fingerprint of layer pixels (samples every ~1KB):
/// detects "the layer changed underneath us" for preview guards. A miss
/// only ever restarts a preview, never destroys work.
fn pixels_hash(pixels: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    h = h
        .wrapping_mul(0x100000001b3)
        .wrapping_add(pixels.len() as u64);
    let step = (pixels.len() / 4096).max(1);
    for (i, &b) in pixels.iter().enumerate().step_by(step) {
        h ^= (b as u64).wrapping_add(i as u64);
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Minimum time between live preview re-applies while a slider is
/// scrubbed. Full-canvas filters cost 15–60 ms at 1280x800 plus a
/// re-flatten and texture upload per tick; without a floor, every slider
/// tick queues one and the UI visibly hangs during the drag.
const PREVIEW_MIN_DT: std::time::Duration = std::time::Duration::from_millis(50);

/// M41 Time-lapse: minimum time between staged snapshots (with the
/// frame cap, this bounds a session to ~180 small thumbs, not gigabytes).
const LAPSE_MIN_DT: std::time::Duration = std::time::Duration::from_secs(10);

/// M35 Selection upgrades: saved snapshot (canvas dims, rect, mask).
type SavedSelection = ((u32, u32), Option<(u32, u32, u32, u32)>, Option<Vec<u8>>);

/// In-progress multi-finger tap (undo/redo): tracks the widest finger
/// count, start time/center, and whether it moved (a pan/pinch is not
/// a tap). Fires on clean release; see the top of `App::ui`.
struct TapTrack {
    n: usize,
    t0: f64,
    c0: egui::Pos2,
    moved: bool,
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
    ImagePalette,
    Gif,
    Reference,
    Font,
    DualTip,
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

    fn as_str(self) -> &'static str {
        match self {
            Theme::Dark => "dark",
            Theme::Light => "light",
            Theme::Paper => "paper",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Theme::Dark => "Dark",
            Theme::Light => "Light",
            Theme::Paper => "Paper",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        match s {
            "dark" => Some(Theme::Dark),
            "light" => Some(Theme::Light),
            "paper" => Some(Theme::Paper),
            _ => None,
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

/// M33 Gallery: how many recent projects to remember (newest first).
const MAX_RECENT: usize = 10;
/// M33 Gallery: thumbnail longest side in px.
const GALLERY_THUMB_SIDE: u32 = 96;

/// M33 Gallery: record a project path as most-recent (dedup, truncate).
fn push_recent(recent: &mut Vec<String>, path: &str) {
    let path = path.trim();
    if path.is_empty() {
        return;
    }
    recent.retain(|p| p != path);
    recent.insert(0, path.to_string());
    recent.truncate(MAX_RECENT);
}

fn page_presets() -> Vec<(&'static str, [u8; 3])> {
    vec![
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
    /// True when the system symbol font loaded (desktop Windows): UI
    /// strings may use icon glyphs. iPad has no such font, so the same
    /// strings fall back to plain words (see `tool_label`).
    has_symbols: bool,
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
    /// Drag-crop state: the live drag plus the committed rect (inclusive
    /// canvas px) waiting for Apply. Re-drag replaces, Esc cancels.
    crop_anchor: Option<(f32, f32)>,
    crop_current: Option<(f32, f32)>,
    crop_rect: Option<(u32, u32, u32, u32)>,
    /// Canvas-size panel: wanted dimensions plus where the old canvas sits
    /// (-1 left/top, 0 center, 1 right/bottom per axis). `canvas_sync`
    /// tracks the doc dims so the fields re-adopt the real size after an
    /// open/crop/apply without clobbering typing mid-edit.
    canvas_w: u32,
    canvas_h: u32,
    canvas_sync: (u32, u32),
    canvas_anchor: (i32, i32),
    lasso_points: Vec<(f32, f32)>,
    lasso_poly: Vec<(f32, f32)>,
    /// Cached mask-edge points for marching ants, rebuilt when
    /// `sel_edges_dirty` (set at every selection-mutating site).
    sel_edges: Vec<(u32, u32)>,
    sel_edges_dirty: bool,
    /// Canvas pan offset in screen px (two-finger drag on touch; the
    /// desktop scrolls the view with the wheel instead).
    view_offset: egui::Vec2,
    /// Canvas rotation in radians (two-finger twist on touch, slider on
    /// desktop). View-only like zoom/pan: paint math stays axis-aligned.
    view_angle: f32,
    /// A stroke applied at least one dab (guards pinch-cancel revert).
    stroke_painted: bool,
    /// Multi-finger tap in progress (two = undo, three = redo on release).
    tap_track: Option<TapTrack>,
    /// Autosave recovery file (v8 project snapshot every 120 s) plus the
    /// last write time and whether to offer it at startup.
    recovery_path: std::path::PathBuf,
    last_autosave: f64,
    recovery_offer: bool,
    fill_tolerance: u8,
    fill_preview: Option<FillPreview>,
    smudge_strength: f32,
    filter_preview: Option<FilterPreview>,
    sel_feather: u32,
    sel_grow: i32,
    sel_opaque_t: u8,
    sel_stroke_w: u32,
    filt_brightness: i32,
    filt_contrast: f32,
    filt_blur: i32,
    filt_sharpen: f32,
    /// M36 Adjustments I: Levels black/white points + mid gamma (%).
    filt_levels_shadow: u8,
    filt_levels_gamma: f32,
    filt_levels_high: u8,
    /// M36 Curves (parametric): shadow/mid/highlight lift (+) or crush (-).
    filt_curve_shadow: i32,
    filt_curve_mid: i32,
    filt_curve_high: i32,
    /// M37 Adjustments II: HSL, color balance, recolor strength (the
    /// recolor target is the current paint color).
    filt_hue: f32,
    filt_sat: f32,
    filt_light: i32,
    filt_bal_r: i32,
    filt_bal_g: i32,
    filt_bal_b: i32,
    filt_recolor_strength: f32,
    renaming: Option<usize>,
    renaming_text: String,
    group_renaming: Option<u32>,
    group_name_edit: String,
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
    palettes: PaletteSet,
    pal_renaming: bool,
    pal_name_edit: String,
    guides: Guides,
    vp_move: bool,
    vp_grab: Option<usize>,
    sym_move: bool,
    sym_grab: bool,
    assist: bool,
    assist_start: Option<(f32, f32)>,
    assist_lock: Option<AssistLine>,
    // Animation Assist: layers are frames (bottom-to-top), the active
    // layer is the current frame. Ghost handles are half-res overlays,
    // rebuilt when the neighbor pair changes or `ghost_dirty` is set by
    // undo/redo and layer edits (painting only touches the current frame,
    // so strokes never invalidate ghosts).
    anim_assist: bool,
    anim_playing: bool,
    anim_fps: u32,
    anim_loop: LoopMode,
    /// JPEG export quality 1-100 (M28 slider; TIFF is lossless, no dial).
    jpeg_quality: u8,
    anim_fwd: bool,
    anim_last_t: f64,
    anim_shown: Option<usize>,
    onion: bool,
    /// Onion ghost strengths per side (red = previous, blue = next).
    onion_prev: f32,
    onion_next: f32,
    /// M41 Time-lapse: record flag, staged snapshots (bytes, w, h),
    /// and the last stage time (min-interval gate).
    lapse_on: bool,
    lapse_frames: Vec<(Vec<u8>, u32, u32)>,
    lapse_last: std::time::Instant,
    ghost_prev: Option<TextureHandle>,
    ghost_next: Option<TextureHandle>,
    ghost_key: (Option<usize>, Option<usize>, u32, u32),
    ghost_dirty: bool,
    move_from: Option<(f32, f32)>,
    move_acc: (f32, f32),
    /// M40 Liquify: drag anchor plus brush size/strength.
    liquify_from: Option<(f32, f32)>,
    liquify_size: f32,
    liquify_strength: f32,
    shape_anchor: Option<(f32, f32)>,
    shape_current: Option<(f32, f32)>,
    shape_kind: ShapeKind,
    shape_fill: bool,
    shape_sides: u8,
    text_anchor: Option<(f32, f32)>,
    text_open: bool,
    text_body: String,
    text_size: f32,
    /// M38 Typography: imported face (session-only bytes + name),
    /// spacing, and outline. Render bakes through `TextStyle`.
    text_font_name: Option<String>,
    text_font_bytes: Option<Vec<u8>>,
    text_leading: f32,
    text_tracking: f32,
    text_outline: bool,
    text_outline_w: u32,
    text_outline_rgb: [u8; 3],
    zoom: f32,
    status: String,
    /// M33 Gallery: recent project paths (newest first, max 10),
    /// the browser window flag, cached per-path thumbnails, the key
    /// they were built from, and the New-canvas size fields.
    recent: Vec<String>,
    gallery_open: bool,
    gallery_thumbs: Vec<Option<TextureHandle>>,
    gallery_keys: Vec<String>,
    gallery_new_w: u32,
    gallery_new_h: u32,
    /// M34 Reference companion: pinned image alongside the canvas
    /// (session-only, not saved in the project). Texture rebuilds lazily.
    ref_open: bool,
    ref_image: Option<(Vec<u8>, u32, u32)>,
    ref_tex: Option<TextureHandle>,
    ref_scale: f32,
    /// M35 Selection upgrades: saved snapshot ((w,h), rect, mask) plus
    /// the nudge steppers. Session-only; Load refuses size mismatches.
    saved_sel: Option<SavedSelection>,
    sel_nudge_x: i32,
    sel_nudge_y: i32,
}

impl ProCraftApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Extra glyph coverage (tool icons) from a system symbol font; the
        // app works fine without it (UI strings check `has_symbols`).
        let mut fonts = egui::FontDefinitions::default();
        let has_symbols = std::fs::read("C:\\Windows\\Fonts\\seguisym.ttf")
            .map(|bytes| {
                fonts.font_data.insert(
                    "segui-symbol".to_owned(),
                    std::sync::Arc::new(egui::FontData::from_owned(bytes)),
                );
                if let Some(list) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
                    list.push("segui-symbol".to_owned());
                }
            })
            .is_ok();
        cc.egui_ctx.set_fonts(fonts);
        // Autosave recovery lives next to the UI state (created eagerly
        // so a crash in the first two minutes still offers recovery only
        // if a snapshot actually landed).
        let mut dir = std::env::var_os("APPDATA")
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(std::path::PathBuf::from))
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        dir.push("procraft");
        let _ = std::fs::create_dir_all(&dir);
        dir.push("recovery.pcraft");
        let recovery_path = dir;
        let mut app = Self {
            doc: Document::new(1280, 800),
            brush: Brush::default(),
            color: [30, 144, 255],
            tool: Tool::Brush,
            theme: Theme::Paper,
            theme_applied: None,
            has_symbols,
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
            crop_anchor: None,
            crop_current: None,
            crop_rect: None,
            canvas_w: 1280,
            canvas_h: 800,
            canvas_sync: (1280, 800),
            canvas_anchor: (0, 0),
            lasso_points: Vec::new(),
            lasso_poly: Vec::new(),
            sel_edges: Vec::new(),
            sel_edges_dirty: true,
            view_offset: egui::Vec2::ZERO,
            view_angle: 0.0,
            stroke_painted: false,
            tap_track: None,
            recovery_path: recovery_path.clone(),
            last_autosave: 0.0,
            recovery_offer: std::path::Path::new(&recovery_path).exists(),
            fill_tolerance: 32,
            fill_preview: None,
            smudge_strength: 0.5,
            filter_preview: None,
            sel_feather: 4,
            sel_grow: 4,
            sel_opaque_t: 8,
            sel_stroke_w: 2,
            filt_brightness: 0,
            filt_contrast: 100.0,
            filt_blur: 0,
            filt_sharpen: 0.0,
            filt_levels_shadow: 0,
            filt_levels_gamma: 100.0,
            filt_levels_high: 255,
            filt_curve_shadow: 0,
            filt_curve_mid: 0,
            filt_curve_high: 0,
            filt_hue: 0.0,
            filt_sat: 100.0,
            filt_light: 0,
            filt_bal_r: 0,
            filt_bal_g: 0,
            filt_bal_b: 0,
            filt_recolor_strength: 100.0,
            renaming: None,
            renaming_text: String::new(),
            group_renaming: None,
            group_name_edit: String::new(),
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
            palettes: PaletteSet::new(),
            pal_renaming: false,
            pal_name_edit: String::new(),
            guides: Guides::default(),
            vp_move: false,
            vp_grab: None,
            sym_move: false,
            sym_grab: false,
            assist: false,
            assist_start: None,
            assist_lock: None,
            anim_assist: false,
            anim_playing: false,
            anim_fps: 12,
            anim_loop: LoopMode::Forward,
            jpeg_quality: 90,
            anim_fwd: true,
            anim_last_t: 0.0,
            anim_shown: None,
            onion: true,
            onion_prev: 0.3,
            onion_next: 0.3,
            lapse_on: false,
            lapse_frames: Vec::new(),
            lapse_last: std::time::Instant::now(),
            ghost_prev: None,
            ghost_next: None,
            ghost_key: (None, None, 0, 0),
            ghost_dirty: true,
            move_from: None,
            move_acc: (0.0, 0.0),
            liquify_from: None,
            liquify_size: 128.0,
            liquify_strength: 0.5,
            shape_anchor: None,
            shape_current: None,
            shape_kind: ShapeKind::Rect,
            shape_fill: false,
            shape_sides: 6,
            text_anchor: None,
            text_open: false,
            text_body: String::new(),
            text_size: 48.0,
            text_font_name: None,
            text_font_bytes: None,
            text_leading: 0.0,
            text_tracking: 0.0,
            text_outline: false,
            text_outline_w: 3,
            text_outline_rgb: [0, 0, 0],
            zoom: 1.0,
            status:
                "Drag on the canvas to paint. Ctrl+Z undoes, Ctrl+Y redoes, Esc clears selection."
                    .to_string(),
            recent: Vec::new(),
            gallery_open: false,
            gallery_thumbs: Vec::new(),
            gallery_keys: Vec::new(),
            gallery_new_w: 1280,
            gallery_new_h: 800,
            ref_open: false,
            ref_image: None,
            ref_tex: None,
            ref_scale: 1.0,
            saved_sel: None,
            sel_nudge_x: 0,
            sel_nudge_y: 0,
        };
        // Restore disk-persisted UI state (theme, panels, zoom); defaults
        // stand when nothing was saved yet.
        if let Some(storage) = cc.storage {
            if let Some(name) = storage.get_string("theme") {
                if let Some(theme) = Theme::from_str(&name) {
                    app.theme = theme;
                }
            }
            if let Some(v) = storage.get_string("show_tools") {
                app.show_tools = v == "1";
            }
            if let Some(v) = storage.get_string("show_layers") {
                app.show_layers = v == "1";
            }
            if let Some(v) = storage.get_string("zoom") {
                if let Ok(z) = v.parse::<f32>() {
                    if z.is_finite() && z > 0.0 {
                        app.zoom = z;
                    }
                }
            }
            // M33 Gallery recents: newline-joined paths (iOS has no
            // disk persistence, so the list is session-only there).
            if let Some(v) = storage.get_string("recent") {
                app.recent = v
                    .lines()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .take(MAX_RECENT)
                    .map(str::to_string)
                    .collect();
            }
        }
        app
    }

    fn base_brush(&self) -> Brush {
        Brush {
            eraser: self.tool == Tool::Eraser,
            ..self.brush.clone()
        }
    }

    fn current_color(&self) -> Color {
        Color::rgb(self.color[0], self.color[1], self.color[2])
    }

    /// Rebuild the cached mask-edge points for marching ants. Rect-only
    /// selections contribute their perimeter; masks contribute pixels on
    /// the nonzero boundary (feather fringe included, so the crawl shows
    /// the true extent). Runs once per selection change, not per frame.
    fn rebuild_sel_edges(&mut self) {
        self.sel_edges.clear();
        let (w, h) = (self.doc.w, self.doc.h);
        if w == 0 || h == 0 {
            return;
        }
        if let Some(mask) = self.doc.selection_mask.as_deref() {
            if mask.len() != (w * h) as usize {
                return;
            }
            let at = |x: u32, y: u32| mask[(y * w + x) as usize];
            for y in 0..h {
                for x in 0..w {
                    if at(x, y) == 0 {
                        continue;
                    }
                    let edge = x == 0
                        || y == 0
                        || x + 1 == w
                        || y + 1 == h
                        || at(x - 1, y) == 0
                        || at(x + 1, y) == 0
                        || at(x, y - 1) == 0
                        || at(x, y + 1) == 0;
                    if edge {
                        self.sel_edges.push((x, y));
                    }
                }
            }
        } else if let Some((x0, y0, x1, y1)) = self.doc.selection {
            let (x1, y1) = (x1.min(w - 1), y1.min(h - 1));
            for x in x0..=x1 {
                self.sel_edges.push((x, y0));
                if y1 != y0 {
                    self.sel_edges.push((x, y1));
                }
            }
            for y in y0 + 1..y1 {
                self.sel_edges.push((x0, y));
                if x1 != x0 {
                    self.sel_edges.push((x1, y));
                }
            }
        }
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
    /// per-side strengths, or layer content (`ghost_dirty`) changed.
    /// No-op otherwise.
    fn refresh_ghosts(&mut self, ctx: &egui::Context) {
        let active = clamp_frame(&self.doc, self.doc.active).unwrap_or(0);
        let bits = |v: f32| (v.clamp(0.05, 0.8) * 1000.0).round() as u32;
        let pair = if self.anim_assist && self.onion && !self.doc.layers.is_empty() {
            (
                prev_frame(&self.doc, active),
                next_frame(&self.doc, active),
                bits(self.onion_prev),
                bits(self.onion_next),
            )
        } else {
            (None, None, bits(self.onion_prev), bits(self.onion_next))
        };
        if !self.ghost_dirty && pair == self.ghost_key {
            return;
        }
        self.ghost_dirty = false;
        self.ghost_key = pair;
        let (w, h) = (self.doc.w as usize, self.doc.h as usize);
        let load = |ctx: &egui::Context,
                    name: &str,
                    idx: Option<usize>,
                    amount: f32|
         -> Option<TextureHandle> {
            let full = onion_ghost(&self.doc, idx?, amount);
            Some(ctx.load_texture(name, downsample_half(&full, w, h), TextureOptions::LINEAR))
        };
        self.ghost_prev = load(ctx, "ghost-prev", pair.0, self.onion_prev);
        self.ghost_next = load(ctx, "ghost-next", pair.1, self.onion_next);
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
                    let s = path.to_string_lossy().to_string();
                    self.adopt_project(doc, &s, "Project opened");
                }
                Err(e) => self.status = format!("Project open failed: {e}"),
            },
            PendingOpen::Psd => match open_psd(&path) {
                Ok(doc) => {
                    self.doc = doc;
                    self.clear_recovery();
                    self.texture = None;
                    self.lasso_points.clear();
                    self.lasso_poly.clear();
                    self.status = format!("PSD opened {} (masks do not import)", path.display());
                }
                Err(e) => self.status = format!("PSD open failed: {e}"),
            },
            PendingOpen::Brushes => match load_brush_file(&path) {
                Ok(libs) => self.status = self.absorb_brushes(libs, &path),
                Err(e) => self.status = format!("Brush import failed: {e}"),
            },
            PendingOpen::Palette => match load_palette_file(&path) {
                Ok(pal) => self.status = self.absorb_colors(pal, &format!("{}", path.display())),
                Err(e) => self.status = format!("Palette import failed: {e}"),
            },
            PendingOpen::ImagePalette => {
                let stem = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("Image")
                    .to_string();
                let from = format!("{}", path.display());
                match image::open(&path).map(|i| i.to_rgba8()) {
                    Ok(img) => {
                        let (w, h) = (img.width(), img.height());
                        self.status = self.image_palette_status(&stem, img.as_raw(), w, h, &from);
                    }
                    Err(e) => self.status = format!("Image open failed: {e}"),
                }
            }
            PendingOpen::Gif => match std::fs::read(&path)
                .map_err(|e| e.to_string())
                .and_then(|b| import_gif_document(&b))
            {
                Ok((doc, fps)) => {
                    self.status = format!(
                        "GIF imported {} ({} frames, {} fps).",
                        path.display(),
                        doc.layers.len(),
                        fps
                    );
                    self.adopt_gif(doc, fps);
                }
                Err(e) => self.status = format!("GIF import failed: {e}"),
            },
            // M34 Reference: pin the image beside the canvas (not a layer).
            PendingOpen::Reference => match image::open(&path).map(|i| i.to_rgba8()) {
                Ok(img) => {
                    let (w, h) = (img.width(), img.height());
                    self.set_reference(img.into_raw(), w, h, &format!("{}", path.display()));
                }
                Err(e) => self.status = format!("Reference load failed: {e}"),
            },
            // M38 Typography: validate an imported face, keep its bytes.
            PendingOpen::Font => match std::fs::read(&path) {
                Ok(bytes) => {
                    let name = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("font")
                        .to_string();
                    self.status = self.absorb_font(&bytes, &name);
                }
                Err(e) => self.status = format!("Font import failed: {e}"),
            },
            // M39 Brush depth: take the first stamp tip in the file.
            PendingOpen::DualTip => {
                self.status = self.absorb_dual_tip(&path);
            }
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
            let brush = Brush {
                eraser: false,
                ..preset
            };
            let mut last = None;
            let mut left = 0.0;
            let mut x = 8.0;
            while x <= 136.0 {
                let y = 18.0 + 8.0 * ((x - 8.0) / 128.0 * std::f32::consts::PI * 2.0).sin();
                let (l, lo) = doc.stroke_to(&brush, ink, last, (x, y), left);
                last = l;
                left = lo;
                x += 2.0;
            }
            let img = ColorImage::from_rgba_unmultiplied([144, 36], &doc.layers[0].pixels.clone());
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
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext == "gpl" {
        import_gpl_file(path)
    } else {
        open_palette(path)
    }
}
/// Dispatch a brush file by extension into presets (shared by the desktop
/// Import arm and the iOS picker resolve path).
fn load_brush_file(path: &std::path::Path) -> Result<Vec<BrushPreset>, String> {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext == "pcbrush" || ext == "json" {
        open_brush_library(path).map(|libs| {
            libs.into_iter()
                .map(|p| BrushPreset {
                    name: p.name,
                    brush: p.brush,
                })
                .collect()
        })
    } else {
        import_brush_file(path).map(|libs| {
            libs.into_iter()
                .map(|b| BrushPreset {
                    name: b.name,
                    brush: b.brush,
                })
                .collect()
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
            self.custom_brushes.push(BrushPreset {
                name,
                brush: p.brush,
            });
            added += 1;
        }
        format!("Imported {added} brushes from {}.", path.display())
    }

    /// Append an imported palette's swatches to the active palette,
    /// skipping exact duplicates (shared by desktop Import, image/canvas
    /// extraction and the iOS picker resolve path).
    fn absorb_colors(&mut self, pal: Palette, from: &str) -> String {
        let mut added = 0;
        if let Some(active) = self.palettes.active_mut() {
            for c in pal.colors {
                if active.colors.len() >= MAX_PALETTE_COLORS {
                    break;
                }
                if !active.colors.contains(&c) {
                    active.colors.push(c);
                    added += 1;
                }
            }
        }
        format!("Imported {added} swatches from {from}.")
    }

    /// Extract a palette from RGBA pixels and absorb it (shared by the
    /// image-file and canvas arms on both platforms).
    fn image_palette_status(
        &mut self,
        name: &str,
        pixels: &[u8],
        w: u32,
        h: u32,
        from: &str,
    ) -> String {
        match palette_from_rgba(name, pixels, w, h, MAX_IMAGE_COLORS) {
            Ok(pal) => self.absorb_colors(pal, from),
            Err(e) => format!("Palette from image failed: {e}"),
        }
    }

    /// Scrub a filter slider with a live preview: checkpoint + snapshot on
    /// first touch, then restore-and-reapply on change, so the whole
    /// gesture is one undo step and values never compound. `kind` selects
    /// the baseline slot (see [`FilterPreview`]); a changed layer (or
    /// mismatched pixels after undo/layer edits) restarts it. Re-applies
    /// closer than [`PREVIEW_MIN_DT`] are skipped and flag the preview
    /// `stale` — Apply restamps the latest values (see `restamp_preview`).
    /// A preview whose layer changed underneath (paint stroke, undo, layer
    /// op, new/opened doc…) never restores its stale snapshot — that wipe
    /// is exactly "my paint disappeared" — it restarts on current pixels.
    fn filter_scrub(&mut self, kind: u8, apply: impl Fn(&mut Document, usize)) {
        let layer = self.doc.active;
        // Stale-snapshot guard: the live layer must still match what our
        // last apply left behind (or the snapshot on a fresh preview).
        let expired = match &self.filter_preview {
            Some(fp) => match self.doc.layers.get(fp.layer) {
                Some(l) => pixels_hash(&l.pixels) != fp.live_hash,
                None => true,
            },
            None => false,
        };
        if expired {
            self.filter_preview = None;
        }
        let fresh = match &self.filter_preview {
            Some(fp) => {
                fp.kind != kind
                    || fp.layer != layer
                    || self
                        .doc
                        .layers
                        .get(layer)
                        .is_none_or(|l| l.pixels.len() != fp.pixels.len())
            }
            None => true,
        };
        if fresh {
            self.doc.checkpoint();
            match self.doc.layers.get(layer) {
                Some(l) => {
                    let live_hash = pixels_hash(&l.pixels);
                    self.filter_preview = Some(FilterPreview {
                        kind,
                        layer,
                        pixels: l.pixels.clone(),
                        last: std::time::Instant::now() - PREVIEW_MIN_DT,
                        stale: false,
                        live_hash,
                    });
                }
                None => {
                    self.filter_preview = None;
                    return;
                }
            }
        }
        if let Some(mut fp) = self.filter_preview.take() {
            if self.doc.layers.get(fp.layer).is_none() {
                return;
            }
            // Throttle: the layer keeps the last preview; Apply catches up.
            if !fresh && fp.last.elapsed() < PREVIEW_MIN_DT {
                fp.stale = true;
                self.filter_preview = Some(fp);
                return;
            }
            if let Some(l) = self.doc.layers.get_mut(fp.layer) {
                l.pixels = fp.pixels.clone();
            }
            apply(&mut self.doc, fp.layer);
            if let Some(l) = self.doc.layers.get(fp.layer) {
                fp.live_hash = pixels_hash(&l.pixels);
            }
            fp.last = std::time::Instant::now();
            fp.stale = false;
            self.filter_preview = Some(fp);
        }
    }

    /// Re-apply current slider values over the scrub snapshot (for a
    /// throttled-stale preview at Apply time, or to refresh a current
    /// one). The scrub's checkpoint still covers undo — no new step.
    /// If the layer moved on underneath (see `filter_scrub`), the stale
    /// preview is dropped and the values apply fresh on the active layer
    /// (new undo step) — the intervening work is always kept.
    fn restamp_preview(&mut self, apply: impl Fn(&mut Document, usize)) {
        let expired = match &self.filter_preview {
            Some(fp) => match self.doc.layers.get(fp.layer) {
                Some(l) => pixels_hash(&l.pixels) != fp.live_hash,
                None => true,
            },
            None => return,
        };
        if expired {
            self.filter_preview = None;
            self.doc.checkpoint();
            let layer = self.doc.active;
            if self.doc.layers.get(layer).is_some() {
                apply(&mut self.doc, layer);
            }
            return;
        }
        if let Some(mut fp) = self.filter_preview.take() {
            if self.doc.layers.get(fp.layer).is_some() {
                if let Some(l) = self.doc.layers.get_mut(fp.layer) {
                    l.pixels = fp.pixels.clone();
                }
                apply(&mut self.doc, fp.layer);
                if let Some(l) = self.doc.layers.get(fp.layer) {
                    fp.live_hash = pixels_hash(&l.pixels);
                }
            }
            self.filter_preview = Some(fp);
        }
    }

    /// M41 Time-lapse: stage a downscaled composite snapshot while
    /// recording. The min-interval gate plus the frame cap bound a
    /// session to small thumbs (ring-evicted past the cap); callers
    /// invoke this at every commit point and it stays cheap otherwise.
    /// Session-only — staged frames never enter the project file.
    fn capture_timelapse(&mut self) {
        if !self.lapse_on || self.lapse_last.elapsed() < LAPSE_MIN_DT {
            return;
        }
        self.lapse_last = std::time::Instant::now();
        let (bytes, tw, th) =
            gallery_thumb(&self.doc.flatten(), self.doc.w, self.doc.h, LAPSE_SIDE);
        if bytes.is_empty() {
            return;
        }
        if self.lapse_frames.len() >= MAX_LAPSE_FRAMES {
            self.lapse_frames.remove(0);
        }
        self.lapse_frames.push((bytes, tw, th));
    }

    /// Adopt an imported GIF as the document (one layer per frame, active
    /// on frame 1): enables Assist and resets the view, ghosts and
    /// playback so the first frame shows at once.
    fn adopt_gif(&mut self, doc: Document, fps: u32) {
        self.doc = doc;
        self.clear_recovery();
        self.anim_fps = fps;
        self.anim_assist = true;
        self.anim_playing = false;
        self.anim_fwd = true;
        self.anim_shown = None;
        self.ghost_dirty = true;
        self.texture = None;
        self.lasso_points.clear();
        self.lasso_poly.clear();
    }

    /// Best-effort delete of the autosave recovery file (after an explicit
    /// save, load, or clean exit — anything that makes it stale).
    fn clear_recovery(&self) {
        let _ = std::fs::remove_file(&self.recovery_path);
    }

    /// Draw the eyedropper loupe for canvas point `(cx, cy)` (already
    /// clamped): magnified composite ring parked above the pointer (below
    /// near the top edge, shifted inside near the sides) with the
    /// would-be picked color read out.
    fn paint_picker_loupe(
        &self,
        painter: &egui::Painter,
        ctx: &egui::Context,
        pos: egui::Pos2,
        cx: f32,
        cy: f32,
    ) {
        const SIDE: u32 = 11;
        const CELL: f32 = 12.0;
        let size = SIDE as f32 * CELL;
        let (cells, ci, cj) = loupe_cells(&self.doc, cx as u32, cy as u32, SIDE);
        let center = cells[(cj * SIDE + ci) as usize];
        let screen = ctx.viewport_rect();
        let mut at = egui::Pos2::new(pos.x + 24.0, pos.y - size - 48.0);
        if at.y < screen.min.y + 8.0 {
            at.y = pos.y + 28.0;
        }
        if at.x + size + 12.0 > screen.max.x - 8.0 {
            at.x = screen.max.x - 8.0 - size - 12.0;
        }
        if at.x < screen.min.x + 8.0 {
            at.x = screen.min.x + 8.0;
        }
        paint_loupe(
            painter,
            &cells,
            SIDE,
            (ci, cj),
            at,
            CELL,
            [center[0], center[1], center[2]],
        );
    }

    /// Reset view-side state after the canvas changes size (crop, canvas
    /// resize): the texture rebuilds at the new dims, ghosts and mask
    /// edges recompute, transient drags die, and the size panel re-adopts
    /// the real dimensions.
    fn after_canvas_change(&mut self) {
        self.texture = None;
        self.ghost_dirty = true;
        self.sel_edges_dirty = true;
        self.lasso_poly.clear();
        self.crop_rect = None;
        self.crop_anchor = None;
        self.crop_current = None;
        self.move_from = None;
        self.liquify_from = None;
        self.canvas_sync = (self.doc.w, self.doc.h);
        self.canvas_w = self.doc.w;
        self.canvas_h = self.doc.h;
    }

    /// M33 Gallery: adopt an opened/saved/duplicated project — one place
    /// for the reset + recent + status so every entry path behaves alike.
    fn adopt_project(&mut self, doc: Document, path: &str, verb: &str) {
        self.doc = doc;
        self.clear_recovery();
        push_recent(&mut self.recent, path);
        self.after_canvas_change();
        self.gallery_keys.clear();
        self.status = format!("{verb} {path}");
    }

    /// M33 Gallery: start a blank canvas at `w` x `h` (clamped to the
    /// engine cap). No recent entry until it is saved somewhere.
    fn new_canvas(&mut self, w: u32, h: u32) {
        let w = w.clamp(1, MAX_CANVAS_SIDE);
        let h = h.clamp(1, MAX_CANVAS_SIDE);
        self.doc = Document::new(w, h);
        self.clear_recovery();
        self.after_canvas_change();
        self.status = format!("New canvas {w} x {h}.");
    }

    /// M33 Gallery: rebuild cached thumbnails when the recent list
    /// changed. Each path is opened, flattened, and downscaled; missing
    /// or corrupt files yield `None` (placeholder row, kept so the user
    /// can Remove it). Runs on demand, not per frame.
    fn refresh_gallery_thumbs(&mut self, ctx: &egui::Context) {
        if self.gallery_keys == self.recent {
            return;
        }
        self.gallery_thumbs.clear();
        for (idx, path) in self.recent.iter().enumerate() {
            let tex = Document::open_project(std::path::Path::new(path))
                .ok()
                .and_then(|doc| {
                    let (bytes, tw, th) =
                        gallery_thumb(&doc.flatten(), doc.w, doc.h, GALLERY_THUMB_SIDE);
                    if bytes.is_empty() {
                        return None;
                    }
                    let img =
                        ColorImage::from_rgba_unmultiplied([tw as usize, th as usize], &bytes);
                    Some(ctx.load_texture(format!("gallery-{idx}"), img, TextureOptions::LINEAR))
                });
            self.gallery_thumbs.push(tex);
        }
        self.gallery_keys = self.recent.clone();
    }

    /// M33 Gallery: copy `recent[idx]` to a sibling ` copyN.procraft`
    /// file and adopt it. Errors surface in the status line.
    fn duplicate_recent(&mut self, idx: usize) {
        let Some(src) = self.recent.get(idx).cloned() else {
            return;
        };
        let src_path = std::path::PathBuf::from(&src);
        let doc = match Document::open_project(&src_path) {
            Ok(d) => d,
            Err(e) => {
                self.status = format!("Duplicate failed to open {src}: {e}");
                return;
            }
        };
        let stem = src_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("artwork");
        // Strip an older " copyN" tail so re-duplicates don't stack names.
        let base = stem.rsplit_once(" copy").map(|(b, _)| b).unwrap_or(stem);
        let mut dest = src_path.clone();
        let mut n = 1u32;
        loop {
            let name = if n == 1 {
                format!("{base} copy.procraft")
            } else {
                format!("{base} copy{n}.procraft")
            };
            dest.set_file_name(&name);
            if !dest.exists() {
                break;
            }
            n += 1;
            if n > 999 {
                self.status = "Duplicate failed: too many copies.".to_string();
                return;
            }
        }
        match doc.save_project(&dest) {
            Ok(()) => {
                let s = dest.to_string_lossy().to_string();
                self.adopt_project(doc, &s, "Duplicated");
            }
            Err(e) => self.status = format!("Duplicate save failed: {e}"),
        }
    }

    /// M34 Reference: pin RGBA bytes as the companion image (session-only;
    /// never touches the canvas or undo). Oversized images are downscaled
    /// through the Gallery helper so a phone photo can't OOM the session.
    fn set_reference(&mut self, pixels: Vec<u8>, w: u32, h: u32, from: &str) {
        if pixels.is_empty() || w == 0 || h == 0 {
            self.status = format!("Reference load failed: empty image ({from}).");
            return;
        }
        const MAX_SIDE: u32 = 1024;
        let (bytes, tw, th) = if w.max(h) > MAX_SIDE {
            gallery_thumb(&pixels, w, h, MAX_SIDE)
        } else {
            (pixels, w, h)
        };
        if bytes.is_empty() {
            self.status = format!("Reference load failed: bad pixels ({from}).");
            return;
        }
        self.ref_image = Some((bytes, tw, th));
        self.ref_tex = None;
        self.ref_open = true;
        self.status = format!("Reference pinned ({from}).");
    }

    /// M34 Reference: upload the companion texture on demand (load/replace
    /// only, never per frame).
    fn refresh_ref_tex(&mut self, ctx: &egui::Context) {
        if self.ref_tex.is_none() {
            if let Some((bytes, w, h)) = &self.ref_image {
                let img = ColorImage::from_rgba_unmultiplied([*w as usize, *h as usize], bytes);
                self.ref_tex = Some(ctx.load_texture("reference", img, TextureOptions::LINEAR));
            }
        }
    }

    /// M38 Typography: validate imported TTF/OTF bytes and keep them as
    /// the session face (shared by desktop Import and the iOS resolve).
    fn absorb_font(&mut self, bytes: &[u8], name: &str) -> String {
        match check_font(bytes) {
            Ok(()) => {
                self.text_font_name = Some(name.to_string());
                self.text_font_bytes = Some(bytes.to_vec());
                format!("Font {name} ready.")
            }
            Err(e) => format!("Font {name} failed: {e}"),
        }
    }

    /// M39 Brush depth: take the first stamp tip in a brush file as the
    /// current brush's dual tip (shared by desktop Import and iOS resolve).
    fn absorb_dual_tip(&mut self, path: &std::path::Path) -> String {
        match load_brush_file(path) {
            Ok(libs) => match libs.into_iter().find_map(|p| p.brush.tip) {
                Some(tip) => {
                    let (w, h) = (tip.w, tip.h);
                    self.brush.dual_tip = Some(tip);
                    format!("Dual tip set ({w}x{h}).")
                }
                None => "No stamp tip in that file.".to_string(),
            },
            Err(e) => format!("Dual import failed: {e}"),
        }
    }

    /// M38 Typography: the current dialog settings as an engine style.
    fn text_style(&self) -> TextStyle {
        TextStyle {
            font_bytes: self.text_font_bytes.clone(),
            leading: self.text_leading,
            tracking: self.text_tracking,
            outline: self.text_outline.then(|| {
                (
                    Color::rgb(
                        self.text_outline_rgb[0],
                        self.text_outline_rgb[1],
                        self.text_outline_rgb[2],
                    ),
                    self.text_outline_w,
                )
            }),
        }
    }
}

/// Screen position of a vanishing-point handle: the true point when it is
/// on-canvas, otherwise clamped to the canvas edge (with a margin) so
/// off-canvas VPs — like the 2-point defaults — stay visible and grabbable.
/// Returns the position plus whether it was clamped.
fn vp_handle_pos(rect: egui::Rect, z: f32, vx: f32, vy: f32) -> (egui::Pos2, bool) {
    let p = egui::Pos2::new(rect.min.x + vx * z, rect.min.y + vy * z);
    if rect.contains(p) {
        return (p, false);
    }
    // Clamp into the rect with a finger-sized margin; degenerate tiny
    // rects fall back to the center instead of panicking in `clamp`.
    let m = 14.0;
    let (lo_x, hi_x) = (rect.min.x + m, rect.max.x - m);
    let (lo_y, hi_y) = (rect.min.y + m, rect.max.y - m);
    let cx = if lo_x <= hi_x {
        p.x.clamp(lo_x, hi_x)
    } else {
        (rect.min.x + rect.max.x) * 0.5
    };
    let cy = if lo_y <= hi_y {
        p.y.clamp(lo_y, hi_y)
    } else {
        (rect.min.y + rect.max.y) * 0.5
    };
    (egui::Pos2::new(cx, cy), true)
}

/// Slider with −/+ step buttons flush around it: one compact control
/// (`[-]bar[+] value name`) instead of widgets spread across the row.
/// `stepped_slider` puts the name first; `stepped_slider_label_end` (the
/// Filters convention) leaves value + name trailing. `step` is in value
/// units, endpoints clamp. Rows wrap on narrow screens; the step buttons
/// are 28x20.
const SLIDER_W: f32 = 100.0;

fn stepped_slider<Num: egui::emath::Numeric>(
    ui: &mut egui::Ui,
    value: &mut Num,
    range: std::ops::RangeInclusive<Num>,
    step: f64,
    text: &str,
) {
    stepped_slider_inner(ui, value, range, step, text, false);
}

fn stepped_slider_label_end<Num: egui::emath::Numeric>(
    ui: &mut egui::Ui,
    value: &mut Num,
    range: std::ops::RangeInclusive<Num>,
    step: f64,
    text: &str,
) {
    stepped_slider_inner(ui, value, range, step, text, true);
}

fn stepped_slider_inner<Num: egui::emath::Numeric>(
    ui: &mut egui::Ui,
    value: &mut Num,
    range: std::ops::RangeInclusive<Num>,
    step: f64,
    text: &str,
    label_end: bool,
) {
    let lo = range.start().to_f64();
    let hi = range.end().to_f64();
    // Trailing readout: value first, then the name; a % in the name
    // fuses onto the value ("100%") instead of floating on its own.
    let v = value.to_f64();
    let num = if v.fract() == 0.0 {
        format!("{v:.0}")
    } else {
        format!("{v:.2}")
    };
    ui.horizontal_wrapped(|ui| {
        if !label_end {
            ui.label(text);
        }
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
            if ui.add_sized([28.0, 20.0], egui::Button::new("−")).clicked() {
                *value = Num::from_f64((value.to_f64() - step).clamp(lo, hi));
            }
            ui.add_sized(
                egui::Vec2::new(SLIDER_W, 20.0),
                egui::Slider::new(value, range).show_value(false),
            );
            if ui.add_sized([28.0, 20.0], egui::Button::new("+")).clicked() {
                *value = Num::from_f64((value.to_f64() + step).clamp(lo, hi));
            }
        });
        if text.contains('%') {
            ui.label(format!("{num}%"));
            let name = text.replace('%', "");
            let name = name.trim();
            if !name.is_empty() {
                ui.label(name);
            }
        } else {
            ui.label(num);
            if label_end {
                ui.label(text);
            }
        }
    });
}

/// Swatch grid at full height (never an inner scroller — the panel's own
/// scroll handles overflow, and separate scrollers with no explicit ids
/// share one scroll offset). Wrap width is pinned to 8 swatches per row
/// so Starter always lands 3x8 at any panel width.
fn swatch_grid(ui: &mut egui::Ui, mut add_contents: impl FnMut(&mut egui::Ui)) {
    ui.scope(|ui| {
        ui.set_max_width(8.0 * 27.0 + 8.0);
        add_contents(ui);
    });
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
                px[o + c] = acc[c].checked_div(n).unwrap_or(0) as u8;
            }
        }
    }
    ColorImage::from_rgba_unmultiplied([hw, hh], &px)
}

/// Rotate a screen point about a center (view rotation).
fn rot_pt(p: egui::Pos2, c: egui::Pos2, ang: f32) -> egui::Pos2 {
    let (s, co) = ang.sin_cos();
    let (dx, dy) = (p.x - c.x, p.y - c.y);
    egui::Pos2::new(c.x + dx * co - dy * s, c.y + dx * s + dy * co)
}

/// Inverse of [`rot_pt`] (screen back to the unrotated view).
fn unrot_pt(p: egui::Pos2, c: egui::Pos2, ang: f32) -> egui::Pos2 {
    rot_pt(p, c, -ang)
}

/// Textured quad for a rotated canvas view: `corners` are TL, TR, BR, BL
/// with matching unit UVs, tinted (egui has no canvas transform, so the
/// texture stays axis-aligned and the quad does the rotating).
fn paint_textured_quad(
    painter: &egui::Painter,
    tex: egui::TextureId,
    corners: [egui::Pos2; 4],
    tint: Color32,
) {
    let uvs = [
        egui::Pos2::new(0.0, 0.0),
        egui::Pos2::new(1.0, 0.0),
        egui::Pos2::new(1.0, 1.0),
        egui::Pos2::new(0.0, 1.0),
    ];
    let mut mesh = egui::Mesh::with_texture(tex);
    for (pos, uv) in corners.iter().zip(uvs) {
        mesh.vertices.push(egui::epaint::Vertex {
            pos: *pos,
            uv,
            color: tint,
        });
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(mesh);
}

/// Marching-ants quad (a rectangle in an unrotated view): dark base line
/// plus moving light dashes along the four corners in order.
fn paint_selection(painter: &egui::Painter, corners: [egui::Pos2; 4], time: f64) {
    let dark = Color32::BLACK;
    let light = Color32::WHITE;
    for w in corners.windows(2) {
        painter.line_segment([w[0], w[1]], (1.5, dark));
    }
    painter.line_segment([corners[3], corners[0]], (1.5, dark));
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
    for w in corners.windows(2) {
        draw_dashed(w[0], w[1]);
    }
    draw_dashed(corners[3], corners[0]);
}

/// Eyedropper loupe cells: an odd-sided square of composited colors
/// around `(cx, cy)`, missing edge cells filled with the page background.
/// Returns the cells plus the cursor's cell position inside the grid.
fn loupe_cells(doc: &Document, cx: u32, cy: u32, side: u32) -> (Vec<[u8; 4]>, u32, u32) {
    let half = side / 2;
    let (w, h) = (doc.w, doc.h);
    let (rx0, ry0) = (cx.saturating_sub(half), cy.saturating_sub(half));
    let (rx1, ry1) = (
        (cx + half).min(w.saturating_sub(1)),
        (cy + half).min(h.saturating_sub(1)),
    );
    let region = doc.flatten_region(rx0, ry0, rx1, ry1);
    let bg = doc.background;
    let mut cells = Vec::with_capacity((side * side) as usize);
    for j in 0..side {
        for i in 0..side {
            let (x, y) = (rx0 + i, ry0 + j);
            if x <= rx1 && y <= ry1 {
                let o = (((y - ry0) * (rx1 - rx0 + 1) + (x - rx0)) * 4) as usize;
                cells.push([region[o], region[o + 1], region[o + 2], region[o + 3]]);
            } else {
                cells.push(bg);
            }
        }
    }
    (cells, cx - rx0, cy - ry0)
}

/// Magnifier ring for the eyedropper: `cells` render as squares at `cell`
/// px with the cursor cell outlined, plus a chip + hex readout. Screen
/// space (never rotated with the canvas); the caller parks it clear of
/// the pointer so fingers never cover the target.
fn paint_loupe(
    painter: &egui::Painter,
    cells: &[[u8; 4]],
    side: u32,
    center: (u32, u32),
    at: egui::Pos2,
    cell: f32,
    picked: [u8; 3],
) {
    let size = side as f32 * cell;
    let pad = 6.0;
    let row = 22.0;
    painter.rect_filled(
        egui::Rect::from_min_size(
            at - egui::Vec2::splat(pad),
            egui::Vec2::new(size + pad * 2.0, size + pad * 2.0 + row),
        ),
        8.0,
        Color32::from_black_alpha(220),
    );
    for (k, c) in cells.iter().enumerate() {
        let (i, j) = ((k as u32 % side) as f32, (k as u32 / side) as f32);
        painter.rect_filled(
            egui::Rect::from_min_size(
                at + egui::Vec2::new(i * cell, j * cell),
                egui::Vec2::splat(cell),
            ),
            0.0,
            Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]),
        );
    }
    painter.rect_stroke(
        egui::Rect::from_min_size(
            at + egui::Vec2::new(center.0 as f32 * cell, center.1 as f32 * cell),
            egui::Vec2::splat(cell),
        ),
        0.0,
        (2.0, Color32::WHITE),
        egui::StrokeKind::Middle,
    );
    let chip = egui::Rect::from_min_size(
        at + egui::Vec2::new(0.0, size + pad),
        egui::Vec2::new(row - 6.0, row - 6.0),
    );
    painter.rect_filled(
        chip,
        3.0,
        Color32::from_rgb(picked[0], picked[1], picked[2]),
    );
    painter.text(
        at + egui::Vec2::new(row, size + pad + (row - 6.0) * 0.5),
        egui::Align2::LEFT_CENTER,
        format!("#{:02X}{:02X}{:02X}", picked[0], picked[1], picked[2]),
        egui::FontId::monospace(13.0),
        Color32::WHITE,
    );
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

/// Marching ants along mask edges: dark dots everywhere, light dots on a
/// moving subset (classic two-tone crawl). `to_screen` maps canvas pixels.
fn paint_mask_edges(
    painter: &egui::Painter,
    edges: &[(u32, u32)],
    to_screen: impl Fn(u32, u32) -> egui::Pos2,
    time: f64,
) {
    let tick = (time * 28.0 / 3.0) as usize;
    for &(x, y) in edges.iter() {
        let p = to_screen(x, y);
        let r = egui::Rect::from_center_size(p, egui::Vec2::splat(2.5));
        painter.rect_filled(r, 0.0, Color32::BLACK);
    }
    for (i, &(x, y)) in edges.iter().enumerate() {
        if (i + tick) % 12 < 6 {
            let p = to_screen(x, y);
            let r = egui::Rect::from_center_size(p, egui::Vec2::splat(1.5));
            painter.rect_filled(r, 0.0, Color32::WHITE);
        }
    }
}

impl eframe::App for ProCraftApp {
    /// Persist UI state to disk (theme, panel visibility, zoom). Window
    /// rect and panel widths persist via eframe once `persistence_path`
    /// is set in `main`. Runs on clean exit (and iOS backgrounding), so
    /// the recovery file is dropped here: anything still on disk next
    /// launch means the session actually crashed. (iOS caveat: background
    /// suspension also clears it, so a later kill loses the tail — the
    /// 120 s cadence bounds that window.)
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        storage.set_string("theme", self.theme.as_str().to_string());
        storage.set_string(
            "show_tools",
            if self.show_tools { "1" } else { "0" }.to_string(),
        );
        storage.set_string(
            "show_layers",
            if self.show_layers { "1" } else { "0" }.to_string(),
        );
        storage.set_string("zoom", self.zoom.to_string());
        storage.set_string("recent", self.recent.join("\n"));
        self.clear_recovery();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.theme_applied != Some(self.theme) {
            ui.ctx().set_visuals(self.theme.visuals());
            self.theme_applied = Some(self.theme);
        }
        // Ctrl+Z undo, Ctrl+Y / Ctrl+Shift+Z redo, Esc clears selection.
        if ui
            .ctx()
            .input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::Z))
            && self.doc.undo()
        {
            self.ghost_dirty = true;
            self.status = "Undone.".to_string();
        }
        if ui
            .ctx()
            .input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::Y))
            && self.doc.redo()
        {
            self.ghost_dirty = true;
            self.status = "Redone.".to_string();
        }
        if ui.ctx().input_mut(|i| {
            i.consume_key(
                egui::Modifiers::CTRL.plus(egui::Modifiers::SHIFT),
                egui::Key::Z,
            )
        }) && self.doc.redo()
        {
            self.ghost_dirty = true;
            self.status = "Redone.".to_string();
        }
        if ui
            .ctx()
            .input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            self.doc.clear_selection();
            self.sel_anchor = None;
            self.sel_current = None;
            self.crop_anchor = None;
            self.crop_current = None;
            self.crop_rect = None;
            self.lasso_points.clear();
            self.lasso_poly.clear();
            self.sel_edges_dirty = true;
        }
        // Multi-finger taps (touch): two fingers = undo, three = redo.
        // A tap is a quick clean release with no travel; pans and pinches
        // set `moved` and never fire. Single-finger taps never arm this.
        {
            let now = ui.ctx().input(|i| i.time);
            let mt = ui.ctx().input(|i| i.multi_touch());
            if let Some(m) = mt {
                if let Some(t) = self.tap_track.as_mut() {
                    t.n = t.n.max(m.num_touches);
                    if (m.center_pos - t.c0).length() > 12.0 {
                        t.moved = true;
                    }
                } else {
                    self.tap_track = Some(TapTrack {
                        n: m.num_touches,
                        t0: now,
                        c0: m.center_pos,
                        moved: false,
                    });
                }
            } else if let Some(t) = self.tap_track.take() {
                let any_down = ui.ctx().input(|i| i.pointer.any_down());
                if !t.moved && !any_down && now - t.t0 < 0.35 {
                    match t.n {
                        2 if self.doc.undo() => {
                            self.ghost_dirty = true;
                            self.status = "Undone.".to_string();
                        }
                        3 if self.doc.redo() => {
                            self.ghost_dirty = true;
                            self.status = "Redone.".to_string();
                        }
                        _ => {}
                    }
                }
            }
        }
        // Autosave: silent v8 snapshot every 120 s (unconditional — a few
        // ms on typical canvases, and no change can slip through dirty
        // tracking). Failures surface once in the status line.
        {
            let now = ui.ctx().input(|i| i.time);
            if now - self.last_autosave >= 120.0 {
                self.last_autosave = now;
                if let Err(e) = self.doc.save_project(&self.recovery_path) {
                    self.status = format!("Autosave failed: {e}");
                }
            }
        }
        // Keep frames coming while painting, selecting, lassoing, moving,
        // liquifying, shaping, or marching ants.
        if self.painting
            || self.sel_anchor.is_some()
            || self.crop_anchor.is_some()
            || !self.lasso_points.is_empty()
            || self.shape_anchor.is_some()
            || self.move_from.is_some()
            || self.liquify_from.is_some()
            || self.doc.selection.is_some()
            || self.doc.selection_mask.is_some()
        {
            ui.ctx().request_repaint();
        }
        // Animation playback: advance the current frame on an fps clock.
        // Each frame holds for its own tick count (per-frame hold).
        if self.anim_assist && self.anim_playing && !self.doc.layers.is_empty() {
            ui.ctx().request_repaint();
            let now = ui.ctx().input(|i| i.time);
            let fps = self.anim_fps.clamp(MIN_ANIM_FPS, MAX_ANIM_FPS);
            let hold = self
                .doc
                .layers
                .get(self.doc.active)
                .map(|l| l.hold)
                .unwrap_or(MIN_ANIM_HOLD)
                .clamp(MIN_ANIM_HOLD, MAX_ANIM_HOLD);
            if now - self.anim_last_t >= hold as f64 / fps as f64 {
                self.anim_last_t = now;
                // Visible frames only, so playback matches the GIF export.
                let (next, fwd) = step_playback_visible(
                    &self.doc,
                    self.doc.active,
                    self.anim_fwd,
                    self.anim_loop,
                );
                self.doc.active = next;
                self.anim_fwd = fwd;
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
                rfd::PickPoll::Cancelled(reason) => {
                    // Reasoned cancels (no-main-thread, no window, timed
                    // out) diagnose a dead picker on device at a glance.
                    self.status = reason.unwrap_or("Pick cancelled.").to_string();
                }
            }
        }

        // Top bar: browsers, File menu, direct edit buttons, zoom, fit,
        // rotate, then theme + About. All groups flow inline inside one
        // horizontal_wrapped so the bar folds to 2+ lines on narrow
        // screens instead of overlapping; the popups never clip like
        // the old button soup did.
        egui::Panel::top("menu").show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.button("Gallery").clicked() {
                    self.gallery_open = !self.gallery_open;
                }
                if ui.button("Reference").clicked() {
                    self.ref_open = !self.ref_open;
                }
                ui.menu_button("File", |ui| {
                    if cfg!(target_os = "android") {
                        ui.label("Files on Android: picker coming — paint + tools work, saving out not yet.");
                        ui.separator();
                    }
                    if ui.button("Save PNG").clicked() {
                        ui.close();
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("PNG", &["png"])
                            .save_file()
                        {
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
                    stepped_slider(ui, &mut self.jpeg_quality, 1..=100, 1.0, "JPEG q");
                    if ui.button("Save JPEG").clicked() {
                        ui.close();
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("JPEG", &["jpg", "jpeg"])
                            .save_file()
                        {
                            let path = ensure_extension(path, "jpg");
                            match self.doc.save_jpeg(&path, self.jpeg_quality) {
                                Ok(()) => {
                                    self.status = format!(
                                        "Saved {} (quality {})",
                                        path.display(),
                                        self.jpeg_quality
                                    );
                                    #[cfg(target_os = "ios")]
                                    rfd::ios_share_file(&path);
                                }
                                Err(e) => self.status = format!("Save failed: {e}"),
                            }
                        }
                    }
                    if ui.button("Save TIFF").clicked() {
                        ui.close();
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("TIFF", &["tif", "tiff"])
                            .save_file()
                        {
                            let path = ensure_extension(path, "tiff");
                            match self.doc.save_tiff(&path) {
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
                        ui.close();
                        #[cfg(not(target_os = "ios"))]
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter(
                                "Images",
                                &["png", "jpg", "jpeg", "webp", "bmp", "gif", "tif", "tiff"],
                            )
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
                                .add_filter(
                                    "Images",
                                    &["png", "jpg", "jpeg", "webp", "bmp", "gif", "tif", "tiff"],
                                )
                                .pick_file();
                            self.pending_open = Some(PendingOpen::Image);
                            self.status = "Pick an image…".to_string();
                        }
                    }
                    ui.separator();
                    if ui.button("Save project").clicked() {
                        ui.close();
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("ProCraft project", &["procraft"])
                            .save_file()
                        {
                            let path = ensure_extension(path, "procraft");
                            match self.doc.save_project(&path) {
                                Ok(()) => {
                                    let s = path.to_string_lossy().to_string();
                                    push_recent(&mut self.recent, &s);
                                    self.gallery_keys.clear();
                                    self.status = format!("Project saved {}", path.display());
                                    self.clear_recovery();
                                    #[cfg(target_os = "ios")]
                                    rfd::ios_share_file(&path);
                                }
                                Err(e) => self.status = format!("Project save failed: {e}"),
                            }
                        }
                    }
                    if ui.button("Open project").clicked() {
                        ui.close();
                        #[cfg(not(target_os = "ios"))]
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("ProCraft project", &["procraft"])
                            .pick_file()
                        {
                            match Document::open_project(&path) {
                                Ok(doc) => {
                                    let s = path.to_string_lossy().to_string();
                                    self.adopt_project(doc, &s, "Project opened");
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
                    ui.separator();
                    if ui.button("PSD export").clicked() {
                        ui.close();
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Photoshop document", &["psd"])
                            .save_file()
                        {
                            let path = ensure_extension(path, "psd");
                            match save_psd(&path, &self.doc) {
                                Ok(()) => {
                                    self.status = format!(
                                        "PSD saved {} (masks do not round-trip)",
                                        path.display()
                                    );
                                    #[cfg(target_os = "ios")]
                                    rfd::ios_share_file(&path);
                                }
                                Err(e) => self.status = format!("PSD save failed: {e}"),
                            }
                        }
                    }
                    if ui.button("Open PSD").clicked() {
                        ui.close();
                        #[cfg(not(target_os = "ios"))]
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Photoshop document", &["psd"])
                            .pick_file()
                        {
                            match open_psd(&path) {
                                Ok(doc) => {
                                    self.doc = doc;
                                    self.clear_recovery();
                                    self.texture = None;
                                    self.lasso_points.clear();
                                    self.lasso_poly.clear();
                                    self.status = format!(
                                        "PSD opened {} (masks do not import)",
                                        path.display()
                                    );
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
                });
                if ui.button("Undo").clicked() && self.doc.undo() {
                    self.status = "Undone.".to_string();
                }
                if ui.button("Redo").clicked() && self.doc.redo() {
                    self.status = "Redone.".to_string();
                }
                if ui.button("Clear layer").clicked() {
                    self.doc.checkpoint();
                    if let Some(layer) = self.doc.active_layer_mut() {
                        layer.clear();
                    }
                    self.doc.mark_full_dirty();
                }
                ui.horizontal_wrapped(|ui| {
                    if ui.button("−").clicked() {
                        self.zoom = (self.zoom / 1.25).clamp(0.1, 8.0);
                    }
                    ui.label(format!("{}%", (self.zoom * 100.0).round() as u32));
                    if ui.button("+").clicked() {
                        self.zoom = (self.zoom * 1.25).clamp(0.1, 8.0);
                    }
                    if ui.button("Fit").clicked() {
                        self.zoom = 0.0; // recomputed in the canvas view
                        self.view_offset = egui::Vec2::ZERO;
                        self.view_angle = 0.0;
                    }
                });
                let mut deg = self.view_angle.to_degrees();
                stepped_slider(ui, &mut deg, -180.0..=180.0, 1.0, "Rotate°");
                self.view_angle = deg.to_radians();
                if ui.button("Reset rotation").clicked() {
                    self.view_angle = 0.0;
                }
                // Theme + About flow inline so the bar folds to 2+ lines
                // on narrow displays. A right_to_left pin inside
                // horizontal_wrapped draws over left items when the line
                // is full, which is the overlap seen on small screens.
                ui.separator();
                egui::ComboBox::from_id_salt("theme")
                    .selected_text(self.theme.name())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.theme, Theme::Dark, "Dark");
                        ui.selectable_value(&mut self.theme, Theme::Light, "Light");
                        ui.selectable_value(&mut self.theme, Theme::Paper, "Paper");
                    });
                let about = if self.has_symbols {
                    "ⓘ About"
                } else {
                    "About"
                };
                if ui.button(about).clicked() {
                    self.show_about = !self.show_about;
                }
            });
        });

        // M33 Gallery: file browser (New / Open / Recent / Duplicate).
        // OS dialogs stay as the fallback path; recents persist in
        // ui.json (session-only on iOS, which has no disk persistence).
        if self.gallery_open {
            self.refresh_gallery_thumbs(ui.ctx());
            let mut open = self.gallery_open;
            let mut create = false;
            let mut browse = false;
            let mut open_idx: Option<usize> = None;
            let mut dup_idx: Option<usize> = None;
            let mut remove_idx: Option<usize> = None;
            let mut clear_all = false;
            egui::Window::new("Gallery")
                .open(&mut open)
                .resizable(true)
                .collapsible(false)
                .min_width(280.0)
                .show(ui.ctx(), |ui| {
                    if cfg!(target_os = "android") {
                        ui.label(
                            "Files on Android: picker coming — new canvases + paint fully work.",
                        );
                    }
                    ui.horizontal_wrapped(|ui| {
                        ui.add(
                            egui::DragValue::new(&mut self.gallery_new_w)
                                .range(1..=MAX_CANVAS_SIDE)
                                .prefix("W "),
                        );
                        ui.add(
                            egui::DragValue::new(&mut self.gallery_new_h)
                                .range(1..=MAX_CANVAS_SIDE)
                                .prefix("H "),
                        );
                        if ui.button("New canvas").clicked() {
                            create = true;
                        }
                        if ui.button("Reset").clicked() {
                            self.gallery_new_w = 1280;
                            self.gallery_new_h = 800;
                        }
                        if ui.button("Open…").clicked() {
                            browse = true;
                        }
                    });
                    ui.separator();
                    if self.recent.is_empty() {
                        ui.label("No recent projects yet — start one above or open a file.");
                    }
                    // Snapshot paths: row buttons mutate `recent` below.
                    for (idx, path) in self.recent.clone().iter().enumerate() {
                        ui.horizontal(|ui| {
                            if let Some(tex) = self.gallery_thumbs.get(idx).and_then(|t| t.as_ref())
                            {
                                let sz = tex.size();
                                let (w, h) = (sz[0].max(1) as f32, sz[1].max(1) as f32);
                                let s = 56.0 / w.max(h);
                                ui.image((tex.id(), egui::Vec2::new(w * s, h * s)));
                            } else {
                                let (r, _) = ui.allocate_exact_size(
                                    egui::Vec2::splat(56.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter().rect_stroke(
                                    r,
                                    4.0,
                                    ui.visuals().widgets.noninteractive.bg_stroke,
                                    egui::StrokeKind::Outside,
                                );
                            }
                            ui.vertical(|ui| {
                                let name = std::path::Path::new(path)
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or(path);
                                ui.label(name);
                                ui.horizontal_wrapped(|ui| {
                                    if ui
                                        .add_sized([56.0, 28.0], egui::Button::new("Open"))
                                        .clicked()
                                    {
                                        open_idx = Some(idx);
                                    }
                                    if ui
                                        .add_sized([86.0, 28.0], egui::Button::new("Duplicate"))
                                        .clicked()
                                    {
                                        dup_idx = Some(idx);
                                    }
                                    let (del_w, del_t) = if self.has_symbols {
                                        (36.0, "✕")
                                    } else {
                                        (44.0, "Del")
                                    };
                                    if ui
                                        .add_sized([del_w, 28.0], egui::Button::new(del_t))
                                        .clicked()
                                    {
                                        remove_idx = Some(idx);
                                    }
                                });
                            });
                        });
                    }
                    if !self.recent.is_empty() && ui.button("Clear recents").clicked() {
                        clear_all = true;
                    }
                });
            self.gallery_open = open;
            if create {
                let (w, h) = (self.gallery_new_w, self.gallery_new_h);
                self.new_canvas(w, h);
                self.gallery_open = false;
            }
            if browse {
                #[cfg(not(target_os = "ios"))]
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("ProCraft project", &["procraft"])
                    .pick_file()
                {
                    match Document::open_project(&path) {
                        Ok(doc) => {
                            let s = path.to_string_lossy().to_string();
                            self.adopt_project(doc, &s, "Project opened");
                            self.gallery_open = false;
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
            if let Some(idx) = open_idx {
                if let Some(path) = self.recent.get(idx).cloned() {
                    match Document::open_project(std::path::Path::new(&path)) {
                        Ok(doc) => {
                            self.adopt_project(doc, &path, "Project opened");
                            self.gallery_open = false;
                        }
                        Err(e) => self.status = format!("Project open failed: {e}"),
                    }
                }
            }
            if let Some(idx) = dup_idx {
                self.duplicate_recent(idx);
                self.gallery_open = false;
            }
            if let Some(idx) = remove_idx {
                if idx < self.recent.len() {
                    self.recent.remove(idx);
                    self.gallery_keys.clear();
                }
            }
            if clear_all {
                self.recent.clear();
                self.gallery_keys.clear();
                self.gallery_thumbs.clear();
            }
        }

        // M34 Reference companion window: pinned image beside the canvas.
        // Session-only (not in the project file); sampling from it with
        // the Picker is deferred.
        if self.ref_open {
            self.refresh_ref_tex(ui.ctx());
            let mut open = self.ref_open;
            let mut load = false;
            let mut clear = false;
            egui::Window::new("Reference")
                .open(&mut open)
                .resizable(true)
                .collapsible(false)
                .min_width(200.0)
                .show(ui.ctx(), |ui| {
                    ui.horizontal_wrapped(|ui| {
                        if ui
                            .add_sized([70.0, 28.0], egui::Button::new("Load…"))
                            .clicked()
                        {
                            load = true;
                        }
                        if self.ref_image.is_some()
                            && ui
                                .add_sized([70.0, 28.0], egui::Button::new("Clear"))
                                .clicked()
                        {
                            clear = true;
                        }
                    });
                    if let (Some(tex), Some((_, w, h))) =
                        (self.ref_tex.as_ref(), self.ref_image.as_ref())
                    {
                        // Fit the pin inside 320px, then zoom (slider).
                        let fit = (320.0 / (*w).max(*h).max(1) as f32).min(1.0);
                        let z = fit * self.ref_scale;
                        let dw = (*w as f32 * z).clamp(32.0, 640.0);
                        let dh = (*h as f32 * z).clamp(32.0, 640.0);
                        ui.image((tex.id(), egui::Vec2::new(dw, dh)));
                        stepped_slider(ui, &mut self.ref_scale, 0.25..=2.0, 0.25, "Zoom");
                    } else {
                        ui.label("Pin an image to paint from — nothing here yet.");
                    }
                });
            self.ref_open = open;
            if clear {
                self.ref_image = None;
                self.ref_tex = None;
                self.status = "Reference cleared.".to_string();
            }
            if load {
                #[cfg(not(target_os = "ios"))]
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter(
                        "Images",
                        &["png", "jpg", "jpeg", "webp", "bmp", "gif", "tif", "tiff"],
                    )
                    .pick_file()
                {
                    let from = format!("{}", path.display());
                    match image::open(&path).map(|i| i.to_rgba8()) {
                        Ok(img) => {
                            let (w, h) = (img.width(), img.height());
                            self.set_reference(img.into_raw(), w, h, &from);
                        }
                        Err(e) => self.status = format!("Reference load failed: {e}"),
                    }
                }
                #[cfg(target_os = "ios")]
                {
                    let _ = rfd::FileDialog::new()
                        .add_filter(
                            "Images",
                            &["png", "jpg", "jpeg", "webp", "bmp", "gif", "tif", "tiff"],
                        )
                        .pick_file();
                    self.pending_open = Some(PendingOpen::Reference);
                    self.status = "Pick a reference image…".to_string();
                }
            }
        }

        // Recovery prompt: a previous session died with unsaved work.
        if self.recovery_offer {
            let mut open = self.recovery_offer;
            egui::Window::new("Unsaved work found")
                .open(&mut open)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .resizable(false)
                .collapsible(false)
                .show(ui.ctx(), |ui| {
                    ui.label("The last session ended without saving. Recover the autosave?");
                    ui.horizontal(|ui| {
                        if ui.button("Recover").clicked() {
                            match Document::open_project(&self.recovery_path) {
                                Ok(doc) => {
                                    self.doc = doc;
                                    self.texture = None;
                                    self.ghost_dirty = true;
                                    self.anim_playing = false;
                                    self.anim_shown = None;
                                    self.lasso_points.clear();
                                    self.lasso_poly.clear();
                                    self.sel_edges_dirty = true;
                                    self.status = "Recovered autosave.".to_string();
                                }
                                Err(e) => {
                                    self.clear_recovery();
                                    self.status = format!("Recovery file was corrupt ({e}).");
                                }
                            }
                            self.recovery_offer = false;
                        }
                        if ui.button("Discard").clicked() {
                            self.clear_recovery();
                            self.recovery_offer = false;
                            self.status = "Recovery discarded.".to_string();
                        }
                    });
                });
            // The X button snoozes (file kept, offered again next launch);
            // Recover/Discard already dismissed above and must stick.
            if !open {
                self.recovery_offer = false;
            }
        }

        // About box: version, builder, home page, updates. Version comes
        // from the workspace Cargo.toml at compile time, so release bumps
        // show up here with no code changes.
        if self.show_about {
            egui::Window::new("About ProCraft")
                .open(&mut self.show_about)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
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
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .resizable(true)
                .collapsible(false)
                .show(ui.ctx(), |ui| {
                    // Scrollable: on small screens (iPad keyboard up) the
                    // tall dialog clips Import/Render below the fold.
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.label("Click the canvas to move the anchor, then Render.");
                        // Explicit focus like the layer rename: iOS shows no
                        // keyboard for an unfocused multiline field.
                        let field = ui.add(
                            egui::TextEdit::multiline(&mut self.text_body)
                                .desired_rows(3)
                                .hint_text("Type here (Enter = new line)"),
                        );
                        if !field.has_focus() {
                            field.request_focus();
                        }
                        stepped_slider(
                            ui,
                            &mut self.text_size,
                            MIN_TEXT_PX..=128.0,
                            1.0,
                            "Size px",
                        );
                        // M38 Typography: face + spacing + outline.
                        ui.horizontal_wrapped(|ui| {
                            let face = self.text_font_name.as_deref().unwrap_or("Inter (bundled)");
                            ui.label(format!("Face: {face}"));
                            if ui
                                .add_sized([80.0, 28.0], egui::Button::new("Import…"))
                                .clicked()
                            {
                                #[cfg(not(target_os = "ios"))]
                                if let Some(path) = rfd::FileDialog::new()
                                    .add_filter("Fonts", &["ttf", "otf"])
                                    .pick_file()
                                {
                                    match std::fs::read(&path) {
                                        Ok(bytes) => {
                                            let name = path
                                                .file_name()
                                                .and_then(|n| n.to_str())
                                                .unwrap_or("font")
                                                .to_string();
                                            self.status = self.absorb_font(&bytes, &name);
                                        }
                                        Err(e) => self.status = format!("Font import failed: {e}"),
                                    }
                                }
                                #[cfg(target_os = "ios")]
                                {
                                    let _ = rfd::FileDialog::new()
                                        .add_filter("Fonts", &["ttf", "otf"])
                                        .pick_file();
                                    self.pending_open = Some(PendingOpen::Font);
                                    self.status = "Pick a font…".to_string();
                                }
                            }
                            if self.text_font_name.is_some()
                                && ui
                                    .add_sized([86.0, 28.0], egui::Button::new("Bundled"))
                                    .clicked()
                            {
                                self.text_font_name = None;
                                self.text_font_bytes = None;
                                self.status = "Back to Inter.".to_string();
                            }
                        });
                        stepped_slider(
                            ui,
                            &mut self.text_leading,
                            -MAX_TEXT_SPACING..=MAX_TEXT_SPACING,
                            1.0,
                            "Leading",
                        );
                        stepped_slider(
                            ui,
                            &mut self.text_tracking,
                            -MAX_TEXT_SPACING..=MAX_TEXT_SPACING,
                            1.0,
                            "Tracking",
                        );
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut self.text_outline, "Outline");
                            if self.text_outline {
                                stepped_slider(
                                    ui,
                                    &mut self.text_outline_w,
                                    1..=MAX_OUTLINE_PX,
                                    1.0,
                                    "px",
                                );
                            }
                        });
                        if self.text_outline {
                            ui.label("Outline color:");
                            stepped_slider(ui, &mut self.text_outline_rgb[0], 0..=255, 1.0, "R");
                            stepped_slider(ui, &mut self.text_outline_rgb[1], 0..=255, 1.0, "G");
                            stepped_slider(ui, &mut self.text_outline_rgb[2], 0..=255, 1.0, "B");
                        }
                        ui.horizontal_wrapped(|ui| {
                            if ui
                                .add_sized([80.0, 28.0], egui::Button::new("Render"))
                                .clicked()
                            {
                                match self.text_anchor {
                                    Some((ax, ay)) => {
                                        let style = self.text_style();
                                        self.doc.checkpoint();
                                        match self.doc.draw_text_styled(
                                            ink,
                                            ax,
                                            ay,
                                            self.text_size,
                                            &self.text_body,
                                            &style,
                                        ) {
                                            Ok((w, h)) => {
                                                self.status = format!("Text placed ({w}x{h}px).");
                                                self.text_open = false;
                                                self.capture_timelapse();
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
                            if ui
                                .add_sized([80.0, 28.0], egui::Button::new("Cancel"))
                                .clicked()
                            {
                                self.text_open = false;
                            }
                        });
                    });
                });
            self.text_open = open;
        }

        // Fresh id per width era ("tools" -> "tools-wide" -> here): panel
        // widths persist by id (see `persistence_path` in `main`), so a new
        // id starts at `default_size` while any saved width stays buried
        // under the old id. No max clamp:
        // the panel stays freely resizable, it just starts compact (swatch
        // grids wrap to ~3 rows, page gets the room).
        if self.show_tools {
            egui::Panel::left("tools-compact").default_size(240.0).show(ui, |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Tool");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add_sized([40.0, 24.0], egui::Button::new("◀")).clicked() {
                        self.show_tools = false;
                    }
                });
            });
            ui.radio_value(&mut self.tool, Tool::Brush, tool_label(Tool::Brush, self.has_symbols));
            ui.radio_value(&mut self.tool, Tool::Eraser, tool_label(Tool::Eraser, self.has_symbols));
            ui.radio_value(&mut self.tool, Tool::Smudge, tool_label(Tool::Smudge, self.has_symbols));
            ui.radio_value(&mut self.tool, Tool::Select, tool_label(Tool::Select, self.has_symbols));
            ui.radio_value(&mut self.tool, Tool::Lasso, tool_label(Tool::Lasso, self.has_symbols));
            ui.radio_value(&mut self.tool, Tool::Fill, tool_label(Tool::Fill, self.has_symbols));
            ui.radio_value(&mut self.tool, Tool::Picker, tool_label(Tool::Picker, self.has_symbols));
            ui.radio_value(&mut self.tool, Tool::Move, tool_label(Tool::Move, self.has_symbols));
            ui.radio_value(&mut self.tool, Tool::Liquify, tool_label(Tool::Liquify, self.has_symbols));
            ui.radio_value(&mut self.tool, Tool::Crop, tool_label(Tool::Crop, self.has_symbols));
            ui.radio_value(&mut self.tool, Tool::Shapes, tool_label(Tool::Shapes, self.has_symbols));
            ui.radio_value(&mut self.tool, Tool::Text, tool_label(Tool::Text, self.has_symbols));
            if self.tool == Tool::Fill {
                stepped_slider(ui, &mut self.fill_tolerance, 0..=255, 1.0, "Tolerance");
                ui.label("Tap fills; hold + drag sideways to sweep tolerance.");
            }
            if self.tool == Tool::Picker {
                ui.label("Tap the canvas to lift its color; the loupe magnifies the target.");
            }
            if self.tool == Tool::Smudge {
                stepped_slider(ui, &mut self.smudge_strength, 0.0..=1.0, 0.05, "Strength");
                ui.label("Drag to push paint around (color layers).");
            }
            if self.tool == Tool::Liquify {
                stepped_slider(ui, &mut self.liquify_size, 8.0..=512.0, 1.0, "Size");
                stepped_slider(ui, &mut self.liquify_strength, 0.05..=1.0, 0.05, "Strength");
                ui.label("Drag to push pixels (one undo step per stroke).");
            }
            if self.tool == Tool::Crop {
                ui.label("Drag a rect on the canvas, then Apply.");
                if let Some((x0, y0, x1, y1)) = self.crop_rect {
                    ui.label(format!("Crop {}x{} px.", x1 - x0 + 1, y1 - y0 + 1));
                    if ui.button("Apply crop").clicked() {
                        match self.doc.crop(x0, y0, x1, y1) {
                            Ok(()) => {
                                let (w, h) = (self.doc.w, self.doc.h);
                                self.after_canvas_change();
                                self.status =
                                    format!("Cropped to {w}x{h} (history cleared).");
                            }
                            Err(e) => self.status = format!("Crop failed: {e}"),
                        }
                    }
                    if ui.button("Cancel crop").clicked() {
                        self.crop_rect = None;
                        self.status = "Crop cancelled.".to_string();
                    }
                }
                ui.collapsing("Canvas size", |ui| {
                    // Re-adopt the real dims after an open/crop/apply; typing
                    // never changes the doc, so edits are never clobbered.
                    if self.canvas_sync != (self.doc.w, self.doc.h) {
                        self.canvas_sync = (self.doc.w, self.doc.h);
                        self.canvas_w = self.doc.w;
                        self.canvas_h = self.doc.h;
                    }
                    stepped_slider(ui, &mut self.canvas_w, 1..=MAX_CANVAS_SIDE, 1.0, "Width");
                    stepped_slider(ui, &mut self.canvas_h, 1..=MAX_CANVAS_SIDE, 1.0, "Height");
                    ui.label("Anchor (old canvas sits here):");
                    for ay in -1..=1 {
                        ui.horizontal(|ui| {
                            for ax in -1..=1 {
                                let mark =
                                    if self.canvas_anchor == (ax, ay) { "■" } else { "□" };
                                if ui
                                    .add_sized([32.0, 24.0], egui::Button::new(mark))
                                    .clicked()
                                {
                                    self.canvas_anchor = (ax, ay);
                                }
                            }
                        });
                    }
                    if ui.button("Apply size").clicked() {
                        let (ax, ay) = self.canvas_anchor;
                        let place = |new: u32, old: u32, a: i32| {
                            let rest = new as i32 - old as i32;
                            match a {
                                -1 => 0,
                                1 => rest,
                                _ => rest / 2,
                            }
                        };
                        let dx = place(self.canvas_w, self.doc.w, ax);
                        let dy = place(self.canvas_h, self.doc.h, ay);
                        match self.doc.rehome_canvas(self.canvas_w, self.canvas_h, dx, dy) {
                            Ok(()) => {
                                let (w, h) = (self.doc.w, self.doc.h);
                                self.after_canvas_change();
                                self.status =
                                    format!("Canvas {w}x{h} (history cleared).");
                            }
                            Err(e) => self.status = format!("Resize failed: {e}"),
                        }
                    }
                    ui.label("Crop and resize clear undo history.");
                });
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
                    stepped_slider(ui, &mut self.shape_sides, 3..=12, 1.0, "Sides");
                }
            }
            let has_selection = self.doc.selection.is_some() || self.doc.selection_mask.is_some();
            ui.separator();
            if ui
                .add_enabled(has_selection, egui::Button::new("Clear selection"))
                .clicked()
            {
                self.doc.clear_selection();
                self.lasso_poly.clear();
                self.sel_edges_dirty = true;
                self.status = "Selection cleared.".to_string();
            }
            ui.collapsing("Selection Studio", |ui| {
                if !has_selection {
                    ui.label("No selection yet — drag Marquee/Lasso on the canvas, or Select opaque.");
                }
                stepped_slider(ui, &mut self.sel_feather, 0..=32, 1.0, "Feather px");
                if ui.button("Apply feather").clicked() {
                    if self.doc.feather_selection(self.sel_feather) {
                        self.status = format!("Feathered ({} px).", self.doc.selection_pixel_count());
                    } else {
                        self.status = "No selection to feather.".to_string();
                    }
                    self.sel_edges_dirty = true;
                }
                stepped_slider(ui, &mut self.sel_grow, -32..=32, 1.0, "Grow px");
                if ui.button("Apply grow").clicked() {
                    if self.doc.grow_selection(self.sel_grow) {
                        self.status = format!("Grown ({} px).", self.doc.selection_pixel_count());
                    } else {
                        self.status = "No selection to grow.".to_string();
                    }
                    self.sel_edges_dirty = true;
                }
                if ui.button("Smooth").clicked() {
                    if self.doc.smooth_selection() {
                        self.status = format!("Smoothed ({} px).", self.doc.selection_pixel_count());
                    } else {
                        self.status = "No selection to smooth.".to_string();
                    }
                    self.sel_edges_dirty = true;
                }
                if ui.button("Invert").clicked() {
                    if self.doc.invert_selection() {
                        self.status = format!("Inverted ({} px).", self.doc.selection_pixel_count());
                    } else {
                        self.status = "No selection to invert.".to_string();
                    }
                    self.sel_edges_dirty = true;
                }
                stepped_slider(ui, &mut self.sel_opaque_t, 0..=255, 1.0, "Opacity >");
                if ui.button("Select opaque").clicked() {
                    if self.doc.select_opaque(self.sel_opaque_t) {
                        self.status = format!("Opaque selected ({} px).", self.doc.selection_pixel_count());
                    } else {
                        self.status = "Layer is empty.".to_string();
                    }
                    self.sel_edges_dirty = true;
                }
                stepped_slider(ui, &mut self.sel_stroke_w, 1..=16, 1.0, "Outline px");
                if ui.button("Paint outline").clicked() {
                    self.doc.checkpoint();
                    let brush = self.base_brush();
                    let color = self.current_color();
                    if self.doc.stroke_selection(&brush, color, self.sel_stroke_w) {
                        self.status = "Outlined.".to_string();
                    } else {
                        self.status = "No selection to outline.".to_string();
                    }
                }
                // M35 Selection upgrades: save/load slot (session-only;
                // Load refuses size mismatches) plus nudge/flip transform.
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Save selection").clicked() {
                        if self.doc.selection.is_some() || self.doc.selection_mask.is_some()
                        {
                            self.saved_sel = Some((
                                (self.doc.w, self.doc.h),
                                self.doc.selection,
                                self.doc.selection_mask.clone(),
                            ));
                            self.status = format!(
                                "Selection saved ({} px).",
                                self.doc.selection_pixel_count()
                            );
                        } else {
                            self.status = "No selection to save.".to_string();
                        }
                    }
                    if ui
                        .add_enabled(self.saved_sel.is_some(), egui::Button::new("Load selection"))
                        .clicked()
                    {
                        let dims_ok = self
                            .saved_sel
                            .as_ref()
                            .is_some_and(|(dims, _, _)| *dims == (self.doc.w, self.doc.h));
                        if dims_ok {
                            if let Some((_, rect, mask)) = self.saved_sel.clone() {
                                self.doc.selection = rect;
                                self.doc.selection_mask = mask;
                                self.sel_edges_dirty = true;
                                self.status = format!(
                                    "Selection loaded ({} px).",
                                    self.doc.selection_pixel_count()
                                );
                            }
                        } else {
                            self.status =
                                "Saved selection is a different canvas size.".to_string();
                        }
                    }
                });
                if let Some(((w, h), _, _)) = &self.saved_sel {
                    ui.label(format!("Saved slot: {w}x{h} canvas."));
                }
                stepped_slider(ui, &mut self.sel_nudge_x, -64..=64, 1.0, "Nudge x");
                stepped_slider(ui, &mut self.sel_nudge_y, -64..=64, 1.0, "Nudge y");
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Apply nudge").clicked() {
                        let (dx, dy) = (self.sel_nudge_x, self.sel_nudge_y);
                        if self.doc.translate_selection(dx, dy) {
                            self.status = format!(
                                "Nudged ({dx},{dy}, {} px).",
                                self.doc.selection_pixel_count()
                            );
                        } else {
                            self.status = "Nudge cleared the selection.".to_string();
                        }
                        self.sel_edges_dirty = true;
                    }
                if ui.button(if self.has_symbols { "⇄ Flip H" } else { "Flip H" }).clicked() {
                        if self.doc.flip_selection_h() {
                            self.status = "Flipped horizontal.".to_string();
                        } else {
                            self.status = "No selection to flip.".to_string();
                        }
                        self.sel_edges_dirty = true;
                    }
                if ui.button(if self.has_symbols { "⇅ Flip V" } else { "Flip V" }).clicked() {
                        if self.doc.flip_selection_v() {
                            self.status = "Flipped vertical.".to_string();
                        } else {
                            self.status = "No selection to flip.".to_string();
                        }
                        self.sel_edges_dirty = true;
                    }
                });
            });
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
                            let preset_del = if self.has_symbols { "✕" } else { "Del" };
                            if ui.small_button(preset_del).clicked() {
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
            stepped_slider(ui, &mut self.picker_size, 140.0..=420.0, 5.0, "Picker px");
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
            swatch_grid(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(3.0, 3.0);
                    for rgb in starter_palette().colors {
                        let resp = swatch_button(ui, rgb, self.color == rgb);
                        if resp.clicked() {
                            self.color = rgb;
                        }
                    }
                });
            });
            // Wrapped (not horizontal): long palette names must fold here
            // instead of forcing the whole panel wider.
            ui.horizontal_wrapped(|ui| {
                ui.label("Palettes:");
                let n = self.palettes.palettes.len();
                if ui.button("◀").clicked() && n > 0 {
                    self.palettes.set_active((self.palettes.active + n - 1) % n);
                }
                let (pname, pcount) = self
                    .palettes
                    .active()
                    .map(|p| (p.name.clone(), p.colors.len()))
                    .unwrap_or_else(|| ("-".to_string(), 0));
                ui.label(format!("{pname} ({}/{} · {} swatches)", self.palettes.active + 1, n, pcount));
                if ui.button("▶").clicked() && n > 0 {
                    self.palettes.set_active((self.palettes.active + 1) % n);
                }
                if ui.add_enabled(n < MAX_PALETTES, egui::Button::new("+ New")).clicked() {
                    let name = self.palettes.unique_name("Palette");
                    match self.palettes.add(&name) {
                        Ok(_) => self.status = format!("Palette {name} added."),
                        Err(e) => self.status = format!("Palette not added: {e}"),
                    }
                }
            });
            ui.horizontal(|ui| {
                if !self.pal_renaming {
                    if ui.button("Rename").clicked() {
                        self.pal_name_edit =
                            self.palettes.active().map(|p| p.name.clone()).unwrap_or_default();
                        self.pal_renaming = true;
                    }
                    if ui.button("Delete").clicked() {
                        let idx = self.palettes.active;
                        let name =
                            self.palettes.active().map(|p| p.name.clone()).unwrap_or_default();
                        self.palettes.remove(idx);
                        self.status = format!("Palette {name} deleted.");
                    }
                } else {
                    ui.text_edit_singleline(&mut self.pal_name_edit);
                    if ui.button("Save").clicked() {
                        let idx = self.palettes.active;
                        let edit = self.pal_name_edit.clone();
                        match self.palettes.rename(idx, &edit) {
                            Ok(()) => {
                                self.pal_renaming = false;
                                self.status = format!("Palette renamed {edit}.");
                            }
                            Err(e) => self.status = format!("Rename failed: {e}"),
                        }
                    }
                    if ui.button("Cancel").clicked() {
                        self.pal_renaming = false;
                    }
                }
            });
            ui.label("Active palette (click applies, right-click removes):");
            let swatches =
                self.palettes.active().map(|p| p.colors.clone()).unwrap_or_default();
            if swatches.is_empty() {
                ui.label("Empty — store the current color with +.");
            } else {
                let mut remove = None;
                swatch_grid(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(3.0, 3.0);
                        for (idx, rgb) in swatches.iter().enumerate() {
                            let resp = swatch_button(ui, *rgb, self.color == *rgb);
                            if resp.clicked() {
                                self.color = *rgb;
                            }
                            if resp.secondary_clicked() {
                                remove = Some(idx);
                            }
                        }
                    });
                });
                if let Some(idx) = remove {
                    self.palettes.remove_color(idx);
                }
            }
            ui.horizontal(|ui| {
                if ui.button("+ Current").clicked() {
                    if swatches.contains(&self.color) {
                        self.status = "That color is already stored.".to_string();
                    } else if swatches.len() >= MAX_PALETTE_COLORS {
                        self.status = format!("Swatch set is full ({MAX_PALETTE_COLORS}).");
                    } else {
                        let _ = self.palettes.push_color(self.color);
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
                            Ok(pal) => {
                                self.status = self.absorb_colors(pal, &format!("{}", path.display()))
                            }
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
                        let pal = self.palettes.active().cloned().unwrap_or(Palette {
                            name: "My colors".to_string(),
                            colors: Vec::new(),
                        });
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
            });
            ui.horizontal(|ui| {
                if ui.button("Clear").clicked() {
                    self.palettes.clear_active();
                    self.status = "Active palette cleared.".to_string();
                }
                if ui.button("From image").clicked() {
                    #[cfg(not(target_os = "ios"))]
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter(
                            "Images",
                            &["png", "jpg", "jpeg", "webp", "bmp", "gif", "tif", "tiff"],
                        )
                        .pick_file()
                    {
                        let stem = path
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("Image")
                            .to_string();
                        let from = format!("{}", path.display());
                        match image::open(&path).map(|i| i.to_rgba8()) {
                            Ok(img) => {
                                let (w, h) = (img.width(), img.height());
                                self.status =
                                    self.image_palette_status(&stem, img.as_raw(), w, h, &from);
                            }
                            Err(e) => self.status = format!("Image open failed: {e}"),
                        }
                    }
                    #[cfg(target_os = "ios")]
                    {
                        let _ = rfd::FileDialog::new()
                            .add_filter(
                                "Images",
                                &["png", "jpg", "jpeg", "webp", "bmp", "gif", "tif", "tiff"],
                            )
                            .pick_file();
                        self.pending_open = Some(PendingOpen::ImagePalette);
                        self.status = "Pick an image…".to_string();
                    }
                }
                if ui.button("From canvas").clicked() {
                    let flat = self.doc.flatten();
                    let (w, h) = (self.doc.w, self.doc.h);
                    self.status = self.image_palette_status("Canvas", &flat, w, h, "canvas");
                }
            });
            ui.label("Harmony of the current color (click applies):");
            swatch_grid(ui, |ui| {
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
            });
            ui.separator();
            stepped_slider(ui, &mut self.brush.size, 1.0..=200.0, 1.0, "Size");
            stepped_slider(ui, &mut self.brush.opacity, 0.05..=1.0, 0.05, "Opacity");
            stepped_slider(ui, &mut self.brush.softness, 0.0..=1.0, 0.05, "Softness");
            stepped_slider(ui, &mut self.brush.spacing, 0.05..=1.0, 0.05, "Spacing");
            ui.collapsing("Pressure (touch / Pencil)", |ui| {
                stepped_slider(ui, &mut self.brush.pressure_size, 0.0..=1.0, 0.05, "Size");
                stepped_slider(ui, &mut self.brush.pressure_opacity, 0.0..=1.0, 0.05, "Opacity");
                ui.label("Mouse has no pressure sensor: it paints at full pressure.");
            });
            ui.collapsing("Velocity", |ui| {
                stepped_slider(ui, &mut self.brush.velocity_size, 0.0..=1.0, 0.05, "Thin");
                stepped_slider(ui, &mut self.brush.velocity_opacity, 0.0..=1.0, 0.05, "Fade");
                ui.label("Fast moves thin and fade (this is the old Dynamics, split).");
            });
            ui.collapsing("Texture", |ui| {
                stepped_slider(ui, &mut self.brush.grain, 0.0..=1.0, 0.05, "Grain");
                stepped_slider(ui, &mut self.brush.grain_scale, 0.25..=4.0, 0.05, "Grain size");
            });
            ui.collapsing("Depth", |ui| {
                // M39 per-brush blend mode (Normal = classic source-over).
                ui.horizontal_wrapped(|ui| {
                    for m in BlendMode::ALL {
                        ui.radio_value(&mut self.brush.blend, m, m.name());
                    }
                });
                stepped_slider(ui, &mut self.brush.wet_edge, 0.0..=1.0, 0.05, "Wet edge");
                // M39 dual-tip: a stamp from any brush file textures the dab.
                ui.horizontal_wrapped(|ui| {
                    match &self.brush.dual_tip {
                        Some(t) => ui.label(format!("Dual: {}x{} tip", t.w, t.h)),
                        None => ui.label("Dual: off"),
                    };
                    if ui.button("Import…").clicked() {
                        #[cfg(not(target_os = "ios"))]
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Brushes", &["pcbrush", "json", "abr", "brushset"])
                            .pick_file()
                        {
                            self.status = self.absorb_dual_tip(&path);
                        }
                        #[cfg(target_os = "ios")]
                        {
                            let _ = rfd::FileDialog::new()
                                .add_filter("Brushes", &["pcbrush", "json", "abr", "brushset"])
                                .pick_file();
                            self.pending_open = Some(PendingOpen::DualTip);
                            self.status = "Pick a dual tip…".to_string();
                        }
                    }
                    if self.brush.dual_tip.is_some() && ui.button("Clear").clicked() {
                        self.brush.dual_tip = None;
                        self.status = "Dual tip cleared.".to_string();
                    }
                });
                if self.brush.dual_tip.is_some() {
                    stepped_slider(ui, &mut self.brush.dual_scale, 0.1..=2.0, 0.05, "Dual size");
                }
            });
            ui.collapsing("Dab shape", |ui| {
                stepped_slider(ui, &mut self.brush.roundness, 0.1..=1.0, 0.05, "Roundness");
                ui.checkbox(&mut self.brush.angle_follow, "Follow direction");
                stepped_slider(ui, &mut self.brush.scatter, 0.0..=1.0, 0.05, "Scatter");
            });
            ui.collapsing("Smoothing", |ui| {
                stepped_slider(ui, &mut self.brush.stabilizer, 0.0..=1.0, 0.05, "Stabilizer");
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
            let mut kind_changed = false;
            ui.horizontal_wrapped(|ui| {
                for k in GuideKind::ALL {
                    if ui.radio_value(&mut self.guides.kind, k, k.name()).changed() {
                        kind_changed = true;
                    }
                }
            });
            if kind_changed {
                self.vp_move = false;
                self.vp_grab = None;
            }
            if self.guides.kind != GuideKind::Off {
                match self.guides.kind {
                    GuideKind::Square | GuideKind::Isometric => {
                        stepped_slider(ui, &mut self.guides.spacing, 8.0..=256.0, 1.0, "Spacing");
                    }
                    GuideKind::OnePoint | GuideKind::TwoPoint => {
                        if ui.checkbox(&mut self.vp_move, "Move VPs (drag handles on canvas)").changed()
                        {
                            self.vp_grab = None;
                            self.status = if self.vp_move {
                                "Drag a handle; off-canvas VPs park as diamonds on the edge. Painting pauses while armed.".to_string()
                            } else {
                                "Vanishing-point move off.".to_string()
                            };
                        }
                        if ui.button("Reset VPs").clicked() {
                            self.guides.reset_vps();
                            self.status = "Vanishing points reset to defaults.".to_string();
                        }
                    }
                    GuideKind::Off => {}
                }
                ui.checkbox(&mut self.guides.snap, "Snap while painting");
                if self.guides.snap {
                    stepped_slider(ui, &mut self.guides.snap_px, 2.0..=32.0, 1.0, "Snap range (px)");
                }
                ui.checkbox(&mut self.assist, "Drawing Assist (lock strokes to guides)");
                if self.assist {
                    ui.label("Rays/axes engage as you draw; taps stay free.");
                }
                stepped_slider(ui, &mut self.guides.opacity, 0.05..=1.0, 0.05, "Opacity");
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

        // Fresh id (not "layers"): panel widths persist to disk by id (see
        // `persistence_path` in `main`), and the old narrow width would
        // otherwise override `default_size` forever.
        if self.show_layers {
            egui::Panel::right("layers-wide").default_size(280.0).show(ui, |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.add_sized([40.0, 24.0], egui::Button::new("▶")).clicked() {
                    self.show_layers = false;
                }
                ui.heading("Layers");
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
            if ui.button("New group").clicked() {
                let name = format!("Group {}", self.doc.groups.len() + 1);
                self.doc.new_group(name.clone());
                self.ghost_dirty = true;
                self.status = format!("{name} created.");
            }
            let mut remove = None;
            let mut select = None;
            let mut touched = false;
            let mut grp_collapse = None;
            let mut grp_eye = None;
            let mut grp_add = None;
            let mut grp_ungroup = None;
            let mut grp_opacity: Option<(u32, f32)> = None;
            let mut grp_rename_commit: Option<(u32, String)> = None;
            // Ancestor chains (top-down group ids) per layer, computed up
            // front so the row loop can read them without borrowing.
            let chains: Vec<Vec<u32>> = (0..self.doc.layers.len())
                .map(|i| {
                    let mut up = Vec::new();
                    let mut seen = Vec::new();
                    let mut g = self.doc.layers[i].group;
                    while let Some(id) = g {
                        if seen.contains(&id) {
                            break;
                        }
                        seen.push(id);
                        let Some(gr) = self.doc.groups.iter().find(|x| x.id == id) else {
                            break;
                        };
                        up.push(id);
                        g = gr.parent;
                    }
                    up.reverse();
                    up
                })
                .collect();
            let is_collapsed = |doc: &Document, id: u32| {
                doc.groups
                    .iter()
                    .find(|g| g.id == id)
                    .is_some_and(|g| g.collapsed)
            };
            let mut printed: Vec<u32> = Vec::new();
            for i in (0..self.doc.layers.len()).rev() {
                let chain = chains[i].clone();
                for (d, gid) in chain.iter().enumerate() {
                    if printed.contains(gid) {
                        continue;
                    }
                    // Headers parked under a collapsed ancestor stay hidden
                    // with their subtree.
                    if chain[..d].iter().any(|a| is_collapsed(&self.doc, *a)) {
                        continue;
                    }
                    printed.push(*gid);
                    let (gname, gvis, gcollapsed, gopacity) =
                        match self.doc.groups.iter().find(|g| g.id == *gid) {
                            Some(g) => (g.name.clone(), g.visible, g.collapsed, g.opacity),
                            None => continue,
                        };
                    ui.horizontal(|ui| {
                        ui.add_space(d as f32 * 12.0);
                        if ui
                            .small_button(if gcollapsed { "+" } else { "−" })
                            .on_hover_text(if gcollapsed { "Expand group" } else { "Collapse group" })
                            .clicked()
                        {
                            grp_collapse = Some(*gid);
                        }
                        let mut vis = gvis;
                        if ui
                            .toggle_value(&mut vis, "👁")
                            .on_hover_text("Toggle group visibility")
                            .changed()
                        {
                            grp_eye = Some(*gid);
                        }
                        if Some(*gid) == self.group_renaming {
                            let resp =
                                ui.text_edit_singleline(&mut self.group_name_edit);
                            if !resp.has_focus() {
                                resp.request_focus();
                            }
                            let enter =
                                ui.ctx().input(|inp| inp.key_pressed(egui::Key::Enter));
                            let mut commit = resp.lost_focus() || enter;
                            let mut cancel = false;
                            let (ok_label, x_label) = if self.has_symbols {
                                ("✓", "✕")
                            } else {
                                ("OK", "X")
                            };
                            if ui.add_sized([44.0, 28.0], egui::Button::new(ok_label)).clicked() {
                                commit = true;
                            }
                            if ui.add_sized([44.0, 28.0], egui::Button::new(x_label)).clicked() {
                                cancel = true;
                            }
                            if commit && !cancel {
                                let name = std::mem::take(&mut self.group_name_edit);
                                grp_rename_commit = Some((
                                    *gid,
                                    if name.trim().is_empty() {
                                        gname.clone()
                                    } else {
                                        name
                                    },
                                ));
                            }
                            if commit || cancel {
                                self.group_renaming = None;
                                self.group_name_edit.clear();
                            }
                        } else {
                            if ui.selectable_label(false, &gname).clicked() {
                                self.group_renaming = Some(*gid);
                                self.group_name_edit = gname.clone();
                            }
                        }
                        if ui
                            .small_button("+Add")
                            .on_hover_text("Move active layer into this group")
                            .clicked()
                        {
                            grp_add = Some(*gid);
                        }
                        if ui
                            .small_button("Ungroup")
                            .on_hover_text("Disband group (keeps layers)")
                            .clicked()
                        {
                            grp_ungroup = Some(*gid);
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.add_space(d as f32 * 12.0);
                        let mut gop = gopacity;
                        stepped_slider(ui, &mut gop, 0.0..=1.0, 0.05, "Opacity");
                        if (gop - gopacity).abs() > f32::EPSILON {
                            grp_opacity = Some((*gid, gop));
                        }
                    });
                }
                if chain.iter().any(|id| is_collapsed(&self.doc, *id)) {
                    continue;
                }
                let depth = chain.len();
                let layer = &mut self.doc.layers[i];
                ui.horizontal(|ui| {
                    ui.add_space(depth as f32 * 12.0);
                    let mut vis = layer.visible;
                    if ui
                        .toggle_value(&mut vis, "👁")
                        .on_hover_text("Toggle layer visibility")
                        .changed()
                    {
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
                            let (ok_label, x_label) = if self.has_symbols {
                                ("✓", "✕")
                            } else {
                                ("OK", "X")
                            };
                            if ui.add_sized([44.0, 28.0], egui::Button::new(ok_label)).clicked() {
                                commit = true;
                            }
                            if ui.add_sized([44.0, 28.0], egui::Button::new(x_label)).clicked() {
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
                    if ui
                        .add_sized([44.0, 24.0], egui::Button::new("del"))
                        .clicked()
                    {
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
                if !self.doc.is_layer_shown(i) {
                    self.status = "Active layer is in a hidden group.".to_string();
                }
            }
            if let Some(gid) = grp_collapse {
                let cur = self
                    .doc
                    .groups
                    .iter()
                    .find(|g| g.id == gid)
                    .is_some_and(|g| g.collapsed);
                self.doc.set_group_collapsed(gid, !cur);
            }
            if let Some(gid) = grp_eye {
                let cur = self
                    .doc
                    .groups
                    .iter()
                    .find(|g| g.id == gid)
                    .is_none_or(|g| g.visible);
                self.doc.set_group_visible(gid, !cur);
                self.ghost_dirty = true;
                self.doc.mark_full_dirty();
            }
            if let Some(gid) = grp_add {
                let active = self.doc.active;
                self.doc.move_layer_into_group(active, Some(gid));
                self.ghost_dirty = true;
                self.status = "Active layer moved into the group.".to_string();
            }
            if let Some(gid) = grp_ungroup {
                self.doc.ungroup(gid);
                self.ghost_dirty = true;
                self.doc.mark_full_dirty();
                self.status = "Group disbanded (layers kept).".to_string();
            }
            if let Some((gid, op)) = grp_opacity {
                self.doc.set_group_opacity(gid, op);
                self.ghost_dirty = true;
                self.doc.mark_full_dirty();
            }
            if let Some((gid, name)) = grp_rename_commit {
                self.doc.rename_group(gid, name);
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
                stepped_slider(ui, &mut layer.opacity, 0.0..=1.0, 0.05, "");
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
                        self.anim_fwd = true;
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
                    if ui.button("Duplicate frame").clicked() {
                        let active = self.doc.active;
                        self.doc.duplicate_layer(active);
                        self.ghost_dirty = true;
                        self.status = format!("Frame {} duplicated.", self.doc.active + 1);
                    }
                });
                stepped_slider(ui, &mut self.anim_fps, MIN_ANIM_FPS..=MAX_ANIM_FPS, 1.0, "FPS");
                ui.horizontal(|ui| {
                    ui.label("Loop:");
                    for m in LoopMode::ALL {
                        ui.radio_value(&mut self.anim_loop, m, m.name());
                    }
                });
                // Per-frame hold: the stepper edits the current frame only.
                let mut hold = self
                    .doc
                    .layers
                    .get(self.doc.active)
                    .map(|l| l.hold)
                    .unwrap_or(MIN_ANIM_HOLD)
                    .clamp(MIN_ANIM_HOLD, MAX_ANIM_HOLD);
                stepped_slider(ui, &mut hold, MIN_ANIM_HOLD..=MAX_ANIM_HOLD, 1.0, "Frame hold");
                if let Some(layer) = self.doc.layers.get_mut(self.doc.active) {
                    if layer.hold != hold {
                        layer.hold = hold;
                    }
                }
                if ui.checkbox(&mut self.onion, "Onion skin").changed() {
                    self.ghost_dirty = true;
                }
                if self.onion {
                    let before = (self.onion_prev, self.onion_next);
                    stepped_slider(ui, &mut self.onion_prev, 0.05..=0.8, 0.05, "Prev ghost");
                    stepped_slider(ui, &mut self.onion_next, 0.05..=0.8, 0.05, "Next ghost");
                    if (self.onion_prev, self.onion_next) != before {
                        self.ghost_dirty = true;
                    }
                }
                if ui.button("Export GIF").clicked() {
                    if let Some(path) = rfd::FileDialog::new().add_filter("GIF image", &["gif"]).save_file()
                    {
                        let path = ensure_extension(path, "gif");
                        match export_gif_ext(&self.doc, self.anim_fps, self.anim_loop) {
                            Ok(bytes) => match std::fs::write(&path, &bytes) {
                                Ok(()) => {
                                    let shown =
                                        self.doc.layers.iter().filter(|l| l.visible).count();
                                    let exported = if self.anim_loop == LoopMode::PingPong && shown > 2
                                    {
                                        2 * shown - 2
                                    } else {
                                        shown
                                    };
                                    let held =
                                        self.doc.layers.iter().filter(|l| l.hold > 1).count();
                                    let extra = match (self.anim_loop, held) {
                                        (LoopMode::PingPong, 0) => ", ping-pong".to_string(),
                                        (LoopMode::PingPong, _) => {
                                            format!(", ping-pong, {held} held")
                                        }
                                        (LoopMode::Forward, 0) => String::new(),
                                        (LoopMode::Forward, _) => format!(", {held} held"),
                                    };
                                    self.status = format!(
                                        "GIF saved {} ({} frames, hidden skipped{extra}).",
                                        path.display(),
                                        exported
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
                if ui.button("Import GIF").clicked() {
                    #[cfg(not(target_os = "ios"))]
                    if let Some(path) = rfd::FileDialog::new().add_filter("GIF image", &["gif"]).pick_file()
                    {
                        match std::fs::read(&path)
                            .map_err(|e| e.to_string())
                            .and_then(|b| import_gif_document(&b))
                        {
                            Ok((doc, fps)) => {
                                self.status = format!(
                                    "GIF imported {} ({} frames, {} fps).",
                                    path.display(),
                                    doc.layers.len(),
                                    fps
                                );
                                self.adopt_gif(doc, fps);
                            }
                            Err(e) => self.status = format!("GIF import failed: {e}"),
                        }
                    }
                    #[cfg(target_os = "ios")]
                    {
                        let _ = rfd::FileDialog::new()
                            .add_filter("GIF image", &["gif"])
                            .pick_file();
                        self.pending_open = Some(PendingOpen::Gif);
                        self.status = "Pick GIF…".to_string();
                    }
                }
                // M41 Time-lapse: staged snapshots while recording
                // (session-only, capped small thumbs), exported as GIF.
                ui.separator();
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.lapse_on, "Time-lapse");
                    ui.label(format!("{} frames", self.lapse_frames.len()));
                });
                if !self.lapse_frames.is_empty() {
                    ui.horizontal_wrapped(|ui| {
                        if ui
                            .add_sized([110.0, 28.0], egui::Button::new("Export lapse"))
                            .clicked()
                        {
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter("GIF image", &["gif"])
                                .save_file()
                            {
                                let path = ensure_extension(path, "gif");
                                match export_timelapse(&self.lapse_frames, 10) {
                                    Ok(bytes) => match std::fs::write(&path, &bytes) {
                                        Ok(()) => {
                                            self.status = format!(
                                                "Time-lapse saved {} ({} frames).",
                                                path.display(),
                                                self.lapse_frames.len()
                                            );
                                            #[cfg(target_os = "ios")]
                                            rfd::ios_share_file(&path);
                                        }
                                        Err(e) => {
                                            self.status = format!("Time-lapse save failed: {e}")
                                        }
                                    },
                                    Err(e) => {
                                        self.status = format!("Time-lapse export failed: {e}")
                                    }
                                }
                            }
                        }
                        if ui
                            .add_sized([110.0, 28.0], egui::Button::new("Clear lapse"))
                            .clicked()
                        {
                            self.lapse_frames.clear();
                            self.status = "Time-lapse cleared.".to_string();
                        }
                    });
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
                if ui.button(if self.has_symbols { "⟲ −15°" } else { "−15°" }).clicked() {
                    self.doc.checkpoint();
                    self.doc.affine_layer(self.doc.active, 1.0, -15.0);
                }
                if ui.button(if self.has_symbols { "⟳ +15°" } else { "+15°" }).clicked() {
                    self.doc.checkpoint();
                    self.doc.affine_layer(self.doc.active, 1.0, 15.0);
                }
            });
            ui.horizontal(|ui| {
                if ui.button("Flip H").clicked() {
                    self.doc.checkpoint();
                    self.doc.flip_layer_horizontal(self.doc.active);
                    self.status = "Layer flipped horizontally.".to_string();
                }
                if ui.button("Flip V").clicked() {
                    self.doc.checkpoint();
                    self.doc.flip_layer_vertical(self.doc.active);
                    self.status = "Layer flipped vertically.".to_string();
                }
            });
            ui.separator();
            ui.heading("Symmetry");
            for m in SymmetryMode::ALL {
                if ui.radio_value(&mut self.doc.symmetry, m, m.name()).changed() {
                    self.sym_move = false;
                    self.sym_grab = false;
                    self.status = format!("Symmetry: {}.", m.name());
                }
            }
            if self.doc.symmetry != SymmetryMode::Off {
                if ui.checkbox(&mut self.sym_move, "Move center (drag it on canvas)").changed() {
                    self.sym_grab = false;
                    self.status = if self.sym_move {
                        "Drag the mirror center; painting pauses while armed.".to_string()
                    } else {
                        "Mirror-center move off.".to_string()
                    };
                }
                if ui.button("Reset center").clicked() {
                    self.doc.reset_symmetry_center();
                    self.status = "Mirror center reset to canvas middle.".to_string();
                }
            }
            if self.doc.symmetry == SymmetryMode::Kaleidoscope {
                stepped_slider(ui, &mut self.doc.symmetry_segments, 2..=16, 1.0, "Segments");
            }
            ui.separator();
            ui.heading("Filters");
            ui.label("Active layer. Sliders preview live; Apply keeps. Selections mask filters.");
            // Tone previews live from a snapshot (one undo step); Apply
            // keeps the preview, or applies once when nothing is pending.
            let tone_before = (self.filt_brightness, self.filt_contrast);
            stepped_slider_label_end(ui, &mut self.filt_brightness, -100..=100, 1.0, "Brightness");
            stepped_slider_label_end(ui, &mut self.filt_contrast, 0.0..=200.0, 1.0, "Contrast %");
            if (self.filt_brightness, self.filt_contrast) != tone_before {
                let (b, c) = (self.filt_brightness, self.filt_contrast);
                self.filter_scrub(0, move |doc, layer| {
                    doc.adjust_tone(layer, (b as f32 * 2.55).round() as i32, c / 100.0);
                });
                self.status = "Previewing tone — Apply to keep.".to_string();
            }
            if ui.button("Apply tone").clicked() {
                if self.filter_preview.as_ref().is_none_or(|fp| fp.kind != 0) {
                    let (b, c) = (self.filt_brightness, self.filt_contrast);
                    self.doc.checkpoint();
                    self.doc.adjust_tone(
                        self.doc.active,
                        (b as f32 * 2.55).round() as i32,
                        c / 100.0,
                    );
                } else {
                    // Throttled-stale preview (or a fresh one): stamp the
                    // latest values over the scrub snapshot — same undo step.
                    let (b, c) = (self.filt_brightness, self.filt_contrast);
                    self.restamp_preview(move |doc, layer| {
                        doc.adjust_tone(layer, (b as f32 * 2.55).round() as i32, c / 100.0);
                    });
                }
                self.filter_preview = None;
                self.status = "Tone applied.".to_string();
                self.capture_timelapse();
            }
            let blur_before = self.filt_blur;
            stepped_slider_label_end(ui, &mut self.filt_blur, 0..=25, 1.0, "Blur");
            if self.filt_blur != blur_before {
                let r = self.filt_blur.max(0) as u32;
                self.filter_scrub(1, move |doc, layer| {
                    doc.blur_layer(layer, r);
                });
                self.status = "Previewing blur — Apply to keep.".to_string();
            }
            if ui.button("Apply blur").clicked() {
                if self.filter_preview.as_ref().is_none_or(|fp| fp.kind != 1) {
                    self.doc.checkpoint();
                    self.doc.blur_layer(self.doc.active, self.filt_blur.max(0) as u32);
                } else {
                    let r = self.filt_blur.max(0) as u32;
                    self.restamp_preview(move |doc, layer| {
                        doc.blur_layer(layer, r);
                    });
                }
                self.filter_preview = None;
                self.status = "Blur applied.".to_string();
                self.capture_timelapse();
            }
            let sharp_before = self.filt_sharpen;
            stepped_slider_label_end(ui, &mut self.filt_sharpen, 0.0..=200.0, 1.0, "Sharpen %");
            if self.filt_sharpen != sharp_before {
                let s = self.filt_sharpen / 100.0;
                self.filter_scrub(2, move |doc, layer| {
                    doc.sharpen_layer(layer, s);
                });
                self.status = "Previewing sharpen — Apply to keep.".to_string();
            }
            if ui.button("Apply sharpen").clicked() {
                if self.filter_preview.as_ref().is_none_or(|fp| fp.kind != 2) {
                    self.doc.checkpoint();
                    self.doc.sharpen_layer(self.doc.active, self.filt_sharpen / 100.0);
                } else {
                    let s = self.filt_sharpen / 100.0;
                    self.restamp_preview(move |doc, layer| {
                        doc.sharpen_layer(layer, s);
                    });
                }
                self.filter_preview = None;
                self.status = "Sharpen applied.".to_string();
                self.capture_timelapse();
            }
            // M36 Levels: black/white points + mid gamma, same live
            // preview + Apply rhythm as tone (preview kind 3).
            let levels_before =
                (self.filt_levels_shadow, self.filt_levels_gamma, self.filt_levels_high);
            stepped_slider_label_end(ui, &mut self.filt_levels_shadow, 0..=255, 1.0, "Shadow");
            stepped_slider_label_end(ui, &mut self.filt_levels_gamma, 10.0..=400.0, 1.0, "Gamma %");
            stepped_slider_label_end(ui, &mut self.filt_levels_high, 0..=255, 1.0, "Highlight");
            if (self.filt_levels_shadow, self.filt_levels_gamma, self.filt_levels_high)
                != levels_before
            {
                let (s, g, h) =
                    (self.filt_levels_shadow, self.filt_levels_gamma / 100.0, self.filt_levels_high);
                let lut = levels_lut(s, g, h);
                self.filter_scrub(3, move |doc, layer| {
                    doc.apply_lut(layer, &lut);
                });
                self.status = "Previewing levels — Apply to keep.".to_string();
            }
            if ui.button("Apply levels").clicked() {
                if self.filter_preview.as_ref().is_none_or(|fp| fp.kind != 3) {
                    let lut = levels_lut(
                        self.filt_levels_shadow,
                        self.filt_levels_gamma / 100.0,
                        self.filt_levels_high,
                    );
                    self.doc.checkpoint();
                    self.doc.apply_lut(self.doc.active, &lut);
                } else {
                    let lut = levels_lut(
                        self.filt_levels_shadow,
                        self.filt_levels_gamma / 100.0,
                        self.filt_levels_high,
                    );
                    self.restamp_preview(move |doc, layer| {
                        doc.apply_lut(layer, &lut);
                    });
                }
                self.filter_preview = None;
                self.status = "Levels applied.".to_string();
                self.capture_timelapse();
            }
            // M36 Curves (parametric shadows/mids/highlights through a
            // pinned-endpoint LUT; preview kind 4).
            let curves_before = (self.filt_curve_shadow, self.filt_curve_mid, self.filt_curve_high);
            stepped_slider_label_end(ui, &mut self.filt_curve_shadow, -100..=100, 1.0, "Shadows");
            stepped_slider_label_end(ui, &mut self.filt_curve_mid, -100..=100, 1.0, "Midtones");
            stepped_slider_label_end(ui, &mut self.filt_curve_high, -100..=100, 1.0, "Highlights");
            if (self.filt_curve_shadow, self.filt_curve_mid, self.filt_curve_high) != curves_before
            {
                let (s, m, h) = (self.filt_curve_shadow, self.filt_curve_mid, self.filt_curve_high);
                let lut = curves_lut(s, m, h);
                self.filter_scrub(4, move |doc, layer| {
                    doc.apply_lut(layer, &lut);
                });
                self.status = "Previewing curves — Apply to keep.".to_string();
            }
            if ui.button("Apply curves").clicked() {
                if self.filter_preview.as_ref().is_none_or(|fp| fp.kind != 4) {
                    let lut =
                        curves_lut(self.filt_curve_shadow, self.filt_curve_mid, self.filt_curve_high);
                    self.doc.checkpoint();
                    self.doc.apply_lut(self.doc.active, &lut);
                } else {
                    let lut =
                        curves_lut(self.filt_curve_shadow, self.filt_curve_mid, self.filt_curve_high);
                    self.restamp_preview(move |doc, layer| {
                        doc.apply_lut(layer, &lut);
                    });
                }
                self.filter_preview = None;
                self.status = "Curves applied.".to_string();
                self.capture_timelapse();
            }
            // M37 HSL: hue spin + saturation scale + lightness shift
            // (preview kind 5). Grays only answer to lightness.
            let hsl_before = (self.filt_hue, self.filt_sat, self.filt_light);
            stepped_slider_label_end(ui, &mut self.filt_hue, -180.0..=180.0, 1.0, "Hue°");
            stepped_slider_label_end(ui, &mut self.filt_sat, 0.0..=400.0, 1.0, "Sat %");
            stepped_slider_label_end(ui, &mut self.filt_light, -100..=100, 1.0, "Light");
            if (self.filt_hue, self.filt_sat, self.filt_light) != hsl_before {
                let (h, s, l) = (self.filt_hue, self.filt_sat / 100.0, self.filt_light as f32 / 100.0);
                self.filter_scrub(5, move |doc, layer| {
                    doc.adjust_hsl(layer, h, s, l);
                });
                self.status = "Previewing HSL — Apply to keep.".to_string();
            }
            if ui.button("Apply HSL").clicked() {
                if self.filter_preview.as_ref().is_none_or(|fp| fp.kind != 5) {
                    self.doc.checkpoint();
                    let active = self.doc.active;
                    self.doc.adjust_hsl(
                        active,
                        self.filt_hue,
                        self.filt_sat / 100.0,
                        self.filt_light as f32 / 100.0,
                    );
                } else {
                    let (h, s, l) =
                        (self.filt_hue, self.filt_sat / 100.0, self.filt_light as f32 / 100.0);
                    self.restamp_preview(move |doc, layer| {
                        doc.adjust_hsl(layer, h, s, l);
                    });
                }
                self.filter_preview = None;
                self.status = "HSL applied.".to_string();
                self.capture_timelapse();
            }
            // M37 Color balance: − toward cyan/magenta/yellow, + toward
            // red/green/blue (preview kind 6).
            let bal_before = (self.filt_bal_r, self.filt_bal_g, self.filt_bal_b);
            stepped_slider_label_end(ui, &mut self.filt_bal_r, -100..=100, 1.0, "Red−Cyan");
            stepped_slider_label_end(ui, &mut self.filt_bal_g, -100..=100, 1.0, "Green−Magenta");
            stepped_slider_label_end(ui, &mut self.filt_bal_b, -100..=100, 1.0, "Blue−Yellow");
            if (self.filt_bal_r, self.filt_bal_g, self.filt_bal_b) != bal_before {
                let (r, g, b) = (
                    (self.filt_bal_r as f32 * 2.55).round() as i32,
                    (self.filt_bal_g as f32 * 2.55).round() as i32,
                    (self.filt_bal_b as f32 * 2.55).round() as i32,
                );
                self.filter_scrub(6, move |doc, layer| {
                    doc.color_balance(layer, r, g, b);
                });
                self.status = "Previewing balance — Apply to keep.".to_string();
            }
            if ui.button("Apply balance").clicked() {
                if self.filter_preview.as_ref().is_none_or(|fp| fp.kind != 6) {
                    let (r, g, b) = (
                        (self.filt_bal_r as f32 * 2.55).round() as i32,
                        (self.filt_bal_g as f32 * 2.55).round() as i32,
                        (self.filt_bal_b as f32 * 2.55).round() as i32,
                    );
                    self.doc.checkpoint();
                    self.doc.color_balance(self.doc.active, r, g, b);
                } else {
                    let (r, g, b) = (
                        (self.filt_bal_r as f32 * 2.55).round() as i32,
                        (self.filt_bal_g as f32 * 2.55).round() as i32,
                        (self.filt_bal_b as f32 * 2.55).round() as i32,
                    );
                    self.restamp_preview(move |doc, layer| {
                        doc.color_balance(layer, r, g, b);
                    });
                }
                self.filter_preview = None;
                self.status = "Balance applied.".to_string();
                self.capture_timelapse();
            }
            // M37 Recolor (colorize) toward the current paint color at
            // strength (preview kind 7). White stays white by definition.
            ui.label(format!(
                "Recolor target: #{:02X}{:02X}{:02X} (paint color).",
                self.color[0], self.color[1], self.color[2]
            ));
            let recolor_before = self.filt_recolor_strength;
            stepped_slider_label_end(ui, &mut self.filt_recolor_strength, 0.0..=100.0, 1.0, "Strength %");
            if self.filt_recolor_strength != recolor_before {
                let (target, s) = (self.color, self.filt_recolor_strength / 100.0);
                self.filter_scrub(7, move |doc, layer| {
                    doc.colorize(layer, target, s);
                });
                self.status = "Previewing recolor — Apply to keep.".to_string();
            }
            if ui.button("Apply recolor").clicked() {
                if self.filter_preview.as_ref().is_none_or(|fp| fp.kind != 7) {
                    self.doc.checkpoint();
                    let active = self.doc.active;
                    self.doc.colorize(active, self.color, self.filt_recolor_strength / 100.0);
                } else {
                    let (target, s) = (self.color, self.filt_recolor_strength / 100.0);
                    self.restamp_preview(move |doc, layer| {
                        doc.colorize(layer, target, s);
                    });
                }
                self.filter_preview = None;
                self.status = "Recolor applied.".to_string();
                self.capture_timelapse();
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
            // Pinch-to-zoom (iOS touch; Ctrl+wheel on desktop): no-op when
            // the backend reports no gesture.
            let pinch = ui.ctx().input(|i| i.zoom_delta());
            let fingers = ui.ctx().input(|i| i.multi_touch());
            let gesture = fingers.is_some();
            if (pinch - 1.0).abs() > f32::EPSILON {
                self.zoom = (self.zoom * pinch).clamp(0.1, 8.0);
            }
            // Two-finger drag pans the canvas. Touchscreens have no wheel
            // to scroll the view with, so the offset is applied manually
            // (desktop wheel-scroll leaves it at zero; a desktop trackpad
            // pinch could add to it, harmlessly — same direction).
            if let Some(m) = &fingers {
                let d = m.translation_delta;
                if d != egui::Vec2::ZERO {
                    self.view_offset += d;
                    let (cw, ch) = (w * self.zoom, h * self.zoom);
                    self.view_offset.x = self.view_offset.x.clamp(-cw + 60.0, avail.x - 60.0);
                    self.view_offset.y = self.view_offset.y.clamp(-ch + 60.0, avail.y - 60.0);
                }
                // Two-finger twist rotates the view about the canvas
                // center (paint math stays axis-aligned).
                if m.rotation_delta != 0.0 {
                    self.view_angle += m.rotation_delta;
                    if self.view_angle.abs() < 0.003 {
                        self.view_angle = 0.0;
                    }
                }
            }
            // A gesture aborts any in-progress stroke or drag so pinching
            // never paints, selects, moves, or grabs. A partially painted
            // stroke reverts (one undo step, from the stroke checkpoint).
            if gesture {
                if self.painting && self.stroke_painted {
                    self.stroke_painted = false;
                    if self.doc.undo() {
                        self.ghost_dirty = true;
                        self.status = "Pinch: stroke cancelled.".to_string();
                    }
                }
                self.painting = false;
                self.last = None;
                self.smooth_pt = None;
                self.assist_start = None;
                self.assist_lock = None;
                self.sel_anchor = None;
                self.sel_current = None;
                self.crop_anchor = None;
                self.crop_current = None;
                self.lasso_points.clear();
                self.shape_anchor = None;
                self.shape_current = None;
                self.move_from = None;
                self.liquify_from = None;
                self.fill_preview = None;
                self.vp_grab = None;
                self.sym_grab = false;
            }
            let z = self.zoom;
            egui::ScrollArea::both().show(ui, |ui| {
                let (rect, resp) =
                    ui.allocate_exact_size(Vec2::new(w * z, h * z), Sense::click_and_drag());
                // Pan applies to the drawn canvas and every overlay/mapping
                // below (sizes are translation-invariant).
                let rect = rect.translate(self.view_offset);
                // Rotation about the view center: canvas (x, y) maps through
                // the unrotated rect, then spins into place.
                let ang = self.view_angle;
                let cc = rect.center();
                let to_screen = |x: f32, y: f32| {
                    rot_pt(
                        egui::Pos2::new(rect.min.x + x * z, rect.min.y + y * z),
                        cc,
                        ang,
                    )
                };
                paint_textured_quad(
                    ui.painter(),
                    tex,
                    [
                        to_screen(0.0, 0.0),
                        to_screen(w, 0.0),
                        to_screen(w, h),
                        to_screen(0.0, h),
                    ],
                    Color32::WHITE,
                );
                let to_canvas = |pos: egui::Pos2| {
                    let q = unrot_pt(pos, cc, ang);
                    ((q.x - rect.min.x) / z, (q.y - rect.min.y) / z)
                };
                let clamp_pt = |x: f32, y: f32| (x.clamp(0.0, w - 1.0), y.clamp(0.0, h - 1.0));
                // Mirror axis overlay so the symmetry center is visible.
                let axis_color = Color32::from_rgba_unmultiplied(220, 40, 40, 180);
                let axis = |a: egui::Pos2, b: egui::Pos2| {
                    ui.painter().line_segment([a, b], (1.0, axis_color));
                };
                let (scx, scy) = self.doc.symmetry_center_px();
                let center = to_screen(scx, scy);
                let vline = || {
                    axis(to_screen(scx, -8.0), to_screen(scx, h + 8.0));
                };
                let hline = || {
                    axis(to_screen(-8.0, scy), to_screen(w + 8.0, scy));
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
                            axis(rot_pt(center + dir, cc, ang), rot_pt(center - dir, cc, ang));
                        }
                    }
                }
                // Mirror-center handle (draggable while Move center is
                // armed; off-canvas parks on the edge like VP handles).
                if self.doc.symmetry != SymmetryMode::Off {
                    let (p0, parked) = vp_handle_pos(rect, z, scx, scy);
                    let p = rot_pt(p0, cc, ang);
                    if parked {
                        let r = 10.0;
                        let diamond = vec![
                            p + egui::Vec2::new(0.0, -r),
                            p + egui::Vec2::new(r, 0.0),
                            p + egui::Vec2::new(0.0, r),
                            p + egui::Vec2::new(-r, 0.0),
                        ];
                        let red = Color32::from_rgb(220, 60, 60);
                        ui.painter()
                            .add(egui::Shape::closed_line(diamond, (3.0, red)));
                        ui.painter().circle_filled(p, 2.0, Color32::WHITE);
                    } else {
                        let (fill, r) = if self.sym_move {
                            (
                                Color32::from_rgb(220, 60, 60),
                                if self.sym_grab { 11.0 } else { 9.0 },
                            )
                        } else {
                            (Color32::from_rgb(220, 60, 60), 6.0)
                        };
                        ui.painter().circle_filled(p, r, fill);
                        ui.painter().circle_stroke(p, r, (2.0, Color32::WHITE));
                    }
                }
                // Drawing guides overlay: view-assist only, never baked in.
                if self.guides.kind != GuideKind::Off {
                    let alpha = (self.guides.opacity.clamp(0.05, 1.0) * 255.0).round() as u8;
                    let gc = Color32::from_rgba_unmultiplied(70, 130, 200, alpha);
                    for [x0, y0, x1, y1] in self.guides.overlay_lines(w, h) {
                        ui.painter()
                            .line_segment([to_screen(x0, y0), to_screen(x1, y1)], (1.0, gc));
                    }
                }
                // Drawing Assist: the engaged lock draws bright while the
                // stroke rides it (cleared on release).
                if let Some(lock) = self.assist_lock {
                    let reach = (rect.width().powi(2) + rect.height().powi(2)).sqrt();
                    let (tx, ty) = lock.through;
                    let (dx, dy) = (lock.dir.0 * reach, lock.dir.1 * reach);
                    ui.painter().line_segment(
                        [to_screen(tx - dx, ty - dy), to_screen(tx + dx, ty + dy)],
                        (1.5, Color32::from_rgb(255, 200, 60)),
                    );
                }
                // Vanishing-point handles for perspective guides (draggable
                // while Move VPs is armed in the Guides panel). Off-canvas
                // VPs park as hollow diamonds on the canvas edge so they
                // stay reachable.
                if matches!(self.guides.kind, GuideKind::OnePoint | GuideKind::TwoPoint) {
                    for (i, (vx, vy)) in self.guides.vp_list(w, h).iter().enumerate() {
                        let (p0, parked) = vp_handle_pos(rect, z, *vx, *vy);
                        let p = rot_pt(p0, cc, ang);
                        let grabbed = self.vp_grab == Some(i);
                        if parked {
                            let r = 10.0;
                            let diamond = vec![
                                p + egui::Vec2::new(0.0, -r),
                                p + egui::Vec2::new(r, 0.0),
                                p + egui::Vec2::new(0.0, r),
                                p + egui::Vec2::new(-r, 0.0),
                            ];
                            let amber = Color32::from_rgb(255, 200, 60);
                            ui.painter()
                                .add(egui::Shape::closed_line(diamond, (3.0, amber)));
                            ui.painter().circle_filled(p, 2.0, Color32::WHITE);
                        } else {
                            let (fill, r) = if self.vp_move {
                                (
                                    Color32::from_rgb(255, 200, 60),
                                    if grabbed { 11.0 } else { 9.0 },
                                )
                            } else {
                                (Color32::from_rgb(70, 130, 200), 6.0)
                            };
                            ui.painter().circle_filled(p, r, fill);
                            ui.painter().circle_stroke(p, r, (2.0, Color32::WHITE));
                        }
                    }
                }
                // Onion skins over the current frame (assist only, not in
                // mask view, hidden while playing so ghosts never tint
                // the page as frames change).
                if self.anim_assist && self.onion && !self.view_mask && !self.anim_playing {
                    self.refresh_ghosts(&ctx);
                    if let Some(tex) = &self.ghost_prev {
                        paint_textured_quad(
                            ui.painter(),
                            tex.id(),
                            [
                                to_screen(0.0, 0.0),
                                to_screen(w, 0.0),
                                to_screen(w, h),
                                to_screen(0.0, h),
                            ],
                            Color32::from_rgb(255, 130, 130),
                        );
                    }
                    if let Some(tex) = &self.ghost_next {
                        paint_textured_quad(
                            ui.painter(),
                            tex.id(),
                            [
                                to_screen(0.0, 0.0),
                                to_screen(w, 0.0),
                                to_screen(w, h),
                                to_screen(0.0, h),
                            ],
                            Color32::from_rgb(130, 165, 255),
                        );
                    }
                }
                // Text anchor crosshair while placing type.
                if self.tool == Tool::Text {
                    if let Some((ax, ay)) = self.text_anchor {
                        let p = to_screen(ax, ay);
                        for (color, width) in [(Color32::BLACK, 3.0), (Color32::WHITE, 1.5)] {
                            ui.painter().line_segment(
                                [
                                    p + egui::Vec2::new(-8.0, 0.0),
                                    p + egui::Vec2::new(8.0, 0.0),
                                ],
                                (width, color),
                            );
                            ui.painter().line_segment(
                                [
                                    p + egui::Vec2::new(0.0, -8.0),
                                    p + egui::Vec2::new(0.0, 8.0),
                                ],
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
                        .map(|&(x, y)| to_screen(x, y))
                        .collect();
                    paint_path(ui.painter(), &pts, true);
                }
                // Mask-edge marching ants on every tool: the cached edges
                // rebuild only when the selection changed (dirty flag).
                if self.sel_edges_dirty {
                    self.rebuild_sel_edges();
                    self.sel_edges_dirty = false;
                }
                if self.doc.selection_mask.is_some() && !self.sel_edges.is_empty() {
                    let t = ctx.input(|i| i.time);
                    paint_mask_edges(
                        ui.painter(),
                        &self.sel_edges,
                        |x, y| to_screen(x as f32, y as f32),
                        t,
                    );
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
                                self.status =
                                    format!("Picked #{:02X}{:02X}{:02X}.", rgb[0], rgb[1], rgb[2]);
                            }
                        }
                    }
                    // Same magnifier as the Pick tool, parked clear of the cursor.
                    if let Some(pos) = resp.hover_pos() {
                        let (x, y) = to_canvas(pos);
                        let (cx, cy) = clamp_pt(x, y);
                        self.paint_picker_loupe(ui.painter(), &ctx, pos, cx, cy);
                    }
                    self.painting = false;
                    self.last = None;
                    return;
                }

                // Vanishing-point dragging (armed in the Guides panel):
                // grabs pause painting and move the nearest handle.
                if self.vp_move
                    && matches!(self.guides.kind, GuideKind::OnePoint | GuideKind::TwoPoint)
                {
                    if resp.drag_started() && !gesture {
                        if let Some(pos) = resp.hover_pos() {
                            let mut best: Option<(usize, f32)> = None;
                            for (i, (vx, vy)) in self.guides.vp_list(w, h).iter().enumerate() {
                                // Parked edge markers grab exactly where drawn.
                                let (p0, _) = vp_handle_pos(rect, z, *vx, *vy);
                                let p = rot_pt(p0, cc, ang);
                                let d = (p.x - pos.x).hypot(p.y - pos.y);
                                if best.is_none_or(|(_, bd)| d < bd) {
                                    best = Some((i, d));
                                }
                            }
                            // Generous grab radius so off-center taps still
                            // catch a handle; misses simply paint nothing.
                            if let Some((i, d)) = best {
                                if d <= 28.0 {
                                    self.vp_grab = Some(i);
                                }
                            }
                        }
                    } else if resp.dragged() {
                        if let (Some(i), Some(pos)) = (self.vp_grab, resp.hover_pos()) {
                            let (x, y) = to_canvas(pos);
                            self.guides.set_vp(i, x, y, w, h);
                        }
                    }
                    if resp.drag_stopped() {
                        self.vp_grab = None;
                    }
                    self.painting = false;
                    self.last = None;
                    return;
                }

                // Mirror-center dragging (armed in the Symmetry panel):
                // grabs pause painting and move the axes' crossing.
                if self.sym_move && self.doc.symmetry != SymmetryMode::Off {
                    if resp.drag_started() && !gesture {
                        if let Some(pos) = resp.hover_pos() {
                            let (scx, scy) = self.doc.symmetry_center_px();
                            let (p0, _) = vp_handle_pos(rect, z, scx, scy);
                            let p = rot_pt(p0, cc, ang);
                            if (p.x - pos.x).hypot(p.y - pos.y) <= 28.0 {
                                self.sym_grab = true;
                            }
                        }
                    } else if resp.dragged() && self.sym_grab {
                        if let Some(pos) = resp.hover_pos() {
                            let (x, y) = to_canvas(pos);
                            self.doc.set_symmetry_center(x, y);
                        }
                    }
                    if resp.drag_stopped() {
                        self.sym_grab = false;
                    }
                    self.painting = false;
                    self.last = None;
                    return;
                }

                if self.tool == Tool::Select {
                    // Marquee selection.
                    if resp.drag_started() && !gesture {
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
                        if let (Some((ax, ay)), Some((bx, by))) =
                            (self.sel_anchor, self.sel_current)
                        {
                            let (x0, x1) = if ax <= bx {
                                (ax as u32, bx as u32)
                            } else {
                                (bx as u32, ax as u32)
                            };
                            let (y0, y1) = if ay <= by {
                                (ay as u32, by as u32)
                            } else {
                                (by as u32, ay as u32)
                            };
                            if x1.saturating_sub(x0) >= 2 && y1.saturating_sub(y0) >= 2 {
                                self.doc.selection = Some((x0, y0, x1, y1));
                                self.doc.selection_mask = None;
                                self.lasso_poly.clear();
                                self.sel_edges_dirty = true;
                                self.status =
                                    format!("Selected {}x{} px.", x1 - x0 + 1, y1 - y0 + 1);
                            } else {
                                self.doc.clear_selection();
                                self.sel_edges_dirty = true;
                            }
                        }
                        self.sel_anchor = None;
                        self.sel_current = None;
                    }
                    // Live rubber band + committed marching ants (mask edges
                    // when a graduated mask exists, bbox rect otherwise).
                    let band = match (self.sel_anchor, self.sel_current) {
                        (Some(a), Some(b)) => Some((a, b)),
                        _ => self.doc.selection.map(|(x0, y0, x1, y1)| {
                            ((x0 as f32, y0 as f32), ((x1 + 1) as f32, (y1 + 1) as f32))
                        }),
                    };
                    if self.sel_anchor.is_none() && self.doc.selection_mask.is_some() {
                        if self.sel_edges_dirty {
                            self.rebuild_sel_edges();
                            self.sel_edges_dirty = false;
                        }
                        if !self.sel_edges.is_empty() {
                            let t = ctx.input(|i| i.time);
                            paint_mask_edges(
                                ui.painter(),
                                &self.sel_edges,
                                |x, y| to_screen(x as f32, y as f32),
                                t,
                            );
                        }
                    } else if let Some(((ax, ay), (bx, by))) = band {
                        let (x0, x1) = if ax <= bx { (ax, bx) } else { (bx, ax) };
                        let (y0, y1) = if ay <= by { (ay, by) } else { (by, ay) };
                        paint_selection(
                            ui.painter(),
                            [
                                to_screen(x0, y0),
                                to_screen(x1, y0),
                                to_screen(x1, y1),
                                to_screen(x0, y1),
                            ],
                            ctx.input(|i| i.time),
                        );
                    }
                    self.painting = false;
                    self.last = None;
                    return;
                }

                if self.tool == Tool::Lasso {
                    // Freehand loop: collect points, build the mask on release.
                    if resp.drag_started() && !gesture {
                        if let Some(pos) = resp.hover_pos() {
                            let (x, y) = to_canvas(pos);
                            self.lasso_points = vec![clamp_pt(x, y)];
                        }
                    }
                    if resp.dragged() && !gesture {
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
                            .map(|&(x, y)| to_screen(x, y))
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
                        self.sel_edges_dirty = true;
                        self.lasso_points.clear();
                    }
                    self.painting = false;
                    self.last = None;
                    return;
                }

                if self.tool == Tool::Fill {
                    // ColorDrop: tap fills; keep holding and drag sideways to
                    // sweep the tolerance with a live preview (one undo step
                    // for the whole gesture). `clicked` covers motionless
                    // taps: a click-and-drag widget only reports a drag once
                    // the pointer moves.
                    if (resp.drag_started() || resp.clicked()) && !gesture {
                        if let Some(pos) = resp.hover_pos() {
                            let (x, y) = to_canvas(pos);
                            let (cx, cy) = clamp_pt(x, y);
                            let layer = self.doc.active;
                            let pixels = self
                                .doc
                                .layers
                                .get(layer)
                                .map(|l| l.pixels.clone())
                                .unwrap_or_default();
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
                            self.fill_preview = Some(FillPreview {
                                layer,
                                seed: (cx as u32, cy as u32),
                                pixels: pixels.clone(),
                                start_x: pos.x,
                                start_tol: self.fill_tolerance,
                                applied: self.fill_tolerance,
                                live_hash: pixels_hash(&pixels),
                            });
                            self.status = format!(
                                "Fill preview — drag sideways for tolerance ({}).",
                                self.fill_tolerance
                            );
                        }
                    } else if resp.dragged() && !gesture {
                        if let Some(mut fp) = self.fill_preview.take() {
                            // Same stale-snapshot guard as filters: if the
                            // layer changed underneath (paint, undo, layer
                            // op…), drop the sweep instead of wiping work.
                            let expired = match self.doc.layers.get(fp.layer) {
                                Some(l) => pixels_hash(&l.pixels) != fp.live_hash,
                                None => true,
                            };
                            if expired {
                                self.status = "Fill expired — layer changed.".to_string();
                            } else {
                                if let Some(pos) = resp.hover_pos() {
                                    let tol = (fp.start_tol as i32
                                        + ((pos.x - fp.start_x) * 0.5) as i32)
                                        .clamp(0, 255)
                                        as u8;
                                    if tol != fp.applied {
                                        if let Some(layer) = self.doc.layers.get_mut(fp.layer) {
                                            layer.pixels = fp.pixels.clone();
                                        }
                                        let brush = self.base_brush();
                                        let color = self.current_color();
                                        self.doc
                                            .flood_fill(&brush, color, fp.seed.0, fp.seed.1, tol);
                                        fp.applied = tol;
                                        self.fill_tolerance = tol;
                                        if let Some(layer) = self.doc.layers.get(fp.layer) {
                                            fp.live_hash = pixels_hash(&layer.pixels);
                                        }
                                        self.status = format!("Fill preview — tolerance {tol}.");
                                    }
                                }
                                // Keep the preview alive only while its layer exists.
                                if self.doc.layers.get(fp.layer).is_some() {
                                    self.fill_preview = Some(fp);
                                }
                            }
                        }
                    } else if resp.drag_stopped() {
                        if let Some(fp) = self.fill_preview.take() {
                            self.status = format!("Filled (tolerance {}).", fp.applied);
                            self.capture_timelapse();
                        }
                    }
                    self.painting = false;
                    self.last = None;
                    return;
                }

                if self.tool == Tool::Picker {
                    // Tap or drag to lift the composited color into the
                    // brush; the loupe shows a magnified ring so a finger
                    // never covers the target (touch-first, iPad-visible).
                    // `clicked` covers motionless taps (see the Fill note).
                    if (resp.drag_started() || resp.dragged() || resp.clicked()) && !gesture {
                        if let Some(pos) = resp.hover_pos() {
                            let (x, y) = to_canvas(pos);
                            let (cx, cy) = clamp_pt(x, y);
                            if let Some(rgb) = self.doc.sample(cx as u32, cy as u32) {
                                self.color = rgb;
                                self.status =
                                    format!("Picked #{:02X}{:02X}{:02X}.", rgb[0], rgb[1], rgb[2]);
                            }
                        }
                    }
                    // Loupe on hover and mid-drag alike.
                    if let Some(pos) = resp.hover_pos() {
                        let (x, y) = to_canvas(pos);
                        let (cx, cy) = clamp_pt(x, y);
                        self.paint_picker_loupe(ui.painter(), &ctx, pos, cx, cy);
                    }
                    self.painting = false;
                    self.last = None;
                    return;
                }

                if self.tool == Tool::Move {
                    // Drag the active layer around (integer steps).
                    if resp.drag_started() && !gesture {
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

                if self.tool == Tool::Liquify {
                    // M40 push: warp the active layer along each drag
                    // segment (one checkpoint per stroke, like paint).
                    if resp.drag_started() && !gesture {
                        if let Some(pos) = resp.hover_pos() {
                            self.doc.checkpoint();
                            self.liquify_from = Some(to_canvas(pos));
                        }
                    }
                    if self.liquify_from.is_some() && resp.dragged() {
                        if let Some(pos) = resp.hover_pos() {
                            let (x, y) = to_canvas(pos);
                            let (fx, fy) = self.liquify_from.unwrap();
                            let (dx, dy) = (x - fx, y - fy);
                            if dx != 0.0 || dy != 0.0 {
                                let active = self.doc.active;
                                let (size, strength) = (self.liquify_size, self.liquify_strength);
                                self.doc.liquify(active, x, y, dx, dy, size, strength);
                                self.ghost_dirty = true;
                                self.liquify_from = Some((x, y));
                            }
                        }
                    }
                    if resp.drag_stopped() {
                        self.liquify_from = None;
                        self.capture_timelapse();
                        self.status = "Liquified.".to_string();
                    }
                    self.painting = false;
                    self.last = None;
                    return;
                }

                if self.tool == Tool::Crop {
                    // Drag-crop rect: commits to `crop_rect` on release; Apply
                    // (in the Tools panel) crops, re-drag replaces, Esc cancels.
                    if resp.drag_started() && !gesture {
                        if let Some(pos) = resp.hover_pos() {
                            let (x, y) = to_canvas(pos);
                            self.crop_anchor = Some(clamp_pt(x, y));
                            self.crop_current = self.crop_anchor;
                        }
                    }
                    if self.crop_anchor.is_some() && resp.dragged() {
                        if let Some(pos) = resp.hover_pos() {
                            let (x, y) = to_canvas(pos);
                            self.crop_current = Some(clamp_pt(x, y));
                        }
                    }
                    if resp.drag_stopped() {
                        if let (Some((ax, ay)), Some((bx, by))) =
                            (self.crop_anchor, self.crop_current)
                        {
                            let (x0, x1) = if ax <= bx {
                                (ax as u32, bx as u32)
                            } else {
                                (bx as u32, ax as u32)
                            };
                            let (y0, y1) = if ay <= by {
                                (ay as u32, by as u32)
                            } else {
                                (by as u32, ay as u32)
                            };
                            if x1.saturating_sub(x0) >= 2 && y1.saturating_sub(y0) >= 2 {
                                self.crop_rect = Some((x0, y0, x1, y1));
                                self.status = format!(
                                    "Crop {}x{} — Apply or re-drag.",
                                    x1 - x0 + 1,
                                    y1 - y0 + 1
                                );
                            } else {
                                self.crop_rect = None;
                            }
                        }
                        self.crop_anchor = None;
                        self.crop_current = None;
                    }
                    // Live rubber band + committed rect (rotation-mapped like
                    // the selection overlay).
                    let band = match (self.crop_anchor, self.crop_current) {
                        (Some(a), Some(b)) => Some((a, b)),
                        _ => self.crop_rect.map(|(x0, y0, x1, y1)| {
                            ((x0 as f32, y0 as f32), ((x1 + 1) as f32, (y1 + 1) as f32))
                        }),
                    };
                    if let Some(((ax, ay), (bx, by))) = band {
                        let (x0, x1) = if ax <= bx { (ax, bx) } else { (bx, ax) };
                        let (y0, y1) = if ay <= by { (ay, by) } else { (by, ay) };
                        paint_selection(
                            ui.painter(),
                            [
                                to_screen(x0, y0),
                                to_screen(x1, y0),
                                to_screen(x1, y1),
                                to_screen(x0, y1),
                            ],
                            ctx.input(|i| i.time),
                        );
                    }
                    self.painting = false;
                    self.last = None;
                    return;
                }

                if self.tool == Tool::Shapes {
                    // Drag a preview, rasterize on release.
                    if resp.drag_started() && !gesture {
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
                    if let (Some((ax, ay)), Some((bx, by))) =
                        (self.shape_anchor, self.shape_current)
                    {
                        paint_selection(
                            ui.painter(),
                            [
                                to_screen(ax.min(bx), ay.min(by)),
                                to_screen(ax.max(bx), ay.min(by)),
                                to_screen(ax.max(bx), ay.max(by)),
                                to_screen(ax.min(bx), ay.max(by)),
                            ],
                            ctx.input(|i| i.time),
                        );
                    }
                    if resp.drag_stopped() {
                        if let (Some((ax, ay)), Some((bx, by))) =
                            (self.shape_anchor, self.shape_current)
                        {
                            self.doc.checkpoint();
                            let brush = self.base_brush();
                            let color = self.current_color();
                            match self.shape_kind {
                                ShapeKind::Line => {
                                    self.doc.shape_line(
                                        &brush,
                                        color,
                                        Some((ax, ay)),
                                        (bx, by),
                                        0.0,
                                    );
                                }
                                ShapeKind::Rect => {
                                    if self.shape_fill {
                                        self.doc.fill_rect(
                                            &brush, color, ax as u32, ay as u32, bx as u32,
                                            by as u32,
                                        );
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
                                        self.doc.fill_poly(
                                            &brush,
                                            color,
                                            ShapePoly::Triangle,
                                            ax,
                                            ay,
                                            bx,
                                            by,
                                            self.shape_sides,
                                        );
                                    } else {
                                        self.doc.shape_poly(
                                            &brush,
                                            color,
                                            ShapePoly::Triangle,
                                            ax,
                                            ay,
                                            bx,
                                            by,
                                            self.shape_sides,
                                        );
                                    }
                                }
                                ShapeKind::Diamond => {
                                    if self.shape_fill {
                                        self.doc.fill_poly(
                                            &brush,
                                            color,
                                            ShapePoly::Diamond,
                                            ax,
                                            ay,
                                            bx,
                                            by,
                                            self.shape_sides,
                                        );
                                    } else {
                                        self.doc.shape_poly(
                                            &brush,
                                            color,
                                            ShapePoly::Diamond,
                                            ax,
                                            ay,
                                            bx,
                                            by,
                                            self.shape_sides,
                                        );
                                    }
                                }
                                ShapeKind::Star => {
                                    if self.shape_fill {
                                        self.doc.fill_poly(
                                            &brush,
                                            color,
                                            ShapePoly::Star,
                                            ax,
                                            ay,
                                            bx,
                                            by,
                                            self.shape_sides,
                                        );
                                    } else {
                                        self.doc.shape_poly(
                                            &brush,
                                            color,
                                            ShapePoly::Star,
                                            ax,
                                            ay,
                                            bx,
                                            by,
                                            self.shape_sides,
                                        );
                                    }
                                }
                                ShapeKind::Arrow => {
                                    self.doc.shape_arrow(&brush, color, ax, ay, bx, by);
                                }
                                ShapeKind::Polygon => {
                                    if self.shape_fill {
                                        self.doc.fill_poly(
                                            &brush,
                                            color,
                                            ShapePoly::Polygon,
                                            ax,
                                            ay,
                                            bx,
                                            by,
                                            self.shape_sides,
                                        );
                                    } else {
                                        self.doc.shape_poly(
                                            &brush,
                                            color,
                                            ShapePoly::Polygon,
                                            ax,
                                            ay,
                                            bx,
                                            by,
                                            self.shape_sides,
                                        );
                                    }
                                }
                            }
                            self.status = format!("{} drawn.", self.shape_kind.name());
                            self.capture_timelapse();
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
                    if resp.clicked() && !gesture {
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

                if self.tool == Tool::Smudge {
                    // Finger-paint mixing: pick up color and push it along
                    // the path (mask layers steer here with a hint; the
                    // engine treats them as a no-op).
                    if resp.drag_started() && !gesture {
                        self.doc.checkpoint();
                        self.painting = true;
                        self.stroke_painted = false;
                        // Smudging edits the current frame: pause playback so
                        // the viewed frame never goes stale mid-stroke.
                        if self.anim_playing {
                            self.anim_playing = false;
                            self.status = "Paused for smudging.".to_string();
                        } else if self.doc.paint_target == PaintTarget::Mask {
                            self.status = "Smudge works on color layers.".to_string();
                        }
                        self.last = None;
                        self.leftover = 0.0;
                    }
                    if self.painting && resp.dragged() {
                        if let Some(pos) = resp.hover_pos() {
                            let (x, y) = to_canvas(pos);
                            let (x, y) = clamp_pt(x, y);
                            let pressure = latest_touch_force(&ctx).unwrap_or(1.0);
                            let (last, leftover) = self.doc.smudge_to(
                                &self.brush,
                                self.smudge_strength,
                                pressure,
                                self.last,
                                (x, y),
                                self.leftover,
                            );
                            self.last = last;
                            self.leftover = leftover;
                            self.stroke_painted = true;
                        }
                    }
                    if resp.drag_stopped() {
                        if self.stroke_painted {
                            self.stroke_painted = false;
                            self.capture_timelapse();
                        }
                        self.painting = false;
                        self.last = None;
                    }
                    return;
                }

                if resp.drag_started() && !gesture {
                    self.doc.checkpoint();
                    self.painting = true;
                    self.stroke_painted = false;
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
                    // Drawing Assist: remember the stroke start; the lock
                    // engages on first travel and projects every dab after.
                    if self.assist && self.guides.kind != GuideKind::Off {
                        if let Some(pos) = resp.hover_pos() {
                            let (x, y) = to_canvas(pos);
                            self.assist_start = Some(clamp_pt(x, y));
                        } else {
                            self.assist_start = None;
                        }
                        self.assist_lock = None;
                    } else {
                        self.assist_start = None;
                        self.assist_lock = None;
                    }
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
                            Some((px, py)) => {
                                (px + (x - px) * (1.0 - a), py + (y - py) * (1.0 - a))
                            }
                            None => (x, y),
                        };
                        self.smooth_pt = Some((sx, sy));
                        // Drawing Assist: ride an engaged ray/axis lock,
                        // else fall back to guide point-snap (shapes stay
                        // free). Projected points re-clamp so rays running
                        // off-canvas never streak the edge.
                        let (sx, sy) = if self.assist && self.guides.kind != GuideKind::Off {
                            match self.assist_lock {
                                Some(lock) => {
                                    let (px, py) = lock.project(sx, sy);
                                    clamp_pt(px, py)
                                }
                                None => {
                                    let lock = self
                                        .assist_start
                                        .and_then(|s| self.guides.assist_lock(w, h, s, (sx, sy)));
                                    match lock {
                                        Some(l) => {
                                            self.assist_lock = Some(l);
                                            let (px, py) = l.project(sx, sy);
                                            clamp_pt(px, py)
                                        }
                                        None => self.guides.snap_point(w, h, sx, sy),
                                    }
                                }
                            }
                        } else {
                            self.guides.snap_point(w, h, sx, sy)
                        };
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
                        let from_dyn = if self.last.is_some() {
                            self.last_dyn
                        } else {
                            DabDynamics::default()
                        };
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
                        self.stroke_painted = true;
                    }
                }
                if resp.drag_stopped() {
                    if self.stroke_painted {
                        self.stroke_painted = false;
                        self.capture_timelapse();
                    }
                    self.painting = false;
                    self.last = None;
                    self.smooth_pt = None;
                    self.assist_start = None;
                    self.assist_lock = None;
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
    Some(egui::IconData {
        rgba: img.into_raw(),
        width: w,
        height: h,
    })
}

fn build_options() -> eframe::NativeOptions {
    let mut viewport = egui::ViewportBuilder::default().with_inner_size([1400.0, 900.0]);
    if let Some(icon) = app_icon() {
        viewport = viewport.with_icon(icon);
    }
    // Disk persistence for window rect, panel widths (by panel id), and
    // the UI state saved in `App::save` (theme, panels, zoom). Mobile
    // keeps it off (same as iOS: sandboxed, session-only).
    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    let persistence_path = std::env::var_os("APPDATA")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(std::path::PathBuf::from))
        .map(|mut dir| {
            dir.push("procraft");
            let _ = std::fs::create_dir_all(&dir);
            dir.push("ui.json");
            dir
        });
    #[cfg(any(target_os = "ios", target_os = "android"))]
    let persistence_path = None;
    eframe::NativeOptions {
        viewport,
        persistence_path,
        ..Default::default()
    }
}

fn run_with(options: eframe::NativeOptions) {
    let _ = eframe::run_native(
        "ProCraft",
        options,
        Box::new(|cc| Ok(Box::new(ProCraftApp::new(cc)))),
    );
}

/// Desktop/iOS entry (the `[[bin]]` wrapper calls this).
pub fn run() {
    run_with(build_options());
}

/// Android entry: `NativeActivity` spawns this on its own thread (not
/// `main`); the `AndroidApp` handle is required for winit's event loop.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: android_activity::AndroidApp) {
    let mut options = build_options();
    options.android_app = Some(app);
    run_with(options);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixels_hash_sees_any_change() {
        // Empty and identical buffers hash equal (length included).
        assert_eq!(pixels_hash(&[]), pixels_hash(&[]));
        let a = vec![7u8; 4096];
        assert_eq!(pixels_hash(&a), pixels_hash(&a.clone()));
        // One dab-sized byte flip anywhere (head, middle, tail) flips it.
        for i in [0usize, 1000, 2048, 4095] {
            let mut b = a.clone();
            b[i] = b[i].wrapping_add(1);
            assert_ne!(pixels_hash(&a), pixels_hash(&b), "byte {i}");
        }
        // Length alone distinguishes.
        assert_ne!(pixels_hash(&a), pixels_hash(&a[..2048]));
    }
}
