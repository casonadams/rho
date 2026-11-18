use super::types::{CommandResult, SlashCommandContext};
use rho_engine::mcp::{McpPromptDefinition, McpPromptResult, McpRuntime};
use rho_harness_core::error::Result;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::Arc;

const USAGE: &str =
    "  Usage: /mcp login|logout <server>, /mcp prompts [server], /mcp prompt <server> <name> [key=value ...]\n";

pub(crate) async fn handle_mcp(ctx: &mut SlashCommandContext<'_>, parts: &[&str]) -> Result<Option<CommandResult>> {
    let result = match (parts.get(1).copied(), parts.get(2)) {
        (Some("login"), Some(server)) => CommandResult::Login {
            provider: Some(format!("mcp:{server}")),
        },
        (Some("logout"), Some(server)) => CommandResult::Logout {
            provider: Some(format!("mcp:{server}")),
        },
        (Some("prompts"), server) => {
            list_prompts(ctx, server.copied()).await;
            CommandResult::Continue
        }
        (Some("prompt"), Some(_)) => expand_prompt(ctx, &parts[2..]).await,
        (Some(_), _) => {
            ctx.renderer.print_notice(USAGE);
            CommandResult::Continue
        }
        (None, _) if ctx.renderer.has_interactive_ui() => CommandResult::OpenMcpSelector,
        (None, _) => {
            ctx.renderer.print_notice("  Configure MCP in ~/.config/rho/mcp.json\n");
            CommandResult::Continue
        }
    };
    Ok(Some(result))
}

fn runtime(ctx: &SlashCommandContext<'_>) -> Option<Arc<McpRuntime>> {
    let runtime = ctx.engine.and_then(|e| e.mcp_runtime()).cloned();
    if runtime.is_none() {
        ctx.renderer
            .print_notice("  No MCP servers are loaded in this session.\n");
    }
    runtime
}

fn format_prompt_line(server: &str, prompt: &McpPromptDefinition) -> String {
    let args: Vec<String> = prompt
        .arguments
        .iter()
        .map(|a| {
            let required = if a.required == Some(true) { "*" } else { "" };
            format!("{}{required}", a.name)
        })
        .collect();
    let mut line = format!("    /mcp prompt {server} {}", prompt.name);
    if !args.is_empty() {
        let _ = write!(line, " [{}]", args.join(", "));
    }
    if let Some(desc) = prompt.description.as_deref().filter(|d| !d.is_empty()) {
        let _ = write!(line, " - {desc}");
    }
    line
}

async fn list_prompts(ctx: &SlashCommandContext<'_>, server: Option<&str>) {
    let Some(runtime) = runtime(ctx) else { return };
    let servers: Vec<String> = match server {
        Some(name) => vec![name.to_string()],
        None => runtime.server_names().map(str::to_string).collect(),
    };
    let listings = futures::future::join_all(servers.iter().map(|name| runtime.list_prompts(name))).await;
    let mut out = String::from("  MCP prompts (* = required argument):\n");
    let mut unavailable = Vec::new();
    for (name, listing) in servers.iter().zip(listings) {
        match listing {
            Ok(prompts) => prompts
                .iter()
                .for_each(|p| out.push_str(&format!("{}\n", format_prompt_line(name, p)))),
            Err(e) => unavailable.push(format!("{name} ({e})")),
        }
    }
    if !unavailable.is_empty() {
        let _ = writeln!(out, "  Without prompts or unavailable: {}", unavailable.join("; "));
    }
    ctx.renderer.print_notice(&out);
}

pub(crate) fn parse_prompt_args(tokens: &[&str]) -> std::result::Result<BTreeMap<String, String>, String> {
    tokens
        .iter()
        .map(|token| match token.split_once('=') {
            Some((key, value)) if !key.is_empty() => Ok((key.to_string(), value.to_string())),
            _ => Err(format!("Prompt arguments must be key=value, got '{token}'")),
        })
        .collect()
}

pub(crate) fn render_prompt(result: &McpPromptResult) -> String {
    result
        .messages
        .iter()
        .map(|message| {
            let text = rho_engine::mcp::result::describe_content(&message.content);
            if message.role == "user" {
                text
            } else {
                format!("[{}]\n{text}", message.role)
            }
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

async fn expand_prompt(ctx: &SlashCommandContext<'_>, rest: &[&str]) -> CommandResult {
    let (Some(server), Some(name)) = (rest.first(), rest.get(1)) else {
        ctx.renderer.print_notice(USAGE);
        return CommandResult::Continue;
    };
    let arguments = match parse_prompt_args(&rest[2..]) {
        Ok(arguments) => arguments,
        Err(message) => {
            ctx.renderer.print_notice(&format!("  {message}\n"));
            return CommandResult::Continue;
        }
    };
    let Some(runtime) = runtime(ctx) else {
        return CommandResult::Continue;
    };
    match runtime.get_prompt(server, name, &arguments).await {
        Ok(result) if !result.messages.is_empty() => CommandResult::ExpandedPrompt {
            text: render_prompt(&result),
        },
        Ok(_) => {
            ctx.renderer
                .print_notice(&format!("  MCP prompt '{name}' on '{server}' returned no messages.\n"));
            CommandResult::Continue
        }
        Err(e) => {
            ctx.renderer
                .print_notice(&format!("  MCP prompt '{name}' on '{server}' failed: {e}\n"));
            CommandResult::Continue
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rho_engine::mcp::{McpContent, McpPromptArgument, McpPromptMessage};

    #[test]
    fn prompt_arguments_parse_as_key_value_pairs() {
        assert_eq!(
            parse_prompt_args(&["team=ENG", "title=a=b"]).unwrap(),
            BTreeMap::from([("team".into(), "ENG".into()), ("title".into(), "a=b".into())])
        );
        assert!(parse_prompt_args(&["loose"]).is_err());
        assert!(parse_prompt_args(&["=x"]).is_err());
    }

    #[test]
    fn prompt_messages_render_user_text_and_label_other_roles() {
        let message = |role: &str, text: &str| McpPromptMessage {
            role: role.into(),
            content: McpContent {
                kind: "text".into(),
                text: Some(text.into()),
                ..Default::default()
            },
        };
        let result = McpPromptResult {
            description: None,
            messages: vec![message("user", "Review PR 7"), message("assistant", "Sure.")],
        };
        assert_eq!(render_prompt(&result), "Review PR 7\n\n[assistant]\nSure.");
    }

    #[test]
    fn prompt_listing_shows_invocation_and_required_arguments() {
        let prompt = McpPromptDefinition {
            name: "review".into(),
            description: Some("Review a PR".into()),
            arguments: vec![
                McpPromptArgument {
                    name: "pr".into(),
                    description: None,
                    required: Some(true),
                },
                McpPromptArgument {
                    name: "focus".into(),
                    description: None,
                    required: None,
                },
            ],
        };
        assert_eq!(
            format_prompt_line("github", &prompt),
            "    /mcp prompt github review [pr*, focus] - Review a PR"
        );
    }
}
