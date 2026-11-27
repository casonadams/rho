use crate::engine::compactor::llm::ModelHandle;
use crate::engine::tracking::ContextPolicy;
use crate::locking::{read, write};
use std::sync::{Arc, RwLock};

/// Resolves a configured `<provider>/<model>` name to a handle; the error text is shown to the caller.
pub type SubagentModelResolver = Arc<dyn Fn(&str) -> Result<ModelHandle, String> + Send + Sync>;

/// The session model and resolver subagents run on, and how their windows compact, shared by every
/// copy of the tool so a model, thinking or context change reaches all of them.
#[derive(Clone, Default)]
pub struct SubagentModels(Arc<RwLock<ModelSource>>);

#[derive(Clone, Default)]
pub(super) struct ModelSource {
    pub(super) default: Option<ModelHandle>,
    pub(super) resolver: Option<SubagentModelResolver>,
    /// Without one, a subagent compacts only after its model rejects a request as too long.
    pub(super) context: Option<ContextPolicy>,
}

impl SubagentModels {
    pub(super) fn new(session: Option<ModelHandle>) -> Self {
        Self(Arc::new(RwLock::new(ModelSource {
            default: session.map(short_lived),
            resolver: None,
            context: None,
        })))
    }

    /// Moves subagents to the session model (short-lived where it has a cache lifetime), `resolver`
    /// and `context`.
    pub(crate) fn replace(
        &self,
        session: Option<ModelHandle>,
        resolver: SubagentModelResolver,
        context: ContextPolicy,
    ) {
        *write(&self.0) = ModelSource {
            default: session.map(short_lived),
            resolver: Some(resolver),
            context: Some(context),
        };
    }

    pub(crate) fn set_context_policy(&self, context: ContextPolicy) {
        write(&self.0).context = Some(context);
    }

    pub(super) fn set_resolver(&self, resolver: SubagentModelResolver) {
        write(&self.0).resolver = Some(resolver);
    }

    pub(super) fn current(&self) -> ModelSource {
        read(&self.0).clone()
    }
}

fn short_lived(model: ModelHandle) -> ModelHandle {
    model.for_short_lived_work().unwrap_or(model)
}
