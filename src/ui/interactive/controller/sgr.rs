use std::collections::BTreeMap;

const FOREGROUND: u16 = 30;
const BACKGROUND: u16 = 40;
const UNDERLINE: u16 = 4;

/// The one `CSI parameters m` sequence that reproduces the text style a stream of output leaves
/// the terminal in, however many sequences produced it.
#[derive(Debug, Default, Clone)]
pub(super) struct SgrStyle {
    slots: BTreeMap<u16, String>,
    rendered: String,
}

impl SgrStyle {
    pub(super) fn as_str(&self) -> &str {
        &self.rendered
    }

    pub(super) fn clear(&mut self) {
        self.slots.clear();
        self.rendered.clear();
    }

    /// Folds in one `CSI parameters m` sequence.
    pub(super) fn apply(&mut self, parameters: &str) {
        let mut tokens = parameters.split(';').peekable();
        while let Some(token) = tokens.next() {
            let code = token.split(':').next().and_then(|code| code.parse().ok());
            match code {
                None | Some(0) => self.slots.clear(),
                Some(38) => self.extended(FOREGROUND, token, &mut tokens),
                Some(48) => self.extended(BACKGROUND, token, &mut tokens),
                Some(code) => self.simple(code, token),
            }
        }
        self.render();
    }

    fn simple(&mut self, code: u16, token: &str) {
        match code {
            1..=3 | 5..=9 | 53 => self.set(code, token),
            4 | 21 => self.set(UNDERLINE, token),
            22 => self.remove(&[1, 2]),
            23 => self.remove(&[3]),
            24 => self.remove(&[UNDERLINE]),
            25 => self.remove(&[5, 6]),
            27..=29 => self.remove(&[code - 20]),
            55 => self.remove(&[53]),
            30..=37 | 90..=97 => self.set(FOREGROUND, token),
            39 => self.remove(&[FOREGROUND]),
            40..=47 | 100..=107 => self.set(BACKGROUND, token),
            49 => self.remove(&[BACKGROUND]),
            _ => {}
        }
    }

    /// A `38`/`48` colour: either one colon-separated token or the next 2 or 4 `;` parameters.
    fn extended<'a>(
        &mut self,
        slot: u16,
        token: &'a str,
        rest: &mut std::iter::Peekable<impl Iterator<Item = &'a str>>,
    ) {
        let mut color = token.to_owned();
        if !token.contains(':') {
            let argument_count = match rest.peek() {
                Some(&"5") => 1,
                Some(&"2") => 3,
                _ => return,
            };
            for argument in rest.by_ref().take(1 + argument_count) {
                color.push(';');
                color.push_str(argument);
            }
        }
        self.slots.insert(slot, color);
    }

    fn set(&mut self, slot: u16, token: &str) {
        self.slots.insert(slot, token.to_owned());
    }

    fn remove(&mut self, slots: &[u16]) {
        for slot in slots {
            self.slots.remove(slot);
        }
    }

    fn render(&mut self) {
        self.rendered.clear();
        if self.slots.is_empty() {
            return;
        }
        let parameters: Vec<&str> = self.slots.values().map(String::as_str).collect();
        self.rendered = format!("\x1b[{}m", parameters.join(";"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn after(sequences: &[&str]) -> String {
        let mut style = SgrStyle::default();
        sequences.iter().for_each(|parameters| style.apply(parameters));
        style.as_str().to_owned()
    }

    #[test]
    fn attributes_accumulate_into_one_sequence() {
        assert_eq!(after(&["1", "31", "44"]), "\x1b[1;31;44m");
        assert_eq!(after(&["2;3;4"]), "\x1b[2;3;4m");
    }

    #[test]
    fn resets_clear_everything_before_what_follows_them() {
        assert_eq!(after(&["1", "0"]), "");
        assert_eq!(after(&["1", ""]), "");
        assert_eq!(after(&["1", "0;32"]), "\x1b[32m");
    }

    #[test]
    fn a_later_colour_replaces_the_earlier_one_and_default_removes_it() {
        assert_eq!(after(&["31", "32"]), "\x1b[32m");
        assert_eq!(after(&["31", "42", "39"]), "\x1b[42m");
        assert_eq!(after(&["41", "49"]), "");
    }

    #[test]
    fn attribute_off_codes_remove_only_their_attribute() {
        assert_eq!(after(&["1", "2", "3", "22"]), "\x1b[3m");
        assert_eq!(after(&["7", "4", "27"]), "\x1b[4m");
        assert_eq!(after(&["9", "29"]), "");
    }

    #[test]
    fn extended_colours_keep_their_arguments() {
        assert_eq!(after(&["38;5;208"]), "\x1b[38;5;208m");
        assert_eq!(after(&["1;48;2;1;2;3;4"]), "\x1b[1;4;48;2;1;2;3m");
        assert_eq!(after(&["38:2::1:2:3", "31"]), "\x1b[31m");
        assert_eq!(after(&["4:3"]), "\x1b[4:3m");
    }

    #[test]
    fn repeating_the_same_styles_never_grows_the_sequence() {
        let mut style = SgrStyle::default();
        for _ in 0..1000 {
            style.apply("1;34");
        }
        assert_eq!(style.as_str(), "\x1b[1;34m");
    }
}
