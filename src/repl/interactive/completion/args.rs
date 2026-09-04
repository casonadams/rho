use super::builder::CompletionSet;
use super::types::{Completion, ModelItem, ProviderItem, SkillItem, THINKING_LEVELS};
use crate::repl::interactive::fuzzy::fuzzy_match;

pub(super) fn complete_slash_args(set: &CompletionSet, prefix: &str, cursor: usize) -> Option<Vec<Completion>> {
    if let Some(argument) = prefix
        .strip_prefix("/skill ")
        .or_else(|| prefix.strip_prefix("/skills "))
    {
        Some(complete_skills(&set.skills, argument, cursor))
    } else if let Some(argument) = prefix.strip_prefix("/model ") {
        Some(complete_models(&set.models, argument, cursor))
    } else if let Some(argument) = prefix.strip_prefix("/thinking ") {
        Some(complete_thinking(argument, cursor))
    } else if let Some(argument) = prefix.strip_prefix("/login ") {
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

fn complete_skills(skills: &[SkillItem], argument: &str, cursor: usize) -> Vec<Completion> {
    let mut scored: Vec<(i32, &SkillItem)> = skills
        .iter()
        .filter_map(|s| {
            if argument.is_empty() {
                Some((0, s))
            } else {
                fuzzy_match(argument, &s.name).map(|score| (score, s))
            }
        })
        .collect();
    scored.sort_by_key(|(score, s)| (*score, s.name.clone()));

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
    let mut scored: Vec<(i32, &ModelItem)> = models
        .iter()
        .filter_map(|m| {
            if argument.is_empty() {
                Some((0, m))
            } else {
                let query_target = format!("{}:{}", m.provider, m.id);
                fuzzy_match(argument, &m.id)
                    .or_else(|| fuzzy_match(argument, &query_target))
                    .map(|score| (score, m))
            }
        })
        .collect();
    scored.sort_by_key(|(score, m)| (*score, m.id.clone()));

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
    let mut scored: Vec<(i32, &(&str, &str))> = THINKING_LEVELS
        .iter()
        .filter_map(|lvl| {
            if argument.is_empty() {
                Some((0, lvl))
            } else {
                fuzzy_match(argument, lvl.0).map(|score| (score, lvl))
            }
        })
        .collect();
    scored.sort_by_key(|(score, lvl)| (*score, lvl.0.to_string()));

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
    let mut scored: Vec<(i32, &ProviderItem)> = providers
        .iter()
        .filter_map(|p| {
            if target.argument.is_empty() {
                Some((0, p))
            } else {
                fuzzy_match(target.argument, &p.name).map(|score| (score, p))
            }
        })
        .collect();
    scored.sort_by_key(|(score, p)| (*score, p.name.clone()));

    scored
        .into_iter()
        .map(|(_, p)| Completion {
            value: format!("{} {}", target.cmd, p.name),
            description: Some(p.auth_mode.clone()),
            replacement: 0..target.cursor,
        })
        .collect()
}
