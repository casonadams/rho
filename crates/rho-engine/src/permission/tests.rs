mod bash;
mod eval;
mod hook;
mod matcher;
pub(crate) mod mock;
mod path;
mod policy;
mod prompt;

pub(crate) static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
