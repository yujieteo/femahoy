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
                    let button = egui::Button::new(text).fill(fill).stroke(stroke);
                    if ui.add(button).on_hover_text(hint).clicked() {
                        clicked = Some(i);
                    }
                }
            });
        });
    clicked
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

/// The keyboard-shortcut help row: each key or mouse action, then what it does.
pub fn keys(ui: &mut egui::Ui, items: &[(&str, &str)]) {
    let line = hex(colors(ui).line);
    ui.horizontal_wrapped(|ui| {
        for (key, action) in items {
            egui::Frame::new()
                .stroke(egui::Stroke::new(1.0, line))
                .corner_radius(4.0)
                .inner_margin(egui::Margin::symmetric(5, 1))
                .show(ui, |ui| ui.small(*key));
            ui.weak(*action);
            ui.add_space(ui::SPACE[1]);
        }
    });
}

/// A small box of the colour `color`.
pub fn swatch(ui: &mut egui::Ui, color: [f32; 3]) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(18.0, 14.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 0.0, rgb(color));
}

/// The legend of a contour: the field name and its unit, where the values are, the colour
/// bands with their limits (the highest band at the top), and the smallest and largest value.
pub fn legend(ui: &mut egui::Ui, units: &str, field: &results::ScalarField, lo: f64, hi: f64) {
    let unit = view::unit_text(units, field.quantity);
    ui.strong(format!("{} ({unit})", field.name));
    ui.weak(match field.location {
        Location::Point => "At the nodes",
        Location::Cell => "Per element",
    });
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
    let limits = results::band_limits(lo, hi);
    let texts = view::format_limits(&limits);
    for i in (0..BANDS).rev() {
        band(ui, i, format!("{} to {}", texts[i], texts[i + 1]));
    }
    readout(ui, "Min", &view::format_value(lo));
    readout(ui, "Max", &view::format_value(hi));
}
