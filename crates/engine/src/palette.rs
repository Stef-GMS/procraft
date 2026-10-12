//! Color palettes, GIMP-palette import and color-harmony helpers.
//!
//! Original clean-room code: a named swatch set (`.pcpalette` JSON files,
//! validated like `.pcbrush`), a GIMP `.gpl` text importer for interchange,
//! and small RGB<->HSL math plus harmony derivations (complementary,
//! triadic, analogous) that power the desktop harmony row.

/// Maximum swatches in one palette (keeps files and UI grids bounded).
pub const MAX_PALETTE_COLORS: usize = 48;

/// A named swatch set: display name plus sRGB triples.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Palette {
    pub name: String,
    pub colors: Vec<[u8; 3]>,
}

fn check_palette(name: &str, colors: &[[u8; 3]]) -> Result<String, String> {
    let clean = name.trim().to_string();
    if clean.is_empty() {
        return Err("palette has an empty name".to_string());
    }
    if clean.chars().count() > 64 {
        return Err(format!("palette name too long: {clean}"));
    }
    if colors.is_empty() {
        return Err("palette has no colors".to_string());
    }
    if colors.len() > MAX_PALETTE_COLORS {
        return Err(format!("too many colors (max {MAX_PALETTE_COLORS})"));
    }
    Ok(clean)
}

/// Save a palette to a `.pcpalette` file (pretty JSON `{name, colors}`).
pub fn save_palette(path: &std::path::Path, palette: &Palette) -> Result<(), String> {
    let name = check_palette(&palette.name, &palette.colors)?;
    let clean = Palette { name, colors: palette.colors.clone() };
    let text = serde_json::to_string_pretty(&clean).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| e.to_string())
}

/// Load a `.pcpalette` file, validating name and color count.
pub fn open_palette(path: &std::path::Path) -> Result<Palette, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let raw: Palette = serde_json::from_str(&text).map_err(|_| "not a palette file".to_string())?;
    let name = check_palette(&raw.name, &raw.colors)?;
    Ok(Palette { name, colors: raw.colors })
}

/// Parse GIMP `.gpl` text into a palette (interchange import; the file's
/// own swatch names are dropped, only the name header and RGB triples are
/// kept). `stem` names the palette when the file has no `Name:` line.
pub fn import_gpl_text(text: &str, stem: &str) -> Result<Palette, String> {
    let mut lines = text.lines();
    let first = lines.next().unwrap_or("").trim_end();
    if first != "GIMP Palette" {
        return Err("not a GIMP palette file".to_string());
    }
    let mut name = stem.trim().to_string();
    let mut colors = Vec::new();
    for line in lines {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("Name:") {
            let n = rest.trim().to_string();
            if !n.is_empty() {
                name = n;
            }
            continue;
        }
        if line.starts_with("Columns:") {
            continue;
        }
        // Swatch rows: `R G B [optional name]` with 0..=255 channels.
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 3 {
            continue;
        }
        let mut rgb = [0u8; 3];
        let mut ok = true;
        for (i, p) in parts.iter().take(3).enumerate() {
            match p.parse::<i32>() {
                Ok(v) if (0..=255).contains(&v) => rgb[i] = v as u8,
                _ => {
                    ok = false;
                    break;
                }
            }
        }
        if ok {
            colors.push(rgb);
        }
    }
    let clean = check_palette(&name, &colors)?;
    let mut palette = Palette { name: clean, colors };
    palette.colors.truncate(MAX_PALETTE_COLORS);
    Ok(palette)
}

/// Import a `.gpl` file from disk (`stem` falls back to the file name).
pub fn import_gpl_file(path: &std::path::Path) -> Result<Palette, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Palette")
        .to_string();
    import_gpl_text(&text, &stem)
}

/// Starter swatch set: an original 24-color spread (primaries, earths,
/// pastels, grays) so the swatch grid is useful on first launch.
pub fn starter_palette() -> Palette {
    Palette {
        name: "Starter".to_string(),
        colors: vec![
            [0, 0, 0],
            [255, 255, 255],
            [128, 128, 128],
            [90, 90, 90],
            [200, 200, 200],
            [230, 57, 70],
            [255, 133, 27],
            [255, 202, 58],
            [46, 134, 193],
            [30, 144, 255],
            [114, 221, 247],
            [42, 157, 143],
            [106, 176, 76],
            [151, 125, 70],
            [123, 90, 45],
            [210, 180, 140],
            [155, 89, 182],
            [241, 148, 138],
            [250, 218, 221],
            [253, 253, 150],
            [255, 183, 197],
            [176, 224, 230],
            [144, 238, 144],
            [255, 218, 185],
        ],
    }
}

/// RGB byte triple to HSL: h in degrees 0..<360, s/l in 0..=1.
pub fn rgb_to_hsl([r, g, b]: [u8; 3]) -> (f32, f32, f32) {
    let (r, g, b) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = (max + min) * 0.5;
    if (max - min).abs() < f32::EPSILON {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
    let h = if max == r {
        60.0 * (((g - b) / d) % 6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    ((h + 360.0) % 360.0, s.clamp(0.0, 1.0), l.clamp(0.0, 1.0))
}

/// HSL (same ranges as [`rgb_to_hsl`]) back to an RGB byte triple.
pub fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [u8; 3] {
    let (h, s, l) = (h.rem_euclid(360.0), s.clamp(0.0, 1.0), l.clamp(0.0, 1.0));
    if s <= 0.0 {
        let v = (l * 255.0).round() as u8;
        return [v, v, v];
    }
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c * 0.5;
    let (r, g, b) = match (h / 60.0).floor() as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    [
        ((r + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((g + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((b + m) * 255.0).round().clamp(0.0, 255.0) as u8,
    ]
}

fn shift_hue(rgb: [u8; 3], degrees: f32) -> [u8; 3] {
    let (h, s, l) = rgb_to_hsl(rgb);
    // Grays have no hue: harmonies of gray stay gray (honest, no tint).
    if s <= 0.0 {
        return rgb;
    }
    hsl_to_rgb(h + degrees, s, l)
}

/// Opposite-hue swatch (red -> cyan). Grays map to themselves.
pub fn complementary(rgb: [u8; 3]) -> [u8; 3] {
    shift_hue(rgb, 180.0)
}

/// The two triadic partners (+/-120 degrees).
pub fn triadic(rgb: [u8; 3]) -> [[u8; 3]; 2] {
    [shift_hue(rgb, 120.0), shift_hue(rgb, 240.0)]
}

/// The two analogous neighbors (+/-30 degrees).
pub fn analogous(rgb: [u8; 3]) -> [[u8; 3]; 2] {
    [shift_hue(rgb, 30.0), shift_hue(rgb, 330.0)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_roundtrips() {
        let pal = starter_palette();
        let dir = std::env::temp_dir();
        let path = dir.join("procraft-pal-roundtrip.pcpalette");
        save_palette(&path, &pal).unwrap();
        let back = open_palette(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(back, pal);
    }

    #[test]
    fn palette_rejects_garbage() {
        let dir = std::env::temp_dir();
        let path = dir.join("procraft-pal-bad.pcpalette");
        // Empty set.
        assert!(save_palette(&path, &Palette { name: "E".to_string(), colors: vec![] }).is_err());
        // Empty name.
        assert!(
            save_palette(&path, &Palette { name: "  ".to_string(), colors: vec![[1, 2, 3]] })
                .is_err()
        );
        // Too many colors.
        let many = vec![[9u8, 9, 9]; MAX_PALETTE_COLORS + 1];
        assert!(save_palette(&path, &Palette { name: "Many".to_string(), colors: many }).is_err());
        // Not JSON at all.
        std::fs::write(&path, "nope").unwrap();
        assert!(open_palette(&path).is_err());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn gpl_parses_name_comments_and_swatches() {
        let text = "GIMP Palette\nName: Favs\nColumns: 4\n# a comment\n255   0   0\tRed\n  0 255   0 Green\n0   0 255\n999 0 0\n";
        let pal = import_gpl_text(text, "stem").unwrap();
        assert_eq!(pal.name, "Favs");
        assert_eq!(pal.colors, vec![[255, 0, 0], [0, 255, 0], [0, 0, 255]]);
    }

    #[test]
    fn gpl_rejects_non_palettes_and_empties() {
        assert!(import_gpl_text("hello", "S").is_err());
        assert!(import_gpl_text("GIMP Palette\nName: X\n", "S").is_err());
        // Missing Name falls back to the file stem.
        let pal = import_gpl_text("GIMP Palette\n10 20 30\n", "MyFile").unwrap();
        assert_eq!(pal.name, "MyFile");
    }

    #[test]
    fn hsl_roundtrips_primary_colors() {
        for rgb in [[255, 0, 0], [0, 255, 0], [0, 0, 255], [0, 0, 0], [255, 255, 255], [30, 144, 255]] {
            let (h, s, l) = rgb_to_hsl(rgb);
            let back = hsl_to_rgb(h, s, l);
            for (a, b) in rgb.iter().zip(back.iter()) {
                assert!(((*a as i32) - (*b as i32)).abs() <= 1, "{rgb:?} -> {back:?}");
            }
        }
    }

    #[test]
    fn harmonies_match_color_theory() {
        // Red complements cyan, triads green/blue, analogs orange/rose.
        let comp = complementary([255, 0, 0]);
        assert!(comp[1] > 200 && comp[2] > 200 && comp[0] < 55, "{comp:?}");
        let [t1, t2] = triadic([255, 0, 0]);
        assert!(t1[1] > 200 && t2[2] > 200, "{t1:?} {t2:?}");
        let [a1, a2] = analogous([255, 0, 0]);
        assert!(a1[0] > 200 && a1[1] > 100 && a1[1] < 200, "{a1:?}");
        assert!(a2[0] > 200 && a2[2] > 100, "{a2:?}");
        // Grays stay gray through every harmony.
        assert_eq!(complementary([128, 128, 128]), [128, 128, 128]);
        assert_eq!(triadic([0, 0, 0]), [[0, 0, 0], [0, 0, 0]]);
    }
}
