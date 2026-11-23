//! Interactive CLI login, logout, and terminal OAuth callback handlers.

mod callbacks;
mod provider;
mod terminal;

#[cfg(test)]
mod tests;

pub use callbacks::TerminalOAuthCallbacks;
pub use provider::{
    AuthMethod, login_choices, prompt_auth_method, prompt_select_login_provider, resolve_provider_name,
};
pub use terminal::{open_url_in_browser_async, prompt_password, read_key_from_reader, read_key_from_stdin};

use crate::auth::AuthStore;
use crate::config::Config;
use crate::error::{AppError, Result};
use rho_engine::auth::perform_oauth_login;
use rho_harness_core::provider::{Connection, ProviderId};

async fn perform_oauth_and_save(id: ProviderId, config: &Config, auth_store: &mut AuthStore) -> Result<()> {
    let callbacks = TerminalOAuthCallbacks;
    let cred = perform_oauth_login(id, &callbacks).await?;
    let login = auth_store.login_key(id, &cred);
    auth_store.set_credential(&login.to_string(), cred)?;
    println!(
        "Logged in to {} as {login}. Credentials saved to {}",
        id.info().display_name,
        config.auth_file.display()
    );
    if login.account.is_some() {
        println!("Saved as an additional account; select its models as {login}/<model>.");
    }
    Ok(())
}

fn resolve_login_target(provider: Option<&str>, config: &Config, auth_store: &AuthStore) -> Result<String> {
    match provider {
        Some(name) => Ok(resolve_provider_name(Some(name), &config.provider)),
        None => prompt_select_login_provider(config, auth_store),
    }
}

fn is_default_oauth_provider(id: ProviderId) -> bool {
    matches!(
        id,
        ProviderId::ChatGpt | ProviderId::Antigravity | ProviderId::ClaudeCode
    )
}

fn should_default_oauth(id: ProviderId) -> Result<bool> {
    if is_default_oauth_provider(id) {
        Ok(true)
    } else if id == ProviderId::OpenRouter {
        Ok(prompt_auth_method("OpenRouter")? == AuthMethod::OAuth)
    } else {
        Ok(false)
    }
}

async fn try_oauth_login(id: ProviderId, config: &Config, auth_store: &mut AuthStore) -> Result<bool> {
    if should_default_oauth(id)? {
        perform_oauth_and_save(id, config, auth_store).await?;
        Ok(true)
    } else {
        Ok(false)
    }
}

fn read_api_key(target: &str, key_stdin: bool) -> Result<String> {
    use std::io::IsTerminal;
    if key_stdin || !std::io::stdin().is_terminal() {
        read_key_from_stdin()
    } else {
        prompt_password(&format!("Enter API key for {target}:"))
    }
}

fn store_api_key(target: &str, key: &str, auth_store: &mut AuthStore) -> Result<()> {
    let key = key.trim();
    if key.is_empty() {
        return Err(AppError::Auth("API key cannot be empty".to_string()));
    }
    auth_store.set_key(target, key)?;
    println!("Stored API key for {target}");
    Ok(())
}

fn login_api_key(target: &str, key_stdin: bool, auth_store: &mut AuthStore) -> Result<()> {
    let key = read_api_key(target, key_stdin)?;
    store_api_key(target, &key, auth_store)
}

pub async fn login_provider(
    provider: Option<&str>,
    key_stdin: bool,
    config: &Config,
    auth_store: &mut AuthStore,
) -> Result<()> {
    if let Some(server) = provider.and_then(|p| p.strip_prefix("mcp:")) {
        return crate::cli::mcp::login_mcp_server(server, config).await;
    }
    if key_stdin && provider.is_none() {
        return Err(AppError::Auth(
            "Provider name required when using --key-stdin (e.g. `rho login gemini --key-stdin`)".to_string(),
        ));
    }

    let target = resolve_login_target(provider, config, auth_store)?;
    if target == "local" {
        println!("Local models run offline and do not require credentials.");
        return Ok(());
    }
    if !key_stdin
        && let Some(id) = Connection::parse(&target).provider_id()
        && try_oauth_login(id, config, auth_store).await?
    {
        return Ok(());
    }
    login_api_key(&target, key_stdin, auth_store)
}

fn parse_logout_choice(input: &str, configured: &[String]) -> Result<String> {
    let idx = input
        .trim()
        .parse::<usize>()
        .map_err(|_| AppError::Cancelled("Logout cancelled".to_string()))?;
    if (1..=configured.len()).contains(&idx) {
        Ok(configured[idx - 1].clone())
    } else {
        Err(AppError::Cancelled("Logout cancelled".to_string()))
    }
}

fn prompt_logout_selection<R: std::io::BufRead, W: std::io::Write>(
    reader: &mut R,
    writer: &mut W,
    configured: &[String],
) -> Result<String> {
    writeln!(writer, "\nSelect provider credentials to remove:").map_err(|e| AppError::Other(e.into()))?;
    for (i, p) in configured.iter().enumerate() {
        writeln!(writer, "  {}. {p}", i + 1).map_err(|e| AppError::Other(e.into()))?;
    }
    write!(writer, "Enter choice [1-{}]: ", configured.len()).map_err(|e| AppError::Other(e.into()))?;
    writer.flush().map_err(|e| AppError::Other(e.into()))?;
    let mut input = String::new();
    reader.read_line(&mut input).map_err(|e| AppError::Other(e.into()))?;
    parse_logout_choice(&input, configured)
}

fn select_logout_target(auth_store: &AuthStore) -> Result<Option<String>> {
    let configured = auth_store.list_stored_providers();
    if configured.is_empty() {
        println!("No stored credentials to remove.");
        return Ok(None);
    }
    let mut stdin = std::io::stdin().lock();
    let mut stdout = std::io::stdout();
    let target = prompt_logout_selection(&mut stdin, &mut stdout, &configured)?;
    Ok(Some(target))
}

pub fn logout_provider(provider: Option<&str>, config: &Config, auth_store: &mut AuthStore) -> Result<()> {
    if let Some(server) = provider.and_then(|p| p.strip_prefix("mcp:")) {
        return crate::cli::mcp::logout_mcp_server(server, config);
    }
    let target = match provider {
        Some(name) => resolve_provider_name(Some(name), &config.provider),
        None => match select_logout_target(auth_store)? {
            Some(target) => target,
            None => return Ok(()),
        },
    };

    auth_store.remove_key(&target)?;
    println!("Removed stored credentials for {target}");
    Ok(())
}
