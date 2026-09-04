use crate::engine::AgentEngine;
use crate::error::Result;
use crate::ui::TerminalRenderer;

pub async fn run_agent_turn(
    engine: &AgentEngine,
    renderer: &TerminalRenderer,
    request: crate::engine::runner::TurnRequest<'_>,
) -> Result<()> {
    let run_future = engine.run_turn(request, std::sync::Arc::new(renderer.clone()));
    tokio::select! {
        run_res = run_future => {
            renderer.flush();
            renderer.write_output("\n");
            if let Err(error) = run_res {
                renderer.write_output(&format!("\nError: {error}\n"));
            }
        }
        _ = tokio::signal::ctrl_c() => {
            rho_engine::process::kill_all_tracked_processes();
            renderer.flush();
            engine.record_cancellation("operator interrupt").await?;
            renderer.write_output("\nCanceled.\n");
        }
    }
    Ok(())
}
