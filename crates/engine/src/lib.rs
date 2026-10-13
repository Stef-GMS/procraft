//! ProCraft engine by surfrpt1 (https://github.com/surfrpt1).
//! Layered raster documents, a soft-round brush, undo and PNG import/export.
//! Original clean-room code, MIT licensed; no GUI types in this crate.

mod animation;
mod brush;
mod document;
mod guides;
mod palette;
mod probrush;
mod psd;
mod text;

pub use animation::{
    clamp_frame, export_gif, next_frame, onion_ghost, prev_frame, rasterize_frame, rasterize_frame_region,
    MAX_ANIM_FPS, MAX_GIF_FRAMES, MIN_ANIM_FPS,
};
pub use brush::{open_brush_library, presets, save_brush_library, Brush, BrushPreset, BrushTip, Color, DabDynamics, ShapePoly, Stroke, MAX_LIBRARY_BRUSHES, MAX_TIP_SIDE};
pub use document::{BlendMode, Document, Layer, PaintTarget, SymmetryMode};
pub use guides::{GuideKind, Guides};
pub use palette::{analogous, complementary, hsl_to_rgb, import_gpl_file, import_gpl_text, open_palette, rgb_to_hsl, save_palette, starter_palette, triadic, Palette, MAX_PALETTE_COLORS};
pub use probrush::{export_tip_png, import_brush_file, ImportedBrush};
pub use psd::{open_psd, save_psd};
pub use text::{layout_text, TextLayout, FONT_BYTES, MAX_LAYOUT_SIDE, MAX_TEXT_CHARS, MAX_TEXT_LINES, MAX_TEXT_PX, MIN_TEXT_PX};
