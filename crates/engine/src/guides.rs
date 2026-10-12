//! Drawing guides: grid overlays plus snap-assist math.
//!
//! Original clean-room code, GUI-free and unit-tested. The desktop app
//! draws [`Guides::overlay_lines`] over the canvas and runs pointer input
//! through [`Guides::snap_point`] before painting. Perspective vanishing
//! points are fixed (canvas center for 1-point; two horizon points for
//! 2-point) in this first version — draggable VPs are a later track.

/// What the guide overlay shows (and what snap assists to).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GuideKind {
    #[default]
    Off,
    Square,
    Isometric,
    OnePoint,
    TwoPoint,
}

impl GuideKind {
    pub const ALL: [GuideKind; 5] =
        [GuideKind::Off, GuideKind::Square, GuideKind::Isometric, GuideKind::OnePoint, GuideKind::TwoPoint];

    pub fn name(self) -> &'static str {
        match self {
            GuideKind::Off => "Off",
            GuideKind::Square => "2D grid",
            GuideKind::Isometric => "Isometric",
            GuideKind::OnePoint => "1-point",
            GuideKind::TwoPoint => "2-point",
        }
    }
}

/// Guide settings. View-assist only: deliberately not part of the project
/// file format (guides never bake into the artwork).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Guides {
    pub kind: GuideKind,
    /// Grid cell size in canvas pixels, 8..=512.
    pub spacing: f32,
    /// Snap pointer input to the guide lines while painting.
    pub snap: bool,
    /// Snap capture distance in canvas pixels, 2..=32.
    pub snap_px: f32,
    /// Overlay line opacity, 0.05..=1.0.
    pub opacity: f32,
}

impl Default for Guides {
    fn default() -> Self {
        Self { kind: GuideKind::Off, spacing: 64.0, snap: true, snap_px: 8.0, opacity: 0.35 }
    }
}

/// Ray step for perspective guides (15 degrees, Procreate-like density).
const RAY_STEP: f32 = std::f32::consts::PI / 12.0;
/// Isometric diagonals run at +/-30 degrees.
const ISO_TAN: f32 = 0.5773503; // tan(30deg)

impl Guides {
    fn spacing_clamped(self) -> f32 {
        self.spacing.clamp(8.0, 512.0)
    }

    fn snap_px_clamped(self) -> f32 {
        self.snap_px.clamp(2.0, 32.0)
    }

    /// Snap a canvas point to the active guides. Returns the point
    /// unchanged when guides are off, snap is disabled, or nothing is
    /// within capture distance.
    pub fn snap_point(self, w: f32, h: f32, x: f32, y: f32) -> (f32, f32) {
        if self.kind == GuideKind::Off || !self.snap || w <= 0.0 || h <= 0.0 {
            return (x, y);
        }
        match self.kind {
            GuideKind::Off => (x, y),
            GuideKind::Square => {
                let s = self.spacing_clamped();
                let t = self.snap_px_clamped();
                let nx = (x / s).round() * s;
                let ny = (y / s).round() * s;
                ((if (nx - x).abs() <= t { nx } else { x }), (if (ny - y).abs() <= t { ny } else { y }))
            }
            GuideKind::Isometric => snap_iso(self, x, y),
            GuideKind::OnePoint => snap_rays(self, &[(w * 0.5, h * 0.5)], x, y),
            GuideKind::TwoPoint => {
                snap_rays(self, &[(w * 0.5 - w * 0.75, h * 0.5), (w * 0.5 + w * 0.75, h * 0.5)], x, y)
            }
        }
    }

    /// Overlay segments in canvas coordinates, clipped to the canvas rect.
    /// The app maps these to screen space (`rect.min + p * zoom`).
    pub fn overlay_lines(self, w: f32, h: f32) -> Vec<[f32; 4]> {
        let mut out = Vec::new();
        if self.kind == GuideKind::Off || w <= 0.0 || h <= 0.0 {
            return out;
        }
        let s = self.spacing_clamped();
        match self.kind {
            GuideKind::Off => {}
            GuideKind::Square => {
                let nx = (w / s).ceil() as i32;
                for k in 0..=nx {
                    push_clipped(&mut out, k as f32 * s, 0.0, k as f32 * s, h, w, h);
                }
                let ny = (h / s).ceil() as i32;
                for k in 0..=ny {
                    push_clipped(&mut out, 0.0, k as f32 * s, w, k as f32 * s, w, h);
                }
            }
            GuideKind::Isometric => {
                // Verticals.
                let nx = (w / s).ceil() as i32;
                for k in 0..=nx {
                    push_clipped(&mut out, k as f32 * s, 0.0, k as f32 * s, h, w, h);
                }
                // Diagonals y = +/-x*tan30 + b, stepped so they cross the
                // verticals at grid nodes.
                let lo = (-w * ISO_TAN / s).floor() as i32 - 1;
                let hi = ((h + w * ISO_TAN) / s).ceil() as i32 + 1;
                for k in lo..=hi {
                    let b = k as f32 * s;
                    push_clipped(&mut out, 0.0, b, w, w * ISO_TAN + b, w, h);
                    push_clipped(&mut out, 0.0, b, w, -w * ISO_TAN + b, w, h);
                }
            }
            GuideKind::OnePoint => {
                emit_rays(&mut out, w * 0.5, h * 0.5, w, h);
            }
            GuideKind::TwoPoint => {
                emit_rays(&mut out, w * 0.5 - w * 0.75, h * 0.5, w, h);
                emit_rays(&mut out, w * 0.5 + w * 0.75, h * 0.5, w, h);
            }
        }
        out
    }
}

/// Snap to the isometric lattice: verticals plus two 30-degree families.
/// Each family is described by a unit normal and a perpendicular pitch;
/// the point projects onto the nearest line of the closest family.
fn snap_iso(g: Guides, x: f32, y: f32) -> (f32, f32) {
    let s = g.spacing_clamped();
    let t = g.snap_px_clamped();
    // (normal, pitch): verticals, +30deg, -30deg.
    let pitch = s * 0.8660254; // s * cos(30deg)
    let families = [
        ((1.0f32, 0.0f32), s),
        ((-0.5f32, 0.8660254f32), pitch),
        ((0.5f32, 0.8660254f32), pitch),
    ];
    let mut best: Option<(f32, (f32, f32))> = None;
    for ((nx, ny), d) in families {
        let along = x * nx + y * ny;
        let nearest = (along / d).round() * d;
        let dist = (nearest - along).abs();
        if best.map_or(true, |(bd, _)| dist < bd) {
            best = Some((dist, (x + (nearest - along) * nx, y + (nearest - along) * ny)));
        }
    }
    match best {
        Some((dist, pt)) if dist <= t => pt,
        _ => (x, y),
    }
}

/// Snap to the nearest vanishing-point ray (fixed 15-degree fans). Points
/// within capture of a VP collapse onto the VP itself.
fn snap_rays(g: Guides, vps: &[(f32, f32)], x: f32, y: f32) -> (f32, f32) {
    let t = g.snap_px_clamped();
    let mut best: Option<(f32, (f32, f32))> = None;
    for &(cx, cy) in vps {
        let (dx, dy) = (x - cx, y - cy);
        let r = (dx * dx + dy * dy).sqrt();
        if r <= t {
            return (cx, cy);
        }
        if r <= 0.0 {
            continue;
        }
        let ang = dy.atan2(dx);
        let nearest = (ang / RAY_STEP).round() * RAY_STEP;
        let arc = (nearest - ang).abs() * r;
        if best.map_or(true, |(ba, _)| arc < ba) {
            best = Some((arc, (cx + r * nearest.cos(), cy + r * nearest.sin())));
        }
    }
    match best {
        Some((arc, pt)) if arc <= t => pt,
        _ => (x, y),
    }
}

/// Emit a 15-degree ray fan from `(cx, cy)`, clipped to the canvas.
fn emit_rays(out: &mut Vec<[f32; 4]>, cx: f32, cy: f32, w: f32, h: f32) {
    let len = (w * w + h * h).sqrt();
    let n = (std::f32::consts::TAU / RAY_STEP).round() as i32;
    for k in 0..n {
        let a = k as f32 * RAY_STEP;
        push_clipped(&mut *out, cx, cy, cx + len * a.cos(), cy + len * a.sin(), w, h);
    }
}

fn push_clipped(out: &mut Vec<[f32; 4]>, x0: f32, y0: f32, x1: f32, y1: f32, w: f32, h: f32) {
    if let Some(seg) = clip_segment(x0, y0, x1, y1, w, h) {
        out.push(seg);
    }
}

/// Cohen-Sutherland clip to `[0, w] x [0, h]`. Returns `None` when the
/// segment lies fully outside.
fn clip_segment(x0: f32, y0: f32, x1: f32, y1: f32, w: f32, h: f32) -> Option<[f32; 4]> {
    const LEFT: u8 = 1;
    const RIGHT: u8 = 2;
    const BOTTOM: u8 = 4;
    const TOP: u8 = 8;
    let code = |x: f32, y: f32| {
        (if x < 0.0 { LEFT } else if x > w { RIGHT } else { 0 })
            | (if y < 0.0 { BOTTOM } else if y > h { TOP } else { 0 })
    };
    let (mut x0, mut y0, mut x1, mut y1) = (x0, y0, x1, y1);
    let (mut c0, mut c1) = (code(x0, y0), code(x1, y1));
    loop {
        if c0 == 0 && c1 == 0 {
            return Some([x0, y0, x1, y1]);
        }
        if c0 & c1 != 0 {
            return None;
        }
        let c = if c0 != 0 { c0 } else { c1 };
        let (mut x, mut y) = (0.0, 0.0);
        if c & TOP != 0 {
            x = x0 + (x1 - x0) * (h - y0) / (y1 - y0);
            y = h;
        } else if c & BOTTOM != 0 {
            x = x0 + (x1 - x0) * (0.0 - y0) / (y1 - y0);
            y = 0.0;
        } else if c & RIGHT != 0 {
            y = y0 + (y1 - y0) * (w - x0) / (x1 - x0);
            x = w;
        } else if c & LEFT != 0 {
            y = y0 + (y1 - y0) * (0.0 - x0) / (x1 - x0);
            x = 0.0;
        }
        if c == c0 {
            x0 = x;
            y0 = y;
            c0 = code(x0, y0);
        } else {
            x1 = x;
            y1 = y;
            c1 = code(x1, y1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(kind: GuideKind) -> Guides {
        Guides { kind, spacing: 64.0, snap: true, snap_px: 8.0, opacity: 0.35 }
    }

    #[test]
    fn off_guides_are_identity_and_empty() {
        let off = Guides::default();
        assert_eq!(off.kind, GuideKind::Off);
        assert_eq!(off.snap_point(128.0, 128.0, 10.0, 20.0), (10.0, 20.0));
        assert!(off.overlay_lines(128.0, 128.0).is_empty());
        // Snap toggle off also passes points through.
        let nosnap = Guides { snap: false, ..g(GuideKind::Square) };
        assert_eq!(nosnap.snap_point(128.0, 128.0, 63.0, 63.0), (63.0, 63.0));
    }

    #[test]
    fn square_grid_counts_and_snaps() {
        // 128px canvas at 64px cells: 3 verticals + 3 horizontals.
        assert_eq!(g(GuideKind::Square).overlay_lines(128.0, 128.0).len(), 6);
        let guides = g(GuideKind::Square);
        // Near a line: snaps that axis only.
        assert_eq!(guides.snap_point(128.0, 128.0, 66.0, 30.0), (64.0, 30.0));
        assert_eq!(guides.snap_point(128.0, 128.0, 30.0, 127.0), (30.0, 128.0));
        // Far from any line: untouched.
        assert_eq!(guides.snap_point(128.0, 128.0, 30.0, 40.0), (30.0, 40.0));
    }

    #[test]
    fn iso_grid_has_verticals_and_snaps() {
        let lines = g(GuideKind::Isometric).overlay_lines(128.0, 128.0);
        assert!(lines.len() > 6, "iso adds diagonals: {}", lines.len());
        assert!(lines.iter().any(|&[x0, _, x1, _]| (x0 - x1).abs() < 0.001));
        // Every segment stays inside the canvas.
        for &[x0, y0, x1, y1] in &lines {
            for v in [x0, y0, x1, y1] {
                assert!(v >= -0.01 && v <= 128.01, "{v}");
            }
        }
        // Near a vertical (away from diagonal crossings): x snaps, y survives.
        let (sx, sy) = g(GuideKind::Isometric).snap_point(256.0, 256.0, 66.0, 64.0);
        assert!((sx - 64.0).abs() < 0.01 && (sy - 64.0).abs() < 0.01, "{sx} {sy}");
        // Mid-cell: untouched.
        assert_eq!(g(GuideKind::Isometric).snap_point(256.0, 256.0, 32.0, 200.0), (32.0, 200.0));
    }

    #[test]
    fn one_point_fan_and_snap() {
        // 24 rays at 15 degrees.
        assert_eq!(g(GuideKind::OnePoint).overlay_lines(200.0, 200.0).len(), 24);
        let guides = g(GuideKind::OnePoint);
        // A point exactly on the east ray stays put.
        let (sx, sy) = guides.snap_point(200.0, 200.0, 150.0, 100.0);
        assert!((sx - 150.0).abs() < 0.02 && (sy - 100.0).abs() < 0.02, "{sx} {sy}");
        // A point just off the ray collapses onto it (y -> horizon).
        let (qx, qy) = guides.snap_point(200.0, 200.0, 150.0, 102.0);
        assert!((qy - 100.0).abs() < (102.0 - 100.0), "{qx} {qy}");
        // A point midway between rays at large radius stays (arc > capture).
        assert_eq!(guides.snap_point(200.0, 200.0, 192.0, 138.0), (192.0, 138.0));
        // At the vanishing point: collapses onto it.
        assert_eq!(guides.snap_point(200.0, 200.0, 102.0, 101.0), (100.0, 100.0));
    }

    #[test]
    fn two_point_covers_both_fans() {
        // Off-canvas VPs: only rays reaching the canvas survive clipping.
        let lines = g(GuideKind::TwoPoint).overlay_lines(200.0, 200.0);
        assert!(!lines.is_empty() && lines.len() < 48, "{}", lines.len());
        for &[x0, y0, x1, y1] in &lines {
            for v in [x0, y0, x1, y1] {
                assert!(v >= -0.01, "{v}");
            }
            assert!(x0 <= 200.01 && x1 <= 200.01 && y0 <= 200.01 && y1 <= 200.01);
        }
        // Point on vp1's east ray stays (vp1 sits off-canvas left).
        let guides = g(GuideKind::TwoPoint);
        let (sx, sy) = guides.snap_point(200.0, 200.0, 60.0, 100.0);
        assert!((sy - 100.0).abs() < 0.5, "{sx} {sy}");
        // Far off-ray point survives (nearest arc > capture).
        assert_eq!(guides.snap_point(200.0, 200.0, 100.0, 40.0), (100.0, 40.0));
    }
}
