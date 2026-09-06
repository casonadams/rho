struct ArgParser {
    args: Vec<String>,
    current: String,
    quote_char: Option<char>,
}

impl ArgParser {
    fn new() -> Self {
        Self {
            args: Vec::new(),
            current: String::new(),
            quote_char: None,
        }
    }

    fn push_char(&mut self, ch: char) {
        match (ch, self.quote_char) {
            (q, Some(active)) if q == active => self.quote_char = None,
            ('"' | '\'', None) => self.quote_char = Some(ch),
            (c, None) if c.is_whitespace() => {
                if !self.current.is_empty() {
                    self.args.push(std::mem::take(&mut self.current));
                }
            }
            (c, _) => self.current.push(c),
        }
    }

    fn finish(mut self) -> Vec<String> {
        if !self.current.is_empty() {
            self.args.push(self.current);
        }
        self.args
    }
}

pub fn parse_command_args(input: &str) -> Vec<String> {
    let mut parser = ArgParser::new();
    for ch in input.trim().chars() {
        parser.push_char(ch);
    }
    parser.finish()
}
