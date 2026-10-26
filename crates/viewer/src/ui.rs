//! The 1 UI layer of femahoy: the design tokens of each view. The page of `fh serve`
//! (`crates/fh/src/daemon/page.html`) and a report (`report.html`) get them as CSS from
//! [`css`], with the components of `ui.css`. The model view gets them in `widgets`, with the
//! same components in egui. A new view uses these tokens and components, and adds no colour,
//! size or control style of its own (DESIGN.md section 8).
//!
//! The colours are those of the site teoyujie.org. The site has no dark theme: the dark colours
//! are the light colours turned over. `series` are the series of the plots, in a fixed order
//! that keeps adjacent series apart for colour-blind readers.

/// The colours of 1 appearance, as `0xRRGGBB`.
pub struct Colors {
    pub bg: u32,
    pub side: u32,
    pub raised: u32,
    pub line: u32,
    pub code: u32,
    pub text: u32,
    pub muted: u32,
    pub accent: u32,
    pub error: u32,
    /// The fill of a primary button, and its text.
    pub button: u32,
    pub on_button: u32,
    pub series: [u32; 8],
}

pub const LIGHT: Colors = Colors {
    bg: 0xf4f1ea,
    side: 0xefebe2,
    raised: 0xe6e0d3,
    line: 0xd8d2c4,
    code: 0xebe6dc,
    text: 0x1c1b18,
    muted: 0x5c5950,
    accent: 0xc4613f,
    error: 0xb3261e,
    button: 0x1c1b18,
    on_button: 0xf4f1ea,
    series: [
        0x2a6fc4, 0xc4613f, 0x138a5f, 0xa8740a, 0xc2457a, 0x2f7d1f, 0x6250d6, 0xc43d3c,
    ],
};

pub const DARK: Colors = Colors {
    bg: 0x1c1b18,
    side: 0x201f1b,
    raised: 0x2e2c27,
    line: 0x3a3730,
    code: 0x252420,
    text: 0xece8df,
    muted: 0xa39e92,
    accent: 0xe0805c,
    error: 0xf2877b,
    button: 0xece8df,
    on_button: 0x1c1b18,
    series: [
        0x3987e5, 0xd95926, 0x199e70, 0xc98500, 0xd55181, 0x3a9d3a, 0x9085e9, 0xe66767,
    ],
};

/// The type sizes, in CSS pixels (points in egui): a note or a hint, the text of the controls
/// and the chat, the text of a report, and a title.
pub const SMALL: f32 = 13.0;
pub const BODY: f32 = 15.0;
pub const READ: f32 = 17.0;
pub const TITLE: f32 = 20.0;
pub const MONO: f32 = 12.5;

/// The steps of the space between and in the controls.
pub const SPACE: [f32; 5] = [4.0, 8.0, 12.0, 16.0, 24.0];

/// The corner radius of a control.
pub const RADIUS: f32 = 8.0;

/// The smallest height and width of a control that the person clicks (Apple HIG).
pub const TARGET: f32 = 44.0;

/// The tokens as CSS custom properties, the light appearance in `:root` and the dark appearance
/// for `prefers-color-scheme: dark`, then the components of `ui.css`.
pub fn css() -> String {
    let colors = |c: &Colors| {
        let mut s = String::new();
        for (name, value) in [
            ("bg", c.bg),
            ("side", c.side),
            ("raised", c.raised),
            ("line", c.line),
            ("code", c.code),
            ("text", c.text),
            ("muted", c.muted),
            ("accent", c.accent),
            ("error", c.error),
            ("button", c.button),
            ("on-button", c.on_button),
        ] {
            s.push_str(&format!(" --{name}: #{value:06x};"));
        }
        for (i, value) in c.series.iter().enumerate() {
            s.push_str(&format!(" --s{}: #{value:06x};", i + 1));
        }
        s
    };
    let mut css = format!(":root {{{}", colors(&LIGHT));
    for (name, value) in [
        ("small", SMALL),
        ("body", BODY),
        ("read", READ),
        ("title", TITLE),
        ("mono", MONO),
        ("radius", RADIUS),
        ("target", TARGET),
    ] {
        css.push_str(&format!(" --{name}: {value}px;"));
    }
    for (i, value) in SPACE.iter().enumerate() {
        css.push_str(&format!(" --space-{}: {value}px;", i + 1));
    }
    css.push_str(&format!(
        " }}\n@media (prefers-color-scheme: dark) {{ :root {{{} }} }}\n",
        colors(&DARK)
    ));
    css.push_str(include_str!("ui.css"));
    css
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The WCAG contrast ratio of 2 colours.
    fn contrast(a: u32, b: u32) -> f64 {
        let luminance = |c: u32| {
            let [r, g, b] = [16, 8, 0].map(|s| {
                let x = f64::from((c >> s) & 0xff) / 255.0;
                if x <= 0.03928 {
                    x / 12.92
                } else {
                    ((x + 0.055) / 1.055).powf(2.4)
                }
            });
            0.2126 * r + 0.7152 * g + 0.0722 * b
        };
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    /// Each text colour is legible on each background of its appearance (WCAG AA, 4.5), and
    /// each series of a plot differs from the plot background (3).
    #[test]
    fn the_text_and_the_series_have_contrast_in_both_appearances() {
        for c in [&LIGHT, &DARK] {
            for bg in [c.bg, c.side, c.raised, c.code] {
                for text in [c.text, c.muted, c.error] {
                    assert!(contrast(text, bg) >= 4.5, "{text:06x} on {bg:06x}");
                }
            }
            assert!(contrast(c.on_button, c.button) >= 4.5);
            for s in c.series {
                assert!(contrast(s, c.side) >= 3.0, "series {s:06x}");
            }
        }
    }
}
