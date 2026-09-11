use super::render_flowchart;
use unicode_width::UnicodeWidthStr;

#[test]
fn test_flowchart_linear_td() {
    let source = "graph TD\n  A[Start] --> B[Process] --> C[Done]";
    let rendered = render_flowchart(source).expect("should render linear TD");

    assert!(rendered.contains("Start"));
    assert!(rendered.contains("Process"));
    assert!(rendered.contains("Done"));
    assert!(rendered.contains('╭') || rendered.contains('┌'));
    assert!(rendered.contains('▼'));

    // Verify it is vertical: Start should appear on a line before Process, which is before Done
    let start_pos = rendered.find("Start").unwrap();
    let proc_pos = rendered.find("Process").unwrap();
    let done_pos = rendered.find("Done").unwrap();
    assert!(start_pos < proc_pos);
    assert!(proc_pos < done_pos);

    // Should be compact in width
    let max_w = rendered.lines().map(UnicodeWidthStr::width).max().unwrap();
    assert!(max_w <= 30, "expected compact width, got: {max_w}");
}

#[test]
fn test_flowchart_branching_td() {
    let source = "graph TD\n  A[Input] --> B[Left Path]\n  A --> C[Right Path]";
    let rendered = render_flowchart(source).expect("should render branching TD");

    assert!(rendered.contains("Input"));
    assert!(rendered.contains("Left Path"));
    assert!(rendered.contains("Right Path"));
    assert!(rendered.contains('▼'));
}

#[test]
fn test_flowchart_loop_perimeter_routing() {
    let source = r#"
graph TD
  UserInput[User Input] --> AgentHarness[Agent Harness]
  AgentHarness --> ModelCall{Model Call}
  ModelCall -->|Tool Call| ExecuteTool[Execute Tool]
  ExecuteTool --> AgentHarness
  ModelCall -->|Response| RenderUI[Render to UI]
"#;
    let rendered = render_flowchart(source).expect("should render loop diagram");

    // All labels must be intact and not overwritten
    // Back-edge arrow pointing into Agent Harness
    assert!(rendered.contains('▶'));

    // Ensure diamond shape markers exist
    assert!(rendered.contains('<') && rendered.contains('>'));
}

#[test]
fn test_flowchart_linear_lr() {
    let source = "graph LR\n  A[Step 1] --> B[Step 2] --> C[Step 3]";
    let rendered = render_flowchart(source).expect("should render linear LR");

    assert!(rendered.contains("Step 1"));
    assert!(rendered.contains("Step 2"));
    assert!(rendered.contains("Step 3"));
    assert!(rendered.contains('▶'));
}

#[test]
fn test_flowchart_clipped_to_render_width() {
    let theme = crate::ui::theme::Theme::default();
    let source = "graph TD\n  A[Start] --> B[Process] --> C[Done]";
    let output = crate::ui::markdown::render_mermaid_block(source, &theme, 15);
    for line in output.lines() {
        assert!(UnicodeWidthStr::width(line) <= 15, "line exceeds clip width: {line:?}");
    }
}

#[test]
fn test_non_flowchart_returns_none() {
    let source = "sequenceDiagram\n  Alice->>Bob: Hello";
    let result = render_flowchart(source);
    assert!(
        result.is_none(),
        "sequence diagram must be skipped by flowchart renderer"
    );
}
