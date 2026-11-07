use super::modal_action::setup_modal_action_harness;
use crate::repl::commands::CommandResult;
use crate::repl::live::message::handle_session_diagnostics;
use crate::repl::live::message::types::{LiveCommandContext, SessionCommandIo};

#[tokio::test]
async fn session_diagnostics_open_name_and_rewind_commands() {
    let temp = tempfile::tempdir().unwrap();
    let (mut controller, mut history, _batch, mut session, mut engine, mut input) =
        setup_modal_action_harness(temp.path()).await;
    let mut ctx = LiveCommandContext {
        session: &mut session,
        engine: &mut engine,
    };
    let mut io = SessionCommandIo {
        controller: &mut controller,
        history: &mut history,
        input: &mut input,
    };

    assert!(
        handle_session_diagnostics(&mut ctx, &mut io, &CommandResult::OpenSessionSelector)
            .await
            .unwrap()
    );
    assert!(io.controller.state().active_modal().is_some());
    io.controller.state_mut().pop_modal();

    assert!(
        handle_session_diagnostics(
            &mut ctx,
            &mut io,
            &CommandResult::NameSession {
                name: "daily".to_string(),
            },
        )
        .await
        .unwrap()
    );
    assert!(
        handle_session_diagnostics(&mut ctx, &mut io, &CommandResult::Rewind { turn: 0 })
            .await
            .unwrap()
    );
    assert!(
        !handle_session_diagnostics(&mut ctx, &mut io, &CommandResult::Continue)
            .await
            .unwrap()
    );
}
