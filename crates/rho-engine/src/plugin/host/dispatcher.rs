use super::guards::HeadlessGuard;
use super::prompt::{build_confirm_prompt, build_input_prompt, build_select_prompt};
use super::types::{
    HostUiConfirmParams, HostUiConfirmResult, HostUiInputParams, HostUiInputResult, HostUiNotifyParams,
    HostUiSelectParams, HostUiSelectResult,
};
use crate::plugin::protocol::{JsonRpcRequest, JsonRpcResponse};
use rho_harness_core::presentation::presenter::Presenter;
use rho_harness_core::presentation::{InteractionPrompt, InteractionResponse};
use serde_json::json;
use std::sync::Arc;

pub struct HostDispatcher {
    presenter: Arc<dyn Presenter>,
}

impl HostDispatcher {
    pub fn new(presenter: Arc<dyn Presenter>) -> Self {
        Self { presenter }
    }

    pub async fn dispatch(&self, req: JsonRpcRequest) -> JsonRpcResponse {
        match req.method.as_str() {
            "host/ui/confirm" => self.handle_confirm(req).await,
            "host/ui/select" => self.handle_select(req).await,
            "host/ui/input" => self.handle_input(req).await,
            "host/ui/notify" => self.handle_notify(req),
            "ui/prompt" => self.handle_legacy_prompt(req).await,
            _ => JsonRpcResponse::err(req.id, -32601, format!("Method not found: {}", req.method)),
        }
    }

    async fn handle_confirm(&self, req: JsonRpcRequest) -> JsonRpcResponse {
        let Ok(params) = serde_json::from_value::<HostUiConfirmParams>(req.params) else {
            return JsonRpcResponse::err(req.id, -32602, "Invalid params for host/ui/confirm");
        };
        if HeadlessGuard::is_headless(self.presenter.as_ref()) {
            return JsonRpcResponse::ok(req.id, json!(HeadlessGuard::fail_closed_confirm()));
        }
        let prompt = build_confirm_prompt(params);
        let response = self.presenter.request_interaction(prompt).await;
        let confirmed = matches!(response, Some(InteractionResponse::Selected(0)));
        JsonRpcResponse::ok(req.id, json!(HostUiConfirmResult { confirmed }))
    }

    async fn handle_select(&self, req: JsonRpcRequest) -> JsonRpcResponse {
        let Ok(params) = serde_json::from_value::<HostUiSelectParams>(req.params) else {
            return JsonRpcResponse::err(req.id, -32602, "Invalid params for host/ui/select");
        };
        if HeadlessGuard::is_headless(self.presenter.as_ref()) {
            return JsonRpcResponse::ok(req.id, json!(HeadlessGuard::fail_closed_select()));
        }
        let prompt = build_select_prompt(params);
        let result = match self.presenter.request_interaction(prompt).await {
            Some(InteractionResponse::Selected(idx)) => HostUiSelectResult {
                selected: Some(idx),
                custom: None,
                cancelled: false,
            },
            Some(InteractionResponse::Custom(text)) => HostUiSelectResult {
                selected: None,
                custom: Some(text),
                cancelled: false,
            },
            Some(InteractionResponse::Cancelled) | None => HostUiSelectResult {
                selected: None,
                custom: None,
                cancelled: true,
            },
        };
        JsonRpcResponse::ok(req.id, json!(result))
    }

    async fn handle_input(&self, req: JsonRpcRequest) -> JsonRpcResponse {
        let Ok(params) = serde_json::from_value::<HostUiInputParams>(req.params) else {
            return JsonRpcResponse::err(req.id, -32602, "Invalid params for host/ui/input");
        };
        if HeadlessGuard::is_headless(self.presenter.as_ref()) {
            return JsonRpcResponse::ok(req.id, json!(HeadlessGuard::fail_closed_input()));
        }
        let prompt = build_input_prompt(params);
        let result = match self.presenter.request_interaction(prompt).await {
            Some(InteractionResponse::Custom(text)) => HostUiInputResult {
                value: Some(text),
                cancelled: false,
            },
            _ => HostUiInputResult {
                value: None,
                cancelled: true,
            },
        };
        JsonRpcResponse::ok(req.id, json!(result))
    }

    fn handle_notify(&self, req: JsonRpcRequest) -> JsonRpcResponse {
        let Ok(params) = serde_json::from_value::<HostUiNotifyParams>(req.params) else {
            return JsonRpcResponse::err(req.id, -32602, "Invalid params for host/ui/notify");
        };
        self.presenter.print_notice(&params.message);
        JsonRpcResponse::ok(req.id, json!({ "success": true }))
    }

    async fn handle_legacy_prompt(&self, req: JsonRpcRequest) -> JsonRpcResponse {
        let prompt = match serde_json::from_value::<InteractionPrompt>(req.params) {
            Ok(p) => p,
            Err(_) => return JsonRpcResponse::err(req.id, -32602, "Invalid params for ui/prompt"),
        };
        let response = self.presenter.request_interaction(prompt).await;
        JsonRpcResponse::ok(req.id, json!(response))
    }
}
