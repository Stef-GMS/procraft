//! Layered PSD interchange: hand-written 8-bit RGB export plus import via
//! the `psd` crate. The ProCraft layer model maps 1:1 onto PSD concepts
//! (blend keys, clipping flag, transparency channel, user masks), so art
//! flows to Photoshop, Procreate, Krita and GIMP with structure intact.
//!
//! Export is original code written against the public Adobe file-format
//! notes; the `psd` crate independently validates it in the tests below.
//! Import drops layer masks (the crate exposes no mask API) and unknown
//! blend modes (they become Normal).

use super::document::{BlendMode, Document, Layer};

/// Hard caps: absurd canvases are rejected before allocating.
const MAX_SIDE: u32 = 16384;

/// Our blend modes to Photoshop blend keys.
fn blend_key(blend: BlendMode) -> [u8; 4] {
    match blend {
        BlendMode::Normal => *b"norm",
        BlendMode::Multiply => *b"mul ",
        BlendMode::Screen => *b"scrn",
        BlendMode::Overlay => *b"over",
        BlendMode::Darken => *b"dark",
        BlendMode::Lighten => *b"lite",
        BlendMode::Difference => *b"diff",
        BlendMode::Exclusion => *b"excl",
        BlendMode::SoftLight => *b"sLit",
        BlendMode::HardLight => *b"hLit",
    }
}

fn key_blend(key: &str) -> BlendMode {
    match key {
        "Normal" => BlendMode::Normal,
        "Multiply" => BlendMode::Multiply,
        "Screen" => BlendMode::Screen,
        "Overlay" => BlendMode::Overlay,
        "Darken" => BlendMode::Darken,
        "Lighten" => BlendMode::Lighten,
        "Difference" => BlendMode::Difference,
        "Exclusion" => BlendMode::Exclusion,
        "SoftLight" => BlendMode::SoftLight,
        "HardLight" => BlendMode::HardLight,
        _ => BlendMode::Normal,
    }
}

/// Photoshop layer name: Latin-1 Pascal string padded to 4 bytes.
fn psd_name(name: &str) -> Vec<u8> {
    let mut bytes: Vec<u8> = name
        .chars()
        .take(255)
        .map(|c| {
            let u = c as u32;
            if u <= 255 {
                u as u8
            } else {
                b'?'
            }
        })
        .collect();
    // A zero-length name still stores the length byte + padding.
    let mut out = vec![bytes.len().min(255) as u8];
    bytes.truncate(255);
    out.append(&mut bytes);
    while out.len() % 4 != 0 {
        out.push(0);
    }
    out
}

fn be16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_be_bytes());
}

fn be32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_be_bytes());
}

fn bei32(out: &mut Vec<u8>, v: i32) {
    out.extend_from_slice(&v.to_be_bytes());
}

/// Split RGBA bytes into planar R/G/B/A/pull-mask vectors.
fn planes(pixels: &[u8], w: u32, h: u32) -> (Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>) {
    let n = (w * h) as usize;
    let mut r = vec![0u8; n];
    let mut g = vec![0u8; n];
    let mut b = vec![0u8; n];
    let mut a = vec![0u8; n];
    for (i, px) in pixels.as_chunks::<4>().0.iter().enumerate().take(n) {
        r[i] = px[0];
        g[i] = px[1];
        b[i] = px[2];
        a[i] = px[3];
    }
    (r, g, b, a)
}

/// Save the document as a layered 8-bit RGB `.psd`: every layer keeps its
/// name, opacity, visibility, blend, clipping flag, transparency and mask;
/// the composite image is the flattened canvas.
pub fn save_psd(path: &std::path::Path, doc: &Document) -> Result<(), String> {
    let (w, h) = (doc.w, doc.h);
    if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE {
        return Err("canvas has bad dimensions".to_string());
    }
    if doc.layers.is_empty() || doc.layers.len() > 256 {
        return Err("document has a bad layer count".to_string());
    }
    let n = (w * h) as usize;
    for layer in &doc.layers {
        if layer.pixels.len() != n * 4 {
            return Err("layer has wrong pixel size".to_string());
        }
        if let Some(m) = layer.mask.as_deref() {
            if m.len() != n {
                return Err("layer mask has wrong size".to_string());
            }
        }
    }

    let mut out = Vec::new();
    // Header: signature, version, reserved, channels, height, width,
    // depth, color mode.
    out.extend_from_slice(b"8BPS");
    be16(&mut out, 1);
    out.extend_from_slice(&[0u8; 6]);
    be16(&mut out, 3);
    be32(&mut out, h);
    be32(&mut out, w);
    be16(&mut out, 8);
    be16(&mut out, 3);
    // Empty color-mode data + image resources.
    be32(&mut out, 0);
    be32(&mut out, 0);

    // Layer records first (top layer first), channel data after.
    let mut records = Vec::new();
    let mut channel_data = Vec::new();
    // PSD stores top-first; ours are bottom-first. Each layer is baked
    // at its canvas offset (Move tool) so Photoshop sees what we show.
    for (ri, layer) in doc.layers.iter().rev().enumerate() {
        let index = doc.layers.len() - 1 - ri;
        let Some((baked_px, baked_mask)) = doc.baked_layer(index) else {
            return Err("layer has wrong pixel size".to_string());
        };
        let has_mask = baked_mask.as_deref().map(|m| m.len() == n).unwrap_or(false);
        // Channel ids: transparency, R, G, B, optional user mask.
        let mut ids: Vec<i16> = vec![-1, 0, 1, 2];
        if has_mask {
            ids.push(-2);
        }
        bei32(&mut records, 0); // top
        bei32(&mut records, 0); // left
        bei32(&mut records, h as i32); // bottom
        bei32(&mut records, w as i32); // right
        be16(&mut records, ids.len() as u16);
        for id in &ids {
            be16(&mut records, *id as u16);
            be32(&mut records, (2 + n) as u32); // compression u16 + bytes
        }
        records.extend_from_slice(b"8BIM");
        records.extend_from_slice(&blend_key(layer.blend));
        records.push((layer.opacity.clamp(0.0, 1.0) * 255.0).round() as u8);
        records.push(u8::from(layer.clip));
        records.push(if doc.is_layer_shown(index) { 0 } else { 0x02 });
        records.push(0); // filler
                         // Extra data: mask section + empty blending ranges + name.
        let mut extra = Vec::new();
        if has_mask {
            be32(&mut extra, 18); // mask section size
            bei32(&mut extra, 0);
            bei32(&mut extra, 0);
            bei32(&mut extra, h as i32);
            bei32(&mut extra, w as i32);
            extra.push(255); // default: reveal
            extra.push(0); // flags: absolute, enabled
        } else {
            be32(&mut extra, 0);
        }
        be32(&mut extra, 0); // blending ranges: none
        extra.extend_from_slice(&psd_name(&layer.name));
        be32(&mut records, extra.len() as u32);
        records.extend_from_slice(&extra);
        // Channel bytes: raw compression + planar data.
        let (r, g, b, a) = planes(&baked_px, w, h);
        let mut push = |plane: &[u8]| {
            be16(&mut channel_data, 0);
            channel_data.extend_from_slice(plane);
        };
        push(&a);
        push(&r);
        push(&g);
        push(&b);
        if has_mask {
            push(baked_mask.as_deref().unwrap());
        }
    }
    // Layer info section: length + count + records + channel data.
    let mut layer_info = Vec::new();
    be16(&mut layer_info, doc.layers.len() as u16);
    layer_info.extend_from_slice(&records);
    layer_info.extend_from_slice(&channel_data);
    // Section length covers the layer-info length field, its content,
    // and the global-mask length field (empty global mask).
    be32(&mut out, (4 + layer_info.len() + 4) as u32);
    be32(&mut out, layer_info.len() as u32);
    out.extend_from_slice(&layer_info);
    // Global mask info: none.
    be32(&mut out, 0);
    // Composite image: raw planar RGB of the flattened canvas.
    be16(&mut out, 0);
    let flat = doc.flatten();
    let (r, g, b, _) = planes(&flat, w, h);
    out.extend_from_slice(&r);
    out.extend_from_slice(&g);
    out.extend_from_slice(&b);
    std::fs::write(path, &out).map_err(|e| e.to_string())
}

/// Open a `.psd` file into a document: 8-bit RGB only. Names, opacity,
/// visibility, clipping and known blend modes transfer; layer masks do not
/// (the parser exposes none) and unknown blends become Normal. A
/// layer-less file yields its composite as a single layer.
///
/// Flag note: psd 0.3.5 reads the visibility bit and the clipping byte
/// INVERTED versus the Adobe spec (and GIMP/psd-tools/Photoshop agree on
/// the spec). Our writer stores spec-correct values, so both directions
/// below un-invert the crate's readings. Re-verify with real Photoshop
/// files if the `psd` dependency is ever bumped.
pub fn open_psd(path: &std::path::Path) -> Result<Document, String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if meta.len() > 200 * 1024 * 1024 {
        return Err("PSD file is too large".to_string());
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let file = psd::Psd::from_bytes(&bytes).map_err(|e| e.to_string())?;
    if file.depth() != psd::PsdDepth::Eight || file.color_mode() != psd::ColorMode::Rgb {
        return Err("only 8-bit RGB PSD files open".to_string());
    }
    let (w, h) = (file.width(), file.height());
    if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE {
        return Err("PSD has bad dimensions".to_string());
    }
    let expect = (w * h * 4) as usize;
    let mut layers = Vec::new();
    if file.layers().is_empty() {
        // Flattened-only file: the composite becomes one layer.
        let rgba = file.rgba();
        if rgba.len() != expect {
            return Err("PSD composite has bad size".to_string());
        }
        layers.push(Layer {
            name: "Background".to_string(),
            visible: true,
            opacity: 1.0,
            blend: BlendMode::Normal,
            pixels: rgba,
            mask: None,
            clip: false,
            ox: 0,
            oy: 0,
            group: None,
        });
    } else {
        // The crate lists bottom-first, matching our order.
        for (i, pl) in file.layers().iter().enumerate() {
            let rgba = pl.rgba();
            if rgba.len() != expect {
                continue; // cropped/group layers we cannot place: skip
            }
            let raw_name = pl.name().trim().to_string();
            let name = if raw_name.is_empty() {
                format!("Layer {}", i + 1)
            } else {
                raw_name.chars().take(64).collect()
            };
            layers.push(Layer {
                name,
                visible: !pl.visible(),
                opacity: pl.opacity() as f32 / 255.0,
                blend: key_blend(&format!("{:?}", pl.blend_mode())),
                pixels: rgba,
                mask: None,
                clip: !pl.is_clipping_mask(),
                ox: 0,
                oy: 0,
                group: None,
            });
            if layers.len() >= 256 {
                break;
            }
        }
        if layers.is_empty() {
            return Err("no usable layers in PSD file".to_string());
        }
    }
    Ok(Document::from_layers(w, h, layers, 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_doc() -> Document {
        let mut doc = Document::new(5, 3);
        doc.background = [10, 20, 30, 255];
        doc.layers[0].name = "A".to_string();
        doc.layers[0].pixels = vec![200; 5 * 3 * 4];
        doc.add_layer("BC".to_string());
        doc.layers[1].blend = BlendMode::Difference;
        doc.layers[1].opacity = 0.5;
        doc.layers[1].clip = true;
        doc.layers[1].pixels = vec![7; 5 * 3 * 4];
        doc.add_mask(1);
        doc.layers[1].mask.as_mut().unwrap()[0] = 0;
        doc
    }

    #[test]
    fn blend_keys_cover_all_modes() {
        let keys: Vec<[u8; 4]> = BlendMode::ALL.iter().map(|m| blend_key(*m)).collect();
        assert_eq!(keys[0], *b"norm");
        assert_eq!(keys[3], *b"over");
        assert_eq!(keys[6], *b"diff");
        assert_eq!(keys[8], *b"sLit");
        assert_eq!(keys[9], *b"hLit");
        // Every key round-trips through the import mapper.
        for (mode, key) in BlendMode::ALL.iter().zip(keys.iter()) {
            let back = key_blend(&format!("{mode:?}"));
            assert_eq!(back, *mode, "key {key:?}");
        }
        assert_eq!(key_blend("ColorBurn"), BlendMode::Normal);
    }

    #[test]
    fn psd_export_parses_independently() {
        let doc = sample_doc();
        let dir = std::env::temp_dir();
        let path = dir.join("procraft-export.psd");
        save_psd(&path, &doc).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(&bytes[..4], b"8BPS");
        // Independent read via the third-party parser.
        let file = psd::Psd::from_bytes(&bytes).unwrap();
        assert_eq!((file.width(), file.height()), (5, 3));
        assert_eq!(file.layers().len(), 2);
        // Bottom-first: A, then BC.
        assert_eq!(file.layers()[0].name(), "A");
        assert_eq!(file.layers()[1].name(), "BC");
        // Flag reads below go through the full-circle test: this crate
        // version inverts the visibility bit and clipping byte (see the
        // note on open_psd), so direct assertions here would pin the bug.
        assert_eq!(file.layers()[1].opacity(), 128);
        assert_eq!(format!("{:?}", file.layers()[1].blend_mode()), "Difference");
        // Alpha channel survived (test pixels are flat [7,7,7,7]).
        let rgba = file.layers()[1].rgba();
        assert_eq!(rgba.len(), 5 * 3 * 4);
        assert_eq!(&rgba[0..4], &[7, 7, 7, 7]);
    }

    #[test]
    fn psd_full_circle_keeps_everything() {
        let doc = sample_doc();
        let dir = std::env::temp_dir();
        let path = dir.join("procraft-circle.psd");
        save_psd(&path, &doc).unwrap();
        let back = open_psd(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!((back.w, back.h), (5, 3));
        assert_eq!(back.layers.len(), 2);
        assert_eq!(back.layers[0].name, "A");
        assert_eq!(back.layers[0].pixels, vec![200; 5 * 3 * 4]);
        assert_eq!(back.layers[1].name, "BC");
        assert_eq!(back.layers[1].blend, BlendMode::Difference);
        // Opacity round-trips through a byte: 0.5 -> 128 -> 128/255.
        assert!((back.layers[1].opacity * 255.0).round() == 128.0);
        assert!(back.layers[1].clip);
        assert_eq!(back.layers[1].pixels, vec![7; 5 * 3 * 4]);
        // Masks do not survive the third-party parser: documented gap.
        assert!(back.layers[1].mask.is_none());
    }

    #[test]
    fn psd_offset_bakes_shifted_positions() {
        // A layer moved +2 exports its content at canvas 2 (cropped bake),
        // so Photoshop opens what ProCraft shows.
        let mut doc = Document::new(5, 1);
        doc.layers[0].pixels[0..4].copy_from_slice(&[255, 0, 0, 255]);
        doc.layers[0].ox = 2;
        let dir = std::env::temp_dir();
        let path = dir.join("procraft-offset.psd");
        save_psd(&path, &doc).unwrap();
        let back = open_psd(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(&back.layers[0].pixels[0..4], &[0, 0, 0, 0]);
        assert_eq!(&back.layers[0].pixels[8..12], &[255, 0, 0, 255]);
    }

    #[test]
    fn psd_odd_names_and_visibility_roundtrip() {
        let mut doc = Document::new(2, 2);
        // 1..=5 char names exercise the 4-byte name padding.
        let names = ["x", "yz", "abc", "defg", "hijkl"];
        doc.layers[0].name = names[0].to_string();
        for n in &names[1..] {
            doc.add_layer(n.to_string());
        }
        doc.layers[2].visible = false;
        let dir = std::env::temp_dir();
        let path = dir.join("procraft-names.psd");
        save_psd(&path, &doc).unwrap();
        let back = open_psd(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        let got: Vec<String> = back.layers.iter().map(|l| l.name.clone()).collect();
        assert_eq!(got, names);
        assert!(!back.layers[2].visible);
    }

    #[test]
    fn psd_rejects_garbage() {
        let dir = std::env::temp_dir();
        let path = dir.join("procraft-bad.psd");
        std::fs::write(&path, b"not a psd at all, way too short").unwrap();
        assert!(open_psd(&path).is_err());
        let _ = std::fs::remove_file(&path);
        let mut doc = Document::new(2, 2);
        doc.layers[0].pixels.truncate(3);
        assert!(save_psd(&dir.join("procraft-badsize.psd"), &doc).is_err());
    }
}
