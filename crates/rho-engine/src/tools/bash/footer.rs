//! The line that ends a successful bash result whose output was long: written by the bash tool and
//! read back by the history pruner, so both sides share this one definition.

/// Summary of a successful command whose full output went to a log file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BashFooter {
    pub lines: usize,
    pub size: String,
    pub log_path: Option<String>,
}

const MARKER: &str = "[exit 0; ";
const LEGACY_MARKER: &str = "[Command completed successfully with exit code 0 (";

impl BashFooter {
    pub fn render(&self) -> String {
        let log = self.log_path.as_deref().unwrap_or("temp log");
        format!("[exit 0; {} lines, {}; full log: {log}]", self.lines, self.size)
    }

    /// Reads the last footer in `text`, in the current form or the one older sessions contain.
    pub fn parse(text: &str) -> Option<Self> {
        if let Some(start) = text.rfind(MARKER) {
            return Self::parse_current(&text[start + MARKER.len()..]);
        }
        Self::parse_legacy(&text[text.rfind(LEGACY_MARKER)? + LEGACY_MARKER.len()..])
    }

    fn parse_current(after_marker: &str) -> Option<Self> {
        let inner = &after_marker[..after_marker.find(']')?];
        let mut fields = inner.splitn(2, "; full log: ");
        let (lines, size) = fields.next()?.split_once(", ")?;
        Some(Self {
            lines: lines.strip_suffix(" lines")?.parse().ok()?,
            size: size.to_string(),
            log_path: fields.next().map(str::to_string),
        })
    }

    fn parse_legacy(after_marker: &str) -> Option<Self> {
        let close = after_marker.find(')')?;
        let (lines, size) = after_marker[..close].split_once(',')?;
        let rest = &after_marker[close..];
        let log_path = rest
            .split_once("Full log:")
            .and_then(|(_, tail)| tail.split_whitespace().next())
            .map(|path| path.trim_matches(']').to_string());
        Some(Self {
            lines: lines.split_whitespace().next()?.parse().ok()?,
            size: size.trim().to_string(),
            log_path,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn footer(log_path: Option<&str>) -> BashFooter {
        BashFooter {
            lines: 33,
            size: "1.1KB".to_string(),
            log_path: log_path.map(str::to_string),
        }
    }

    #[test]
    fn rendered_footers_parse_back_after_the_output() {
        let rendered = footer(Some("/tmp/rho-bash/ab12cd.log")).render();
        assert_eq!(
            rendered,
            "[exit 0; 33 lines, 1.1KB; full log: /tmp/rho-bash/ab12cd.log]"
        );
        let text = format!("first\nsecond\n\n{rendered}");
        assert_eq!(BashFooter::parse(&text), Some(footer(Some("/tmp/rho-bash/ab12cd.log"))));
    }

    #[test]
    fn footers_from_older_sessions_still_parse() {
        let with_log =
            "out\n\n[Command completed successfully with exit code 0 (30 lines, 300B). Full log: /tmp/log.txt]";
        assert_eq!(
            BashFooter::parse(with_log),
            Some(BashFooter {
                lines: 30,
                size: "300B".to_string(),
                log_path: Some("/tmp/log.txt".to_string()),
            })
        );
        let without_log = "[Command completed successfully with exit code 0 (30 lines, 300B).]";
        assert_eq!(BashFooter::parse(without_log).unwrap().log_path, None);
    }

    #[test]
    fn text_without_a_footer_does_not_parse() {
        for text in [
            "plain output",
            "[exit 0; lines, big; full log: x]",
            "[exit 1; 3 lines, 1B; full log: x]",
        ] {
            assert_eq!(BashFooter::parse(text), None, "{text}");
        }
    }
}
