//! Color palettes, GIMP-palette import and color-harmony helpers.
//!
//! Original clean-room code: a named swatch set (`.pcpalette` JSON files,
//! validated like `.pcbrush`), a GIMP `.gpl` text importer for interchange,
//! and small RGB<->HSL math plus harmony derivations (complementary,
//! triadic, analogous) that power the desktop harmony row.

/// Maximum swatches in one palette (keeps files and UI grids bounded).
pub const MAX_PALETTE_COLORS: usize = 48;
/// Maximum named palettes in one set (keeps the manager UI bounded).
pub const MAX_PALETTES: usize = 16;

/// A named swatch set: display name plus sRGB triples.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Palette {
    pub name: String,
    pub colors: Vec<[u8; 3]>,
}

/// A named palette collection with an active selection: the multi-palette
/// manager model. Palettes may be empty while edited (saving still
/// validates through [`save_palette`]); names stay unique via [`PaletteSet::unique_name`].
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PaletteSet {
    pub palettes: Vec<Palette>,
    pub active: usize,
}

fn check_name(name: &str) -> Result<String, String> {
    let clean = name.trim().to_string();
    if clean.is_empty() {
        return Err("palette has an empty name".to_string());
    }
    if clean.chars().count() > 64 {
        return Err(format!("palette name too long: {clean}"));
    }
    Ok(clean)
}

impl PaletteSet {
    /// One empty "My colors" palette selected (matches the old single-set UI).
    pub fn new() -> Self {
        Self {
            palettes: vec![Palette {
                name: "My colors".to_string(),
                colors: Vec::new(),
            }],
            active: 0,
        }
    }

    pub fn active(&self) -> Option<&Palette> {
        self.palettes.get(self.active)
    }

    pub fn active_mut(&mut self) -> Option<&mut Palette> {
        self.palettes.get_mut(self.active)
    }

    /// Select palette `idx`. Out-of-range selections are ignored.
    pub fn set_active(&mut self, idx: usize) -> bool {
        if idx < self.palettes.len() {
            self.active = idx;
            true
        } else {
            false
        }
    }

    /// Unique display name: "Foo", "Foo (2)", ... (mirrors the brush
    /// library so imports never collide with existing palettes).
    pub fn unique_name(&self, base: &str) -> String {
        let clean = base.trim();
        let clean = if clean.is_empty() { "Palette" } else { clean };
        if !self.palettes.iter().any(|p| p.name == clean) {
            return clean.to_string();
        }
        for n in 2..=MAX_PALETTES + 2 {
            let cand = format!("{clean} ({n})");
            if !self.palettes.iter().any(|p| p.name == cand) {
                return cand;
            }
        }
        clean.to_string()
    }

    /// Add an empty palette, selected at once. Names are cleaned and
    /// de-duplicated; errors on empty names and a full set.
    pub fn add(&mut self, name: &str) -> Result<usize, String> {
        if self.palettes.len() >= MAX_PALETTES {
            return Err(format!("too many palettes (max {MAX_PALETTES})"));
        }
        let clean = check_name(name)?;
        let name = self.unique_name(&clean);
        self.palettes.push(Palette {
            name,
            colors: Vec::new(),
        });
        self.active = self.palettes.len() - 1;
        Ok(self.active)
    }

    /// Rename palette `idx` (cleaned, must stay unique).
    pub fn rename(&mut self, idx: usize, name: &str) -> Result<(), String> {
        let clean = check_name(name)?;
        if idx >= self.palettes.len() {
            return Err("no such palette".to_string());
        }
        if self
            .palettes
            .iter()
            .enumerate()
            .any(|(i, p)| i != idx && p.name == clean)
        {
            return Err(format!("palette name taken: {clean}"));
        }
        self.palettes[idx].name = clean;
        Ok(())
    }

    /// Remove palette `idx`. Removing the last palette clears its colors
    /// instead (the set never empties, so the UI always has a target).
    pub fn remove(&mut self, idx: usize) -> bool {
        if idx >= self.palettes.len() {
            return false;
        }
        if self.palettes.len() == 1 {
            self.palettes[0].colors.clear();
            return true;
        }
        self.palettes.remove(idx);
        if self.active > idx {
            self.active -= 1;
        }
        self.active = self.active.min(self.palettes.len() - 1);
        true
    }

    /// Store a color in the active palette: skipping exact duplicates,
    /// honoring the per-palette cap.
    pub fn push_color(&mut self, c: [u8; 3]) -> Result<(), String> {
        let Some(pal) = self.active_mut() else {
            return Err("no active palette".to_string());
        };
        if pal.colors.contains(&c) {
            return Err("that color is already stored".to_string());
        }
        if pal.colors.len() >= MAX_PALETTE_COLORS {
            return Err(format!("swatch set is full ({MAX_PALETTE_COLORS})"));
        }
        pal.colors.push(c);
        Ok(())
    }

    /// Remove swatch `idx` from the active palette.
    pub fn remove_color(&mut self, idx: usize) -> bool {
        match self.active_mut() {
            Some(pal) if idx < pal.colors.len() => {
                pal.colors.remove(idx);
                true
            }
            _ => false,
        }
    }

    /// Empty the active palette (keeps the palette itself).
    pub fn clear_active(&mut self) {
        if let Some(pal) = self.active_mut() {
            pal.colors.clear();
        }
    }
}

impl Default for PaletteSet {
    fn default() -> Self {
        Self::new()
    }
}

fn check_palette(name: &str, colors: &[[u8; 3]]) -> Result<String, String> {
    let clean = check_name(name)?;
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
    let clean = Palette {
        name,
        colors: palette.colors.clone(),
    };
    let text = serde_json::to_string_pretty(&clean).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| e.to_string())
}

/// Load a `.pcpalette` file, validating name and color count.
pub fn open_palette(path: &std::path::Path) -> Result<Palette, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let raw: Palette = serde_json::from_str(&text).map_err(|_| "not a palette file".to_string())?;
    let name = check_palette(&raw.name, &raw.colors)?;
    Ok(Palette {
        name,
        colors: raw.colors,
    })
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
    let mut palette = Palette {
        name: clean,
        colors,
    };
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

/// Maximum swatches pulled from one image (keeps the import grid tidy;
/// the palette cap still applies downstream).
pub const MAX_IMAGE_COLORS: usize = 12;

/// Build a palette from RGBA pixels: 15-bit histogram buckets (5 bits per
/// channel) ranked by pixel count, each swatch the bucket's mean color.
/// Mostly-transparent pixels (alpha < 128) never vote — they contribute no
/// visible color. Ties break by bucket key so output is deterministic.
/// `max_colors` clamps to 1..=[`MAX_PALETTE_COLORS`].
pub fn palette_from_rgba(
    name: &str,
    pixels: &[u8],
    w: u32,
    h: u32,
    max_colors: usize,
) -> Result<Palette, String> {
    use std::collections::HashMap;
    let clean = check_name(name)?;
    if w == 0 || h == 0 {
        return Err("image has bad dimensions".to_string());
    }
    if pixels.len() != (w * h * 4) as usize {
        return Err("image has wrong pixel size".to_string());
    }
    let mut bins: HashMap<u16, (u32, u32, u32, u32)> = HashMap::new();
    for px in pixels.as_chunks::<4>().0 {
        if px[3] < 128 {
            continue;
        }
        let key = ((px[0] as u16 >> 3) << 10) | ((px[1] as u16 >> 3) << 5) | (px[2] as u16 >> 3);
        let e = bins.entry(key).or_insert((0, 0, 0, 0));
        e.0 += 1;
        e.1 += px[0] as u32;
        e.2 += px[1] as u32;
        e.3 += px[2] as u32;
    }
    if bins.is_empty() {
        return Err("image has no visible pixels".to_string());
    }
    let mut ranked: Vec<(u16, (u32, u32, u32, u32))> = bins.into_iter().collect();
    ranked.sort_by(|a, b| b.1 .0.cmp(&a.1 .0).then_with(|| a.0.cmp(&b.0)));
    let want = max_colors.clamp(1, MAX_PALETTE_COLORS).min(ranked.len());
    let colors = ranked[..want]
        .iter()
        .map(|(_, (n, r, g, b))| [(r / n) as u8, (g / n) as u8, (b / n) as u8])
        .collect();
    Ok(Palette {
        name: clean,
        colors,
    })
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
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };
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
    fn palette_from_image_extracts_dominant_colors() {
        // Four solid quadrants: every swatch lands exactly.
        let mut px = Vec::new();
        for y in 0..4u32 {
            for x in 0..4u32 {
                let c = match (x < 2, y < 2) {
                    (true, true) => [255, 0, 0, 255],
                    (false, true) => [0, 255, 0, 255],
                    (true, false) => [0, 0, 255, 255],
                    (false, false) => [255, 255, 255, 255],
                };
                px.extend_from_slice(&c);
            }
        }
        let pal = palette_from_rgba("Quads", &px, 4, 4, 12).unwrap();
        assert_eq!(pal.name, "Quads");
        let mut got = pal.colors.clone();
        got.sort();
        assert_eq!(
            got,
            vec![[0, 0, 255], [0, 255, 0], [255, 0, 0], [255, 255, 255]]
        );
        // The count caps, and transparent pixels never vote.
        let pal = palette_from_rgba("Quads", &px, 4, 4, 2).unwrap();
        assert_eq!(pal.colors.len(), 2);
        let mut ghost = px.clone();
        for i in 0..ghost.len() / 4 {
            ghost[i * 4 + 3] = 0;
        }
        assert!(palette_from_rgba("Ghost", &ghost, 4, 4, 12).is_err());
        assert!(palette_from_rgba("Empty", &[], 0, 0, 12).is_err());
        assert!(palette_from_rgba("  ", &px, 4, 4, 12).is_err());
        assert!(palette_from_rgba("Short", &px[..8], 4, 4, 12).is_err());
    }

    #[test]
    fn palette_rejects_garbage() {
        let dir = std::env::temp_dir();
        let path = dir.join("procraft-pal-bad.pcpalette");
        // Empty set.
        assert!(save_palette(
            &path,
            &Palette {
                name: "E".to_string(),
                colors: vec![]
            }
        )
        .is_err());
        // Empty name.
        assert!(save_palette(
            &path,
            &Palette {
                name: "  ".to_string(),
                colors: vec![[1, 2, 3]]
            }
        )
        .is_err());
        // Too many colors.
        let many = vec![[9u8, 9, 9]; MAX_PALETTE_COLORS + 1];
        assert!(save_palette(
            &path,
            &Palette {
                name: "Many".to_string(),
                colors: many
            }
        )
        .is_err());
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
        for rgb in [
            [255, 0, 0],
            [0, 255, 0],
            [0, 0, 255],
            [0, 0, 0],
            [255, 255, 255],
            [30, 144, 255],
        ] {
            let (h, s, l) = rgb_to_hsl(rgb);
            let back = hsl_to_rgb(h, s, l);
            for (a, b) in rgb.iter().zip(back.iter()) {
                assert!(
                    ((*a as i32) - (*b as i32)).abs() <= 1,
                    "{rgb:?} -> {back:?}"
                );
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

    #[test]
    fn palette_set_starts_with_my_colors() {
        let set = PaletteSet::new();
        assert_eq!(set.palettes.len(), 1);
        assert_eq!(set.active, 0);
        assert_eq!(set.active().unwrap().name, "My colors");
        assert!(set.active().unwrap().colors.is_empty());
    }

    #[test]
    fn palette_set_add_rename_select() {
        let mut set = PaletteSet::new();
        let a = set.add("Inks").unwrap();
        assert_eq!(a, 1);
        assert_eq!(set.active, 1);
        // Duplicate names de-duplicate, blanks fail, the set caps out.
        let b = set.add("Inks").unwrap();
        assert_eq!(set.palettes[b].name, "Inks (2)");
        assert!(set.add("   ").is_err());
        assert!(set.set_active(0));
        assert!(!set.set_active(99));
        assert_eq!(set.active, 0);
        assert!(set.rename(0, "Mine").is_ok());
        assert_eq!(set.palettes[0].name, "Mine");
        assert!(set.rename(0, "Inks").is_err());
        assert!(set.rename(0, "  ").is_err());
        assert!(set.rename(99, "X").is_err());
        for n in 0..MAX_PALETTES {
            let _ = set.add(&format!("P{n}"));
        }
        assert!(set.add("Overflow").is_err());
    }

    #[test]
    fn palette_set_remove_never_empties() {
        let mut set = PaletteSet::new();
        set.push_color([1, 2, 3]).unwrap();
        // Last palette: removal clears instead.
        assert!(set.remove(0));
        assert_eq!(set.palettes.len(), 1);
        assert!(set.palettes[0].colors.is_empty());
        // Earlier removal keeps the active palette pointed at itself.
        set.add("A").unwrap();
        set.add("B").unwrap();
        assert!(set.set_active(2));
        assert!(set.remove(0));
        assert_eq!(set.active().unwrap().name, "B");
        assert!(!set.remove(9));
    }

    #[test]
    fn palette_set_colors_validate() {
        let mut set = PaletteSet::new();
        assert!(set.push_color([10, 20, 30]).is_ok());
        assert!(set.push_color([10, 20, 30]).is_err());
        assert!(set.remove_color(0));
        assert!(!set.remove_color(0));
        for i in 0..MAX_PALETTE_COLORS as u8 {
            set.push_color([i, i, i]).unwrap();
        }
        assert!(set.push_color([1, 2, 4]).is_err());
        set.clear_active();
        assert!(set.active().unwrap().colors.is_empty());
    }
}
