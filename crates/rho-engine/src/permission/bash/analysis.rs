use super::filter::{collect_arg_paths, extract_redirect_targets, strip_assignments, strip_wrappers};
use super::lexer::tokenize;
use super::token::{Token, TokenKind};

pub struct BashAnalysis {
    pub commands: Vec<String>,
    pub path_tokens: Vec<String>,
    pub suspicious: bool,
}

struct Segment {
    tokens: Vec<Token>,
    dangling: bool,
}

struct SegmentAnalysis {
    command: Option<String>,
    path_tokens: Vec<String>,
}

struct AnalysisCollector {
    commands: Vec<String>,
    path_tokens: Vec<String>,
    has_dangling: bool,
}

impl AnalysisCollector {
    fn new() -> Self {
        Self {
            commands: Vec::new(),
            path_tokens: Vec::new(),
            has_dangling: false,
        }
    }

    fn record_segment(&mut self, segment: Segment) {
        if segment.dangling {
            self.has_dangling = true;
        }
        let analysis = analyze_segment(segment);
        if let Some(cmd) = analysis.command {
            self.commands.push(cmd);
        }
        for token in analysis.path_tokens {
            if !self.path_tokens.contains(&token) {
                self.path_tokens.push(token);
            }
        }
    }
}

pub fn analyze_bash_command(command: &str) -> BashAnalysis {
    let token_res = tokenize(command);
    let mut collector = AnalysisCollector::new();
    for segment in split_segments(&token_res.tokens) {
        collector.record_segment(segment);
    }

    let suspicious = token_res.suspicious || collector.commands.is_empty() || collector.has_dangling;
    BashAnalysis {
        commands: collector.commands,
        path_tokens: collector.path_tokens,
        suspicious,
    }
}

fn split_segments(tokens: &[Token]) -> Vec<Segment> {
    let mut segments = Vec::new();
    let mut current = Vec::new();
    let mut last_sep = None;

    for token in tokens {
        if token.kind == TokenKind::Separator {
            segments.push(Segment {
                dangling: current.is_empty(),
                tokens: std::mem::take(&mut current),
            });
            last_sep = Some(token.raw.clone());
            continue;
        }
        current.push(token.clone());
    }

    let trailing_hard = check_trailing_hard(tokens, last_sep.as_deref());
    segments.push(Segment {
        dangling: trailing_hard,
        tokens: current,
    });
    segments
}

fn check_trailing_hard(tokens: &[Token], last_sep: Option<&str>) -> bool {
    let Some(last) = tokens.last() else {
        return false;
    };
    if last.kind != TokenKind::Separator {
        return false;
    }
    last_sep != Some(";") && last_sep != Some("\n")
}

fn analyze_segment(segment: Segment) -> SegmentAnalysis {
    let mut words = segment.tokens;
    let mut path_tokens = Vec::new();

    extract_redirect_targets(&words, &mut path_tokens);
    strip_assignments(&mut words, &mut path_tokens);
    strip_wrappers(&mut words);

    if words.is_empty() || words[0].kind != TokenKind::Word {
        return SegmentAnalysis {
            command: None,
            path_tokens,
        };
    }

    let command = words.iter().map(|t| t.raw.as_str()).collect::<Vec<_>>().join(" ");
    collect_arg_paths(&words, &mut path_tokens);

    SegmentAnalysis {
        command: Some(command),
        path_tokens,
    }
}
