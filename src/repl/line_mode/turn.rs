use crate::engine::AgentEngine;
use crate::error::Result;
use crate::ui::TerminalRenderer;

async fn handle_turn_cancellation(engine: &AgentEngine, renderer: &TerminalRenderer) -> Result<()> {
    rho_engine::process::kill_all_tracked_processes();
    renderer.flush();
    engine.record_cancellation("operator interrupt").await?;
    renderer.write_output("\nCanceled.\n");
    Ok(())
}

fn handle_turn_completion(res: Result<crate::engine::runner::TurnOutput>, renderer: &TerminalRenderer) {
    renderer.flush();
    renderer.write_output("\n");
    if let Err(error) = res {
        renderer.write_output(&format!("\nError: {error}\n"));
    }
}

enum TurnSignal {
    Done(Box<Result<crate::engine::runner::TurnOutput>>),
    Interrupted,
}

async fn wait_turn_or_interrupt(
    future: impl std::future::Future<Output = Result<crate::engine::runner::TurnOutput>>,
) -> TurnSignal {
    tokio::pin!(future);
    tokio::select! {
        res = &mut future => TurnSignal::Done(Box::new(res)),
        _ = tokio::signal::ctrl_c() => TurnSignal::Interrupted,
    }
}

pub async fn run_agent_turn(
    engine: &AgentEngine,
    renderer: &TerminalRenderer,
    request: crate::engine::runner::TurnRequest<'_>,
) -> Result<()> {
    match wait_turn_or_interrupt(engine.run_turn(request, std::sync::Arc::new(renderer.clone()))).await {
        TurnSignal::Done(res) => {
            handle_turn_completion(*res, renderer);
            Ok(())
        }
        TurnSignal::Interrupted => handle_turn_cancellation(engine, renderer).await,
    }
}
