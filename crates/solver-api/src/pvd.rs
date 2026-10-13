//! A strict reader for the PVD collection file of a run. It reads only the XML that a PVD
//! file needs: a declaration, comments, the elements `VTKFile`, `Collection` and `DataSet`,
//! and their attributes. The solver is not trusted, so all other text is an error.

/// The `file` attribute of each `DataSet` of the collection, in order.
pub(crate) fn data_set_files(text: &str) -> Result<Vec<String>, String> {
    let mut rest = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut open: Vec<&str> = Vec::new();
    let mut roots = 0;
    let mut files = Vec::new();
    loop {
        rest = rest.trim_start();
        if rest.is_empty() {
            break;
        }
        let Some(tag) = rest.strip_prefix('<') else {
            return Err(format!("text outside a tag: {}", start(rest)));
        };
        if let Some(r) = tag.strip_prefix('?') {
            rest = after(r, "?>")?;
        } else if let Some(r) = tag.strip_prefix("!--") {
            rest = after(r, "-->")?;
        } else if let Some(r) = tag.strip_prefix('/') {
            let (name, r) = name(r)?;
            rest = r
                .trim_start()
                .strip_prefix('>')
                .ok_or_else(|| format!("the tag </{name} has no end"))?;
            if open.pop() != Some(name) {
                return Err(format!("</{name}> does not close the open element"));
            }
        } else {
            let (name, mut r) = name(tag)?;
            let mut attributes = Vec::new();
            let closed = loop {
                r = r.trim_start();
                if let Some(x) = r.strip_prefix("/>") {
                    r = x;
                    break true;
                }
                if let Some(x) = r.strip_prefix('>') {
                    r = x;
                    break false;
                }
                let (key, value, x) = attribute(r)?;
                attributes.push((key, value));
                r = x;
            };
            let get = |key: &str| attributes.iter().find(|(k, _)| *k == key).map(|(_, v)| v);
            match (open.as_slice(), name) {
                ([], "VTKFile") if roots == 0 => {
                    roots = 1;
                    if get("type").map(String::as_str) != Some("Collection") {
                        return Err("the root is not a VTKFile of the type Collection".into());
                    }
                }
                (["VTKFile"], "Collection") => {}
                (["VTKFile", "Collection"], "DataSet") => {
                    let file = get("file").ok_or("a DataSet has no file attribute")?;
                    files.push(file.clone());
                }
                _ => return Err(format!("the element <{name}> is not in its place")),
            }
            if !closed {
                open.push(name);
            }
            rest = r;
        }
    }
    if roots == 0 {
        return Err("there is no VTKFile element".into());
    }
    if let Some(name) = open.last() {
        return Err(format!("the element <{name}> is not closed"));
    }
    Ok(files)
}

/// The text after the first `end`.
fn after<'a>(text: &'a str, end: &str) -> Result<&'a str, String> {
    text.find(end)
        .map(|i| &text[i + end.len()..])
        .ok_or_else(|| format!("no {end} for {}", start(text)))
}

/// An XML name at the start of `text`, and the text after it.
fn name(text: &str) -> Result<(&str, &str), String> {
    let end = text
        .find(|c: char| !(c.is_alphanumeric() || matches!(c, '_' | ':' | '-' | '.')))
        .unwrap_or(text.len());
    if end == 0 {
        return Err(format!("a name must come at {}", start(text)));
    }
    Ok(text.split_at(end))
}

/// `key="value"` or `key='value'` at the start of `text`: the key, the value with its
/// entities replaced, and the text after it.
fn attribute(text: &str) -> Result<(&str, String, &str), String> {
    let (key, r) = name(text)?;
    let r = r
        .trim_start()
        .strip_prefix('=')
        .map(str::trim_start)
        .unwrap_or_default();
    let Some(quote) = r.chars().next().filter(|c| matches!(c, '"' | '\'')) else {
        return Err(format!("the attribute {key} has no quoted value"));
    };
    let r = &r[1..];
    let end = r
        .find(quote)
        .ok_or_else(|| format!("the value of {key} has no end"))?;
    let raw = &r[..end];
    if raw.contains('<') {
        return Err(format!("the value of {key} has a <"));
    }
    Ok((key, unescape(raw)?, &r[end + 1..]))
}

/// Replace the XML entities in an attribute value.
fn unescape(raw: &str) -> Result<String, String> {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let r = &rest[i + 1..];
        let end = r.find(';').ok_or("an entity has no ;")?;
        let entity = &r[..end];
        let c = match entity {
            "amp" => '&',
            "lt" => '<',
            "gt" => '>',
            "quot" => '"',
            "apos" => '\'',
            _ => entity
                .strip_prefix("#x")
                .map(|h| u32::from_str_radix(h, 16))
                .or_else(|| entity.strip_prefix('#').map(str::parse))
                .and_then(Result::ok)
                .and_then(char::from_u32)
                .ok_or_else(|| format!("the entity &{entity}; is not known"))?,
        };
        out.push(c);
        rest = &r[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// The first characters of `text`, for an error.
fn start(text: &str) -> String {
    text.chars().take(30).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entities_and_both_quotes_are_read() {
        let pvd = "<VTKFile type=\"Collection\"><Collection>\
<DataSet file='a&amp;b.vtu'/><DataSet file=\"&#x63;&#100;&quot;.vtu\"/></Collection></VTKFile>";
        assert_eq!(data_set_files(pvd).unwrap(), ["a&b.vtu", "cd\".vtu"]);
    }

    #[test]
    fn a_second_root_and_an_unknown_entity_are_errors() {
        let one = "<VTKFile type=\"Collection\"><Collection/></VTKFile>";
        assert!(data_set_files(one).unwrap().is_empty());
        assert!(data_set_files(&format!("{one}{one}")).is_err());
        let bad = one.replace(
            "<Collection/>",
            "<Collection><DataSet file=\"&x;\"/></Collection>",
        );
        assert!(data_set_files(&bad).unwrap_err().contains("&x;"));
    }
}
