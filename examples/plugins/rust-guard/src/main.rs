use async_trait::async_trait;
use rho_plugin_sdk::{Flow, HostContext, Plugin, StepEvent, serve};

struct RustGuard;

#[async_trait]
impl Plugin for RustGuard {
    fn name(&self) -> &str {
        "rust-guard"
    }

    fn subscriptions(&self) -> Vec<String> {
        vec!["tool_call".into(), "invalid_tool_call".into()]
    }

    async fn on_event(&self, event: StepEvent, ctx: &HostContext) -> Flow {
        match event {
            StepEvent::ToolCall { tool_name, args } => {
                if tool_name == "bash" && args.get("command").unwrap_or(&serde_json::Value::Null).as_str().unwrap_or("").contains("sudo") {
                    let allowed = ctx.confirm("Security Alert", "Allow sudo execution?").await;
                    if !allowed {
                        return Flow::skip("Permission denied by user. Do not retry.");
                    }
                }
                Flow::cont()
            }
            StepEvent::InvalidToolCall { tool_name, .. } => {
                if tool_name == "sh" || tool_name == "shell" {
                    return Flow::repair("bash");
                }
                Flow::cont()
            }
            _ => Flow::cont(),
        }
    }
}

#[tokio::main]
async fn main() {
    serve(RustGuard).await;
}
