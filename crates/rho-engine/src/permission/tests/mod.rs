mod baseline;
mod bash;
mod eval;
mod external;
mod hook;
mod matcher;
mod mock;
mod path;
mod policy;
mod prompt;

pub(super) static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
