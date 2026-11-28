//! The line that ends a bash result whose output was long: written by the bash tool and read back
//! by the history pruner, so both sides share this one definition.

/// Summary of a finished command whose full output went to a log file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BashFooter {
    pub exit_code: i32,
    pub lines: usize,
    pub size: String,
    pub log_path: Option<String>,
}

const MARKER: &str = "[exit ";
const LEGACY_MARKER: &str = "[Command completed successfully with exit code 0 (";

impl BashFooter {
    pub fn render(&self) -> String {
        format!("[exit {}; {}]", self.exit_code, self.counts())
    }

    /// `N lines, SIZE` and the log path, without the exit status.
    pub fn counts(&self) -> String {
        let counts = format!("{} lines, {}", self.lines, self.size);
        match &self.log_path {
            Some(log) => format!("{counts}; log: {log}"),
            None => counts,
        }
    }

    /// Reads the last footer in `text`, in the current form or the one older sessions contain.
    pub fn parse(text: &str) -> Option<Self> {
        if let Some(start) = text.rfind(MARKER) {
            return Self::parse_current(&text[start + MARKER.len()..]);
        }
        Self::parse_legacy(&text[text.rfind(LEGACY_MARKER)? + LEGACY_MARKER.len()..])
    }

    fn parse_current(after_marker: &str) -> Option<Self> {
        let (exit_code, inner) = after_marker[..after_marker.find(']')?].split_once("; ")?;
        let (counts, log_path) = match inner.split_once("; log: ").or_else(|| inner.split_once("; full log: ")) {
            Some((counts, path)) => (counts, Some(path.to_string())),
            None => (inner, None),
        };
        let (lines, size) = counts.split_once(", ")?;
        Some(Self {
            exit_code: exit_code.parse().ok()?,
            lines: lines.strip_suffix(" lines")?.parse().ok()?,
            size: size.to_string(),
            log_path,
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
            exit_code: 0,
            lines: lines.split_whitespace().next()?.parse().ok()?,
            size: size.trim().to_string(),
            log_path,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_case::test_case;

    fn footer(log_path: Option<&str>) -> BashFooter {
        BashFooter {
            exit_code: 0,
            lines: 33,
            size: "1.1KB".to_string(),
            log_path: log_path.map(str::to_string),
        }
    }

    #[test]
    fn rendered_footers_parse_back_after_the_output() {
        let rendered = footer(Some("/tmp/rho-bash/ab12cd.log")).render();
        assert_eq!(rendered, "[exit 0; 33 lines, 1.1KB; log: /tmp/rho-bash/ab12cd.log]");
        let text = format!("first\nsecond\n\n{rendered}");
        assert_eq!(BashFooter::parse(&text), Some(footer(Some("/tmp/rho-bash/ab12cd.log"))));
    }

    #[test]
    fn footers_without_a_log_name_no_path_and_still_parse() {
        let rendered = footer(None).render();
        assert_eq!(rendered, "[exit 0; 33 lines, 1.1KB]");
        assert_eq!(BashFooter::parse(&rendered), Some(footer(None)));
    }

    #[test]
    fn failure_footers_keep_their_exit_code() {
        let failed = BashFooter {
            exit_code: 101,
            ..footer(Some("/tmp/rho-bash/ab12cd.log"))
        };
        let rendered = failed.render();
        assert_eq!(rendered, "[exit 101; 33 lines, 1.1KB; log: /tmp/rho-bash/ab12cd.log]");
        assert_eq!(BashFooter::parse(&format!("out\n\n{rendered}")), Some(failed));
    }

    #[test]
    fn footers_from_older_sessions_still_parse() {
        let with_log =
            "out\n\n[Command completed successfully with exit code 0 (30 lines, 300B). Full log: /tmp/log.txt]";
        assert_eq!(
            BashFooter::parse(with_log),
            Some(BashFooter {
                exit_code: 0,
                lines: 30,
                size: "300B".to_string(),
                log_path: Some("/tmp/log.txt".to_string()),
            })
        );
        let without_log = "[Command completed successfully with exit code 0 (30 lines, 300B).]";
        assert_eq!(BashFooter::parse(without_log).unwrap().log_path, None);
    }

    #[test_case("plain output"; "plain output")]
    #[test_case("[exit 0; lines, big; log: x]"; "non numeric fields")]
    #[test_case("[exit 1]"; "exit only")]
    #[test_case("[exit 0; no output]"; "no output")]
    fn text_without_a_footer_does_not_parse(text: &str) {
        assert_eq!(BashFooter::parse(text), None);
    }
}
