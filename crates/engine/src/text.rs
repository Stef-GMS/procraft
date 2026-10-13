//! Text tool: rasterize the bundled Inter typeface into coverage masks
//! and stamp them onto the active layer.
//!
//! Original clean-room code. V1 bakes glyphs to layer pixels (undoable via
//! the caller's checkpoint); live editable text layers are a later track.
//! Like shapes and fills, text always paints color (never masks) and is
//! never mirrored by symmetry — mirrored type reads wrong.
//!
//! Font bytes: `assets/fonts/Inter-400-latin.ttf`, SIL Open Font License
//! (see `assets/fonts/OFL.txt`).

use super::brush::Color;
use super::document::Document;

/// Bundled typeface bytes (Inter Regular, latin subset, OFL).
pub const FONT_BYTES: &[u8] = include_bytes!("../../../assets/fonts/Inter-400-latin.ttf");
/// Glyph size bounds in canvas pixels.
pub const MIN_TEXT_PX: f32 = 8.0;
pub const MAX_TEXT_PX: f32 = 256.0;
/// Input caps (bound layout memory; the layout itself is capped below too).
pub const MAX_TEXT_CHARS: usize = 1024;
pub const MAX_TEXT_LINES: usize = 64;
/// Layout bitmap cap per side (canvases are far smaller in practice).
pub const MAX_LAYOUT_SIDE: u32 = 2048;

/// A laid-out text block: row-major coverage mask (0 = clear, 255 = full).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextLayout {
    pub w: u32,
    pub h: u32,
    pub mask: Vec<u8>,
}

fn load_font() -> Result<fontdue::Font, String> {
    fontdue::Font::from_bytes(FONT_BYTES, fontdue::FontSettings::default())
        .map_err(|e| format!("bundled font failed to parse: {e:?}"))
}

/// Lay out `text` (with `\n` line breaks) at `size_px` canvas pixels.
/// Returns the coverage mask. Validates emptiness, size and caps.
pub fn layout_text(text: &str, size_px: f32) -> Result<TextLayout, String> {
    if !(MIN_TEXT_PX..=MAX_TEXT_PX).contains(&size_px) {
        return Err(format!("bad text size: {size_px}"));
    }
    if text.chars().count() > MAX_TEXT_CHARS {
        return Err(format!("text too long (max {MAX_TEXT_CHARS} chars)"));
    }
    let lines: Vec<&str> = text.split('\n').collect();
    if lines.len() > MAX_TEXT_LINES {
        return Err(format!("too many lines (max {MAX_TEXT_LINES})"));
    }
    if !text
        .chars()
        .any(|c| c != '\n' && c != '\r' && !c.is_whitespace())
    {
        return Err("nothing to render: text is blank".to_string());
    }
    let font = load_font()?;
    let lm = font
        .horizontal_line_metrics(size_px)
        .unwrap_or(fontdue::LineMetrics {
            ascent: size_px * 0.8,
            descent: -size_px * 0.2,
            line_gap: 0.0,
            new_line_size: size_px,
        });
    let line_height = (lm.new_line_size).max(1.0);
    let total_h = ((lines.len() as f32 * line_height).ceil() as u32).max(1);
    if total_h > MAX_LAYOUT_SIDE {
        return Err("text block too tall".to_string());
    }
    // First pass: advances (+ kerning) give each line's width.
    let mut widths = Vec::with_capacity(lines.len());
    for line in &lines {
        let mut pen = 0.0f32;
        let mut prev: Option<char> = None;
        for c in line.chars() {
            if c == '\r' {
                continue;
            }
            if c == '\t' {
                let space = font.metrics(' ', size_px).advance_width;
                let tab = (space * 4.0).max(1.0);
                pen = ((pen / tab).floor() + 1.0) * tab;
                prev = None;
                continue;
            }
            if let Some(p) = prev {
                pen += font.horizontal_kern(p, c, size_px).unwrap_or(0.0);
            }
            pen += font.metrics(c, size_px).advance_width;
            prev = Some(c);
        }
        widths.push(pen);
    }
    let total_w = (widths.into_iter().fold(0.0f32, f32::max).ceil() as u32).max(1);
    if total_w > MAX_LAYOUT_SIDE {
        return Err("text block too wide".to_string());
    }
    // Second pass: blit glyph bitmaps. Pen origin is the baseline start;
    // bitmap top row sits at baseline - ymin - height (fontdue Metrics:
    // xmin = left-edge offset, ymin = bottom-edge offset, y grows down
    // on canvas).
    let mut mask = vec![0u8; (total_w * total_h) as usize];
    for (li, line) in lines.iter().enumerate() {
        let baseline = line_height * li as f32 + lm.ascent;
        let mut pen = 0.0f32;
        let mut prev: Option<char> = None;
        for c in line.chars() {
            if c == '\r' {
                continue;
            }
            if c == '\t' {
                let space = font.metrics(' ', size_px).advance_width;
                let tab = (space * 4.0).max(1.0);
                pen = ((pen / tab).floor() + 1.0) * tab;
                prev = None;
                continue;
            }
            if let Some(p) = prev {
                pen += font.horizontal_kern(p, c, size_px).unwrap_or(0.0);
            }
            let (m, bmp) = font.rasterize(c, size_px);
            if m.width > 0 && m.height > 0 && bmp.len() == m.width * m.height {
                let left = (pen + m.xmin as f32).round() as i32;
                let top = (baseline - m.ymin as f32 - m.height as f32).round() as i32;
                for (gy, row) in bmp.chunks(m.width).enumerate() {
                    for (gx, &cov) in row.iter().enumerate() {
                        if cov == 0 {
                            continue;
                        }
                        let (px, py) = (left + gx as i32, top + gy as i32);
                        if px < 0 || py < 0 || px >= total_w as i32 || py >= total_h as i32 {
                            continue;
                        }
                        let o = (py as u32 * total_w + px as u32) as usize;
                        if cov > mask[o] {
                            mask[o] = cov;
                        }
                    }
                }
            }
            pen += m.advance_width;
            prev = Some(c);
        }
    }
    Ok(TextLayout {
        w: total_w,
        h: total_h,
        mask,
    })
}

impl Document {
    /// Stamp laid-out text with its top-left at canvas pixel `(x, y)`,
    /// source-over in `color` at full opacity. Honors the active selection;
    /// ignores symmetry and always paints color (see module docs). Clips to
    /// the canvas instead of failing. Caller checkpoints first for undo.
    /// Returns the block size in pixels.
    pub fn draw_text(
        &mut self,
        color: Color,
        x: f32,
        y: f32,
        size_px: f32,
        text: &str,
    ) -> Result<(u32, u32), String> {
        let layout = layout_text(text, size_px)?;
        let (w_u32, h_u32) = (self.w, self.h);
        if self.layers.get(self.active).is_none() {
            return Err("no active layer".to_string());
        }
        let ox = x.floor() as i32;
        let oy = y.floor() as i32;
        // Fully outside: a complete no-op (mirrors the dab contract).
        if ox >= w_u32 as i32
            || oy >= h_u32 as i32
            || ox + layout.w as i32 <= 0
            || oy + layout.h as i32 <= 0
        {
            return Ok((layout.w, layout.h));
        }
        let x0 = ox.max(0) as u32;
        let y0 = oy.max(0) as u32;
        let x1 = (ox + layout.w as i32 - 1).min(w_u32 as i32 - 1).max(0) as u32;
        let y1 = (oy + layout.h as i32 - 1).min(h_u32 as i32 - 1).max(0) as u32;
        self.mark_dirty(x0, y0, x1, y1);
        let selection = self.selection;
        let active = self.active;
        let sel_mask = self.selection_mask.as_deref();
        // Layer offset (Move tool): canvas pixel -> buffer slot.
        let (lox, loy) = (self.layers[active].ox, self.layers[active].oy);
        let layer = &mut self.layers[active];
        let c = color.0;
        for py in y0..=y1 {
            for px in x0..=x1 {
                if let Some((sx0, sy0, sx1, sy1)) = selection {
                    if px < sx0 || px > sx1 || py < sy0 || py > sy1 {
                        continue;
                    }
                }
                if let Some(m) = sel_mask {
                    if m.get((py * w_u32 + px) as usize).copied().unwrap_or(0) == 0 {
                        continue;
                    }
                }
                let lx = (px as i32 - ox) as u32;
                let ly = (py as i32 - oy) as u32;
                let cov = layout.mask[(ly * layout.w + lx) as usize] as f32 / 255.0;
                if cov <= 0.0 {
                    continue;
                }
                // Store at the layer-offset slot (see brush dabs).
                let (bx, by) = (px as i32 - lox, py as i32 - loy);
                if bx < 0 || by < 0 || bx >= w_u32 as i32 || by >= h_u32 as i32 {
                    continue;
                }
                let o = ((by as u32 * w_u32 + bx as u32) * 4) as usize;
                if o + 3 >= layer.pixels.len() {
                    continue;
                }
                let dst_a = layer.pixels[o + 3] as f32 / 255.0;
                let out_a = cov + dst_a * (1.0 - cov);
                if out_a > 0.0 {
                    layer.pixels[o] = ((c[0] as f32 * cov
                        + layer.pixels[o] as f32 * dst_a * (1.0 - cov))
                        / out_a)
                        .round() as u8;
                    layer.pixels[o + 1] = ((c[1] as f32 * cov
                        + layer.pixels[o + 1] as f32 * dst_a * (1.0 - cov))
                        / out_a)
                        .round() as u8;
                    layer.pixels[o + 2] = ((c[2] as f32 * cov
                        + layer.pixels[o + 2] as f32 * dst_a * (1.0 - cov))
                        / out_a)
                        .round() as u8;
                    layer.pixels[o + 3] = (out_a * 255.0).round() as u8;
                }
            }
        }
        Ok((layout.w, layout.h))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_font_parses_and_renders() {
        let font = load_font().unwrap();
        assert!(font.glyph_count() > 100);
        let layout = layout_text("Ag", 48.0).unwrap();
        assert!(layout.w > 10 && layout.h > 10);
        assert!(layout.mask.iter().any(|&b| b > 128));
        // Deterministic across calls.
        assert_eq!(layout, layout_text("Ag", 48.0).unwrap());
    }

    #[test]
    fn layout_validates_input() {
        assert!(layout_text("", 32.0).is_err());
        assert!(layout_text("   \n  ", 32.0).is_err());
        assert!(layout_text("hi", 0.0).is_err());
        assert!(layout_text("hi", 1000.0).is_err());
        assert!(layout_text(&"x".repeat(MAX_TEXT_CHARS + 1), 16.0).is_err());
        assert!(layout_text(&"x\n".repeat(MAX_TEXT_LINES + 1), 16.0).is_err());
    }

    #[test]
    fn multiline_is_taller_than_single() {
        let one = layout_text("Hello", 32.0).unwrap();
        let two = layout_text("Hello\nWorld", 32.0).unwrap();
        assert!(two.w >= one.w);
        assert!(two.h > one.h);
        assert!(
            two.mask.iter().filter(|&&b| b > 0).count()
                > one.mask.iter().filter(|&&b| b > 0).count()
        );
    }

    #[test]
    fn draw_text_paints_and_clips() {
        use super::super::brush::Brush;
        let mut doc = Document::new(64, 64);
        doc.checkpoint();
        let (w, h) = doc
            .draw_text(Color::rgb(255, 0, 0), 4.0, 4.0, 24.0, "Hi")
            .unwrap();
        assert!(w > 0 && h > 0);
        assert!(doc.layers[0].pixels.iter().any(|&b| b != 0));
        // Half off-canvas: clips, never panics.
        let mut doc2 = Document::new(64, 64);
        doc2.checkpoint();
        doc2.draw_text(Color::rgb(0, 255, 0), -20.0, -20.0, 48.0, "Clip")
            .unwrap();
        // Fully outside: no-op, no pixels, no panic.
        let mut doc3 = Document::new(64, 64);
        doc3.checkpoint();
        doc3.draw_text(Color::rgb(0, 0, 255), 500.0, 500.0, 24.0, "Far")
            .unwrap();
        assert!(doc3.layers[0].pixels.iter().all(|&b| b == 0));
        let _ = Brush::default();
    }

    #[test]
    fn draw_text_respects_selection_and_undo() {
        let mut doc = Document::new(64, 64);
        // Selection far from the text: nothing paints.
        doc.selection = Some((48, 48, 63, 63));
        doc.checkpoint();
        doc.draw_text(Color::rgb(255, 0, 0), 2.0, 2.0, 20.0, "Nope")
            .unwrap();
        assert!(doc.layers[0].pixels.iter().all(|&b| b == 0));
        // Undo restores a painted stroke.
        doc.selection = None;
        doc.checkpoint();
        doc.draw_text(Color::rgb(255, 0, 0), 2.0, 2.0, 20.0, "Yep")
            .unwrap();
        assert!(doc.layers[0].pixels.iter().any(|&b| b != 0));
        assert!(doc.undo());
        assert!(doc.layers[0].pixels.iter().all(|&b| b == 0));
    }
}
