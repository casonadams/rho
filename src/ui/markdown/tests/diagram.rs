use crate::ui::markdown::render_mermaid_block;
use crate::ui::theme::Theme;
use unicode_width::UnicodeWidthStr;

#[test]
fn test_flowchart_linear_td() {
    let theme = Theme::default();
    let source = "graph TD\n  A[Start] --> B[Process] --> C[Done]";
    let rendered = render_mermaid_block(source, &theme, 0);

    assert!(rendered.contains("Start"));
    assert!(rendered.contains("Process"));
    assert!(rendered.contains("Done"));
    assert!(rendered.contains('┌') || rendered.contains('╭'));
    assert!(rendered.contains('▼'));

    let start_pos = rendered.find("Start").unwrap();
    let proc_pos = rendered.find("Process").unwrap();
    let done_pos = rendered.find("Done").unwrap();
    assert!(start_pos < proc_pos, "Start must precede Process in TD layout");
    assert!(proc_pos < done_pos, "Process must precede Done in TD layout");
}

#[test]
fn test_flowchart_branching_td() {
    let theme = Theme::default();
    let source = "graph TD\n  A[Input] --> B[Left Path]\n  A --> C[Right Path]";
    let rendered = render_mermaid_block(source, &theme, 0);

    assert!(rendered.contains("Input"));
    assert!(rendered.contains("Left Path"));
    assert!(rendered.contains("Right Path"));
    assert!(rendered.contains('▼'));
}

#[test]
fn test_flowchart_loop_routing() {
    let theme = Theme::default();
    let source = r#"
graph TD
  UserInput[User Input] --> AgentHarness[Agent Harness]
  AgentHarness --> ModelCall{Model Call}
  ModelCall -->|Tool Call| ExecuteTool[Execute Tool]
  ExecuteTool --> AgentHarness
  ModelCall -->|Response| RenderUI[Render to UI]
"#;
    let rendered = render_mermaid_block(source, &theme, 0);

    assert!(rendered.contains("User Input"));
    assert!(rendered.contains("Agent Harness"));
    assert!(rendered.contains("Model Call"));
    assert!(rendered.contains("Execute Tool"));
    assert!(rendered.contains("Render to UI"));

    for line in rendered.lines() {
        assert!(UnicodeWidthStr::width(line) < 120, "line too wide in loop: {line:?}");
    }
}

#[test]
fn test_flowchart_subgraphs() {
    let theme = Theme::default();
    let source = r#"
flowchart TD
  subgraph Cluster
    W1[Worker 1] --> W2[Worker 2]
  end
  Client[Client App] --> Cluster
"#;
    let rendered = render_mermaid_block(source, &theme, 0);

    assert!(rendered.contains("Cluster"));
    assert!(rendered.contains("Worker 1"));
    assert!(rendered.contains("Worker 2"));
    assert!(rendered.contains("Client App"));
}

#[test]
fn test_sequence_diagram() {
    let theme = Theme::default();
    let source = r#"
sequenceDiagram
  Alice->>Bob: Hello Bob
  Bob-->>Alice: Hi Alice
"#;
    let rendered = render_mermaid_block(source, &theme, 0);

    assert!(rendered.contains("Alice"));
    assert!(rendered.contains("Bob"));
    assert!(rendered.contains("Hello Bob"));
    assert!(rendered.contains("Hi Alice"));
    assert!(rendered.contains('│'));
    assert!(rendered.contains('►') || rendered.contains('>'));
}

#[test]
fn test_state_diagram() {
    let theme = Theme::default();
    let source = r#"
stateDiagram-v2
  [*] --> Idle
  Idle --> Processing: Event
  Processing --> [*]
"#;
    let rendered = render_mermaid_block(source, &theme, 0);

    assert!(rendered.contains("Idle"));
    assert!(rendered.contains("Processing"));
    assert!(rendered.contains("Event"));
}

#[test]
fn test_class_diagram() {
    let theme = Theme::default();
    let source = r#"
classDiagram
  class BankAccount {
    +String owner
    +deposit()
  }
"#;
    let rendered = render_mermaid_block(source, &theme, 0);

    assert!(rendered.contains("BankAccount"));
    assert!(rendered.contains("owner"));
    assert!(rendered.contains("deposit"));
}

#[test]
fn test_viewport_clipping_bounds() {
    let theme = Theme::default();
    let source = "graph LR\n  A[Very Long Node Name One] --> B[Second Long Node Name] --> C[Third Long Node Name]";
    let rendered = render_mermaid_block(source, &theme, 40);

    for line in rendered.lines() {
        assert!(
            UnicodeWidthStr::width(line) <= 40,
            "line exceeded clip width of 40: {line:?}"
        );
    }
}

#[test]
fn test_invalid_syntax_fallback() {
    let theme = Theme::default();
    let source = "this is completely invalid mermaid syntax %%@#$%^";
    let rendered = render_mermaid_block(source, &theme, 0);

    assert!(rendered.contains("```mermaid"));
    assert!(rendered.contains("this is completely invalid"));
    assert!(rendered.contains("```"));
}
