//! Clean-room approximations of Procreate's built-in brush textures.
//!
//! A Procreate brush may reference a *bundled* asset that ships with the
//! Procreate app instead of the brush itself (`bundledShapePath` /
//! `bundledGrainPath`, e.g. `Brush-Artery-Burst.jpg`). Those files are
//! proprietary and are not part of a `.brushset`, so they cannot be read
//! from the archive. This module generates deterministic, original
//! stand-ins that reproduce the *character* of the referenced asset: a
//! hard-edged round tip, a square bristle tip, a bleeding blob, or a
//! speckled charcoal grain. No Procreate pixels are copied.

/// Deterministic 0.0..=1.0 hash for lattice noise.
fn hash01(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32)
        .wrapping_mul(374_761_393)
        .wrapping_add((y as u32).wrapping_mul(668_265_263))
        .wrapping_add(seed.wrapping_mul(2_246_822_519));
    h ^= h >> 13;
    h = h.wrapping_mul(1_274_126_177);
    h ^= h >> 16;
    (h >> 8) as f32 / 16_777_215.0
}

/// Periodic value noise on an integer lattice of `period` cells. The noise
/// tiles seamlessly, which matters because the engine repeats the texture
/// across the canvas.
fn value_noise(x: f32, y: f32, period: i32, seed: u32) -> f32 {
    let period = period.max(1);
    let x0 = x.floor();
    let y0 = y.floor();
    let fx = x - x0;
    let fy = y - y0;
    // Smoothstep keeps the noise free of lattice creases.
    let sx = fx * fx * (3.0 - 2.0 * fx);
    let sy = fy * fy * (3.0 - 2.0 * fy);
    let wrap = |v: f32| (v as i32).rem_euclid(period);
    let (ix, iy) = (wrap(x0), wrap(y0));
    let n00 = hash01(ix, iy, seed);
    let n10 = hash01(ix + 1, iy, seed);
    let n01 = hash01(ix, iy + 1, seed);
    let n11 = hash01(ix + 1, iy + 1, seed);
    let top = n00 + (n10 - n00) * sx;
    let bottom = n01 + (n11 - n01) * sx;
    top + (bottom - top) * sy
}

/// Fractal sum of [`value_noise`]; every octave doubles the frequency and
/// keeps the same period so the result still tiles.
fn fbm(x: f32, y: f32, period: i32, octaves: u32, seed: u32) -> f32 {
    let mut total = 0.0;
    let mut amplitude = 1.0;
    let mut norm = 0.0;
    let mut freq = 1.0;
    for octave in 0..octaves {
        total += amplitude * value_noise(x * freq, y * freq, period, seed + octave);
        norm += amplitude;
        amplitude *= 0.5;
        freq *= 2.0;
    }
    total / norm.max(f32::EPSILON)
}

/// Character of one built-in grain asset.
struct GrainSpec {
    /// Lattice cells across the texture. Larger means coarser clumps.
    cells: f32,
    /// Noise value below which the grain drops out entirely.
    threshold: f32,
    /// Width of the soft transition around the threshold.
    softness: f32,
    /// Extra fine speckle mixed on top, 0.0 ..= 1.0.
    speckle: f32,
    seed: u32,
}

/// Look up the character of a built-in grain by file name. Unknown names get
/// a neutral fine speckle.
fn grain_spec(name: &str) -> GrainSpec {
    let lower = name.to_ascii_lowercase();
    if lower.contains("charcoal") {
        // 6B charcoal: dense, fine tooth with the odd dropout.
        GrainSpec {
            cells: 150.0,
            threshold: 0.34,
            softness: 0.10,
            speckle: 0.45,
            seed: 0x0c4a_2c1b,
        }
    } else if lower.contains("paper") {
        // Paper: fine tooth with slightly larger pits.
        GrainSpec {
            cells: 90.0,
            threshold: 0.34,
            softness: 0.10,
            speckle: 0.30,
            seed: 0x9a17_3b52,
        }
    } else if lower.contains("bonobo") {
        // Bonobo: mottled blotches, coarser than the pencil grains.
        GrainSpec {
            cells: 44.0,
            threshold: 0.40,
            softness: 0.12,
            speckle: 0.35,
            seed: 0x51d3_7e09,
        }
    } else if lower.contains("noise") {
        // Artery noise: very fine, even speckle.
        GrainSpec {
            cells: 200.0,
            threshold: 0.33,
            softness: 0.08,
            speckle: 0.55,
            seed: 0x2f6b_1a4d,
        }
    } else {
        GrainSpec {
            cells: 110.0,
            threshold: 0.35,
            softness: 0.12,
            speckle: 0.35,
            seed: 0x7a41_c905,
        }
    }
}

/// Generate a grayscale grain texture for a built-in Procreate grain.
///
/// White (255) keeps the ink, black (0) drops it out, matching how the
/// engine applies `grain_tip` (see `Brush::grain`).
pub(super) fn bundled_grain(name: &str, side: u32, grain_scale: f32) -> (u32, u32, Vec<u8>) {
    let side = side.max(2);
    let spec = grain_spec(name);
    // `grain_scale` magnifies the texture on the canvas, so a coarser brush
    // setting is served by a finer lattice here.
    let cells = (spec.cells / grain_scale.clamp(0.25, 4.0)).max(2.0);
    let period = cells.round() as i32;
    let mut mask = Vec::with_capacity((side * side) as usize);
    for y in 0..side {
        for x in 0..side {
            let u = x as f32 / side as f32 * period as f32;
            let v = y as f32 / side as f32 * period as f32;
            let mut n = fbm(u, v, period, 3, spec.seed);
            if spec.speckle > 0.0 {
                let fine = hash01(x as i32, y as i32, spec.seed ^ 0x5bf0_3635);
                n = (n * (1.0 - spec.speckle) + fine * spec.speckle).clamp(0.0, 1.0);
            }
            // Soft threshold: below the threshold the grain is fully gone.
            let t = ((n - spec.threshold) / spec.softness.max(0.01)).clamp(0.0, 1.0);
            let ink = t * t * (3.0 - 2.0 * t);
            mask.push((ink * 255.0).round() as u8);
        }
    }
    (side, side, mask)
}

/// Generate a grayscale stamp tip for a built-in Procreate shape.
///
/// White (255) is ink. The result is a square mask of `side` pixels.
pub(super) fn bundled_shape(name: &str, side: u32) -> (u32, u32, Vec<u8>) {
    let side = side.max(2);
    let lower = name.to_ascii_lowercase();
    let radius = side as f32 * 0.5;
    let mut mask = vec![0u8; (side * side) as usize];
    for y in 0..side {
        for x in 0..side {
            let dx = x as f32 + 0.5 - radius;
            let dy = y as f32 + 0.5 - radius;
            let value = if lower.contains("square") || lower.contains("acrylic") {
                // Flat bristle tip: a square with a soft edge and vertical
                // bristle streaks.
                let half = radius * 0.92;
                let edge_x = (half - dx.abs()).min(half - dy.abs());
                let bristle =
                    0.72 + 0.28 * value_noise((dx / radius * 6.0).floor(), 0.0, 16, 0x71c3_9a05);
                (edge_x / (radius * 0.12)).clamp(0.0, 1.0) * bristle
            } else if lower.contains("bleed") {
                // Bleeding blob: an irregular, soft-edged pool.
                let wobble = 0.82
                    + 0.18
                        * fbm(
                            (dx / radius * 3.0) + 8.0,
                            (dy / radius * 3.0) + 8.0,
                            8,
                            2,
                            0x3d9e_1c74,
                        );
                let d = (dx.hypot(dy) / (radius * wobble)).max(0.0);
                (1.0 - d).clamp(0.0, 1.0).powf(0.55)
            } else if lower.contains("artery") {
                // Artery burst: a soft round core with faint radiating
                // streaks from the bundled burst shape.
                let d = (dx.hypot(dy) / radius).max(0.0);
                let angle = dy.atan2(dx);
                let rays = 0.85 + 0.15 * value_noise(angle * 6.0, d * 2.0, 64, 0x19b7_e504);
                (1.0 - d).clamp(0.0, 1.0).powf(0.8) * rays
            } else {
                // Medium-hard round: a solid disc with a soft-ish edge.
                let d = dx.hypot(dy) / (radius * 0.98);
                ((1.0 - d) / 0.18).clamp(0.0, 1.0)
            };
            mask[(y * side + x) as usize] = (value.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
    (side, side, mask)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn coverage(mask: &[u8], threshold: u8) -> usize {
        mask.iter().filter(|value| **value >= threshold).count()
    }

    #[test]
    fn medium_hard_round_is_solid_to_the_edge() {
        let side = 64usize;
        let (w, h, mask) = bundled_shape("Brush-Medium-Hard.jpg", side as u32);
        assert_eq!((w as usize, h as usize), (side, side));
        assert_eq!(mask.len(), side * side);
        assert_eq!(mask[32 * side + 32], 255, "center must be fully inked");
        // The tip must stay a disc, not shrink into a dot at the center.
        let disc = coverage(&mask, 128);
        assert!(disc > side * side / 2, "disc covers {disc} px");
        assert_eq!(mask[0], 0, "outside the radius must be empty");
    }

    #[test]
    fn acrylic_square_is_flat_and_square() {
        let side = 64usize;
        let (_, _, mask) = bundled_shape("Brush-Acrylic-Square.jpg", side as u32);
        // Bristle streaks modulate the flat face, so the center is bright but
        // not necessarily 255.
        assert!(mask[32 * side + 32] > 200);
        // Extreme corners fall outside the square, mid-edges stay inked.
        assert_eq!(mask[0], 0);
        assert!(mask[32 * side + 6] > 128, "square edge must carry ink");
    }

    #[test]
    fn bleed_and_burst_shapes_are_soft_blobs() {
        for name in ["Brush-Bleed.jpg", "Brush-Artery-Burst.jpg"] {
            let (_, _, mask) = bundled_shape(name, 64);
            let inked = coverage(&mask, 32);
            assert!(inked > 400, "{name} covers only {inked} px");
            assert!(mask[32 * 64 + 32] >= 200, "{name} center is weak");
            assert_eq!(mask[0], 0, "{name} must not fill the corners");
        }
    }

    #[test]
    fn shapes_are_deterministic_and_name_specific() {
        let (_, _, round) = bundled_shape("Brush-Medium-Hard.jpg", 64);
        let (_, _, again) = bundled_shape("Brush-Medium-Hard.jpg", 64);
        let (_, _, square) = bundled_shape("Brush-Acrylic-Square.jpg", 64);
        assert_eq!(round, again);
        assert_ne!(round, square);
    }

    #[test]
    fn grain_keeps_ink_and_creates_true_holes() {
        let side = 128usize;
        for name in ["Charcoal-6B", "Paper-Mush", "Bonobo", "Artery-Noise"] {
            let (w, h, mask) = bundled_grain(name, side as u32, 1.0);
            assert_eq!((w as usize, h as usize), (side, side));
            assert_eq!(mask.len(), side * side);
            let holes = mask.iter().filter(|value| **value == 0).count();
            let ink = coverage(&mask, 200);
            assert!(holes > 256, "{name} has only {holes} empty pixels");
            assert!(ink > 512, "{name} keeps only {ink} inked pixels");
            assert_eq!(*mask.iter().min().unwrap(), 0, "{name} needs holes");
            assert!(mask.iter().max().unwrap() >= &250, "{name} needs solid ink");
        }
    }

    #[test]
    fn grain_names_have_their_own_texture() {
        let (_, _, charcoal) = bundled_grain("Charcoal-6B", 64, 1.0);
        let (_, _, bonobo) = bundled_grain("Bonobo", 64, 1.0);
        assert_ne!(charcoal, bonobo);
    }
}
