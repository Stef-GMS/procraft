//! Animation Assist: layers as frames, onion skins, animated GIF export.
//!
//! Original clean-room code. The model is deliberately Procreate-like and
//! simple: every layer is one frame, bottom-to-top is first-to-last.
//! Hidden layers are skipped by onion skins and GIF export (but stay in
//! the frame count so unhiding never renumbers). Playback state itself
//! lives app-side; this module is pure document math plus the exporter.

use super::document::Document;
use image::{
    codecs::gif::{GifEncoder, Repeat},
    Delay, Frame, RgbaImage,
};

/// Playback/export rate bounds (fps).
pub const MIN_ANIM_FPS: u32 = 1;
pub const MAX_ANIM_FPS: u32 = 60;
/// Sanity cap on exported frames (layer counts never reach it in practice).
pub const MAX_GIF_FRAMES: usize = 256;

/// Clamp a frame position into the layer stack (`None` when empty, which
/// documents never are, but importers must not be trusted).
pub fn clamp_frame(doc: &Document, pos: usize) -> Option<usize> {
    if doc.layers.is_empty() {
        None
    } else {
        Some(pos.min(doc.layers.len() - 1))
    }
}

/// Nearest visible layer strictly below `index` (onion: previous frame).
pub fn prev_frame(doc: &Document, index: usize) -> Option<usize> {
    (0..index.min(doc.layers.len())).rev().find(|&i| doc.layers[i].visible)
}

/// Nearest visible layer strictly above `index` (onion: next frame).
pub fn next_frame(doc: &Document, index: usize) -> Option<usize> {
    ((index + 1)..doc.layers.len()).find(|&i| doc.layers[i].visible)
}

/// Render one frame: the layer (Normal blend, opacity + reveal mask
/// applied, clipping intentionally ignored — a cel stands alone) over the
/// page background. Opaque RGBA out. `index` out of range renders the bare
/// background instead of failing, so playback never panics on a racing
/// layer delete.
pub fn rasterize_frame(doc: &Document, index: usize) -> Vec<u8> {
    let n = (doc.w * doc.h) as usize;
    let mut out = vec![0u8; n * 4];
    for px in out.chunks_exact_mut(4) {
        px.copy_from_slice(&doc.background);
    }
    let Some(layer) = doc.layers.get(index) else {
        return out;
    };
    let op = layer.opacity.clamp(0.0, 1.0);
    let (ox, oy) = (layer.ox, layer.oy);
    for y in 0..doc.h {
        for x in 0..doc.w {
            // Buffer content sits at the layer offset (Move tool).
            let (lx, ly) = (x as i32 - ox, y as i32 - oy);
            if lx < 0 || ly < 0 || lx >= doc.w as i32 || ly >= doc.h as i32 {
                continue;
            }
            let o = ((ly as u32 * doc.w + lx as u32) * 4) as usize;
            if o + 3 >= layer.pixels.len() {
                continue;
            }
            let mut a = layer.pixels[o + 3] as f32 / 255.0 * op;
            if let Some(m) = layer.mask.as_deref() {
                a *= m.get(o / 4).copied().unwrap_or(255) as f32 / 255.0;
            }
            if a <= 0.0 {
                continue;
            }
            let d = ((y * doc.w + x) * 4) as usize;
            out[d] = (layer.pixels[o] as f32 * a + out[d] as f32 * (1.0 - a)).round() as u8;
            out[d + 1] =
                (layer.pixels[o + 1] as f32 * a + out[d + 1] as f32 * (1.0 - a)).round() as u8;
            out[d + 2] =
                (layer.pixels[o + 2] as f32 * a + out[d + 2] as f32 * (1.0 - a)).round() as u8;
            out[d + 3] = 255;
        }
    }
    out
}

/// Render an inclusive canvas region of one frame (clamped), row-major
/// RGBA — the assist-mode equivalent of `flatten_region` for partial
/// texture uploads while painting the current frame.
pub fn rasterize_frame_region(
    doc: &Document,
    index: usize,
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
) -> Vec<u8> {
    let x1 = x1.min(doc.w.saturating_sub(1));
    let y1 = y1.min(doc.h.saturating_sub(1));
    let (x0, y0) = (x0.min(x1), y0.min(y1));
    let full = rasterize_frame(doc, index);
    let mut out = Vec::with_capacity(((x1 - x0 + 1) * (y1 - y0 + 1) * 4) as usize);
    for y in y0..=y1 {
        for x in x0..=x1 {
            let o = ((y * doc.w + x) * 4) as usize;
            if o + 3 < full.len() {
                out.extend_from_slice(&full[o..o + 4]);
            } else {
                out.extend_from_slice(&doc.background);
            }
        }
    }
    out
}

/// Onion ghost of one frame: layer RGB with alpha scaled by `strength`
/// (0.0 = invisible), no background — the app tints prev/next and draws
/// these over the current frame. Out-of-range renders transparent.
pub fn onion_ghost(doc: &Document, index: usize, strength: f32) -> Vec<u8> {
    let n = (doc.w * doc.h) as usize;
    let mut out = vec![0u8; n * 4];
    let strength = strength.clamp(0.0, 1.0);
    if strength <= 0.0 {
        return out;
    }
    let Some(layer) = doc.layers.get(index) else {
        return out;
    };
    let op = layer.opacity.clamp(0.0, 1.0);
    let (ox, oy) = (layer.ox, layer.oy);
    for y in 0..doc.h {
        for x in 0..doc.w {
            // Buffer content sits at the layer offset (Move tool).
            let (lx, ly) = (x as i32 - ox, y as i32 - oy);
            if lx < 0 || ly < 0 || lx >= doc.w as i32 || ly >= doc.h as i32 {
                continue;
            }
            let o = ((ly as u32 * doc.w + lx as u32) * 4) as usize;
            if o + 3 >= layer.pixels.len() {
                continue;
            }
            let mut a = layer.pixels[o + 3] as f32 / 255.0 * op * strength;
            if let Some(m) = layer.mask.as_deref() {
                a *= m.get(o / 4).copied().unwrap_or(255) as f32 / 255.0;
            }
            if a <= 0.0 {
                continue;
            }
            let d = ((y * doc.w + x) * 4) as usize;
            out[d] = layer.pixels[o];
            out[d + 1] = layer.pixels[o + 1];
            out[d + 2] = layer.pixels[o + 2];
            out[d + 3] = (a * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
    out
}

/// Export visible layers as a looping animated GIF at `fps`. Hidden layers
/// are skipped (documented in the app status line). Errors on bad rates,
/// fully-hidden documents, and absurd frame counts.
pub fn export_gif(doc: &Document, fps: u32) -> Result<Vec<u8>, String> {
    if !(MIN_ANIM_FPS..=MAX_ANIM_FPS).contains(&fps) {
        return Err(format!("bad animation rate: {fps}"));
    }
    if doc.w == 0 || doc.h == 0 {
        return Err("nothing to animate: empty canvas".to_string());
    }
    let frames: Vec<usize> = doc
        .layers
        .iter()
        .enumerate()
        .filter(|(_, l)| l.visible)
        .map(|(i, _)| i)
        .collect();
    if frames.is_empty() {
        return Err("nothing to animate: every layer is hidden".to_string());
    }
    if frames.len() > MAX_GIF_FRAMES {
        return Err(format!("too many frames (max {MAX_GIF_FRAMES})"));
    }
    let mut buf = Vec::new();
    {
        let mut enc = GifEncoder::new(&mut buf);
        enc.set_repeat(Repeat::Infinite).map_err(|e| e.to_string())?;
        for idx in frames {
            let px = rasterize_frame(doc, idx);
            let img = RgbaImage::from_raw(doc.w, doc.h, px).ok_or("frame raster failed")?;
            let frame = Frame::from_parts(img, 0, 0, Delay::from_numer_denom_ms(1000, fps));
            enc.encode_frame(frame).map_err(|e| e.to_string())?;
        }
    }
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::super::brush::{Brush, Color};
    use super::*;

    fn paint_layer(doc: &mut Document, idx: usize, rgb: [u8; 3]) {
        let brush = Brush { size: 64.0, softness: 0.0, ..Default::default() };
        doc.active = idx;
        doc.fill_rect(&brush, Color::rgb(rgb[0], rgb[1], rgb[2]), 0, 0, 15, 15);
    }

    #[test]
    fn frame_raster_composites_layer_over_background() {
        let mut doc = Document::new(16, 16);
        paint_layer(&mut doc, 0, [255, 0, 0]);
        let px = rasterize_frame(&doc, 0);
        assert_eq!(&px[0..3], &[255, 0, 0]);
        // Transparent pixels show the page.
        let mut doc2 = Document::new(16, 16);
        doc2.background = [10, 20, 30, 255];
        let px2 = rasterize_frame(&doc2, 0);
        assert_eq!(&px2[0..4], &[10, 20, 30, 255]);
        // Half opacity blends with the page.
        let mut doc3 = Document::new(16, 16);
        paint_layer(&mut doc3, 0, [255, 0, 0]);
        doc3.layers[0].opacity = 0.5;
        let px3 = rasterize_frame(&doc3, 0);
        assert_eq!(&px3[0..4], &[255, 128, 128, 255]);
        // Out-of-range index renders background, never panics.
        assert_eq!(&rasterize_frame(&doc3, 99)[0..4], &[255, 255, 255, 255]);
    }

    #[test]
    fn frame_region_matches_full_raster() {
        let mut doc = Document::new(32, 32);
        paint_layer(&mut doc, 0, [0, 0, 255]);
        let full = rasterize_frame(&doc, 0);
        let reg = rasterize_frame_region(&doc, 0, 4, 6, 11, 13);
        assert_eq!(reg.len(), 8 * 8 * 4);
        for (ry, y) in (6..=13).enumerate() {
            for (rx, x) in (4..=11).enumerate() {
                let ro = (ry * 8 + rx) * 4;
                let fo = ((y * 32 + x) * 4) as usize;
                assert_eq!(&reg[ro..ro + 4], &full[fo..fo + 4]);
            }
        }
    }

    #[test]
    fn frames_and_ghosts_follow_layer_offset() {
        let mut doc = Document::new(16, 16);
        paint_layer(&mut doc, 0, [255, 0, 0]);
        // Shift right 4: canvas 0-3 show page, canvas 4 shows the cel.
        doc.translate_layer(0, 4, 0);
        let px = rasterize_frame(&doc, 0);
        assert_eq!(&px[0..4], &[255, 255, 255, 255]);
        assert_eq!(&px[16..19], &[255, 0, 0]);
        let g = onion_ghost(&doc, 0, 1.0);
        assert_eq!(g[3], 0);
        assert_eq!(&g[16..19], &[255, 0, 0]);
        assert_eq!(g[19], 255);
    }

    #[test]
    fn onion_ghost_scales_alpha_keeps_rgb() {
        let mut doc = Document::new(16, 16);
        paint_layer(&mut doc, 0, [10, 200, 30]);
        let full = onion_ghost(&doc, 0, 1.0);
        assert_eq!(&full[0..3], &[10, 200, 30]);
        assert_eq!(full[3], 255);
        let half = onion_ghost(&doc, 0, 0.5);
        assert_eq!(&half[0..3], &[10, 200, 30]);
        assert!((half[3] as i32 - 128).abs() <= 1, "{}", half[3]);
        assert!(onion_ghost(&doc, 0, 0.0).iter().all(|&b| b == 0));
        assert!(onion_ghost(&doc, 7, 1.0).iter().all(|&b| b == 0));
    }

    #[test]
    fn neighbor_lookup_skips_hidden() {
        let mut doc = Document::new(8, 8);
        doc.add_layer("L2".to_string());
        doc.add_layer("L3".to_string());
        doc.layers[1].visible = false;
        assert_eq!(prev_frame(&doc, 2), Some(0));
        assert_eq!(next_frame(&doc, 0), Some(2));
        assert_eq!(prev_frame(&doc, 0), None);
        assert_eq!(next_frame(&doc, 2), None);
        assert_eq!(clamp_frame(&doc, 99), Some(2));
    }

    #[test]
    fn gif_roundtrips_visible_frames() {
        use image::{codecs::gif::GifDecoder, AnimationDecoder};
        let mut doc = Document::new(16, 16);
        paint_layer(&mut doc, 0, [255, 0, 0]);
        doc.add_layer("F2".to_string());
        paint_layer(&mut doc, 1, [0, 255, 0]);
        doc.add_layer("Hidden".to_string());
        doc.layers[2].visible = false;
        let bytes = export_gif(&doc, 12).unwrap();
        assert_eq!(&bytes[0..6], b"GIF89a");
        let dec = GifDecoder::new(std::io::Cursor::new(&bytes[..])).unwrap();
        let frames: Vec<_> = dec.into_frames().collect::<Result<_, _>>().unwrap();
        // Hidden layer skipped: 2 frames.
        assert_eq!(frames.len(), 2);
        assert_eq!((frames[0].buffer().width(), frames[0].buffer().height()), (16, 16));
        // First frame is the red cel.
        assert_eq!(&frames[0].buffer()[(0, 0)].0[0..3], &[255, 0, 0]);
        assert!(export_gif(&doc, 0).is_err());
        assert!(export_gif(&doc, 61).is_err());
        doc.layers[0].visible = false;
        doc.layers[1].visible = false;
        assert!(export_gif(&doc, 12).is_err());
    }
}
