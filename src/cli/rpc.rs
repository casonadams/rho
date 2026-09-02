use crate::auth::AuthStore;
use crate::config::Config;
use crate::error::Result;
use crate::ui::render::RpcPresenter;
use rho_harness_core::rpc::protocol::{RpcCommand, RpcEvent, RpcRequest, RpcResponse};
use rho_harness_core::rpc::transport::{JsonLinesReader, JsonLinesWriter};
use std::sync::Arc;
use tokio::io::BufReader;
use tokio::sync::mpsc;

pub async fn run_rpc_daemon(config: Config, auth_store: AuthStore) -> Result<()> {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let mut reader = JsonLinesReader::new(BufReader::new(stdin));
    let mut writer = JsonLinesWriter::new(stdout);

    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<RpcEvent>();
    let (presenter, _approval_tx) = RpcPresenter::new(event_tx.clone());
    let presenter: Arc<dyn rho_harness_core::presentation::Presenter> = Arc::new(presenter);

    let mut engine = crate::platform::agent_engine(config.clone(), auth_store.clone(), None).await?;

    let session_id = engine.session_manager.session_id.clone();
    let init_event = RpcEvent::SessionStart {
        session_id: session_id.clone(),
        model: config.model.clone(),
        provider: config.provider.clone(),
    };
    writer.write_message(&init_event).await?;

    loop {
        tokio::select! {
            Some(event) = event_rx.recv() => {
                writer.write_message(&event).await?;
            }
            res = reader.read_message::<RpcRequest>() => {
                let req = match res {
                    Ok(Some(req)) => req,
                    Ok(None) => break,
                    Err(e) => {
                        let err_res = RpcResponse::failure(None, "parse", &e.to_string());
                        writer.write_message(&err_res).await?;
                        continue;
                    }
                };

                let req_id = req.id.clone();
                match req.command {
                    RpcCommand::Prompt { message, .. } => {
                        let ok_res = RpcResponse::success(req_id, "prompt", None);
                        writer.write_message(&ok_res).await?;

                        let req = crate::engine::runner::TurnRequest::new(&message);
                        let _ = engine.run_turn(req, presenter.clone()).await;
                    }
                    RpcCommand::Steer { message } => {
                        let ok_res = RpcResponse::success(req_id, "steer", None);
                        writer.write_message(&ok_res).await?;

                        let req = crate::engine::runner::TurnRequest::new(&message);
                        let _ = engine.run_turn(req, presenter.clone()).await;
                    }
                    RpcCommand::Abort => {
                        let _ = engine.record_cancellation("rpc abort").await;
                        let ok_res = RpcResponse::success(req_id, "abort", None);
                        writer.write_message(&ok_res).await?;
                    }
                    RpcCommand::ToolResponse { approval_id: _, decision: _ } => {
                        let ok_res = RpcResponse::success(req_id, "tool_response", None);
                        writer.write_message(&ok_res).await?;
                    }
                    RpcCommand::Compact { .. } => {
                        let memory = crate::session::context::context_memory(
                            engine.session_manager.clone(),
                            1,
                            config.compaction_max_bytes,
                        );
                        let _ = memory.load(&session_id).await;
                        let ok_res = RpcResponse::success(req_id, "compact", None);
                        writer.write_message(&ok_res).await?;
                    }
                    RpcCommand::SetModel { model, provider } => {
                        let mut new_config = config.clone();
                        new_config.model = model;
                        if let Some(p) = provider {
                            new_config.provider = p;
                        }
                        match engine.rebuild(new_config, auth_store.clone()).await {
                            Ok(rebuilt) => {
                                engine = rebuilt;
                                let ok_res = RpcResponse::success(req_id, "set_model", None);
                                writer.write_message(&ok_res).await?;
                            }
                            Err(e) => {
                                let err_res = RpcResponse::failure(req_id, "set_model", &e.to_string());
                                writer.write_message(&err_res).await?;
                            }
                        }
                    }
                    RpcCommand::GetState => {
                        let data = serde_json::json!({
                            "session_id": engine.session_manager.session_id,
                            "model": config.model,
                            "provider": config.provider,
                            "auto_approve": config.auto_approve,
                        });
                        let ok_res = RpcResponse::success(req_id, "get_state", Some(data));
                        writer.write_message(&ok_res).await?;
                    }
                    RpcCommand::Exit => {
                        let ok_res = RpcResponse::success(req_id, "exit", None);
                        writer.write_message(&ok_res).await?;
                        break;
                    }
                }
            }
        }
    }

    Ok(())
}
