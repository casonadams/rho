use super::formatters::{format_edit_diff, format_write_preview};
use crate::ui::interactive::{InteractionOption, InteractionPrompt, InteractionResponse};
use crate::ui::render::TerminalRenderer;
use rho_core::bash_ast::RiskTier;
use rho_core::presentation::summary::{clean_command_paths, format_tool_args_summary};
use rho_core::presentation::{ApprovalResult, BashApproval};
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolApprovalChoice {
    ApplyOnce,
    Deny,
}

impl fmt::Display for ToolApprovalChoice {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str(match self {
            Self::ApplyOnce => "Apply once",
            Self::Deny => "Deny",
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BashApprovalChoice {
    AllowOnce,
    AllowForSession(String),
    Deny,
}

impl fmt::Display for BashApprovalChoice {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::AllowOnce => formatter.write_str("Allow once"),
            Self::AllowForSession(scope) => write!(formatter, "Allow {scope} for session"),
            Self::Deny => formatter.write_str("Deny"),
        }
    }
}

pub fn interaction_option(label: &str) -> InteractionOption {
    InteractionOption {
        label: label.to_string(),
        description: None,
    }
}

pub fn denied(reason: String) -> ApprovalResult {
    ApprovalResult::Denied {
        reason: reason.trim().to_string(),
    }
}

impl TerminalRenderer {
    pub async fn prompt_continue_budget(&self, max_turns: usize) -> bool {
        if let Some(ui) = &self.ui {
            let response = ui
                .request(InteractionPrompt {
                    title: "Turn Limit Reached".to_string(),
                    body: format!("Agent reached turn budget ({max_turns} calls)."),
                    options: vec![
                        interaction_option("Continue for another 50 turns"),
                        interaction_option("Stop"),
                    ],
                    initial_selection: 0,
                    allow_custom: false,
                })
                .await;
            return matches!(response, Ok(InteractionResponse::Selected(0)));
        }

        let header = self.theme.highlight;
        let dim = self.theme.dimmed;
        println!(
            "\n{header}Turn Limit Reached:{header:#} {dim}Agent reached turn budget ({max_turns} calls).{dim:#}\n"
        );
        let approved = inquire::Confirm::new("Continue execution for another 50 turns?")
            .with_default(true)
            .prompt();
        println!();
        approved.unwrap_or(false)
    }

    pub async fn prompt_tool_approval(&self, name: &str, args: &serde_json::Value) -> ApprovalResult {
        if let Some(ui) = &self.ui {
            let summary = format_tool_args_summary(name, args);
            let mut body = format!("tool   {name}\nscope  {summary}");
            if name == "edit"
                && let Some(diff) = format_edit_diff(args, &self.theme)
            {
                body.push_str("\n\n");
                body.push_str(&diff);
            } else if name == "write"
                && let Some(preview) = format_write_preview(args, &self.theme)
            {
                body.push_str("\n\n");
                body.push_str(&preview);
            }
            let response = ui
                .request(InteractionPrompt {
                    title: format!("Approve {name}"),
                    body,
                    options: vec![
                        InteractionOption {
                            label: "Allow".to_string(),
                            description: Some("Allow this single invocation".to_string()),
                        },
                        InteractionOption {
                            label: "Deny with reason".to_string(),
                            description: Some("Deny and provide feedback to the agent".to_string()),
                        },
                    ],
                    initial_selection: 0,
                    allow_custom: false,
                })
                .await;
            return match response {
                Ok(InteractionResponse::Selected(0)) => ApprovalResult::Approved,
                Ok(InteractionResponse::Selected(1)) => self.prompt_denial_feedback().await,
                Ok(InteractionResponse::Custom(reason)) => denied(reason),
                Ok(InteractionResponse::Selected(_) | InteractionResponse::Cancelled) | Err(_) => denied(String::new()),
            };
        }

        let header = self.theme.highlight;
        let dim = self.theme.dimmed;
        println!(
            "\n{header}Approve {name}:{header:#} {dim}{}{dim:#}\n",
            format_tool_args_summary(name, args)
        );
        if name == "edit"
            && let Some(diff) = format_edit_diff(args, &self.theme)
        {
            println!("{diff}");
        } else if name == "write"
            && let Some(preview) = format_write_preview(args, &self.theme)
        {
            println!("{preview}");
        }
        let choice =
            inquire::Select::new("Action:", vec![ToolApprovalChoice::ApplyOnce, ToolApprovalChoice::Deny]).prompt();
        println!();
        match choice {
            Ok(ToolApprovalChoice::ApplyOnce) => ApprovalResult::Approved,
            Ok(ToolApprovalChoice::Deny) => self.prompt_denial_feedback().await,
            Err(_) => denied(String::new()),
        }
    }

    pub async fn prompt_bash_approval(&self, request: BashApproval) -> ApprovalResult {
        let mut actions = vec![BashApprovalChoice::AllowOnce];
        if let Some(patterns) = crate::tools::analyze_command_safety(&request.command).session_patterns {
            actions.push(BashApprovalChoice::AllowForSession(patterns.join("; ")));
        }
        actions.push(BashApprovalChoice::Deny);

        let starting_cursor = if request.tier == RiskTier::HighRisk {
            actions.len() - 1
        } else {
            0
        };

        if let Some(ui) = &self.ui {
            let mut options = vec![InteractionOption {
                label: "Allow".to_string(),
                description: Some("Allow this single invocation".to_string()),
            }];
            if let Some(patterns) = crate::tools::analyze_command_safety(&request.command).session_patterns {
                options.push(InteractionOption {
                    label: "Allow for session".to_string(),
                    description: Some(format!("Allow {} for session", patterns.join("; "))),
                });
            }
            options.push(InteractionOption {
                label: "Deny with reason".to_string(),
                description: Some("Deny and provide feedback to the agent".to_string()),
            });

            let mut body = format!("tool   bash\nscope  {}", clean_command_paths(&request.command));
            if request.tier == RiskTier::HighRisk && !request.reasons.is_empty() {
                body.push_str("\n\n");
                body.push_str(&request.reasons.join("\n"));
            }

            let response = ui
                .request(InteractionPrompt {
                    title: "Approve bash".to_string(),
                    body,
                    options,
                    initial_selection: starting_cursor,
                    allow_custom: false,
                })
                .await;

            return match response {
                Ok(InteractionResponse::Selected(0)) => ApprovalResult::Approved,
                Ok(InteractionResponse::Selected(1)) if actions.len() > 2 => {
                    if let Some(BashApprovalChoice::AllowForSession(_)) = actions.get(1) {
                        ApprovalResult::ApprovedForSession
                    } else {
                        self.prompt_denial_feedback().await
                    }
                }
                Ok(InteractionResponse::Selected(1)) => self.prompt_denial_feedback().await,
                Ok(InteractionResponse::Selected(2)) => self.prompt_denial_feedback().await,
                Ok(InteractionResponse::Custom(reason)) => denied(reason),
                Ok(InteractionResponse::Selected(_) | InteractionResponse::Cancelled) | Err(_) => denied(String::new()),
            };
        }

        let header = self.theme.highlight;
        let dim = self.theme.dimmed;
        let risk = self.theme.tool_err;
        let command = clean_command_paths(&request.command);
        let reasons = request.reasons.join("\n");
        match request.tier {
            RiskTier::HighRisk => {
                println!(
                    "\n{header}Approve bash (high risk):{header:#} {dim}{command}{dim:#}\n{risk}{reasons}{risk:#}\n"
                )
            }
            _ => println!("\n{header}Approve bash:{header:#} {dim}{command}{dim:#}\n"),
        }
        let choice = inquire::Select::new("Action:", actions)
            .with_starting_cursor(starting_cursor)
            .prompt();
        println!();
        match choice {
            Ok(BashApprovalChoice::AllowOnce) => ApprovalResult::Approved,
            Ok(BashApprovalChoice::AllowForSession(_)) => ApprovalResult::ApprovedForSession,
            Ok(BashApprovalChoice::Deny) => self.prompt_denial_feedback().await,
            Err(_) => denied(String::new()),
        }
    }

    async fn prompt_denial_feedback(&self) -> ApprovalResult {
        if let Some(ui) = &self.ui {
            let response = ui
                .request(InteractionPrompt {
                    title: "Denial Reason".to_string(),
                    body: "Provide optional feedback explaining why this operation was denied:".to_string(),
                    options: Vec::new(),
                    initial_selection: 0,
                    allow_custom: true,
                })
                .await;
            return match response {
                Ok(InteractionResponse::Custom(reason)) => denied(reason),
                _ => denied(String::new()),
            };
        }

        let reason = inquire::Text::new("Reason for denial (optional):").prompt();
        println!();
        denied(reason.unwrap_or_default())
    }
}
