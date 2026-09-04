use super::super::batch::LiveBatch;
use super::super::modal::ModalKeyResult;
use super::super::navigation::update_footer;
use crate::engine::AgentEngine;
use crate::error::Result;
use crate::repl::ReplSession;
use crate::repl::interactive::InteractiveHistory;
use crate::ui::interactive::{TerminalBackend, TerminalController};

pub(super) struct ModalActionContext<'a, 'b, 'c, B: TerminalBackend> {
    pub controller: &'a mut TerminalController<B>,
    pub history: &'a mut InteractiveHistory,
    pub session: &'b mut ReplSession,
    pub engine: &'c mut AgentEngine,
}

pub(super) async fn apply_modal_key_result<B: TerminalBackend>(
    res: ModalKeyResult,
    ctx: ModalActionContext<'_, '_, '_, B>,
    batch: &mut LiveBatch,
) -> Result<bool> {
    match res {
        ModalKeyResult::Handled => Ok(true),
        ModalKeyResult::ModelSelected {
            model,
            provider,
            save_as_default,
        } => {
            ctx.session.config.model = model.clone();
            ctx.session.config.provider = provider.clone();
            let _ = rho_harness_core::state::AppState::set_last_model(
                &ctx.session.config.config_dir,
                &model,
                Some(&provider),
            );
            if save_as_default {
                let _ =
                    rho_harness_core::config::Config::set_file_value(&ctx.session.config.config_dir, "model", &model);
                let _ = rho_harness_core::config::Config::set_file_value(
                    &ctx.session.config.config_dir,
                    "provider",
                    &provider,
                );
                ctx.session
                    .renderer
                    .print_status(&format!("Default model: {model} ({provider})"));
            } else {
                ctx.session
                    .renderer
                    .print_status(&format!("Model: {model} ({provider})"));
            }
            if let Ok(rebuilt) = ctx
                .engine
                .rebuild(ctx.session.config.clone(), ctx.session.auth_store.clone())
                .await
            {
                *ctx.engine = rebuilt;
            }
            update_footer(ctx.controller.state_mut(), ctx.session, ctx.engine);
            batch.flush(ctx.controller, true)?;
            Ok(true)
        }
        ModalKeyResult::TreeNodeSelected { node_id } => {
            match ctx.engine.session_manager.switch_branch(Some(node_id.clone())).await {
                Ok(_) => {
                    if let Ok(tree) = ctx.engine.session_manager.load_tree().await {
                        let _ =
                            super::super::navigation::hydrate_session_transcript(ctx.controller, &tree, ctx.history);
                    }
                    ctx.session
                        .renderer
                        .print_status(&format!("Navigated to checkpoint {node_id}"));
                }
                Err(err) => {
                    ctx.session.renderer.print_status(&format!("Failed to navigate: {err}"));
                }
            }
            ctx.controller.redraw()?;
            Ok(true)
        }
        ModalKeyResult::NodeLabelUpdated { node_id, label } => {
            let label_opt = if label.is_empty() { None } else { Some(label.clone()) };
            match ctx.engine.session_manager.set_node_label(&node_id, label_opt).await {
                Ok(_) => {
                    ctx.session
                        .renderer
                        .print_status(&format!("Checkpoint labeled: \"{label}\" ({node_id})"));
                }
                Err(err) => {
                    ctx.session
                        .renderer
                        .print_status(&format!("Failed to label checkpoint: {err}"));
                }
            }
            ctx.controller.redraw()?;
            Ok(true)
        }
        ModalKeyResult::SessionSelected { session_id } => {
            *ctx.engine = crate::platform::agent_engine(
                ctx.session.config.clone(),
                ctx.session.auth_store.clone(),
                Some(&session_id),
            )
            .await?;
            if let Ok(tree) = ctx.engine.session_manager.load_tree().await {
                let _ = super::super::navigation::hydrate_session_transcript(ctx.controller, &tree, ctx.history);
            }
            ctx.session
                .renderer
                .print_status(&format!("Resumed session {session_id}"));
            update_footer(ctx.controller.state_mut(), ctx.session, ctx.engine);
            ctx.controller.redraw()?;
            Ok(true)
        }
        ModalKeyResult::SessionDeleted { session_id } => {
            let _ = rho_harness_core::session::delete_session(&ctx.session.config.sessions_dir, &session_id);
            ctx.session
                .renderer
                .print_status(&format!("Deleted session {session_id}"));
            ctx.controller.redraw()?;
            Ok(true)
        }
        ModalKeyResult::NotHandled => Ok(false),
    }
}
