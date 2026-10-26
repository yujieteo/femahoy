//! Curves: x-y values that a run or the agent writes as a small CSV file. `fh report` draws each
//! curve as an x-y plot, and `fh fit` fits a model to a series of a curve (DESIGN.md section 4.2).
//!
//! ```text
//! # title: Dam break, the surge front          the title of the plot. Default: the file name
//! # y: Z = x / a [1]                           the y quantity and its unit in [ ]. Required
//! # log: x                                     the log axes: x, y or x y. Default: none
//! # points measured: Martin and Moyce (1952)   the series `measured` shows as points, from this source
//! # note: The front is the largest x.          a line of text under the plot. 0 or more
//! T = t sqrt(2 g / a) [1],model,measured       the x quantity and its unit, then the name of each series
//! 0.28,1.00,                                   x, then the value of each series. An empty cell has no value
//! 1.19,,1.44
//! ```
//!
//! A series that is not `points` shows as a line through its values in the order of the rows.

/// One series of a curve.
#[derive(Debug, Clone, PartialEq)]
pub struct Series {
    pub name: String,
    /// The source of a series that shows as points, for example a measurement. `None` for a line.
    pub source: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Curve {
    pub title: String,
    /// The x quantity with its unit in `[ ]`.
    pub x: String,
    /// The y quantity with its unit in `[ ]`.
    pub y: String,
    pub log_x: bool,
    pub log_y: bool,
    pub series: Vec<Series>,
    pub notes: Vec<String>,
    /// x, then the value of each series. `None` is an empty cell.
    pub rows: Vec<(f64, Vec<Option<f64>>)>,
}

/// The most series of a curve: the number of colours of the plots.
const MOST_SERIES: usize = 8;

/// The unit of a label `quantity [unit]`, or `None`.
pub fn unit(label: &str) -> Option<&str> {
    let open = label.rfind('[')?;
    label
        .ends_with(']')
        .then(|| &label[open + 1..label.len() - 1])
}

fn number(cell: &str) -> Result<Option<f64>, String> {
    if cell.is_empty() {
        return Ok(None);
    }
    match cell.parse::<f64>() {
        Ok(v) if v.is_finite() => Ok(Some(v)),
        _ => Err(format!("`{cell}` is not a finite number")),
    }
}

impl Curve {
    /// Read the text of a curve file. `title` is the title when the file has no `# title:`. An
    /// error starts with the line number.
    pub fn parse(text: &str, title: &str) -> Result<Curve, String> {
        let mut curve = Curve {
            title: title.to_owned(),
            x: String::new(),
            y: String::new(),
            log_x: false,
            log_y: false,
            series: Vec::new(),
            notes: Vec::new(),
            rows: Vec::new(),
        };
        let mut points: Vec<(usize, String, String)> = Vec::new();
        let mut header = false;
        for (i, line) in text.lines().enumerate() {
            let at = |message: String| format!("line {}: {message}", i + 1);
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Some(comment) = line.strip_prefix('#') {
                let (key, value) = comment
                    .split_once(':')
                    .ok_or_else(|| at("a comment line is `# key: value`".into()))?;
                let (key, value) = (key.trim(), value.trim().to_owned());
                match key {
                    "title" => curve.title = value,
                    "y" => curve.y = value,
                    "note" => curve.notes.push(value),
                    "log" => {
                        for axis in value.split_whitespace() {
                            match axis {
                                "x" => curve.log_x = true,
                                "y" => curve.log_y = true,
                                _ => {
                                    return Err(at(format!("`log` takes x, y or x y, not {axis}")));
                                }
                            }
                        }
                    }
                    _ => match key.strip_prefix("points ") {
                        Some(name) if !value.is_empty() => {
                            points.push((i + 1, name.trim().to_owned(), value));
                        }
                        Some(name) => return Err(at(format!("`points {name}` needs a source"))),
                        None => {
                            return Err(at(format!(
                                "unknown key `{key}`: use title, y, log, points NAME or note"
                            )));
                        }
                    },
                }
                continue;
            }
            let cells: Vec<&str> = line.split(',').map(str::trim).collect();
            if !header {
                header = true;
                curve.x = cells[0].to_owned();
                for name in &cells[1..] {
                    if name.is_empty() || curve.series.iter().any(|s| s.name == *name) {
                        return Err(at(format!("each series needs its own name, not `{name}`")));
                    }
                    curve.series.push(Series {
                        name: (*name).to_owned(),
                        source: None,
                    });
                }
                continue;
            }
            if cells.len() > curve.series.len() + 1 {
                return Err(at(format!(
                    "{} cells for {} columns",
                    cells.len(),
                    curve.series.len() + 1
                )));
            }
            let x = number(cells[0])
                .map_err(at)?
                .ok_or_else(|| at("each row needs an x".into()))?;
            let mut values = cells[1..]
                .iter()
                .map(|c| number(c))
                .collect::<Result<Vec<_>, _>>()
                .map_err(at)?;
            values.resize(curve.series.len(), None);
            curve.rows.push((x, values));
        }
        curve.check(points)?;
        Ok(curve)
    }

    /// The rules that need the whole file.
    fn check(&mut self, points: Vec<(usize, String, String)>) -> Result<(), String> {
        if self.series.is_empty() {
            return Err("the header line needs the x quantity, then 1 series or more".into());
        }
        if self.series.len() > MOST_SERIES {
            return Err(format!("a curve has at most {MOST_SERIES} series"));
        }
        if self.y.is_empty() {
            return Err("no `# y: QUANTITY [UNIT]` line".into());
        }
        for label in [&self.x, &self.y] {
            if unit(label).is_none() {
                return Err(format!(
                    "`{label}` needs its unit in [ ] at the end, for example `stress [MPa]` or `[1]`"
                ));
            }
        }
        for (line, name, source) in points {
            let series = self.series.iter_mut().find(|s| s.name == name);
            let series = series.ok_or_else(|| format!("line {line}: no series `{name}`"))?;
            series.source = Some(source);
        }
        for (i, s) in self.series.iter().enumerate() {
            if self.values(i).is_empty() {
                return Err(format!("the series `{}` has no value", s.name));
            }
        }
        let mut ys = self.rows.iter().flat_map(|(_, v)| v.iter().flatten());
        if self.log_x && self.rows.iter().any(|(x, _)| *x <= 0.0) {
            return Err("the log axis x needs values above 0".into());
        }
        if self.log_y && ys.any(|y| *y <= 0.0) {
            return Err("the log axis y needs values above 0".into());
        }
        Ok(())
    }

    /// The points (x, y) of series `i`, in the order of the rows.
    pub fn values(&self, i: usize) -> Vec<(f64, f64)> {
        self.rows
            .iter()
            .filter_map(|(x, v)| Some((*x, v[i]?)))
            .collect()
    }

    /// The curve as the text of a curve file.
    pub fn to_csv(&self) -> String {
        let mut out = format!("# title: {}\n# y: {}\n", self.title, self.y);
        let log: Vec<&str> = [(self.log_x, "x"), (self.log_y, "y")]
            .iter()
            .filter_map(|(on, a)| on.then_some(*a))
            .collect();
        if !log.is_empty() {
            out.push_str(&format!("# log: {}\n", log.join(" ")));
        }
        for s in &self.series {
            if let Some(source) = &s.source {
                out.push_str(&format!("# points {}: {source}\n", s.name));
            }
        }
        for note in &self.notes {
            out.push_str(&format!("# note: {note}\n"));
        }
        out.push_str(&self.x);
        for s in &self.series {
            out.push(',');
            out.push_str(&s.name);
        }
        out.push('\n');
        for (x, values) in &self.rows {
            out.push_str(&x.to_string());
            for v in values {
                out.push(',');
                if let Some(v) = v {
                    out.push_str(&v.to_string());
                }
            }
            out.push('\n');
        }
        out
    }

    /// The curve as an x-y plot: a `figure` with an inline SVG, for the report. The colours are
    /// the CSS variables `--s1` to `--s8` of the report.
    pub fn figure(&self) -> String {
        let all = |axis: usize| {
            (0..self.series.len()).flat_map(move |i| {
                let values = self.values(i);
                values
                    .into_iter()
                    .map(move |p| if axis == 0 { p.0 } else { p.1 })
            })
        };
        let xa = Axis::new(all(0), self.log_x);
        let ya = Axis::new(all(1), self.log_y);
        let px = |x: f64| LEFT + WIDTH * xa.at(x);
        let py = |y: f64| TOP + HEIGHT * (1.0 - ya.at(y));
        let title = escape(&self.title);
        let mut svg = format!(
            "<figure class=\"curve\"><figcaption>{title}</figcaption>\
             <svg viewBox=\"0 0 {W} {H}\" role=\"img\" aria-label=\"{title}\"><g class=\"grid\">"
        );
        for &t in &xa.ticks {
            let x = px(t);
            svg.push_str(&format!(
                "<line x1=\"{x:.1}\" y1=\"{TOP}\" x2=\"{x:.1}\" y2=\"{:.1}\"/>",
                TOP + HEIGHT
            ));
        }
        for &t in &ya.ticks {
            let y = py(t);
            svg.push_str(&format!(
                "<line x1=\"{LEFT}\" y1=\"{y:.1}\" x2=\"{:.1}\" y2=\"{y:.1}\"/>",
                LEFT + WIDTH
            ));
        }
        svg.push_str(&format!(
            "</g><rect class=\"frame\" x=\"{LEFT}\" y=\"{TOP}\" width=\"{WIDTH}\" height=\"{HEIGHT}\"/>"
        ));
        for &t in &xa.ticks {
            svg.push_str(&format!(
                "<text x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"middle\">{}</text>",
                px(t),
                TOP + HEIGHT + 16.0,
                short(t)
            ));
        }
        for &t in &ya.ticks {
            svg.push_str(&format!(
                "<text x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"end\">{}</text>",
                LEFT - 6.0,
                py(t) + 4.0,
                short(t)
            ));
        }
        svg.push_str(&format!(
            "<text class=\"label\" x=\"{:.1}\" y=\"{}\" text-anchor=\"middle\">{}</text>\
             <text class=\"label\" transform=\"translate(14 {:.1}) rotate(-90)\" \
             text-anchor=\"middle\">{}</text>",
            LEFT + WIDTH / 2.0,
            H - 6,
            escape(&self.x),
            TOP + HEIGHT / 2.0,
            escape(&self.y)
        ));
        // The lines first, so that the points are on top.
        for (i, s) in self.series.iter().enumerate() {
            if s.source.is_none() {
                let mut points = Vec::new();
                for (x, values) in self
                    .rows
                    .iter()
                    .chain(std::iter::once(&(0.0, vec![None; self.series.len()])))
                {
                    if let Some(y) = values[i] {
                        points.push(format!("{:.1},{:.1}", px(*x), py(y)));
                    } else if !points.is_empty() {
                        svg.push_str(&format!(
                            "<polyline style=\"stroke:var(--s{})\" points=\"{}\"/>",
                            i + 1,
                            points.join(" ")
                        ));
                        points.clear();
                    }
                }
            }
        }
        for (i, s) in self.series.iter().enumerate() {
            if s.source.is_some() {
                for (x, y) in self.values(i) {
                    svg.push_str(&format!(
                        "<circle style=\"fill:var(--s{})\" cx=\"{:.1}\" cy=\"{:.1}\" r=\"4\">\
                         <title>{}: {}, {}</title></circle>",
                        i + 1,
                        px(x),
                        py(y),
                        escape(&s.name),
                        short(x),
                        short(y)
                    ));
                }
            }
        }
        svg.push_str(&format!("<line class=\"sample-cursor\" x1=\"{LEFT}\" x2=\"{LEFT}\" y1=\"{TOP}\" y2=\"{}\" hidden=\"hidden\"/></svg>", TOP + HEIGHT));
        // Original values form the public sample interface, independent of SVG rounding.
        svg.push_str(&format!("<div data-control=\"values\" hidden><button type=\"button\" aria-label=\"Show values for {title}\" aria-expanded=\"false\">Show values</button><div hidden><label>Sample <input type=\"range\" name=\"sample\" aria-label=\"Sample of {title}\" min=\"0\" max=\"{}\" value=\"0\" step=\"1\"></label><output class=\"readout\" aria-live=\"polite\"></output><p class=\"keys\"><kbd>←</kbd><kbd>→</kbd> select a source sample</p><table><caption>Original values. — means no sample.</caption><thead><tr><th scope=\"col\">{}</th>", self.rows.len().saturating_sub(1), escape(&self.x)));
        for series in &self.series {
            svg.push_str(&format!(
                "<th scope=\"col\">{} ({})</th>",
                escape(&series.name),
                escape(&self.y)
            ));
        }
        svg.push_str("</tr></thead><tbody>");
        for (x, values) in &self.rows {
            svg.push_str(&format!("<tr data-x=\"{}\"><td>{x}</td>", px(*x)));
            for value in values {
                svg.push_str(&format!(
                    "<td>{}</td>",
                    value.map(|v| v.to_string()).unwrap_or_else(|| "—".into())
                ));
            }
            svg.push_str("</tr>");
        }
        svg.push_str("</tbody></table></div></div>");
        // 1 series needs no legend: the title names it.
        if self.series.len() > 1 {
            svg.push_str("<ul class=\"legend\">");
            for (i, s) in self.series.iter().enumerate() {
                let kind = if s.source.is_some() { "point" } else { "line" };
                svg.push_str(&format!(
                    "<li><i class=\"{kind}\" style=\"background:var(--s{})\"></i>{}</li>",
                    i + 1,
                    escape(&s.name)
                ));
            }
            svg.push_str("</ul>");
        }
        for s in &self.series {
            if let Some(source) = &s.source {
                svg.push_str(&format!("<p>{}: {}</p>", escape(&s.name), escape(source)));
            }
        }
        for note in &self.notes {
            svg.push_str(&format!("<p>{}</p>", escape(note)));
        }
        svg.push_str("</figure>\n");
        svg
    }
}

/// The size of the plot and the place of its frame, in the units of the SVG.
const W: u32 = 640;
const H: u32 = 360;
const LEFT: f64 = 72.0;
const TOP: f64 = 12.0;
const WIDTH: f64 = 552.0;
const HEIGHT: f64 = 296.0;

/// The range of an axis and its ticks. The range starts and ends at a tick.
struct Axis {
    lo: f64,
    hi: f64,
    log: bool,
    ticks: Vec<f64>,
}

impl Axis {
    fn new(values: impl Iterator<Item = f64>, log: bool) -> Axis {
        let t = |v: f64| if log { v.log10() } else { v };
        let (min, max) = values.fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| {
            (a.min(t(v)), b.max(t(v)))
        });
        if log {
            let (lo, mut hi) = (min.floor(), max.ceil());
            if hi == lo {
                hi += 1.0;
            }
            let decades = (lo as i32)..=(hi as i32);
            let mut ticks: Vec<f64> = decades.clone().map(|d| 10f64.powi(d)).collect();
            if ticks.len() < 3 {
                ticks = decades
                    .flat_map(|d| [1.0, 2.0, 5.0].map(|m| m * 10f64.powi(d)))
                    .filter(|v| v.log10() <= hi + 1e-9)
                    .collect();
            }
            return Axis { lo, hi, log, ticks };
        }
        let (mut min, mut max) = (min, max);
        if max == min {
            let span = if min == 0.0 { 1.0 } else { min.abs() * 0.1 };
            (min, max) = (min - span, max + span);
        }
        let raw = (max - min) / 5.0;
        let decade = 10f64.powf(raw.log10().floor());
        let step = [1.0, 2.0, 5.0, 10.0]
            .iter()
            .map(|m| m * decade)
            .find(|s| *s >= raw * (1.0 - 1e-9))
            .unwrap_or(10.0 * decade);
        let (first, last) = ((min / step).floor() as i64, (max / step).ceil() as i64);
        let ticks = (first..=last).map(|k| k as f64 * step).collect();
        Axis {
            lo: first as f64 * step,
            hi: last as f64 * step,
            log,
            ticks,
        }
    }

    /// The place of a value on the axis, from 0 at the low end to 1 at the high end.
    fn at(&self, v: f64) -> f64 {
        let v = if self.log { v.log10() } else { v };
        (v - self.lo) / (self.hi - self.lo)
    }
}

/// A number with at most 6 significant digits and no trailing zeros, as `1.25`, `300` or `2e6`.
pub fn short(v: f64) -> String {
    let trim = |s: String| {
        if s.contains('.') {
            s.trim_end_matches('0').trim_end_matches('.').to_owned()
        } else {
            s
        }
    };
    let a = v.abs();
    if v == 0.0 {
        "0".into()
    } else if (1e-3..1e6).contains(&a) {
        let decimals = (5 - a.log10().floor() as i32).clamp(0, 8) as usize;
        trim(format!("{v:.decimals$}"))
    } else {
        let text = format!("{v:.5e}");
        let (mantissa, exponent) = text.split_once('e').unwrap_or((&text, "0"));
        format!("{}e{exponent}", trim(mantissa.to_owned()))
    }
}

pub fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRONT: &str = "# title: Front\n# y: Z [1]\n# points measured: Martin and Moyce (1952)\n\
                         # note: a <b>\nT [1],model,measured\n0.5,1.0,\n1.0,1.5,\n1.19,,1.44\n";

    #[test]
    fn an_absent_sample_leaves_a_gap_in_the_plot() {
        let curve = Curve::parse(
            "# y: residual [1]\niteration [1],p\n1,0.1\n2,0.05\n3,\n4,0.01\n5,0.001\n",
            "residual",
        )
        .unwrap();
        assert_eq!(curve.figure().matches("<polyline").count(), 2);
    }

    #[test]
    fn a_curve_file_gives_its_series_and_round_trips() {
        let c = Curve::parse(FRONT, "front").unwrap();
        assert_eq!(c.title, "Front");
        assert_eq!((c.x.as_str(), c.y.as_str()), ("T [1]", "Z [1]"));
        assert_eq!(c.values(0), vec![(0.5, 1.0), (1.0, 1.5)]);
        assert_eq!(c.values(1), vec![(1.19, 1.44)]);
        assert_eq!(
            c.series[1].source.as_deref(),
            Some("Martin and Moyce (1952)")
        );
        assert_eq!(Curve::parse(&c.to_csv(), "other").unwrap(), c);
    }

    #[test]
    fn a_bad_curve_file_names_the_problem() {
        for (text, problem) in [
            ("# y: Z [1]\nT,a\n1,2\n", "`T` needs its unit"),
            ("T [s],a\n1,2\n", "no `# y:"),
            (
                "# y: Z [1]\nT [s],a\n1,x\n",
                "line 3: `x` is not a finite number",
            ),
            (
                "# y: Z [1]\nT [s],a\n1,2,3\n",
                "line 3: 3 cells for 2 columns",
            ),
            (
                "# y: Z [1]\n# points b: S\nT [s],a\n1,2\n",
                "line 2: no series `b`",
            ),
            (
                "# y: Z [1]\n# log: x\nT [s],a\n0,2\n",
                "the log axis x needs values above 0",
            ),
            (
                "# y: Z [1]\n# colour: red\nT [s],a\n1,2\n",
                "line 2: unknown key `colour`",
            ),
            (
                "# y: Z [1]\nT [s],a,b\n1,2,\n",
                "the series `b` has no value",
            ),
            (
                "# y: Z [1]\nT [s],a,a\n1,2,3\n",
                "line 2: each series needs its own name, not `a`",
            ),
        ] {
            let e = Curve::parse(text, "t").unwrap_err();
            assert!(e.contains(problem), "{e}");
        }
    }

    #[test]
    fn the_plot_has_a_line_points_a_legend_and_escaped_text() {
        let svg = Curve::parse(FRONT, "front").unwrap().figure();
        assert_eq!(svg.matches("<polyline").count(), 1);
        assert_eq!(svg.matches("<circle").count(), 1);
        assert!(svg.contains("<title>measured: 1.19, 1.44</title>"), "{svg}");
        assert!(svg.contains("class=\"legend\""));
        assert!(svg.contains("<p>measured: Martin and Moyce (1952)</p>"));
        assert!(svg.contains("<p>a &lt;b&gt;</p>"));
        assert!(!svg.contains("<b>"));
    }

    // The table is the public, exact sample contract of a curve figure.
    #[test]
    fn exact_samples_keep_source_precision_and_gaps() {
        let mut curve = Curve::parse(FRONT, "front").unwrap();
        curve.rows = vec![(1.2345678901234567, vec![Some(9.876543210123456), None])];
        let html = curve.figure();
        assert!(html.contains("<td>1.2345678901234567</td>"), "{html}");
        assert!(html.contains("<td>9.876543210123456</td>"));
        assert!(html.contains("<td>—</td>"));
    }

    #[test]
    fn the_axes_start_and_end_at_a_tick() {
        let a = Axis::new([0.28, 3.4].into_iter(), false);
        assert_eq!((a.lo, a.hi), (0.0, 4.0));
        assert_eq!(a.ticks.len(), 5);
        let a = Axis::new([150000.0, 3e6].into_iter(), true);
        assert_eq!((a.lo, a.hi), (5.0, 7.0));
        assert_eq!(a.ticks, vec![1e5, 1e6, 1e7]);
        let a = Axis::new([10.0, 80.0].into_iter(), true);
        assert_eq!(a.ticks, vec![10.0, 20.0, 50.0, 100.0]);
        let a = Axis::new([2.0, 2.0].into_iter(), false);
        assert!(a.lo < 2.0 && a.hi > 2.0);
    }

    #[test]
    fn short_numbers() {
        for (v, text) in [
            (0.0, "0"),
            (1.25, "1.25"),
            (0.1 + 0.2, "0.3"),
            (300.0, "300"),
            (2e6, "2e6"),
            (-3.16081871345029e-15, "-3.16082e-15"),
        ] {
            assert_eq!(short(v), text);
        }
    }
}
