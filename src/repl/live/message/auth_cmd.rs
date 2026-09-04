use super::command::LiveCommandContext;
use crate::error::Result;
use crate::repl::commands::CommandResult;
use crate::repl::live::LiveIo;
use crate::ui::interactive::TerminalBackend;

pub(super) async fn handle_auth_command<B: TerminalBackend>(
    ctx: &mut LiveCommandContext<'_, '_>,
    io: &mut LiveIo<'_, B>,
    result: &CommandResult,
) -> Result<bool> {
    match result {
        CommandResult::Login { provider } => {
            let login_res = io
                .suspend_for_async(|| {
                    crate::cli::login_provider(provider.as_deref(), &ctx.session.config, &mut ctx.session.auth_store)
                })
                .await?;
            match login_res {
                Ok(()) => {
                    *ctx.engine = ctx
                        .engine
                        .rebuild(ctx.session.config.clone(), ctx.session.auth_store.clone())
                        .await?;
                }
                Err(crate::error::AppError::Cancelled(_)) => {}
                Err(err) => ctx.session.renderer.print_notice(&format!("  Login failed: {err}\n")),
            }
            Ok(true)
        }
        CommandResult::Logout { provider } => {
            let logout_res = io.suspend_for(|| {
                crate::cli::logout_provider(provider.as_deref(), &ctx.session.config, &mut ctx.session.auth_store)
            })?;
            match logout_res {
                Ok(()) => {
                    *ctx.engine = ctx
                        .engine
                        .rebuild(ctx.session.config.clone(), ctx.session.auth_store.clone())
                        .await?;
                }
                Err(crate::error::AppError::Cancelled(_)) => {}
                Err(err) => ctx.session.renderer.print_notice(&format!("  Logout failed: {err}\n")),
            }
            Ok(true)
        }
        _ => Ok(false),
    }
}
