//! ProCraft engine: layered raster documents, a soft-round brush, undo and
//! PNG import/export. Original code; no GUI types in this crate.

mod brush;
mod document;

pub use brush::{presets, Brush, Color, Stroke};
pub use document::{BlendMode, Document, Layer, SymmetryMode};
