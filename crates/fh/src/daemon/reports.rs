//! `GET /reports`: the list of the retained reports, for a phone. Each row opens 1 report with
//! a single-use ticket (`report-ticket`), so a report never receives the daemon token. Every
//! address in the page is relative, so it works at any host name and behind a proxy.

use crate::curve::escape;

/// The date `YYYY-MM-DD HH:MM` (UTC) of `seconds` since 1970.
fn date(seconds: u64) -> String {
    let (days, rest) = (seconds / 86400, seconds % 86400);
    // The civil date of a day count (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}",
        rest / 3600,
        rest % 3600 / 60
    )
}

/// The last 2 parts of the path of the input of a run.
fn model(source: &str) -> String {
    let mut parts: Vec<&str> = source.trim().rsplit('/').take(2).collect();
    parts.reverse();
    parts.join("/")
}

/// The page for `reports` (see `Store::reports`). `nonce` is the nonce of its script.
pub(super) fn page(reports: &[(String, String, String, u64)], nonce: &str) -> String {
    let mut rows = String::new();
    for (run, title, source, time) in reports {
        // A run in `reports` is `run-` and digits, so it needs no escape.
        rows.push_str(&format!(
            "<li><button type=\"button\" data-run=\"{run}\"><strong>{}</strong>\
             <span>{} · {run}</span><span>{}</span></button></li>\n",
            escape(title),
            escape(&model(source)),
            date(*time),
        ));
    }
    if rows.is_empty() {
        rows.push_str("<p>No report yet. Ask the agent for a report of a run.</p>");
    }
    format!(
        "<!doctype html>\n<html lang=\"en\">\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <meta name=\"color-scheme\" content=\"dark light\">\n<title>femahoy reports</title>\n\
         <style>{}\n\
         body {{ margin: 0; font: 17px/1.4 var(--sans); }}\n\
         main {{ max-width: 40rem; margin: 0 auto; padding: 16px; }}\n\
         h1 {{ font: 400 28px/1.2 var(--serif); margin: 8px 0 16px; }}\n\
         ul {{ list-style: none; margin: 0; padding: 0; display: grid; gap: 8px; }}\n\
         button {{ display: grid; gap: 2px; width: 100%; min-height: 56px; padding: 12px 14px; \
         text-align: left; font: inherit; color: inherit; background: transparent; \
         border: 1px solid currentColor; border-radius: 10px; }}\n\
         button span {{ font-size: 15px; opacity: .7; overflow-wrap: anywhere; }}\n\
         #error {{ color: #c0392b; }}\n</style>\n\
         <main><h1>Reports</h1>\n<p id=\"error\" role=\"alert\"></p>\n<ul>\n{rows}</ul></main>\n\
         <script nonce=\"{nonce}\">\n\
         // The token is in the address only when the page is opened without a proxy that adds it.\n\
         const token = new URLSearchParams(location.search).get(\"token\") || \"\";\n\
         if (token) history.replaceState(null, \"\", location.pathname);\n\
         const headers = token ? {{ Authorization: \"Bearer \" + token }} : {{}};\n\
         for (const b of document.querySelectorAll(\"button[data-run]\")) b.onclick = async () => {{\n\
         \x20 const base = \"api/v1/runs/\" + b.dataset.run + \"/report\";\n\
         \x20 try {{\n\
         \x20   const r = await fetch(base + \"-ticket\", {{ method: \"POST\", headers }});\n\
         \x20   const text = await r.text();\n\
         \x20   const ticket = text.split(\"\\n\").find(l => l.startsWith(\"ticket=\"));\n\
         \x20   if (!r.ok || !ticket) throw new Error(text || r.statusText);\n\
         \x20   location.assign(base + \"?ticket=\" + ticket.slice(7));\n\
         \x20 }} catch (e) {{ document.getElementById(\"error\").textContent = e.message; }}\n\
         }};\n</script>\n",
        super::report::ui_style(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_date_is_utc() {
        assert_eq!(date(0), "1970-01-01 00:00");
        assert_eq!(date(1_793_609_167), "2026-11-02 08:46");
        assert_eq!(date(951_782_400 + 3600 * 5 + 60 * 7), "2000-02-29 05:07");
    }

    #[test]
    fn the_page_escapes_the_title_and_has_relative_links_only() {
        let rows = [(
            "run-000001".into(),
            "<b>x</b> & y".into(),
            "/a/b/c.inp".into(),
            0,
        )];
        let html = page(&rows, "N");
        assert!(html.contains("&lt;b&gt;x&lt;/b&gt; &amp; y"));
        assert!(html.contains("b/c.inp · run-000001"));
        assert!(!html.contains("http://") && !html.contains("https://"));
    }
}
