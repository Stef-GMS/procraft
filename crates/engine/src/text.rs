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
    parse_font(FONT_BYTES).map_err(|e| format!("bundled font failed to parse: {e}"))
}

fn parse_font(bytes: &[u8]) -> Result<fontdue::Font, String> {
    fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
        .map_err(|e| format!("font failed to parse: {e:?}"))
}

/// Validate imported TTF/OTF bytes (shared by the desktop Import arm
/// and the iOS picker resolve path). Returns Ok for any fontdue-readable
/// face; the app stores the bytes on success.
pub fn check_font(bytes: &[u8]) -> Result<(), String> {
    parse_font(bytes).map(|_| ())
}

/// M38 Text style: custom typeface + spacing + outline. `Default` is the
/// old behavior exactly (bundled Inter, no extra spacing, no outline).
#[derive(Clone, Debug, Default)]
pub struct TextStyle {
    /// TTF/OTF bytes; None = bundled Inter.
    pub font_bytes: Option<Vec<u8>>,
    /// Extra px between baselines (negative tightens), clamped ±.
    pub leading: f32,
    /// Extra px per glyph advance (negative tightens), clamped ±.
    pub tracking: f32,
    /// Outline color + width px (1..=32), painted under the fill.
    pub outline: Option<(Color, u32)>,
}

/// Spacing clamp range in px (about a 256px line either way).
pub const MAX_TEXT_SPACING: f32 = 256.0;
/// Outline width clamp in px.
pub const MAX_OUTLINE_PX: u32 = 32;

/// Lay out `text` (with `\n` line breaks) at `size_px` canvas pixels.
/// Returns the coverage mask. Validates emptiness, size and caps.
pub fn layout_text(text: &str, size_px: f32) -> Result<TextLayout, String> {
    layout_text_styled(text, size_px, &TextStyle::default())
}

/// Styled layout: `style` adds a custom face, leading/tracking, and (for
/// the outline pass) pads the block so the ring never clips at the edge.
pub fn layout_text_styled(
    text: &str,
    size_px: f32,
    style: &TextStyle,
) -> Result<TextLayout, String> {
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
    let font = match &style.font_bytes {
        Some(bytes) => parse_font(bytes)?,
        None => load_font()?,
    };
    let leading = style.leading.clamp(-MAX_TEXT_SPACING, MAX_TEXT_SPACING);
    let tracking = style.tracking.clamp(-MAX_TEXT_SPACING, MAX_TEXT_SPACING);
    // Outline pad: the ring dilates outward, so the block grows to hold it.
    let pad = style.outline.map_or(0, |(_, w)| w.min(MAX_OUTLINE_PX));
    let lm = font
        .horizontal_line_metrics(size_px)
        .unwrap_or(fontdue::LineMetrics {
            ascent: size_px * 0.8,
            descent: -size_px * 0.2,
            line_gap: 0.0,
            new_line_size: size_px,
        });
    let line_height = (lm.new_line_size + leading).max(1.0);
    let total_h = ((lines.len() as f32 * line_height).ceil() as u32).max(1) + pad * 2;
    if total_h > MAX_LAYOUT_SIDE {
        return Err("text block too tall".to_string());
    }
    // First pass: advances (+ kerning + tracking) give each line's width.
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
            pen += font.metrics(c, size_px).advance_width + tracking;
            prev = Some(c);
        }
        widths.push(pen);
    }
    let total_w = (widths.into_iter().fold(0.0f32, f32::max).ceil() as u32).max(1) + pad * 2;
    if total_w > MAX_LAYOUT_SIDE {
        return Err("text block too wide".to_string());
    }
    // Second pass: blit glyph bitmaps. Pen origin is the baseline start;
    // bitmap top row sits at baseline - ymin - height (fontdue Metrics:
    // xmin = left-edge offset, ymin = bottom-edge offset, y grows down
    // on canvas). The outline pad offsets every blit.
    let pad_i = pad as i32;
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
                let left = (pen + m.xmin as f32).round() as i32 + pad_i;
                let top = (baseline - m.ymin as f32 - m.height as f32).round() as i32 + pad_i;
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
            pen += m.advance_width + tracking;
            prev = Some(c);
        }
    }
    Ok(TextLayout {
        w: total_w,
        h: total_h,
        mask,
    })
}

/// Outline ring: pixels within `r` (Chebyshev) of a nonzero coverage
/// pixel that are themselves clear, at full strength. Two-pass windowed
/// max (edge-replicated); text blocks are small, so the window scan is
/// instant, and wide outlines avoid the naive quadratic blowup.
fn dilate_ring(mask: &[u8], w: u32, h: u32, r: u32) -> Vec<u8> {
    if r == 0 || w == 0 || h == 0 {
        return vec![0u8; (w * h) as usize];
    }
    let (w, h, r) = (w as usize, h as usize, r as usize);
    // Horizontal max into tmp, then vertical max into out.
    let mut tmp = vec![0u8; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut m = 0u8;
            for dx in 0..=2 * r {
                let xi = (x + dx).saturating_sub(r);
                if xi >= w {
                    break;
                }
                m = m.max(mask[y * w + xi]);
            }
            tmp[y * w + x] = m;
        }
    }
    let mut out = vec![0u8; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut m = 0u8;
            for dy in 0..=2 * r {
                let yi = (y + dy).saturating_sub(r);
                if yi >= h {
                    break;
                }
                m = m.max(tmp[yi * w + x]);
            }
            out[y * w + x] = m;
        }
    }
    // Ring only: dilated but originally clear.
    for (d, s) in out.iter_mut().zip(mask.iter()) {
        *d = if *s == 0 && *d > 0 { 255 } else { 0 };
    }
    out
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
        self.draw_text_styled(color, x, y, size_px, text, &TextStyle::default())
    }

    /// Styled text: `style` adds a custom face, leading/tracking, and an
    /// outline ring painted under the fill (same stamp path, so selection
    /// and clipping behave alike). Returns the fill block size in pixels.
    pub fn draw_text_styled(
        &mut self,
        color: Color,
        x: f32,
        y: f32,
        size_px: f32,
        text: &str,
        style: &TextStyle,
    ) -> Result<(u32, u32), String> {
        let layout = layout_text_styled(text, size_px, style)?;
        if let Some((o_color, o_w)) = style.outline {
            let ow = o_w.min(MAX_OUTLINE_PX);
            if ow > 0 {
                let ring = dilate_ring(&layout.mask, layout.w, layout.h, ow);
                let ring_layout = TextLayout {
                    w: layout.w,
                    h: layout.h,
                    mask: ring,
                };
                self.stamp_layout(&ring_layout, x, y, o_color)?;
            }
        }
        self.stamp_layout(&layout, x, y, color)?;
        Ok((layout.w, layout.h))
    }

    /// Source-over stamp of a coverage layout at canvas pixel `(x, y)`.
    /// Shared by the fill and outline passes. Fully-outside stamps are a
    /// no-op (mirrors the dab contract).
    fn stamp_layout(
        &mut self,
        layout: &TextLayout,
        x: f32,
        y: f32,
        color: Color,
    ) -> Result<(), String> {
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
            return Ok(());
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
        Ok(())
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

    #[test]
    fn styled_default_matches_plain() {
        let plain = layout_text("Ag", 48.0).unwrap();
        assert_eq!(
            plain,
            layout_text_styled("Ag", 48.0, &TextStyle::default()).unwrap()
        );
        // Explicit bundled bytes take the same path.
        let styled = TextStyle {
            font_bytes: Some(FONT_BYTES.to_vec()),
            ..Default::default()
        };
        assert_eq!(plain, layout_text_styled("Ag", 48.0, &styled).unwrap());
        assert!(check_font(FONT_BYTES).is_ok());
        assert!(check_font(b"not a font").is_err());
        assert!(check_font(b"").is_err());
    }

    #[test]
    fn tracking_and_leading_reshape_block() {
        let base = layout_text_styled("Hello", 32.0, &TextStyle::default()).unwrap();
        let wide = layout_text_styled(
            "Hello",
            32.0,
            &TextStyle {
                tracking: 10.0,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(wide.w > base.w && wide.h == base.h);
        let tight = layout_text_styled(
            "Hello",
            32.0,
            &TextStyle {
                tracking: -2.0,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(tight.w < base.w);
        let tall = layout_text_styled(
            "A\nB",
            32.0,
            &TextStyle {
                leading: 20.0,
                ..Default::default()
            },
        )
        .unwrap();
        let flat = layout_text_styled("A\nB", 32.0, &TextStyle::default()).unwrap();
        assert!(tall.h > flat.h);
    }

    #[test]
    fn dilate_ring_matches_naive_reference() {
        // 7x7 with a 2x2 ink block: naive Chebyshev ring vs the passes.
        let (w, h) = (7u32, 7u32);
        let mut mask = vec![0u8; 49];
        mask[3 * 7 + 3] = 200;
        mask[3 * 7 + 4] = 255;
        mask[4 * 7 + 3] = 255;
        mask[4 * 7 + 4] = 100;
        for r in [1u32, 2, 3] {
            let got = dilate_ring(&mask, w, h, r);
            for y in 0..h {
                for x in 0..w {
                    let mut near = false;
                    for yy in 0..h {
                        for xx in 0..w {
                            if mask[(yy * w + xx) as usize] == 0 {
                                continue;
                            }
                            let d = xx.abs_diff(x).max(yy.abs_diff(y));
                            if d <= r {
                                near = true;
                            }
                        }
                    }
                    let want = if mask[(y * w + x) as usize] == 0 && near {
                        255
                    } else {
                        0
                    };
                    assert_eq!(got[(y * w + x) as usize], want, "r={r} x={x} y={y}");
                }
            }
        }
        assert!(dilate_ring(&mask, w, h, 0).iter().all(|&b| b == 0));
    }

    #[test]
    fn outline_paints_more_and_pads_block() {
        let fill = layout_text_styled("Hi", 32.0, &TextStyle::default()).unwrap();
        let style = TextStyle {
            outline: Some((super::super::brush::Color::rgb(0, 0, 0), 4)),
            ..Default::default()
        };
        let outlined = layout_text_styled("Hi", 32.0, &style).unwrap();
        assert_eq!(outlined.w, fill.w + 8);
        assert_eq!(outlined.h, fill.h + 8);
        // The ring is nonempty and disjoint from the fill coverage.
        let ring = dilate_ring(&outlined.mask, outlined.w, outlined.h, 4);
        assert!(ring.iter().any(|&b| b != 0));
        for (g, s) in ring.iter().zip(outlined.mask.iter()) {
            assert!(*g == 0 || *s == 0);
        }
        // End to end: outlined stamp covers strictly more pixels.
        let mut a = Document::new(128, 128);
        a.checkpoint();
        a.draw_text(
            super::super::brush::Color::rgb(255, 255, 255),
            8.0,
            8.0,
            32.0,
            "Hi",
        )
        .unwrap();
        let mut b = Document::new(128, 128);
        b.checkpoint();
        b.draw_text_styled(
            super::super::brush::Color::rgb(255, 255, 255),
            8.0,
            8.0,
            32.0,
            "Hi",
            &style,
        )
        .unwrap();
        let count = |d: &Document| d.layers[0].pixels.chunks(4).filter(|p| p[3] != 0).count();
        assert!(count(&b) > count(&a));
    }
}
