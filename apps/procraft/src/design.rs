//! Design tokens and the style pipeline: one place owns every colour,
//! radius, spacing, control height and type size, per theme and per device
//! class. `lib.rs` only wires it; the primitives land here as they are
//! built.

use crate::{BarLayout, Theme};
use egui::{Color32, CornerRadius, FontId, Stroke, Style, TextStyle};

/// One design decision set for a (theme, device class) pair.
pub(crate) struct Tokens {
    pub(crate) class: BarLayout,
    pub(crate) surface_0: Color32,
    pub(crate) surface_1: Color32,
    pub(crate) surface_2: Color32,
    pub(crate) surface_3: Color32,
    pub(crate) stroke: Color32,
    pub(crate) text_hi: Color32,
    pub(crate) text_lo: Color32,
    pub(crate) accent: Color32,
    pub(crate) accent_soft: Color32,
    pub(crate) danger: Color32,
    pub(crate) radius_control: u8,
    pub(crate) radius_card: u8,
    pub(crate) pad_x: f32,
    pub(crate) pad_y: f32,
    pub(crate) pad_x_card: f32,
    pub(crate) pad_y_card: f32,
    pub(crate) gap: f32,
    pub(crate) control_h: f32,
    pub(crate) slider_track: f32,
    pub(crate) slider_width_wide: f32,
    pub(crate) slider_width_touch: f32,
    pub(crate) type_body: f32,
    pub(crate) type_small: f32,
    pub(crate) type_heading: f32,
}

pub(crate) fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

pub(crate) fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

pub(crate) fn tokens(theme: Theme, class: BarLayout) -> Tokens {
    let (surface_0, surface_1, surface_2, surface_3, stroke, text_hi, text_lo, accent, danger) =
        match theme {
            Theme::Dark => (
                rgb(0x13, 0x13, 0x15),
                rgb(0x1B, 0x1B, 0x1E),
                rgb(0x26, 0x26, 0x2A),
                rgb(0x33, 0x33, 0x3A),
                rgb(0x3D, 0x3D, 0x45),
                rgb(0xF0, 0xF0, 0xF3),
                rgb(0x9C, 0x9C, 0xA6),
                rgb(0x4C, 0x8D, 0xFF),
                rgb(0xFF, 0x6B, 0x6B),
            ),
            Theme::Light => (
                rgb(0xED, 0xEF, 0xF2),
                rgb(0xF7, 0xF8, 0xFA),
                rgb(0xFF, 0xFF, 0xFF),
                rgb(0xDF, 0xE3, 0xE9),
                rgb(0xCD, 0xD3, 0xDB),
                rgb(0x16, 0x18, 0x1C),
                rgb(0x62, 0x6A, 0x76),
                rgb(0x2F, 0x6F, 0xEB),
                rgb(0xC0, 0x39, 0x2B),
            ),
            Theme::Paper => (
                rgb(0xF4, 0xEE, 0xE4),
                rgb(0xFA, 0xF6, 0xEE),
                rgb(0xFF, 0xFD, 0xF8),
                rgb(0xE7, 0xDC, 0xC9),
                rgb(0xD6, 0xC9, 0xB3),
                rgb(0x2A, 0x23, 0x1B),
                rgb(0x6E, 0x62, 0x52),
                rgb(0xB5, 0x65, 0x1D),
                rgb(0xA6, 0x3A, 0x21),
            ),
        };

    let (control_h, gap, pad_x, pad_y, pad_x_card, pad_y_card, radius_control, radius_card, slider_track, type_body, type_small, type_heading) =
        match class {
            BarLayout::Wide => (26.0, 6.0, 10.0, 6.0, 6.0, 8.0, 8, 10, 6.0, 14.0, 12.0, 15.0),
            BarLayout::Tablet => (44.0, 8.0, 14.0, 10.0, 8.0, 10.0, 12, 14, 10.0, 15.0, 13.0, 16.0),
            BarLayout::Narrow => (44.0, 6.0, 10.0, 8.0, 8.0, 10.0, 12, 14, 10.0, 15.0, 13.0, 16.0),
        };

    Tokens {
        class,
        surface_0,
        surface_1,
        surface_2,
        surface_3,
        stroke,
        text_hi,
        text_lo,
        accent,
        accent_soft: rgba(accent.r(), accent.g(), accent.b(), 61),
        danger,
        radius_control,
        radius_card,
        pad_x,
        pad_y,
        pad_x_card,
        pad_y_card,
        gap,
        control_h,
        slider_track,
        slider_width_wide: 110.0,
        slider_width_touch: 130.0,
        type_body,
        type_small,
        type_heading,
    }
}

fn paint_widget(
    w: &mut egui::style::WidgetVisuals,
    weak_bg_fill: Color32,
    bg_fill: Color32,
    bg_stroke: Stroke,
    fg: Color32,
    radius: u8,
) {
    w.weak_bg_fill = weak_bg_fill;
    w.bg_fill = bg_fill;
    w.bg_stroke = bg_stroke;
    w.fg_stroke = Stroke::new(1.0, fg);
    w.corner_radius = CornerRadius::same(radius);
    w.expansion = 0.0;
}

impl Tokens {
    /// Convert this token set into a complete egui style. The palette is
    /// written unconditionally, so the result never depends on what the
    /// base style's own dark/light mode was.
    pub(crate) fn apply(&self, base: &Style) -> Style {
        let mut s = base.clone();
        let r = self.radius_control;

        s.visuals.panel_fill = self.surface_1;
        s.visuals.window_fill = self.surface_2;
        s.visuals.extreme_bg_color = self.surface_0;
        s.visuals.faint_bg_color = self.surface_2;
        s.visuals.window_stroke = Stroke::new(1.0, self.stroke);

        paint_widget(
            &mut s.visuals.widgets.noninteractive,
            self.surface_1,
            self.surface_1,
            Stroke::NONE,
            self.text_hi,
            r,
        );
        paint_widget(
            &mut s.visuals.widgets.inactive,
            self.surface_2,
            self.surface_3,
            Stroke::new(1.0, self.stroke),
            self.text_hi,
            r,
        );
        paint_widget(
            &mut s.visuals.widgets.hovered,
            self.surface_3,
            self.surface_3,
            Stroke::new(1.0, self.accent),
            self.text_hi,
            r,
        );
        paint_widget(
            &mut s.visuals.widgets.active,
            self.surface_3,
            self.surface_3,
            Stroke::new(1.0, self.accent),
            self.text_hi,
            r,
        );
        paint_widget(
            &mut s.visuals.widgets.open,
            self.surface_3,
            self.surface_3,
            Stroke::new(1.0, self.accent),
            self.text_hi,
            r,
        );

        s.visuals.selection.bg_fill = self.accent_soft;
        s.visuals.selection.stroke = Stroke::new(1.0, self.accent);
        s.visuals.override_text_color = None;

        s.spacing.item_spacing = egui::vec2(self.gap, self.gap);
        s.spacing.button_padding = egui::vec2(self.pad_x, self.pad_y);
        s.spacing.interact_size = egui::vec2(self.control_h * 1.4, self.control_h);
        s.spacing.slider_rail_height = self.slider_track;
        s.spacing.slider_width = 140.0;

        s.text_styles
            .insert(TextStyle::Body, FontId::proportional(self.type_body));
        s.text_styles
            .insert(TextStyle::Button, FontId::proportional(self.type_body));
        s.text_styles
            .insert(TextStyle::Small, FontId::proportional(self.type_small));
        s.text_styles
            .insert(TextStyle::Heading, FontId::proportional(self.type_heading));

        s
    }
}

fn luminance_channel(c: u8) -> f32 {
    let c = c as f32 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// WCAG relative-luminance contrast ratio, 1.0 (identical) .. 21.0.
pub(crate) fn contrast_ratio(a: Color32, b: Color32) -> f32 {
    let la = 0.2126 * luminance_channel(a.r())
        + 0.7152 * luminance_channel(a.g())
        + 0.0722 * luminance_channel(a.b());
    let lb = 0.2126 * luminance_channel(b.r())
        + 0.7152 * luminance_channel(b.g())
        + 0.0722 * luminance_channel(b.b());
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BarLayout, Theme};

    #[test]
    fn touch_classes_get_finger_sized_controls() {
        assert_eq!(tokens(Theme::Dark, BarLayout::Wide).control_h, 26.0);
        assert_eq!(tokens(Theme::Dark, BarLayout::Tablet).control_h, 44.0);
        assert_eq!(tokens(Theme::Dark, BarLayout::Narrow).control_h, 44.0);
        assert_eq!(tokens(Theme::Dark, BarLayout::Tablet).slider_track, 10.0);
        assert_eq!(tokens(Theme::Dark, BarLayout::Wide).slider_track, 6.0);
    }

    #[test]
    fn secondary_text_clears_aa_contrast_on_panels() {
        for theme in [Theme::Dark, Theme::Light, Theme::Paper] {
            let t = tokens(theme, BarLayout::Wide);
            let ratio = contrast_ratio(t.text_lo, t.surface_1);
            assert!(ratio >= 4.5, "{theme:?} text_lo on surface_1 = {ratio:.2}");
        }
    }

    #[test]
    fn primary_text_clears_aaa_contrast_on_panels() {
        for theme in [Theme::Dark, Theme::Light, Theme::Paper] {
            let t = tokens(theme, BarLayout::Wide);
            let ratio = contrast_ratio(t.text_hi, t.surface_1);
            assert!(ratio >= 7.0, "{theme:?} text_hi on surface_1 = {ratio:.2}");
        }
    }

    #[test]
    fn device_class_changes_metrics_so_the_cache_must_key_on_both() {
        let wide = tokens(Theme::Dark, BarLayout::Wide);
        let tablet = tokens(Theme::Dark, BarLayout::Tablet);
        assert_ne!(wide.control_h, tablet.control_h);
        assert_ne!(wide.pad_x, tablet.pad_x);
    }

    #[test]
    fn apply_writes_the_palette_and_metrics_into_the_style() {
        let t = tokens(Theme::Dark, BarLayout::Wide);
        let s = t.apply(&egui::Style::default());
        assert_eq!(s.visuals.panel_fill, t.surface_1);
        assert_eq!(s.visuals.extreme_bg_color, t.surface_0);
        assert_eq!(s.visuals.widgets.inactive.bg_stroke.color, t.stroke);
        assert_eq!(s.spacing.interact_size.y, 26.0);
        assert_eq!(s.spacing.slider_rail_height, 6.0);
        // The palette wins over whatever the base style was, so writing the
        // Light slot with Paper never inherits a dark base.
        let paper = tokens(Theme::Paper, BarLayout::Wide);
        let dark_base = tokens(Theme::Dark, BarLayout::Wide).apply(&egui::Style::default());
        assert_eq!(paper.apply(&dark_base).visuals.panel_fill, paper.surface_1);
    }
}
