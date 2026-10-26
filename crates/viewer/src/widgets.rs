//! The components of the UI layer in egui, for the model view: the look from the tokens of
//! [`crate::ui`], the segmented control, the legend, the value readout and the help row. The
//! page and a report have the same components in CSS (`ui.css`).

use std::sync::Arc;

use eframe::egui;

use crate::results::{self, Location, BANDS, BAND_COLORS};
use crate::ui::{self, Colors};
use crate::view;

/// The font of the controls: DM Sans (SIL Open Font License 1.1, `fonts/OFL.txt`), the sans
/// font of the page.
const DM_SANS: &[u8] = include_bytes!("../fonts/DMSans.ttf");

pub fn hex(c: u32) -> egui::Color32 {
    egui::Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

pub fn rgb(c: [f32; 3]) -> egui::Color32 {
    let c = c.map(|x| (x * 255.0).round() as u8);
    egui::Color32::from_rgb(c[0], c[1], c[2])
}

fn colors(ui: &egui::Ui) -> &'static Colors {
    if ui.visuals().dark_mode {
        &ui::DARK
    } else {
        &ui::LIGHT
    }
}

/// The look of the page in the light and the dark appearance: the DM Sans font, and the
/// colours, type sizes, spacing, radius and hit target of the tokens. No widget moves: egui
/// has no reduced motion setting of the browser, so it has no animation.
pub fn style(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let sans = egui::FontData::from_static(DM_SANS);
    fonts.font_data.insert("DM Sans".into(), Arc::new(sans));
    if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
        family.insert(0, "DM Sans".into());
    }
    ctx.set_fonts(fonts);
    for (theme, c) in [
        (egui::Theme::Dark, &ui::DARK),
        (egui::Theme::Light, &ui::LIGHT),
    ] {
        ctx.style_mut_of(theme, |s| {
            use egui::{FontId, TextStyle};
            s.text_styles = [
                (TextStyle::Small, FontId::proportional(ui::SMALL)),
                (TextStyle::Body, FontId::proportional(ui::BODY)),
                (TextStyle::Button, FontId::proportional(ui::BODY)),
                (TextStyle::Heading, FontId::proportional(ui::TITLE)),
                (TextStyle::Monospace, FontId::monospace(ui::MONO)),
            ]
            .into();
            s.animation_time = 0.0;
            s.spacing.item_spacing = egui::vec2(ui::SPACE[1], ui::SPACE[1]);
            s.spacing.button_padding = egui::vec2(ui::SPACE[2], ui::SPACE[0]);
            s.spacing.interact_size.y = ui::TARGET;
            let v = &mut s.visuals;
            v.panel_fill = hex(c.bg);
            v.window_fill = hex(c.bg);
            v.window_stroke.color = hex(c.line);
            v.error_fg_color = hex(c.error);
            v.selection.bg_fill = hex(c.accent).gamma_multiply(0.35);
            v.selection.stroke.color = hex(c.text);
            v.widgets.noninteractive.bg_stroke.color = hex(c.line);
            v.override_text_color = Some(hex(c.text));
            for (w, fill) in [
                (&mut v.widgets.inactive, c.bg),
                (&mut v.widgets.hovered, c.raised),
                (&mut v.widgets.active, c.raised),
                (&mut v.widgets.open, c.raised),
            ] {
                w.bg_fill = hex(fill);
                w.weak_bg_fill = hex(fill);
                w.bg_stroke = egui::Stroke::new(1.0, hex(c.line));
                w.corner_radius = egui::CornerRadius::same(ui::RADIUS as u8);
            }
            // The rail of a slider has the fill of an inactive widget and no border. A button
            // has the weak fill.
            v.widgets.inactive.bg_fill = hex(c.raised);
        });
    }
}

/// The size of the box of a checkbox, the same as in `ui.css`.
const BOX: f32 = 18.0;

/// A checkbox with the look of the page checkbox (`ui.css`): an 18 pt square with a border in
/// the muted colour, which has more than 3:1 contrast (WCAG 1.4.11), and the accent fill when it
/// is on. The egui default has the faint border of a button and the radius makes it a circle.
pub fn checkbox(ui: &mut egui::Ui, on: &mut bool, text: &str) -> egui::Response {
    let c = colors(ui);
    let (fill, line) = if *on {
        (c.accent, c.accent)
    } else {
        (c.bg, c.muted)
    };
    // egui fits the label in the space that is left on the row and breaks it in a word. In a
    // wrapped row, a checkbox that does not fit goes to the next row in 1 piece.
    let width = egui::WidgetText::from(text)
        .into_galley(
            ui,
            Some(egui::TextWrapMode::Extend),
            f32::INFINITY,
            egui::TextStyle::Body,
        )
        .size()
        .x
        + BOX
        + ui.spacing().icon_spacing;
    if ui.layout().main_wrap && width > ui.available_size_before_wrap().x {
        ui.end_row();
    }
    ui.scope(|ui| {
        let s = ui.style_mut();
        s.spacing.icon_width = BOX;
        s.spacing.icon_width_inner = 10.0;
        let w = &mut s.visuals.widgets;
        for w in [
            &mut w.noninteractive,
            &mut w.inactive,
            &mut w.hovered,
            &mut w.active,
        ] {
            w.bg_fill = hex(fill);
            w.bg_stroke = egui::Stroke::new(1.5, hex(line));
            w.fg_stroke = egui::Stroke::new(2.0, hex(c.on_button));
            w.corner_radius = egui::CornerRadius::same(4);
        }
        ui.checkbox(on, text)
    })
    .inner
}

/// A segmented control: 1 segment for each item `(text, chosen)`. The chosen segment has the
/// background, a border and a heavier text. Returns the index of the clicked segment.
pub fn segmented(ui: &mut egui::Ui, items: &[(&str, bool, &str)]) -> Option<usize> {
    let c = colors(ui);
    let mut clicked = None;
    egui::Frame::new()
        .fill(hex(c.raised))
        .stroke(egui::Stroke::new(1.0, hex(c.line)))
        .corner_radius(ui::RADIUS)
        .inner_margin(2.0)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for (i, &(text, chosen, hint)) in items.iter().enumerate() {
                    let (text, fill, stroke) = if chosen {
                        let line = egui::Stroke::new(1.0, hex(c.line));
                        (egui::RichText::new(text).strong(), hex(c.bg), line)
                    } else {
                        let none = egui::Color32::TRANSPARENT;
                        (egui::RichText::new(text), none, egui::Stroke::NONE)
                    };
                    let button = egui::Button::new(text)
                        .fill(fill)
                        .stroke(stroke)
                        .min_size(egui::vec2(ui::TARGET, ui::TARGET));
                    if ui.add(button).on_hover_text(hint).clicked() {
                        clicked = Some(i);
                    }
                }
            });
        });
    clicked
}

/// A standard button with the shared hit target.
pub fn button(ui: &mut egui::Ui, text: &str, hint: &str) -> egui::Response {
    ui.add(egui::Button::new(text).min_size(egui::vec2(ui::TARGET, ui::TARGET)))
        .on_hover_text(hint)
}

/// A row of text with no control: it has the height of its text, not of a hit target.
pub fn text_row(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    ui.scope(|ui| {
        ui.spacing_mut().interact_size.y = 0.0;
        ui.horizontal(add);
    });
}

/// A value readout: the label, then the value in digits of equal width.
pub fn readout(ui: &mut egui::Ui, label: &str, value: &str) {
    text_row(ui, |ui| {
        ui.weak(label);
        ui.monospace(value);
    });
}

/// A small box of the colour `color`.
pub fn swatch(ui: &mut egui::Ui, color: [f32; 3]) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(18.0, 14.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 0.0, rgb(color));
}

/// The legend of a contour: the field name and its unit, where the values are, the colour
/// bands with their limits (the highest band at the top), and the smallest and largest value.
pub fn legend(
    ui: &mut egui::Ui,
    units: &str,
    field: &results::ScalarField,
    lo: f64,
    hi: f64,
    quantity: Option<&str>,
    fixed_range: bool,
) {
    let unit = view::unit_text(units, field.quantity);
    let name = if field.name == "NT" {
        "Temperature · NT"
    } else {
        &field.name
    };
    match quantity {
        Some("velocity") => {
            ui.strong("Speed (m/s)");
        }
        Some("kinematic-pressure") => {
            ui.strong("p/rho (m²/s²)");
        }
        _ => {
            ui.strong(format!("{name} ({unit})"));
        }
    }
    ui.weak(match field.location {
        Location::Point => "At the nodes",
        Location::Cell => "Per element",
    });
    if fixed_range {
        ui.weak("Fixed range from report");
    }
    let band = |ui: &mut egui::Ui, band: usize, text: String| {
        text_row(ui, |ui| {
            swatch(ui, BAND_COLORS[band]);
            ui.monospace(text);
        });
    };
    if hi <= lo {
        band(
            ui,
            BANDS / 2,
            format!("constant {}", view::format_value(lo)),
        );
        return;
    }
    let (min, max) = if fixed_range {
        ("Range min", "Range max")
    } else {
        ("Min", "Max")
    };
    // A short panel (the panel below the model on a phone) shows all bands as one bar on one
    // row: the smallest value, the bands (the lowest at the left), the largest value. The bands
    // are equal, so the two ends give their limits.
    if ui.clip_rect().height() < 400.0 {
        text_row(ui, |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.weak(max);
                ui.monospace(view::format_value(hi));
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.weak(min);
                    ui.monospace(view::format_value(lo));
                    let size = egui::vec2(ui.available_width(), 14.0);
                    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                    let width = rect.width() / BANDS as f32;
                    for (i, &color) in BAND_COLORS.iter().enumerate() {
                        let left = rect.left() + width * i as f32;
                        let cell = egui::Rect::from_x_y_ranges(left..=left + width, rect.y_range());
                        ui.painter().rect_filled(cell, 0.0, rgb(color));
                    }
                });
            });
        });
        return;
    }
    let limits = results::band_limits(lo, hi);
    let texts = view::format_limits(&limits);
    for i in (0..BANDS).rev() {
        band(ui, i, format!("{} to {}", texts[i], texts[i + 1]));
    }
    readout(ui, min, &view::format_value(lo));
    readout(ui, max, &view::format_value(hi));
}
