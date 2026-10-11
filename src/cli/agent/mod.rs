use crate::auth::AuthStore;
use crate::config::Config;
use crate::config::cli::AgentCommands;
use rho_engine::agent_mesh::{AgentClientEndpoint, AgentWorkerConfig, AgentWorkerServer};
use rho_harness_core::agent_mesh::{AgentCapability, AgentTaskEvent, AgentTaskRequest, AgentTicket, RelayUrl};
use std::io::Write;
use std::net::SocketAddr;

pub async fn handle_agent(
    action: AgentCommands,
    config: &Config,
    _auth_store: &AuthStore,
) -> Result<(), Box<dyn std::error::Error>> {
    match action {
        AgentCommands::Serve { cap, relay, bind } => {
            handle_agent_serve(&cap, relay.as_deref(), bind.as_deref(), config).await
        }
        AgentCommands::Run {
            ticket,
            prompt,
            context,
            role,
            max_turns,
        } => handle_agent_run(&ticket, &prompt, context.as_deref(), role.as_deref(), max_turns).await,
    }
}

pub(crate) fn parse_worker_config(
    cap_str: &str,
    relay_str: Option<&str>,
    bind_str: Option<&str>,
    config: &Config,
) -> Result<AgentWorkerConfig, Box<dyn std::error::Error>> {
    let capability: AgentCapability = cap_str.parse().map_err(|e| format!("Invalid capability: {e}"))?;
    let relay_url = match relay_str {
        Some(r) => Some(r.parse::<RelayUrl>()?),
        None => None,
    };
    let bind_addr = match bind_str {
        Some(b) => Some(b.parse::<SocketAddr>()?),
        None => None,
    };

    let cwd = std::env::current_dir()?;
    let tools = rho_engine::tools::build_builtin_tools(&cwd, config)?;

    Ok(AgentWorkerConfig {
        capability,
        relay_url,
        bind_addr,
        model: None,
        tools,
    })
}

fn print_worker_ready(ticket: &AgentTicket) {
    println!("Sovereign Agent Worker active.");
    println!("Capability: {}", ticket.capability);
    println!("Ticket: {ticket}");
    println!("Listening for remote agent tasks. Press Ctrl+C to stop.");
}

async fn handle_agent_serve(
    cap_str: &str,
    relay_str: Option<&str>,
    bind_str: Option<&str>,
    config: &Config,
) -> Result<(), Box<dyn std::error::Error>> {
    let worker_config = parse_worker_config(cap_str, relay_str, bind_str, config)?;
    let server = AgentWorkerServer::start(worker_config).await?;
    print_worker_ready(server.ticket());
    let _ = tokio::signal::ctrl_c().await;
    server.close().await;
    Ok(())
}

fn print_task_event(event: &AgentTaskEvent) {
    match event {
        AgentTaskEvent::Token { text } => {
            print!("{text}");
            let _ = std::io::stdout().flush();
        }
        AgentTaskEvent::ToolCall { name, .. } => {
            println!("\n[agent tool call: {name}]");
        }
        AgentTaskEvent::ToolResult { name, is_error, .. } => {
            if *is_error {
                println!("[agent tool error: {name}]");
            } else {
                println!("[agent tool complete: {name}]");
            }
        }
        AgentTaskEvent::Complete { .. } => {}
        AgentTaskEvent::Error { message } => {
            eprintln!("\nRemote agent error: {message}");
        }
    }
}

async fn handle_agent_run(
    ticket_str: &str,
    prompt: &str,
    context: Option<&str>,
    role: Option<&str>,
    max_turns: Option<usize>,
) -> Result<(), Box<dyn std::error::Error>> {
    let ticket = AgentTicket::parse(ticket_str)?;
    let client = AgentClientEndpoint::bind(None).await?;

    let req = AgentTaskRequest {
        task_id: uuid::Uuid::new_v4().to_string(),
        prompt: prompt.to_string(),
        context_slice: context.map(ToString::to_string),
        max_turns,
        role: role.map(ToString::to_string),
    };

    println!("Delegating task to remote agent (capability: {})...", ticket.capability);

    let output = client
        .run_task(&ticket, req, |ev| {
            print_task_event(&ev);
        })
        .await?;

    println!("\n--- Remote Agent Output ---\n{output}");
    client.close().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_worker_config() {
        let config = Config::default();
        let parsed = parse_worker_config("propose", None, Some("127.0.0.1:8080"), &config).unwrap();
        assert_eq!(parsed.capability, AgentCapability::Propose);
        assert_eq!(parsed.bind_addr, Some("127.0.0.1:8080".parse().unwrap()));
        assert!(parsed.relay_url.is_none());
    }

    #[test]
    fn test_print_task_event_does_not_panic() {
        print_task_event(&AgentTaskEvent::Token { text: "hello".into() });
        print_task_event(&AgentTaskEvent::ToolCall {
            id: "1".into(),
            name: "read".into(),
            args: serde_json::json!({}),
        });
        print_task_event(&AgentTaskEvent::ToolResult {
            id: "1".into(),
            name: "read".into(),
            content: "data".into(),
            is_error: false,
        });
        print_task_event(&AgentTaskEvent::Complete {
            result: "done".into(),
            turns: 2,
        });
        print_task_event(&AgentTaskEvent::Error {
            message: "failure".into(),
        });
    }

    #[tokio::test]
    async fn test_handle_agent_invalid_ticket() {
        let res = handle_agent(
            AgentCommands::Run {
                ticket: "invalid-ticket".into(),
                prompt: "test".into(),
                context: None,
                role: None,
                max_turns: None,
            },
            &Config::default(),
            &AuthStore::default(),
        )
        .await;
        assert!(res.is_err());
    }
}
