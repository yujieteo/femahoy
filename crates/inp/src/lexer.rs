//! Split the text into keyword blocks. A block is one keyword line and its data lines.

/// A keyword line with its data lines. The reader does not fail on a bad line. CalculiX checks
/// the input.
#[derive(Debug, Clone)]
pub(crate) struct Block {
    /// Upper case keyword name without `*`, with single spaces.
    pub keyword: String,
    /// Parameters with upper case names and values.
    pub params: Vec<(String, Option<String>)>,
    /// The fields of each data line. A line that ends in a comma continues on the next line.
    pub data: Vec<Vec<String>>,
}

impl Block {
    pub fn value(&self, name: &str) -> Option<&str> {
        let (_, v) = self.params.iter().find(|(n, _)| n == name)?;
        v.as_deref()
    }

    pub fn has(&self, name: &str) -> bool {
        self.params.iter().any(|(n, _)| n == name)
    }
}

/// A physical line is a comment when it starts with `**`.
fn is_comment(line: &str) -> bool {
    line.trim_start().starts_with("**")
}

fn fields(text: &str) -> Vec<String> {
    text.split(',').map(|f| f.trim().to_owned()).collect()
}

pub(crate) fn lex(text: &str) -> Vec<Block> {
    let lines: Vec<&str> = text
        .lines()
        .filter(|l| !is_comment(l) && !l.trim().is_empty())
        .collect();
    let mut blocks: Vec<Block> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let mut joined = lines[i].trim().to_owned();
        i += 1;
        while joined.ends_with(',') && i < lines.len() && !lines[i].trim_start().starts_with('*') {
            joined.push_str(lines[i].trim());
            i += 1;
        }
        if let Some(rest) = joined.strip_prefix('*') {
            let mut parts = fields(rest).into_iter();
            let name = parts.next().unwrap_or_default();
            let keyword = name.split_whitespace().collect::<Vec<_>>().join(" ");
            let params = parts
                .filter(|p| !p.is_empty())
                .map(|p| match p.split_once('=') {
                    Some((n, v)) => (n.trim().to_uppercase(), Some(v.trim().to_uppercase())),
                    None => (p.to_uppercase(), None),
                })
                .collect();
            blocks.push(Block {
                keyword: keyword.to_uppercase(),
                params,
                data: Vec::new(),
            });
        } else if let Some(block) = blocks.last_mut() {
            let mut f = fields(&joined);
            if joined.ends_with(',') {
                f.pop();
            }
            block.data.push(f);
        }
    }
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keywords_are_case_insensitive_and_params_split() {
        let b = lex("*Node, nset=Top\n1, 0, 0, 0\n");
        assert_eq!(b[0].keyword, "NODE");
        assert_eq!(b[0].value("NSET"), Some("TOP"));
        assert_eq!(b[0].data, vec![vec!["1", "0", "0", "0"]]);
    }

    #[test]
    fn comments_and_blank_lines_are_skipped() {
        let b = lex("** a\n\n*NSET, NSET=A, GENERATE\n** b\n1, 9, 2\n");
        assert_eq!(b.len(), 1);
        assert!(b[0].has("GENERATE"));
        assert_eq!(b[0].data, vec![vec!["1", "9", "2"]]);
    }

    #[test]
    fn a_line_that_ends_in_a_comma_continues() {
        let b = lex("*ELEMENT, TYPE=C3D20\n1, 1, 2, 3,\n 4, 5\n2, 6\n");
        assert_eq!(b[0].data[0], vec!["1", "1", "2", "3", "4", "5"]);
        assert_eq!(b[0].data[1], vec!["2", "6"]);
    }

    #[test]
    fn a_data_line_before_a_keyword_is_dropped() {
        assert!(lex("1, 2, 3\n").is_empty());
    }
}
