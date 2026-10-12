//! Split the text into keyword blocks. A block is one keyword line and its data lines.

use crate::ParseError;

/// A keyword parameter. Names and values are upper case.
#[derive(Debug, Clone)]
pub(crate) struct Param {
    pub name: String,
    pub value: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct DataLine {
    /// Line number of the first physical line.
    pub line: usize,
    pub fields: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct Block {
    /// Upper case keyword name without `*`, with single spaces.
    pub keyword: String,
    pub line: usize,
    pub params: Vec<Param>,
    pub data: Vec<DataLine>,
}

impl Block {
    pub fn param(&self, name: &str) -> Option<&Param> {
        self.params.iter().find(|p| p.name == name)
    }

    /// The value of a parameter that needs a value.
    pub fn value(&self, name: &str) -> Result<Option<&str>, ParseError> {
        match self.param(name) {
            None => Ok(None),
            Some(Param { value: Some(v), .. }) => Ok(Some(v)),
            Some(_) => Err(self.error(self.line, format!("parameter {name} needs a value"))),
        }
    }

    /// Fail on a parameter outside `allowed`.
    pub fn allow(&self, allowed: &[&str]) -> Result<(), ParseError> {
        match self
            .params
            .iter()
            .find(|p| !allowed.contains(&p.name.as_str()))
        {
            Some(p) => Err(self.error(self.line, format!("unsupported parameter {}", p.name))),
            None => Ok(()),
        }
    }

    pub fn error(&self, line: usize, message: impl Into<String>) -> ParseError {
        ParseError::new(line, Some(&self.keyword), message)
    }
}

/// A physical line is a comment when it starts with `**`.
fn is_comment(line: &str) -> bool {
    line.trim_start().starts_with("**")
}

fn split_fields(text: &str) -> Vec<String> {
    text.split(',').map(|f| f.trim().to_owned()).collect()
}

pub(crate) fn lex(text: &str) -> Result<Vec<Block>, ParseError> {
    let lines: Vec<(usize, &str)> = text
        .lines()
        .enumerate()
        .map(|(i, l)| (i + 1, l))
        .filter(|(_, l)| !is_comment(l) && !l.trim().is_empty())
        .collect();
    let mut blocks: Vec<Block> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let (no, raw) = lines[i];
        let trimmed = raw.trim();
        i += 1;
        // A line that ends in a comma continues on the next line.
        let mut joined = trimmed.to_owned();
        while joined.ends_with(',') && i < lines.len() && !lines[i].1.trim_start().starts_with('*')
        {
            joined.push_str(lines[i].1.trim());
            i += 1;
        }
        if let Some(rest) = joined.strip_prefix('*') {
            let mut parts = split_fields(rest).into_iter();
            let name = parts.next().unwrap_or_default();
            let keyword = name
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_uppercase();
            if keyword.is_empty() {
                return Err(ParseError::new(no, None, "keyword line has no name"));
            }
            let mut params = Vec::new();
            for p in parts.filter(|p| !p.is_empty()) {
                let (n, v) = match p.split_once('=') {
                    Some((n, v)) => (n.trim(), Some(v.trim().to_uppercase())),
                    None => (p.trim(), None),
                };
                params.push(Param {
                    name: n.to_uppercase(),
                    value: v,
                });
            }
            blocks.push(Block {
                keyword,
                line: no,
                params,
                data: Vec::new(),
            });
        } else {
            let Some(block) = blocks.last_mut() else {
                return Err(ParseError::new(
                    no,
                    None,
                    "data line before the first keyword",
                ));
            };
            let mut fields = split_fields(&joined);
            if fields.last().is_some_and(String::is_empty) && joined.ends_with(',') {
                fields.pop();
            }
            block.data.push(DataLine { line: no, fields });
        }
    }
    Ok(blocks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keywords_are_case_insensitive_and_params_split() {
        let b = lex("*Node, nset=Top\n1, 0, 0, 0\n").unwrap();
        assert_eq!(b[0].keyword, "NODE");
        assert_eq!(b[0].value("NSET").unwrap(), Some("TOP"));
        assert_eq!(b[0].data[0].fields, ["1", "0", "0", "0"]);
    }

    #[test]
    fn comments_and_blank_lines_are_skipped() {
        let b = lex("** note\n\n*NODE\n** inside\n1, 0, 0, 0\n").unwrap();
        assert_eq!(b[0].data.len(), 1);
        assert_eq!(b[0].data[0].line, 5);
    }

    #[test]
    fn trailing_comma_joins_the_next_line() {
        let b = lex("*ELEMENT, TYPE=C3D8\n1, 1, 2, 3,\n  4, 5\n").unwrap();
        assert_eq!(b[0].data.len(), 1);
        assert_eq!(b[0].data[0].fields, ["1", "1", "2", "3", "4", "5"]);
    }

    #[test]
    fn keyword_line_continues_and_names_collapse_spaces() {
        let b = lex("*end   step\n*STEP, NAME=A,\n  NLGEOM=YES\n").unwrap();
        assert_eq!(b[0].keyword, "END STEP");
        assert_eq!(b[1].value("NLGEOM").unwrap(), Some("YES"));
    }

    #[test]
    fn data_before_keyword_fails_with_line() {
        let e = lex("\n1, 2\n").unwrap_err();
        assert_eq!(e.line, 2);
    }
}
