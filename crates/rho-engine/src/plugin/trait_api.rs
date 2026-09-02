use rho_harness_core::presentation::DisplayTransformer;
use rig::agent::hook::HookStack;
use rig::tool::DynamicTool;
use std::sync::Arc;

pub trait RhoPlugin: Send + Sync {
    fn name(&self) -> &str;

    fn tools(&self) -> Vec<DynamicTool> {
        Vec::new()
    }

    fn register_hooks(&self, _stack: &mut HookStack) {}

    fn display_transformers(&self) -> Vec<Arc<dyn DisplayTransformer>> {
        Vec::new()
    }
}
