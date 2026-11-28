use super::modal_action::setup_modal_action_harness;
use crate::repl::commands::CommandResult;
use crate::repl::live::message::handle_session_diagnostics;
use crate::repl::live::message::types::{LiveCommandContext, SessionCommandIo};

#[tokio::test]
async fn rewind_reports_the_turn_and_retained_count_in_live_mode() {
    let temp = rho_test_support::TestDir::new();
    let (mut controller, _batch, mut session, mut engine, _input) = setup_modal_action_harness(&temp).await;
    let (renderer, mut events) = crate::repl::collecting_renderer();
    session.renderer = renderer;
    let mut ctx = LiveCommandContext {
        session: &mut session,
        engine: &mut engine,
    };
    let mut io = SessionCommandIo {
        controller: &mut controller,
    };

    let handled = handle_session_diagnostics(&mut ctx, &mut io, &CommandResult::Rewind { turn: 999 }).await;

    assert!(handled.unwrap());
    assert!(crate::repl::collected_output(&mut events).contains("Rewound context to Turn 999 (0 messages retained)"));
}

#[tokio::test]
async fn name_session_reports_the_quoted_name_in_live_mode() {
    let temp = rho_test_support::TestDir::new();
    let (mut controller, _batch, mut session, mut engine, _input) = setup_modal_action_harness(&temp).await;
    let (renderer, mut events) = crate::repl::collecting_renderer();
    session.renderer = renderer;
    let mut ctx = LiveCommandContext {
        session: &mut session,
        engine: &mut engine,
    };
    let mut io = SessionCommandIo {
        controller: &mut controller,
    };

    let handled = handle_session_diagnostics(
        &mut ctx,
        &mut io,
        &CommandResult::NameSession {
            name: "daily".to_string(),
        },
    )
    .await;

    assert!(handled.unwrap());
    assert!(crate::repl::collected_output(&mut events).contains("Session name: \"daily\""));
}

#[tokio::test]
async fn session_diagnostics_open_name_and_rewind_commands() {
    let temp = rho_test_support::TestDir::new();
    let (mut controller, _batch, mut session, mut engine, _input) = setup_modal_action_harness(&temp).await;
    let mut ctx = LiveCommandContext {
        session: &mut session,
        engine: &mut engine,
    };
    let mut io = SessionCommandIo {
        controller: &mut controller,
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
