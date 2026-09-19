use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};

use crate::auth::AuthStore;
use crate::config::Config;
use crate::error::Result;
use crate::repl::coordinator::SharedSteeringQueue;
use crate::ui::render::rpc_presenter::PendingApprovals;
use rho_engine::engine::runner::TurnOutput;
use rho_harness_core::rpc::protocol::{RpcEvent, RpcRequest};
use rho_harness_core::rpc::transport::JsonLinesWriter;

pub(crate) enum RpcLoopAction {
    Continue,
    Break,
}

pub(crate) enum NextRpcItem {
    Event(Option<RpcEvent>),
    Request(rho_harness_core::error::Result<Option<RpcRequest>>),
    TurnDone(Box<std::result::Result<Result<TurnOutput>, tokio::task::JoinError>>),
}

pub(crate) struct RpcDaemonContext<'a, W> {
    pub(crate) writer: &'a mut JsonLinesWriter<W>,
    pub(crate) engine: Arc<RwLock<rho_engine::engine::AgentEngine>>,
    pub(crate) presenter: Arc<dyn rho_harness_core::presentation::Presenter>,
    pub(crate) config: Arc<RwLock<Config>>,
    pub(crate) auth_store: Arc<RwLock<AuthStore>>,
    pub(crate) pending_approvals: PendingApprovals,
    pub(crate) steering: Arc<SharedSteeringQueue>,
    pub(crate) active_turn: &'a mut Option<tokio::task::JoinHandle<Result<TurnOutput>>>,
    pub(crate) event_tx: mpsc::UnboundedSender<RpcEvent>,
    pub(crate) auth_bridge: rho_harness_core::rpc::RpcAuthBridge,
}
