#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Element>,
    pub text: Option<String>,
    pub line: usize,
}

impl Element {
    #[must_use]
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
}

struct Reader<'a> {
    src: &'a str,
    pos: usize,
    raw_text: &'a [&'a str],
}

/// Parses the XML subset workflows use. Elements named in `raw_text` keep their body verbatim up to the closing tag.
pub fn parse_document(src: &str, raw_text: &[&str]) -> Result<Element, String> {
    let mut reader = Reader { src, pos: 0, raw_text };
    reader.skip_misc()?;
    let root = reader.element()?;
    reader.skip_misc()?;
    if reader.pos < src.len() {
        return Err(reader.error("unexpected content after the root element"));
    }
    Ok(root)
}

impl Reader<'_> {
    fn rest(&self) -> &str {
        &self.src[self.pos..]
    }

    fn line(&self) -> usize {
        self.src[..self.pos].matches('\n').count() + 1
    }

    fn error(&self, message: &str) -> String {
        format!("line {}: {message}", self.line())
    }

    fn skip_ws(&mut self) {
        let trimmed = self.rest().trim_start();
        self.pos = self.src.len() - trimmed.len();
    }

    fn skip_past(&mut self, end: &str, what: &str) -> Result<(), String> {
        match self.rest().find(end) {
            Some(i) => {
                self.pos += i + end.len();
                Ok(())
            }
            None => Err(self.error(&format!("unterminated {what}"))),
        }
    }

    fn skip_misc(&mut self) -> Result<(), String> {
        loop {
            self.skip_ws();
            if self.rest().starts_with("<!--") {
                self.skip_past("-->", "comment")?;
            } else if self.rest().starts_with("<?") {
                self.skip_past("?>", "declaration")?;
            } else {
                return Ok(());
            }
        }
    }

    fn name(&mut self) -> Result<String, String> {
        let len = self
            .rest()
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == ':'))
            .unwrap_or(self.rest().len());
        if len == 0 {
            return Err(self.error("expected a name"));
        }
        let name = self.rest()[..len].to_string();
        self.pos += len;
        Ok(name)
    }

    fn attr_value(&mut self) -> Result<String, String> {
        let quote = self.rest().chars().next().filter(|c| *c == '"' || *c == '\'');
        let Some(quote) = quote else {
            return Err(self.error("attribute values must be quoted"));
        };
        self.pos += 1;
        let end = self
            .rest()
            .find(quote)
            .ok_or_else(|| self.error("unterminated attribute value"))?;
        let raw = &self.rest()[..end];
        let value = decode_entities(raw).map_err(|e| self.error(&e))?;
        self.pos += end + 1;
        Ok(value)
    }

    fn attrs(&mut self) -> Result<(Vec<(String, String)>, bool), String> {
        let mut attrs: Vec<(String, String)> = Vec::new();
        loop {
            self.skip_ws();
            if let Some(rest) = self.rest().strip_prefix("/>") {
                self.pos = self.src.len() - rest.len();
                return Ok((attrs, true));
            }
            if self.rest().starts_with('>') {
                self.pos += 1;
                return Ok((attrs, false));
            }
            let key = self.name()?;
            self.skip_ws();
            if !self.rest().starts_with('=') {
                return Err(self.error(&format!("attribute '{key}' needs a value")));
            }
            self.pos += 1;
            self.skip_ws();
            let value = self.attr_value()?;
            if attrs.iter().any(|(k, _)| *k == key) {
                return Err(self.error(&format!("duplicate attribute '{key}'")));
            }
            attrs.push((key, value));
        }
    }

    fn element(&mut self) -> Result<Element, String> {
        if !self.rest().starts_with('<') {
            return Err(self.error("expected an element"));
        }
        let line = self.line();
        self.pos += 1;
        let name = self.name()?;
        let (attrs, self_closing) = self.attrs()?;
        let mut element = Element {
            name,
            attrs,
            children: Vec::new(),
            text: None,
            line,
        };
        if self_closing {
            return Ok(element);
        }
        if self.raw_text.contains(&element.name.as_str()) {
            let close = format!("</{}>", element.name);
            let end = self
                .rest()
                .find(&close)
                .ok_or_else(|| format!("line {line}: <{}> is missing {close}", element.name))?;
            element.text = Some(self.rest()[..end].to_string());
            self.pos += end + close.len();
            return Ok(element);
        }
        self.children(&mut element)?;
        Ok(element)
    }

    fn children(&mut self, element: &mut Element) -> Result<(), String> {
        loop {
            self.skip_misc()?;
            if let Some(rest) = self.rest().strip_prefix("</") {
                self.pos = self.src.len() - rest.len();
                let name = self.name()?;
                if name != element.name {
                    return Err(self.error(&format!("expected </{}>, found </{name}>", element.name)));
                }
                self.skip_ws();
                if !self.rest().starts_with('>') {
                    return Err(self.error("expected '>'"));
                }
                self.pos += 1;
                return Ok(());
            }
            if self.rest().is_empty() {
                return Err(format!("line {}: <{}> is never closed", element.line, element.name));
            }
            if !self.rest().starts_with('<') {
                return Err(self.error(&format!("text is not allowed inside <{}>", element.name)));
            }
            let child = self.element()?;
            element.children.push(child);
        }
    }
}

fn decode_entities(raw: &str) -> Result<String, String> {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let after = &rest[i..];
        let end = after.find(';').ok_or("unescaped '&' in attribute (use &amp;)")?;
        let ch = match &after[..=end] {
            "&amp;" => '&',
            "&lt;" => '<',
            "&gt;" => '>',
            "&quot;" => '"',
            "&apos;" => '\'',
            other => return Err(format!("unknown entity '{other}'")),
        };
        out.push(ch);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_case::test_case;

    #[test]
    fn parses_elements_attributes_and_raw_text() {
        let src = r#"<?xml version="1.0"?>
<!-- a comment -->
<workflow name="w" note='single &amp; "double"'>
  <run cmd="echo hi"/>
  <agent use="scout">
    Find Vec<String> && <file> usages.
  </agent>
</workflow>"#;
        let root = parse_document(src, &["agent"]).unwrap();
        assert_eq!(root.name, "workflow");
        assert_eq!(root.attr("note"), Some(r#"single & "double""#));
        assert_eq!(root.children.len(), 2);
        assert_eq!(root.children[0].attr("cmd"), Some("echo hi"));
        assert_eq!(root.children[0].line, 4);
        let agent = &root.children[1];
        assert_eq!(
            agent.text.as_deref().map(str::trim),
            Some("Find Vec<String> && <file> usages.")
        );
    }

    #[test_case("<a>\n<b>\n</a>", "line 3: expected </b>, found </a>"; "mismatched close")]
    #[test_case("<a>\n  stray\n</a>", "line 2: text is not allowed inside <a>"; "stray text")]
    #[test_case("<a x=1/>", "line 1: attribute values must be quoted"; "unquoted attribute")]
    #[test_case("<a x=\"1\" x=\"2\"/>", "duplicate attribute 'x'"; "duplicate attribute")]
    #[test_case("<a x=\"&bogus;\"/>", "unknown entity '&bogus;'"; "unknown entity")]
    #[test_case("<a>\n<agent>never closed", "line 2: <agent> is missing </agent>"; "unclosed text element")]
    #[test_case("<a>", "line 1: <a> is never closed"; "unclosed root")]
    #[test_case("<a/><b/>", "unexpected content after the root element"; "trailing content")]
    fn reports_errors_with_line_numbers(src: &str, expected: &str) {
        let err = parse_document(src, &["agent"]).unwrap_err();
        assert!(err.contains(expected), "{err}");
    }
}
