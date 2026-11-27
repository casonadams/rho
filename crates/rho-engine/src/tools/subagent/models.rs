use crate::engine::compactor::llm::ModelHandle;
use crate::engine::tracking::ContextPolicy;
use std::sync::Arc;

/// Resolves a configured `<provider>/<model>` name to a handle; the error text is shown to the caller.
pub type SubagentModelResolver = Arc<dyn Fn(&str) -> Result<ModelHandle, String> + Send + Sync>;

/// The session model and resolver subagents run on, and how their windows compact, shared by every
/// copy of the tool so a model, thinking or context change reaches all of them.
#[derive(Clone, Default)]
pub struct SubagentModels(Arc<std::sync::RwLock<ModelSource>>);

#[derive(Clone, Default)]
pub(super) struct ModelSource {
    pub(super) default: Option<ModelHandle>,
    pub(super) resolver: Option<SubagentModelResolver>,
    /// Without one, a subagent compacts only after its model rejects a request as too long.
    pub(super) context: Option<ContextPolicy>,
}

impl SubagentModels {
    pub(super) fn new(session: Option<ModelHandle>) -> Self {
        Self(Arc::new(std::sync::RwLock::new(ModelSource {
            default: session.map(short_lived),
            resolver: None,
            context: None,
        })))
    }

    /// Moves subagents to the session model (short-lived where it has a cache lifetime), `resolver`
    /// and `context`.
    pub fn replace(&self, session: Option<ModelHandle>, resolver: SubagentModelResolver, context: ContextPolicy) {
        *self.write() = ModelSource {
            default: session.map(short_lived),
            resolver: Some(resolver),
            context: Some(context),
        };
    }

    pub fn set_context_policy(&self, context: ContextPolicy) {
        self.write().context = Some(context);
    }

    pub(super) fn set_resolver(&self, resolver: SubagentModelResolver) {
        self.write().resolver = Some(resolver);
    }

    pub(super) fn current(&self) -> ModelSource {
        self.0.read().unwrap_or_else(|poisoned| poisoned.into_inner()).clone()
    }

    fn write(&self) -> std::sync::RwLockWriteGuard<'_, ModelSource> {
        self.0.write().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

fn short_lived(model: ModelHandle) -> ModelHandle {
    model.for_short_lived_work().unwrap_or(model)
}
