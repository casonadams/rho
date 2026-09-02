use super::interactive::{CompletionSet, CompletionSources};
use reedline::{Completer, Span, Suggestion};

#[derive(Clone)]
pub struct RhoCompleter {
    completions: CompletionSet,
}

impl RhoCompleter {
    pub fn new(sources: CompletionSources) -> Self {
        Self {
            completions: CompletionSet::from_sources(sources),
        }
    }
}

impl Completer for RhoCompleter {
    fn complete(&mut self, line: &str, pos: usize) -> Vec<Suggestion> {
        self.completions
            .complete(line, pos)
            .into_iter()
            .map(|completion| Suggestion {
                value: completion.value,
                description: None,
                style: None,
                extra: None,
                span: Span::new(completion.replacement.start, completion.replacement.end),
                append_whitespace: true,
            })
            .collect()
    }
}
