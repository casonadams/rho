use super::super::InteractiveState;

#[test]
fn tools_expanded_toggle_and_set() {
    let mut state = InteractiveState::default();
    assert!(!state.tools_expanded());

    assert!(state.toggle_tools_expanded());
    assert!(state.tools_expanded());

    assert!(!state.toggle_tools_expanded());
    assert!(!state.tools_expanded());

    state.set_tools_expanded(true);
    assert!(state.tools_expanded());
}

#[test]
fn thinking_toggle_state() {
    let mut state = InteractiveState::default();
    assert!(!state.hide_thinking());
    assert!(state.toggle_thinking());
    assert!(state.hide_thinking());
    assert!(!state.toggle_thinking());
    assert!(!state.hide_thinking());
}

#[test]
fn active_tool_lifecycle_and_chunk_accumulation() {
    let mut state = InteractiveState::default();
    assert!(state.active_tool().is_none());

    let mut tool = super::super::RunningTool::new("bash", "cargo test", None);
    tool.append_chunk("compiling...\n");
    tool.append_chunk("running 5 tests\n");
    assert_eq!(tool.name, "bash");
    assert_eq!(tool.args_summary, "cargo test");
    assert_eq!(tool.output, "compiling...\nrunning 5 tests\n");

    state.set_active_tool(Some(tool));
    assert!(state.active_tool().is_some());
    assert_eq!(state.active_tool().unwrap().output, "compiling...\nrunning 5 tests\n");

    state.active_tool_mut().unwrap().append_chunk("test result: ok\n");
    assert_eq!(
        state.active_tool().unwrap().output,
        "compiling...\nrunning 5 tests\ntest result: ok\n"
    );

    state.set_active_tool(None);
    assert!(state.active_tool().is_none());
}

#[test]
fn running_tool_rolling_tail_truncation_under_massive_chunks() {
    let mut tool = super::super::RunningTool::new("bash", "seq 1 10000", None);
    for i in 1..=5000 {
        tool.append_chunk(&format!("line {i:04}: detailed execution log output\n"));
    }

    assert!(
        tool.output.len() <= super::super::MAX_RUNNING_BUFFER_BYTES,
        "tool output length ({}) must not exceed MAX_RUNNING_BUFFER_BYTES ({})",
        tool.output.len(),
        super::super::MAX_RUNNING_BUFFER_BYTES
    );
    assert!(
        tool.output.len() <= super::super::MAX_RUNNING_OUTPUT_BYTES + 200,
        "tool output length ({}) should be trimmed close to MAX_RUNNING_OUTPUT_BYTES ({})",
        tool.output.len(),
        super::super::MAX_RUNNING_OUTPUT_BYTES
    );
    assert!(tool.output.ends_with("line 5000: detailed execution log output\n"));
    assert!(!tool.output.contains("line 0001:"));
    assert!(
        tool.output.starts_with("line "),
        "trimmed output should start cleanly on a newline boundary without half-line fragments"
    );
}

#[test]
fn running_tool_preserves_utf8_char_and_newline_boundaries() {
    let mut tool = super::super::RunningTool::new("bash", "unicode stream", None);
    let line = "🦀 🚀 🔥 ✨ 这是一个很长的多字节测试行 🌟 💫\n";
    for _ in 0..2500 {
        tool.append_chunk(line);
    }

    assert!(tool.output.len() <= super::super::MAX_RUNNING_BUFFER_BYTES);
    assert!(
        tool.output.starts_with("🦀 "),
        "trimmed output must start at the beginning of a line on a valid character boundary"
    );
    assert!(tool.output.ends_with("🌟 💫\n"));
}

#[test]
fn running_tool_single_massive_chunk_bounded() {
    let mut tool = super::super::RunningTool::new("bash", "massive chunk", None);
    let massive_chunk = "alpha beta gamma delta epsilon\n".repeat(8000); // ~248 KB
    tool.append_chunk(&massive_chunk);

    assert!(tool.output.len() <= super::super::MAX_RUNNING_BUFFER_BYTES);
    assert!(tool.output.starts_with("alpha "));
    assert!(tool.output.ends_with("epsilon\n"));
}

#[test]
fn running_tool_massive_line_without_newlines_bounded_safely() {
    let mut tool = super::super::RunningTool::new("bash", "one line", None);
    let massive_line = "🔥abc🚀def".repeat(15000); // ~150 KB with multi-byte chars
    tool.append_chunk(&massive_line);

    assert!(tool.output.len() <= super::super::MAX_RUNNING_OUTPUT_BYTES);
    assert!(tool.output.is_char_boundary(0));
}
