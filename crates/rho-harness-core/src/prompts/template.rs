use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PromptTemplateMetadata {
    pub name: String,
    pub description: Option<String>,
    pub argument_hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptTemplate {
    pub metadata: PromptTemplateMetadata,
    pub body: String,
    pub origin: String,
}

impl PromptTemplate {
    pub fn parse(name: &str, content: &str, origin: &str) -> Self {
        let (metadata, body) = parse_frontmatter(name, content);
        Self {
            metadata,
            body,
            origin: origin.to_string(),
        }
    }

    pub fn expand(&self, args: &[&str]) -> String {
        let full_args = args.join(" ");
        let mut result = self.body.clone();

        while let Some(start_idx) = result.find("${") {
            if let Some(end_rel) = result[start_idx..].find('}') {
                let end_idx = start_idx + end_rel;
                let pattern = &result[start_idx + 2..end_idx];
                let replacement = expand_braced_pattern(pattern, args, &full_args);
                result.replace_range(start_idx..=end_idx, &replacement);
            } else {
                break;
            }
        }

        for i in 1..=9 {
            let var = format!("${i}");
            let val = args.get(i - 1).copied().unwrap_or("");
            result = result.replace(&var, val);
        }

        result = result.replace("$ARGUMENTS", &full_args).replace("$@", &full_args);

        result
    }
}

fn expand_slice_pattern(rest: &str, args: &[&str]) -> String {
    let parts: Vec<&str> = rest.split(':').collect();
    let start = parts
        .first()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(1)
        .saturating_sub(1);
    if start >= args.len() {
        return String::new();
    }
    let slice = match parts.get(1).and_then(|s| s.parse::<usize>().ok()) {
        Some(l) => &args[start..(start + l).min(args.len())],
        None => &args[start..],
    };
    slice.join(" ")
}

fn expand_default_pattern((key, default_val): (&str, &str), args: &[&str], full_args: &str) -> String {
    match key.trim() {
        "@" | "ARGUMENTS" => {
            if full_args.trim().is_empty() {
                default_val.to_string()
            } else {
                full_args.to_string()
            }
        }
        num => num
            .parse::<usize>()
            .ok()
            .and_then(|n| args.get(n.saturating_sub(1)))
            .filter(|v| !v.trim().is_empty())
            .map(|v| (*v).to_string())
            .unwrap_or_else(|| default_val.to_string()),
    }
}

fn expand_positional_pattern(pattern: &str, args: &[&str], full_args: &str) -> String {
    match pattern.trim() {
        "@" | "ARGUMENTS" => full_args.to_string(),
        num => {
            if let Ok(n) = num.parse::<usize>() {
                args.get(n.saturating_sub(1)).copied().unwrap_or("").to_string()
            } else {
                format!("${{{pattern}}}")
            }
        }
    }
}

fn expand_braced_pattern(pattern: &str, args: &[&str], full_args: &str) -> String {
    if let Some(rest) = pattern.strip_prefix("@:") {
        expand_slice_pattern(rest, args)
    } else if let Some(pattern_pair) = pattern.split_once(":-") {
        expand_default_pattern(pattern_pair, args, full_args)
    } else {
        expand_positional_pattern(pattern, args, full_args)
    }
}

fn parse_frontmatter_lines(frontmatter_str: &str) -> (Option<String>, Option<String>) {
    let mut description = None;
    let mut argument_hint = None;
    for line in frontmatter_str.lines() {
        let line = line.trim();
        if let Some(val) = line.strip_prefix("description:") {
            description = Some(val.trim().trim_matches('"').trim_matches('\'').to_string());
        } else if let Some(val) = line
            .strip_prefix("argument-hint:")
            .or_else(|| line.strip_prefix("argument_hint:"))
        {
            argument_hint = Some(val.trim().trim_matches('"').trim_matches('\'').to_string());
        }
    }
    (description, argument_hint)
}

fn extract_frontmatter(content: &str) -> Option<(&str, String)> {
    let rest = content.trim_start().strip_prefix("---")?;
    let end_idx = rest.find("\n---")?;
    let frontmatter_str = &rest[..end_idx];
    let body = rest[end_idx + 4..]
        .trim_start_matches('\n')
        .trim_start_matches('\r')
        .to_string();
    Some((frontmatter_str, body))
}

fn parse_frontmatter(name: &str, content: &str) -> (PromptTemplateMetadata, String) {
    if let Some((fm, body)) = extract_frontmatter(content) {
        let (description, argument_hint) = parse_frontmatter_lines(fm);
        return (
            PromptTemplateMetadata {
                name: name.to_string(),
                description,
                argument_hint,
            },
            body,
        );
    }
    let first_line = content
        .lines()
        .find(|l| !l.trim().is_empty())
        .map(|l| l.trim().to_string());
    (
        PromptTemplateMetadata {
            name: name.to_string(),
            description: first_line,
            argument_hint: None,
        },
        content.to_string(),
    )
}

#[cfg(test)]
mod tests;
