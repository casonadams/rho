use super::dispatcher::HostDispatcher;
use super::types::*;
use crate::plugin::protocol::JsonRpcRequest;
use async_trait::async_trait;
use rho_harness_core::presentation::activity::ActivityToken;
use rho_harness_core::presentation::presenter::Presenter;
use rho_harness_core::presentation::stream::ToolStreamPort;
use rho_harness_core::presentation::{InteractionPrompt, InteractionResponse, SessionStatus, ToolLine, WelcomeDisplay};
use serde_json::json;
use std::sync::{Arc, Mutex};

struct MockTestPresenter {
    has_ui: bool,
    interactive_reply: Mutex<Option<InteractionResponse>>,
    notices: Mutex<Vec<String>>,
}

impl MockTestPresenter {
    fn new(has_ui: bool, reply: Option<InteractionResponse>) -> Self {
        Self {
            has_ui,
            interactive_reply: Mutex::new(reply),
            notices: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl Presenter for MockTestPresenter {
    fn write_output(&self, _text: &str) {}
    fn print_welcome(&self, _display: &WelcomeDisplay) {}
    fn print_session_status(&self, _display: &SessionStatus) {}
    fn print_notice(&self, text: &str) {
        if let Ok(mut list) = self.notices.lock() {
            list.push(text.to_string());
        }
    }
    fn print_user_block(&self, _input: &str) {}
    fn print_token(&self, _token: &str) {}
    fn print_thinking_token(&self, _token: &str) {}
    fn finish_tool_line(&self, _line: ToolLine) {}
    fn flush(&self) {}
    fn has_interactive_ui(&self) -> bool {
        self.has_ui
    }
    fn start_spinner(&self, _message: &str) -> ActivityToken {
        ActivityToken::default()
    }
    fn start_tool_spinner(&self, _name: &str, _arguments: &serde_json::Value) -> ActivityToken {
        ActivityToken::default()
    }
    fn start_tool_run(&self, _name: &str, _arguments: &serde_json::Value) {}
    fn stream_port(&self) -> ToolStreamPort {
        ToolStreamPort::default()
    }
    async fn request_interaction(&self, _prompt: InteractionPrompt) -> Option<InteractionResponse> {
        self.interactive_reply.lock().ok().and_then(|r| r.clone())
    }
}

#[tokio::test]
async fn host_ui_confirm_approved_when_yes_selected() {
    let presenter = Arc::new(MockTestPresenter::new(true, Some(InteractionResponse::Selected(0))));
    let dispatcher = HostDispatcher::new(presenter);

    let req = JsonRpcRequest::new(
        1,
        "host/ui/confirm",
        json!(HostUiConfirmParams {
            title: "Permission".into(),
            message: "Allow bash?".into(),
            default_yes: true,
        }),
    );

    let res = dispatcher.dispatch(req).await;
    assert!(res.error.is_none());
    assert_eq!(res.result, Some(json!(HostUiConfirmResult { confirmed: true })));
}

#[tokio::test]
async fn host_ui_confirm_denied_when_no_selected() {
    let presenter = Arc::new(MockTestPresenter::new(true, Some(InteractionResponse::Selected(1))));
    let dispatcher = HostDispatcher::new(presenter);

    let req = JsonRpcRequest::new(
        2,
        "host/ui/confirm",
        json!(HostUiConfirmParams {
            title: "Permission".into(),
            message: "Allow bash?".into(),
            default_yes: true,
        }),
    );

    let res = dispatcher.dispatch(req).await;
    assert_eq!(res.result, Some(json!(HostUiConfirmResult { confirmed: false })));
}

#[tokio::test]
async fn host_ui_confirm_fail_closed_in_headless_mode() {
    let presenter = Arc::new(MockTestPresenter::new(
        false, // headless
        Some(InteractionResponse::Selected(0)),
    ));
    let dispatcher = HostDispatcher::new(presenter);

    let req = JsonRpcRequest::new(
        3,
        "host/ui/confirm",
        json!(HostUiConfirmParams {
            title: "Permission".into(),
            message: "Allow bash?".into(),
            default_yes: true,
        }),
    );

    let res = dispatcher.dispatch(req).await;
    assert_eq!(res.result, Some(json!(HostUiConfirmResult { confirmed: false })));
}

#[tokio::test]
async fn host_ui_select_handles_selection_and_custom() {
    let presenter = Arc::new(MockTestPresenter::new(true, Some(InteractionResponse::Selected(1))));
    let dispatcher = HostDispatcher::new(presenter);

    let req = JsonRpcRequest::new(
        4,
        "host/ui/select",
        json!(HostUiSelectParams {
            title: "Pick option".into(),
            message: "Choose".into(),
            options: vec![
                HostSelectOption {
                    label: "A".into(),
                    description: None
                },
                HostSelectOption {
                    label: "B".into(),
                    description: None
                },
            ],
            initial_selection: 0,
            allow_custom: false,
        }),
    );

    let res = dispatcher.dispatch(req).await;
    assert_eq!(
        res.result,
        Some(json!(HostUiSelectResult {
            selected: Some(1),
            custom: None,
            cancelled: false,
        }))
    );
}

#[tokio::test]
async fn host_ui_notify_and_unknown_method_error() {
    let presenter = Arc::new(MockTestPresenter::new(true, None));
    let dispatcher = HostDispatcher::new(presenter.clone());

    let req = JsonRpcRequest::new(
        5,
        "host/ui/notify",
        json!(HostUiNotifyParams {
            message: "Quota warning".into(),
            level: "warning".into(),
        }),
    );

    let res = dispatcher.dispatch(req).await;
    assert_eq!(res.result, Some(json!({"success": true})));
    assert_eq!(presenter.notices.lock().unwrap().as_slice(), &["Quota warning"]);

    let req_unknown = JsonRpcRequest::new(6, "host/invalid/method", json!({}));
    let res_unknown = dispatcher.dispatch(req_unknown).await;
    assert!(res_unknown.error.is_some());
    assert_eq!(res_unknown.error.unwrap().code, -32601);
}
