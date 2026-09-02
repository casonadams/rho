use super::types::{HostUiConfirmParams, HostUiInputParams, HostUiSelectParams};
use rho_harness_core::presentation::{InteractionOption, InteractionPrompt};

pub fn build_confirm_prompt(params: HostUiConfirmParams) -> InteractionPrompt {
    InteractionPrompt {
        title: params.title,
        body: params.message,
        options: vec![
            InteractionOption {
                label: "Yes".to_string(),
                description: None,
            },
            InteractionOption {
                label: "No".to_string(),
                description: None,
            },
        ],
        initial_selection: if params.default_yes { 0 } else { 1 },
        allow_custom: false,
    }
}

pub fn build_select_prompt(params: HostUiSelectParams) -> InteractionPrompt {
    let options = params
        .options
        .into_iter()
        .map(|opt| InteractionOption {
            label: opt.label,
            description: opt.description,
        })
        .collect();

    InteractionPrompt {
        title: params.title,
        body: params.message,
        options,
        initial_selection: params.initial_selection,
        allow_custom: params.allow_custom,
    }
}

pub fn build_input_prompt(params: HostUiInputParams) -> InteractionPrompt {
    InteractionPrompt {
        title: params.title,
        body: params.message,
        options: vec![InteractionOption {
            label: "Submit text input".to_string(),
            description: None,
        }],
        initial_selection: 0,
        allow_custom: true,
    }
}
