//! ProCraft engine: layered raster documents, a soft-round brush, undo and
//! PNG import/export. Original code; no GUI types in this crate.

mod brush;
mod document;
mod probrush;

pub use brush::{open_brush_library, presets, save_brush_library, Brush, BrushPreset, BrushTip, Color, Stroke, MAX_LIBRARY_BRUSHES, MAX_TIP_SIDE};
pub use document::{BlendMode, Document, Layer, PaintTarget, SymmetryMode};
pub use probrush::{export_tip_png, import_brush_file, ImportedBrush};
