use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;

use super::builder::CompletionSet;
use super::types::{Completion, ModelItem, ProviderItem, SkillItem, THINKING_LEVELS};

fn complete_auth_args(set: &CompletionSet, prefix: &str, cursor: usize) -> Option<Vec<Completion>> {
    if let Some(argument) = prefix.strip_prefix("/login ") {
        Some(complete_provider(
            &set.providers,
            TargetArgs {
                cmd: "/login",
                argument,
                cursor,
            },
        ))
    } else {
        prefix.strip_prefix("/logout ").map(|argument| {
            complete_provider(
                &set.providers,
                TargetArgs {
                    cmd: "/logout",
                    argument,
                    cursor,
                },
            )
        })
    }
}

pub(super) fn complete_slash_args(set: &CompletionSet, prefix: &str, cursor: usize) -> Option<Vec<Completion>> {
    if let Some(argument) = prefix
        .strip_prefix("/skill ")
        .or_else(|| prefix.strip_prefix("/skills "))
    {
        return Some(complete_skills(&set.skills, argument, cursor));
    }
    if let Some(argument) = prefix.strip_prefix("/model ") {
        return Some(complete_models(&set.models, argument, cursor));
    }
    if let Some(argument) = prefix.strip_prefix("/thinking ") {
        return Some(complete_thinking(argument, cursor));
    }
    complete_auth_args(set, prefix, cursor)
}

fn complete_skills(skills: &[SkillItem], argument: &str, cursor: usize) -> Vec<Completion> {
    let matcher = SkimMatcherV2::default();
    let mut scored: Vec<(i64, &SkillItem)> = skills
        .iter()
        .filter_map(|s| {
            if argument.is_empty() {
                Some((0, s))
            } else {
                matcher.fuzzy_match(&s.name, argument).map(|score| (score, s))
            }
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));

    scored
        .into_iter()
        .map(|(_, s)| Completion {
            value: format!("/skill {}", s.name),
            description: Some(format!("{} [{}]", s.description, s.origin)),
            replacement: 0..cursor,
        })
        .collect()
}

fn complete_models(models: &[ModelItem], argument: &str, cursor: usize) -> Vec<Completion> {
    let matcher = SkimMatcherV2::default();
    let mut scored: Vec<(i64, &ModelItem)> = models
        .iter()
        .filter_map(|m| {
            if argument.is_empty() {
                Some((0, m))
            } else {
                let query_target = format!("{}:{}", m.provider, m.id);
                matcher
                    .fuzzy_match(&m.id, argument)
                    .or_else(|| matcher.fuzzy_match(&query_target, argument))
                    .map(|score| (score, m))
            }
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.id.cmp(&b.1.id)));

    scored
        .into_iter()
        .map(|(_, m)| Completion {
            value: format!("/model {}", m.id),
            description: Some(format!("{} · {}", m.provider, m.description)),
            replacement: 0..cursor,
        })
        .collect()
}

fn complete_thinking(argument: &str, cursor: usize) -> Vec<Completion> {
    let matcher = SkimMatcherV2::default();
    let mut scored: Vec<(i64, &(&str, &str))> = THINKING_LEVELS
        .iter()
        .filter_map(|lvl| {
            if argument.is_empty() {
                Some((0, lvl))
            } else {
                matcher.fuzzy_match(lvl.0, argument).map(|score| (score, lvl))
            }
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.0.cmp(b.1.0)));

    scored
        .into_iter()
        .map(|(_, lvl)| Completion {
            value: format!("/thinking {}", lvl.0),
            description: Some(lvl.1.to_string()),
            replacement: 0..cursor,
        })
        .collect()
}

struct TargetArgs<'a> {
    cmd: &'a str,
    argument: &'a str,
    cursor: usize,
}

fn complete_provider(providers: &[ProviderItem], target: TargetArgs<'_>) -> Vec<Completion> {
    let matcher = SkimMatcherV2::default();
    let mut scored: Vec<(i64, &ProviderItem)> = providers
        .iter()
        .filter_map(|p| {
            if target.argument.is_empty() {
                Some((0, p))
            } else {
                matcher.fuzzy_match(&p.name, target.argument).map(|score| (score, p))
            }
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));

    scored
        .into_iter()
        .map(|(_, p)| Completion {
            value: format!("{} {}", target.cmd, p.name),
            description: Some(p.auth_mode.clone()),
            replacement: 0..target.cursor,
        })
        .collect()
}
