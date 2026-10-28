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

    let (
        control_h,
        gap,
        pad_x,
        pad_y,
        pad_x_card,
        pad_y_card,
        radius_control,
        radius_card,
        slider_track,
        type_body,
        type_small,
        type_heading,
    ) = match class {
        BarLayout::Wide => (26.0, 6.0, 6.0, 6.0, 6.0, 8.0, 8, 10, 6.0, 14.0, 12.0, 15.0),
        BarLayout::Tablet => (
            44.0, 8.0, 14.0, 10.0, 8.0, 10.0, 12, 14, 10.0, 15.0, 13.0, 16.0,
        ),
        BarLayout::Narrow => (
            44.0, 6.0, 10.0, 8.0, 8.0, 10.0, 12, 14, 10.0, 15.0, 13.0, 16.0,
        ),
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

/// What a pill shows and how it behaves.
pub(crate) struct PillSpec<'a> {
    pub(crate) glyph: Option<&'a str>,
    pub(crate) label: Option<&'a str>,
    pub(crate) selected: bool,
    pub(crate) enabled: bool,
}

/// Pill width: the text plus padding, but never shorter than one control,
/// so icon-only pills stay square rather than collapsing to a stub.
pub(crate) fn pill_width(t: &Tokens, text_width: f32) -> f32 {
    (text_width + 2.0 * t.pad_x).max(t.control_h)
}

/// True pill: fully rounded ends.
pub(crate) fn pill_radius(t: &Tokens) -> u8 {
    (t.control_h * 0.5).round() as u8
}

/// A pill button. The hit rect is the painted rect, so nothing is clickable
/// outside what is drawn.
pub(crate) fn pill_button(ui: &mut egui::Ui, t: &Tokens, spec: PillSpec<'_>) -> egui::Response {
    let shown = spec.glyph.or(spec.label).unwrap_or("");
    let font = FontId::proportional(t.type_body);
    let ink = if spec.enabled { t.text_hi } else { t.text_lo };
    let galley = ui.painter().layout_no_wrap(shown.to_owned(), font, ink);
    let width = pill_width(t, galley.size().x);

    let sense = if spec.enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, t.control_h), sense);
    let radius = CornerRadius::same(pill_radius(t));
    let hovered = spec.enabled && response.hovered();
    let fill = if spec.selected {
        t.accent
    } else if hovered {
        t.surface_3
    } else {
        t.surface_2
    };
    let painter = ui.painter();
    painter.rect_filled(rect, radius, fill);
    if hovered || spec.selected {
        painter.rect_stroke(
            rect,
            radius,
            Stroke::new(1.0, t.accent),
            egui::StrokeKind::Inside,
        );
    }
    let text_color = if spec.selected { Color32::WHITE } else { ink };
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        shown,
        FontId::proportional(t.type_body),
        text_color,
    );

    // Icon-only pills carry their name in a tooltip.
    match (spec.glyph, spec.label) {
        (Some(_), Some(label)) => response.on_hover_text(label),
        _ => response,
    }
}

/// Inner padding of a section card: vertical padding always, horizontal only
/// when the card is not `flush` (the swatch grid needs every pixel).
pub(crate) fn card_pad(t: &Tokens, flush: bool) -> egui::Vec2 {
    egui::vec2(if flush { 0.0 } else { t.pad_x_card }, t.pad_y_card)
}

/// Content width a card can offer inside a panel of `panel_width`
/// (`16.0` is the panel's own ~8 px margin on each side).
#[cfg(test)]
pub(crate) fn card_available_width(panel_width: f32, t: &Tokens, flush: bool) -> f32 {
    panel_width - 16.0 - 2.0 * card_pad(t, flush).x
}

/// A surface panel section: one rounded, stroked card per existing top-level
/// heading. Cards only wrap what was already there — they never re-cut it.
pub(crate) fn section_card(
    ui: &mut egui::Ui,
    t: &Tokens,
    title: Option<&str>,
    flush: bool,
    add: impl FnOnce(&mut egui::Ui),
) {
    section_card_actions(ui, t, title, flush, |_| {}, add);
}

/// A card whose title row also carries one or more right-aligned controls
/// (the panel collapse buttons), so a card does not spend a whole row on a
/// lone arrow.
pub(crate) fn section_card_actions(
    ui: &mut egui::Ui,
    t: &Tokens,
    title: Option<&str>,
    flush: bool,
    title_actions: impl FnOnce(&mut egui::Ui),
    add: impl FnOnce(&mut egui::Ui),
) {
    egui::Frame::NONE
        .fill(t.surface_2)
        .stroke(Stroke::new(1.0, t.stroke))
        .corner_radius(CornerRadius::same(t.radius_card))
        .inner_margin(egui::Margin::from(card_pad(t, flush)))
        .show(ui, |ui| {
            // Cards are one column: keep every card as wide as the panel.
            ui.set_width(ui.available_width());
            if let Some(title) = title {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(title.to_uppercase())
                            .font(FontId::proportional(t.type_small))
                            .color(t.text_lo),
                    );
                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        title_actions,
                    );
                });
                ui.add_space(6.0);
            }
            add(ui);
        });
}

/// Horizontal extent of a thick slider's rail: narrow enough that a bar of
/// controls still fits on desktop, wide enough for a finger to land on.
pub(crate) fn slider_track_width(t: &Tokens) -> f32 {
    if t.class == BarLayout::Wide {
        t.slider_width_wide
    } else {
        t.slider_width_touch
    }
}

/// Value at `x` px along a `width` px rail, clamped to the range. A
/// degenerate range (`lo == hi`) or a zero-width rail has one possible value.
pub(crate) fn value_from_track(x: f32, width: f32, range: &std::ops::RangeInclusive<f32>) -> f32 {
    let (lo, hi) = (*range.start(), *range.end());
    if hi <= lo || width <= 0.0 {
        return lo;
    }
    let frac = (x / width).clamp(0.0, 1.0);
    lo + frac * (hi - lo)
}

/// How much of the rail the accent fill covers, 0..=1.
pub(crate) fn fill_fraction(v: f32, range: &std::ops::RangeInclusive<f32>) -> f32 {
    let (lo, hi) = (*range.start(), *range.end());
    if hi <= lo {
        return 0.0;
    }
    ((v - lo) / (hi - lo)).clamp(0.0, 1.0)
}

/// A thick rail slider for the quick bar: label + value, `−`, the rail, `+`.
/// Only the rail is hit-tested, so the two steppers keep their own taps.
pub(crate) fn thick_slider(
    ui: &mut egui::Ui,
    t: &Tokens,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    step: f64,
    label: &str,
    default: f32,
) -> egui::Response {
    let (lo, hi) = (*range.start(), *range.end());
    let value_text = if step.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        format!("{value:.2}")
    };
    let btn_w = if t.class == BarLayout::Wide {
        24.0
    } else {
        t.control_h
    };
    let mut track: Option<egui::Response> = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
        ui.label(format!("{label} {value_text}"));
        if ui
            .add_sized([btn_w, t.control_h], egui::Button::new("−"))
            .clicked()
        {
            *value = (*value - step as f32).clamp(lo, hi);
        }

        let (rect, resp) = ui.allocate_exact_size(
            egui::vec2(slider_track_width(t), t.control_h),
            egui::Sense::click_and_drag(),
        );
        let radius = CornerRadius::same((t.slider_track / 2.0) as u8);
        let painter = ui.painter();
        painter.rect_filled(rect, radius, t.surface_3);
        let filled = rect.width() * fill_fraction(*value, &range);
        if filled > 0.0 {
            let fill = egui::Rect::from_min_size(rect.min, egui::vec2(filled, rect.height()));
            painter.rect_filled(fill, radius, t.accent);
        }
        if resp.clicked() || resp.dragged() {
            if let Some(pos) = resp.interact_pointer_pos() {
                *value = value_from_track(pos.x - rect.left(), rect.width(), &range);
            }
        }
        if resp.double_clicked() {
            *value = default;
        }
        resp.widget_info(|| {
            egui::WidgetInfo::slider(true, *value as f64, format!("{label} {value_text}"))
        });
        track = Some(resp);

        if ui
            .add_sized([btn_w, t.control_h], egui::Button::new("+"))
            .clicked()
        {
            *value = (*value + step as f32).clamp(lo, hi);
        }
    });
    track.expect("thick_slider always allocates its rail")
}

/// Three-state cycle for the top-bar toggle: Dark → Light → Paper → Dark.
pub(crate) fn next_theme(theme: Theme) -> Theme {
    match theme {
        Theme::Dark => Theme::Light,
        Theme::Light => Theme::Paper,
        Theme::Paper => Theme::Dark,
    }
}

/// What the toggle shows: the *current* theme, so the glyph reads as a state
/// (☀ light, ☾ dark, 🖹 paper) rather than as the action.
pub(crate) fn theme_glyph(theme: Theme) -> &'static str {
    match theme {
        Theme::Dark => "☾",
        Theme::Light => "☀",
        Theme::Paper => "🖹",
    }
}

/// The one pill that must never be folded into a menu: cycles Dark → Light →
/// Paper and returns true when it changed the theme.
pub(crate) fn theme_toggle(ui: &mut egui::Ui, t: &Tokens, theme: &mut Theme) -> bool {
    let next = next_theme(*theme);
    let response = pill_button(
        ui,
        t,
        PillSpec {
            glyph: Some(theme_glyph(*theme)),
            label: None,
            selected: false,
            enabled: true,
        },
    )
    .on_hover_text(format!(
        "{} theme · click for {}.",
        theme.name(),
        next.name()
    ));
    if response.clicked() {
        *theme = next;
        return true;
    }
    false
}

#[cfg(test)]
fn luminance_channel(c: u8) -> f32 {
    let c = c as f32 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// WCAG relative-luminance contrast ratio, 1.0 (identical) .. 21.0.
#[cfg(test)]
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
    fn pill_is_at_least_one_control_tall_and_half_height_rounded() {
        let t = tokens(Theme::Dark, BarLayout::Tablet);
        assert_eq!(pill_width(&t, 0.0), 44.0);
        // 8 px of label plus 14 px padding a side still fits inside the
        // touch-height floor, so short labels keep a square pill.
        assert_eq!(pill_width(&t, 8.0), 44.0);
        assert!(pill_width(&t, 80.0) >= 80.0 + 2.0 * t.pad_x);
        assert_eq!(pill_radius(&t), 22);

        // Wide is 26 px tall: floor 26, radius 13.
        let w = tokens(Theme::Dark, BarLayout::Wide);
        assert_eq!(pill_width(&w, 0.0), 26.0);
        assert_eq!(pill_radius(&w), 13);
    }

    #[test]
    fn flush_cards_leave_the_swatch_grid_its_full_224px() {
        let t = tokens(Theme::Dark, BarLayout::Wide);
        assert_eq!(card_pad(&t, false).x, 6.0);
        assert_eq!(card_pad(&t, true).x, 0.0);
        assert_eq!(card_available_width(240.0, &t, true), 224.0);
        assert!(card_available_width(240.0, &t, false) < 224.0);
    }

    #[test]
    fn track_position_maps_to_the_range_and_clamps() {
        let r = 0.0..=100.0;
        assert_eq!(value_from_track(0.0, 200.0, &r), 0.0);
        assert_eq!(value_from_track(100.0, 200.0, &r), 50.0);
        assert_eq!(value_from_track(200.0, 200.0, &r), 100.0);
        assert_eq!(value_from_track(-40.0, 200.0, &r), 0.0);
        assert_eq!(value_from_track(999.0, 200.0, &r), 100.0);
        assert_eq!(fill_fraction(25.0, &r), 0.25);
        assert_eq!(fill_fraction(500.0, &r), 1.0);
    }

    #[test]
    fn slider_hit_area_is_only_the_track() {
        let wide = tokens(Theme::Dark, BarLayout::Wide);
        let tablet = tokens(Theme::Dark, BarLayout::Tablet);
        assert_eq!(slider_track_width(&wide), 110.0);
        assert_eq!(slider_track_width(&tablet), 130.0);
        assert!(slider_track_width(&tablet) < tablet.control_h * 7.0);
    }

    #[test]
    fn toggle_cycles_dark_light_paper_with_one_glyph_each() {
        assert_eq!(next_theme(Theme::Dark), Theme::Light);
        assert_eq!(next_theme(Theme::Light), Theme::Paper);
        assert_eq!(next_theme(Theme::Paper), Theme::Dark);
        assert_eq!(theme_glyph(Theme::Dark), "☾");
        assert_eq!(theme_glyph(Theme::Light), "☀");
        assert_eq!(theme_glyph(Theme::Paper), "🖹");
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
