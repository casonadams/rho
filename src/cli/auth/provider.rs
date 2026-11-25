use crate::auth::AuthStore;
use crate::config::Config;
use crate::error::Result;
use rho_harness_core::provider::{Connection, ProviderId};
use std::io::IsTerminal;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMethod {
    ApiKey,
    OAuth,
}

/// A provider offered by both `rho login` and `/login`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginChoice {
    pub name: String,
    pub description: String,
    pub signed_in: bool,
}

/// The provider's description and login mode, plus each signed-in account's email.
fn builtin_description(auth_store: &AuthStore, provider: ProviderId) -> String {
    let desc = format!("{} ({})", provider.info().description, provider.auth_mode_label());
    let logins = auth_store.logins(provider);
    let emails: Vec<&str> = logins.iter().filter_map(|login| login.email.as_deref()).collect();
    if emails.is_empty() {
        desc
    } else {
        format!("{desc} · {}", emails.join(", "))
    }
}

/// Every builtin provider that takes a login, then each custom provider from the config.
pub fn login_choices(config: &Config, auth_store: &AuthStore) -> Vec<LoginChoice> {
    let configured = auth_store.list_configured_providers();
    let signed_in = |name: &str| configured.iter().any(|p| Connection::parse(p).provider == name);
    let builtin = ProviderId::ALL
        .into_iter()
        .filter(|&id| id != ProviderId::Local)
        .map(|id| LoginChoice {
            name: id.as_str().to_string(),
            description: builtin_description(auth_store, id),
            signed_in: signed_in(id.as_str()),
        });
    let custom = config
        .providers
        .keys()
        .filter(|name| ProviderId::from_str(name).is_err() && name.as_str() != "exa-search")
        .map(|name| LoginChoice {
            name: name.clone(),
            description: "Configured provider (API key)".to_string(),
            signed_in: signed_in(name),
        });
    let exa_signed_in = signed_in("exa-search") || std::env::var("EXA_API_KEY").is_ok_and(|v| !v.trim().is_empty());
    let search_tools = [LoginChoice {
        name: "exa-search".to_string(),
        description: "Exa Web Search (API key)".to_string(),
        signed_in: exa_signed_in,
    }];
    builtin.chain(custom).chain(search_tools).collect()
}

fn login_choice_item(choice: &LoginChoice) -> String {
    let mark = if choice.signed_in { "  ✓" } else { "" };
    format!("{:<14} {}{mark}", choice.name, choice.description)
}

pub(crate) fn parse_selection_choice(input: &str, count: usize) -> Result<usize> {
    let idx = input
        .trim()
        .parse::<usize>()
        .map_err(|_| crate::error::AppError::Cancelled("Invalid selection".to_string()))?;
    if idx >= 1 && idx <= count {
        Ok(idx - 1)
    } else {
        Err(crate::error::AppError::Cancelled("Selection out of range".to_string()))
    }
}

pub(crate) fn prompt_select_from<R: std::io::BufRead, W: std::io::Write>(
    reader: &mut R,
    writer: &mut W,
    prompt: &str,
    items: &[String],
) -> Result<usize> {
    writeln!(writer, "\n{prompt}").map_err(|e| crate::error::AppError::Other(e.into()))?;
    for (idx, item) in items.iter().enumerate() {
        writeln!(writer, "  {}. {item}", idx + 1).map_err(|e| crate::error::AppError::Other(e.into()))?;
    }
    write!(writer, "Enter choice [1-{}]: ", items.len()).map_err(|e| crate::error::AppError::Other(e.into()))?;
    writer.flush().map_err(|e| crate::error::AppError::Other(e.into()))?;
    let mut input = String::new();
    reader
        .read_line(&mut input)
        .map_err(|e| crate::error::AppError::Other(e.into()))?;
    parse_selection_choice(&input, items.len())
}

fn prompt_select_from_list(prompt: &str, items: &[String]) -> Result<usize> {
    if !std::io::stdin().is_terminal() || cfg!(test) {
        return Err(crate::error::AppError::Cancelled(
            "Interactive selection unavailable".to_string(),
        ));
    }
    let mut stdin = std::io::stdin().lock();
    let mut stdout = std::io::stdout();
    prompt_select_from(&mut stdin, &mut stdout, prompt, items)
}

pub(crate) fn select_login_provider(
    choices: &[LoginChoice],
    select: impl FnOnce(&str, &[String]) -> Result<usize>,
) -> Result<String> {
    let items: Vec<String> = choices.iter().map(login_choice_item).collect();
    let idx = select("Select provider to log in:", &items)?;
    Ok(choices[idx].name.clone())
}

pub fn prompt_select_login_provider(config: &Config, auth_store: &AuthStore) -> Result<String> {
    select_login_provider(&login_choices(config, auth_store), prompt_select_from_list)
}

pub(crate) fn prompt_auth_choice(
    select: impl FnOnce(&str, &[String]) -> Result<usize>,
    prompt: &str,
    options: [&str; 2],
    methods: [AuthMethod; 2],
) -> Result<AuthMethod> {
    let items: Vec<String> = options.iter().map(|option| option.to_string()).collect();
    let idx = select(prompt, &items)?;
    Ok(methods[idx])
}

pub fn prompt_auth_method(provider_label: &str) -> Result<AuthMethod> {
    prompt_auth_choice(
        prompt_select_from_list,
        &format!("Select authentication method for {provider_label}:"),
        [
            "OAuth (sign in with browser to generate API key)",
            "API key (enter manually)",
        ],
        [AuthMethod::OAuth, AuthMethod::ApiKey],
    )
}

pub fn resolve_provider_name(requested: Option<&str>, configured: &str) -> String {
    Connection::parse(requested.unwrap_or(configured)).to_string()
}
