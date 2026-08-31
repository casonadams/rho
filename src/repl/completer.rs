use super::interactive::CompletionSet;
use reedline::{Completer, Span, Suggestion};

#[derive(Clone)]
pub struct RhoCompleter {
    completions: CompletionSet,
}

impl RhoCompleter {
    pub fn new(extension_commands: &[(&str, &str)], skill_names: Vec<String>, prompt_templates: Vec<String>) -> Self {
        Self {
            completions: CompletionSet::rho(extension_commands, skill_names, prompt_templates),
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
