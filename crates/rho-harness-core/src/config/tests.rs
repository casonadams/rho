mod merge;
mod parse;
mod storage;
mod validation;

fn sample_cli(model: &str, max_turns: Option<usize>) -> crate::config::cli::Cli {
    crate::config::cli::Cli {
        prompt: None,
        model: Some(model.to_string()),
        smol: false,
        slow: false,
        provider: None,
        max_output_tokens: None,
        max_turns,
        thinking: None,
        name: None,
        export: None,
        resume: None,
        r#continue: false,
        resume_picker: false,
        mode: "interactive".to_string(),
        message: Vec::new(),
        system_prompt: None,
        append_system_prompt: None,
        no_context_files: false,
        no_permission: false,
        command: None,
        hook_dir: Vec::new(),
    }
}
