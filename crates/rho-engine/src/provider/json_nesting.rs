/// How deeply a JSON text streamed in fragments nests, counting brackets outside its strings.
#[derive(Debug, Default)]
pub(crate) struct JsonNesting {
    depth: usize,
    in_string: bool,
    escaped: bool,
}

impl JsonNesting {
    /// Feeds the next fragment and returns the shallowest depth a closing bracket in it returned
    /// to, so `Some(0)` means a top-level value has just closed.
    pub(crate) fn feed(&mut self, fragment: &str) -> Option<usize> {
        let mut shallowest = None;
        for byte in fragment.bytes() {
            if let Some(depth) = self.step(byte) {
                shallowest = Some(shallowest.map_or(depth, |s: usize| s.min(depth)));
            }
        }
        shallowest
    }

    fn step(&mut self, byte: u8) -> Option<usize> {
        if self.in_string {
            match (self.escaped, byte) {
                (true, _) => self.escaped = false,
                (false, b'\\') => self.escaped = true,
                (false, b'"') => self.in_string = false,
                _ => {}
            }
            return None;
        }
        match byte {
            b'"' => self.in_string = true,
            b'{' | b'[' => self.depth += 1,
            b'}' | b']' if self.depth > 0 => {
                self.depth -= 1;
                return Some(self.depth);
            }
            _ => {}
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_case::test_case;

    #[test_case(&["{\"a\": [1, {\"b\"", ": 2}]", "}"], &[None, Some(1), Some(0)]; "nested closes")]
    #[test_case(&["{\"s\": \"}]\\\"{", "\"", "}  "], &[None, None, Some(0)]; "brackets inside string")]
    #[test_case(&["{\"s\": \"x\\", "\"}\"", "}"], &[None, None, Some(0)]; "escape split across fragments")]
    #[test_case(&["{}", "]"], &[Some(0), None]; "unbalanced close ignored")]
    #[test_case(&["{\"e\": [{}, {", "}]}"], &[Some(2), Some(0)]; "multiple closes in fragment")]
    fn reports_the_depth_closing_brackets_return_to_outside_strings(fragments: &[&str], expected: &[Option<usize>]) {
        let mut nesting = JsonNesting::default();
        let closes: Vec<Option<usize>> = fragments.iter().map(|f| nesting.feed(f)).collect();
        assert_eq!(closes, expected);
    }
}
