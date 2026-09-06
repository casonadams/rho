use super::command::LiveCommandContext;
use crate::error::{AppError, Result};
use crate::repl::commands::CommandResult;
use crate::repl::live::LiveIo;
use crate::ui::interactive::TerminalBackend;

fn handle_auth_result(ctx: &mut LiveCommandContext<'_, '_>, res: std::result::Result<(), AppError>, verb: &str) {
    match res {
        Ok(()) => {}
        Err(AppError::Cancelled(_)) => {}
        Err(err) => ctx.session.renderer.print_notice(&format!("  {verb} failed: {err}\n")),
    }
}

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
            handle_auth_result(ctx, login_res, "Login");
            rebuild_after_auth(ctx).await?;
            Ok(true)
        }
        CommandResult::Logout { provider } => {
            let logout_res = io.suspend_for(|| {
                crate::cli::logout_provider(provider.as_deref(), &ctx.session.config, &mut ctx.session.auth_store)
            })?;
            handle_auth_result(ctx, logout_res, "Logout");
            rebuild_after_auth(ctx).await?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

async fn rebuild_after_auth(ctx: &mut LiveCommandContext<'_, '_>) -> Result<()> {
    *ctx.engine = ctx
        .engine
        .rebuild(ctx.session.config.clone(), ctx.session.auth_store.clone())
        .await?;
    Ok(())
}
