//! The 1 UI layer of femahoy: the design tokens of each view. The page of `fh serve`
//! (`crates/fh/src/daemon/page.html`) and a report (`report.html`) get them as CSS from
//! [`css`], with the components of `ui.css`. The model view gets them in `widgets`, with the
//! same components in egui. A new view uses these tokens and components, and adds no colour,
//! size or control style of its own (DESIGN.md section 8).
//!
//! The light colours, the type sizes and the radius are those of the site teoyujie.org
//! (`/shared/kowalski.css`, the light gray scale of emilkowal.ski). The site has no dark theme:
//! the dark colours are the same steps of the dark gray scale of emilkowal.ski. The site has no
//! error colour and no plots: `error` and `series` are those of femahoy. `series` are the series
//! of the plots, in a fixed order that keeps adjacent series apart for colour-blind readers.

/// Named report controls of the shared UI layer. They use the existing HTML components.
/// `data-controls` includes these names beside the names of the model controls.
pub const REPORT_CONTROLS: &[&str] = &[
    "cfd-field",
    "cfd-mesh",
    "cfd-flap",
    "cfd-load",
    "cfd-chord-station",
    "cfd-force-interval",
    "cfd-streamlines",
    "cfd-flap-seed",
];

/// The colours of 1 appearance, as `0xRRGGBB`.
pub struct Colors {
    pub bg: u32,
    pub side: u32,
    /// The hover background, and the fill of a message of the person.
    pub raised: u32,
    pub line: u32,
    pub code: u32,
    pub text: u32,
    pub muted: u32,
    pub accent: u32,
    pub error: u32,
    /// The text selection, and the underline of a link and its hover colour.
    pub select: u32,
    pub underline: u32,
    pub underline_hover: u32,
    /// A button (the Sonner `.button` of the site): its fill and its border, at rest and on
    /// hover, and the border of the chosen state, which has the fill `button_line`.
    pub button: u32,
    pub button_line: u32,
    pub button_hover: u32,
    pub button_hover_line: u32,
    pub button_on_line: u32,
    /// The tick on a checkbox that has the `accent` fill.
    pub on_accent: u32,
    pub series: [u32; 8],
    /// The CSS `box-shadow` of the keyboard focus of a button, and of a control that floats
    /// over the model.
    pub focus_ring: &'static str,
    pub shadow: &'static str,
}

pub const LIGHT: Colors = Colors {
    bg: 0xfdfdfc,
    side: 0xfdfdfc,
    raised: 0xf5f4f4,
    line: 0xe9e9e7,
    code: 0xf9f9f8,
    text: 0x21201c,
    muted: 0x63635e,
    accent: 0x63635e,
    error: 0xb3261e,
    select: 0xe2e1de,
    underline: 0xbcbbb5,
    underline_hover: 0x8d8d86,
    button: 0xfcfcfc,
    button_line: 0xf3f3f3,
    button_hover: 0xf8f8f8,
    button_hover_line: 0xededed,
    button_on_line: 0xdbdbdb,
    on_accent: 0xfdfdfc,
    series: [
        0x2a6fc4, 0xc4613f, 0x138a5f, 0xa8740a, 0xc2457a, 0x2f7d1f, 0x6250d6, 0xc43d3c,
    ],
    focus_ring: "0 0 0 1px rgba(0,0,0,.06), 0 1px 0 0 rgba(0,0,0,.08), 0 2px 2px 0 rgba(0,0,0,.04), \
                 0 3px 3px 0 rgba(0,0,0,.02), 0 4px 4px 0 rgba(0,0,0,.01), 0 0 0 2px rgba(0,0,0,.15)",
    shadow: "0 4px 12px rgba(0,0,0,.1)",
};

pub const DARK: Colors = Colors {
    bg: 0x111110,
    side: 0x111110,
    raised: 0x222221,
    line: 0x2a2a28,
    code: 0x191918,
    text: 0xeeeeec,
    muted: 0xb5b3ad,
    accent: 0xb5b3ad,
    error: 0xf2877b,
    select: 0x31312e,
    underline: 0x62605b,
    underline_hover: 0x6f6d66,
    button: 0x111110,
    button_line: 0x222221,
    button_hover: 0x191918,
    button_hover_line: 0x2a2a28,
    button_on_line: 0x3b3a37,
    on_accent: 0x111110,
    series: [
        0x3987e5, 0xd95926, 0x199e70, 0xc98500, 0xd55181, 0x3a9d3a, 0x9085e9, 0xe66767,
    ],
    // The `--ds-focus-ring` and `--ds-shadow-border-medium` of the dark theme of emilkowal.ski.
    focus_ring: "0 0 0 2px #111110, 0 0 0 4px #b5b3ad",
    shadow:
        "0 0 0 1px rgba(255,255,255,.08), 0 2px 2px rgba(0,0,0,.32), 0 8px 8px -8px rgba(0,0,0,.08)",
};

/// The type sizes, in CSS pixels (points in egui): a note or a hint, the text of the controls
/// and the chat, the text of a report, and a title.
pub const SMALL: f32 = 14.0;
pub const BODY: f32 = 16.0;
pub const READ: f32 = 16.0;
pub const TITLE: f32 = 16.0;
pub const MONO: f32 = 13.0;
/// The text of a button and a menu (the Sonner `.button` of the site), weight 500.
pub const BUTTON: f32 = 13.0;

/// The steps of the space between and in the controls.
pub const SPACE: [f32; 5] = [4.0, 8.0, 12.0, 16.0, 24.0];

/// The corner radius of a control.
pub const RADIUS: f32 = 6.0;

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
            ("select", c.select),
            ("underline", c.underline),
            ("underline-hover", c.underline_hover),
            ("button", c.button),
            ("button-line", c.button_line),
            ("button-hover", c.button_hover),
            ("button-hover-line", c.button_hover_line),
            ("button-on-line", c.button_on_line),
        ] {
            s.push_str(&format!(" --{name}: #{value:06x};"));
        }
        for (i, value) in c.series.iter().enumerate() {
            s.push_str(&format!(" --s{}: #{value:06x};", i + 1));
        }
        s.push_str(&format!(
            " --focus-ring: {}; --shadow: {};",
            c.focus_ring, c.shadow
        ));
        // The menu chevron, in the muted colour: a data URL cannot use a CSS variable.
        s.push_str(&format!(
            " --chevron: url(\"data:image/svg+xml,%3Csvg xmlns='http%3A//www.w3.org/2000/svg' viewBox='0 0 16 16'%3E%3Cpath d='m4 6 4 4 4-4' fill='none' stroke='%23{:06x}' stroke-width='1.6' stroke-linecap='round' stroke-linejoin='round'/%3E%3C/svg%3E\");",
            c.muted
        ));
        s
    };
    let type_sizes = [
        ("small", SMALL),
        ("body", BODY),
        ("read", READ),
        ("title", TITLE),
        ("mono", MONO),
        ("button-size", BUTTON),
    ];
    let mut css = format!(":root {{{}", colors(&LIGHT));
    for (name, value) in type_sizes
        .into_iter()
        .chain([("radius", RADIUS), ("target", TARGET)])
    {
        css.push_str(&format!(" --{name}: {value}px;"));
    }
    for (i, value) in SPACE.iter().enumerate() {
        css.push_str(&format!(" --space-{}: {value}px;", i + 1));
    }
    css.push_str(&format!(
        " }}\n@media (prefers-color-scheme: dark) {{ :root {{{} }} }}\n",
        colors(&DARK)
    ));
    // Dynamic Type: on iOS the root font is the text size that the person set (17px at the
    // default size), so the type sizes grow and shrink with it. Desktop Safari also knows
    // this font (13px), so only a touch screen uses it.
    css.push_str("@media (pointer: coarse) { @supports (font: -apple-system-body) { html { font: -apple-system-body; } :root {");
    for (name, value) in type_sizes {
        css.push_str(&format!(" --{name}: calc({value}rem / 17);"));
    }
    css.push_str(" } } }\n");
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
            assert!(contrast(c.on_accent, c.accent) >= 4.5);
            assert!(contrast(c.text, c.button) >= 4.5);
            for s in c.series {
                assert!(contrast(s, c.side) >= 3.0, "series {s:06x}");
            }
        }
    }
}
