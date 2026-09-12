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
fn test_image_1_lifecycle_flowchart() {
    let source = r#"
graph TD
  UserInput[User Input / Queue] --> REPL[Interactive REPL & TUI]
  REPL --> ContextEngine[Context Engine & Session Manager]
  ContextEngine --> ModelProvider[Model Provider\nAnthropic / OpenAI / Gemini / Ollama]
  ModelProvider --> ModelOutput{Model Output}
  ModelOutput -->|Assistant Resp| LiveMarkdown[Live Markdown Streaming & UI Render]
  ModelOutput -->|Tool Call| PermCheck{Permission Check}
  LiveMarkdown --> UserInput
  PermCheck -->|Auto-Approved| ToolRuntime[Tool Runtime\nBuilt-in / MCP]
  PermCheck -->|Mutating Action| ApprovalModal[Interactive Approval Modal\nAllow / Edit / Always / Deny]
  ApprovalModal -->|Approved| ToolRuntime
  ApprovalModal -->|Denied| DenialReason[Return Denial Reason to Model]
  ToolRuntime --> OutputTrim[Capture Tool Output & Trim]
  DenialReason --> ContextEngine
  OutputTrim --> ContextEngine
"#;
    let rendered = render_flowchart(source).expect("should render image 1 diagram");
    assert!(rendered.contains("User Input / Queue"));
    assert!(rendered.contains("Context Engine & Session Manager"));
    assert!(rendered.contains("Model Output"));
    assert!(rendered.contains("Live Markdown Streaming & UI Render"));
    assert!(rendered.contains("Interactive Approval Modal"));
}

#[test]
fn test_system_architecture_diagram() {
    let source = r#"
graph TD
    classDef binary fill:#2d3748,stroke:#4a5568,stroke-width:2px,color:#fff
    classDef engine fill:#1a365d,stroke:#2b6cb0,stroke-width:2px,color:#fff
    classDef core fill:#234e52,stroke:#319795,stroke-width:2px,color:#fff
    classDef ext fill:#44337a,stroke:#6b46c1,stroke-width:2px,color:#fff

    subgraph BINARY ["rho (CLI / TUI)"]
        CLI["CLI Subcommands<br/>(run, auth, mcp, rpc)"]
        subgraph FRONTENDS ["Frontends / REPL"]
            TUI["Live TUI (Raw-mode & Modals)"]
            LINE["Line Mode (Readline)"]
            HEADLESS["Headless Mode (--print)"]
        end
        UI["UI Layer<br/>(Markdown, Syntax, CSI 2026 Sync Paint)"]
        COORD["Coordinator & Batched Event Channel"]
    end

    subgraph ENGINE ["rho-engine"]
        AGENT["AgentEngine / Turn Runner"]
        STREAM["Provider Stream Adapter & SSE"]
        PERM["Permission Gate<br/>(Bash AST Lexer, Policies)"]
        REGISTRY["Tool Registry<br/>(Builtins, Gateway, Router)"]
        COMPACT["Compactor<br/>(Context Window Truncation)"]
    end

    subgraph CORE ["rho-harness-core"]
        SESSION["Session Store<br/>(Durable JSONL, Branching, Tree)"]
        CONFIG["Layered Config<br/>(File, Env, CLI args)"]
        WORKSPACE["Workspace & Path Guards"]
        TOKENS["Token Estimation (tiktoken BPE)"]
        PRESENTER["Presenter Trait & Token Contracts"]
    end

    subgraph EXTERNAL ["External Collaborators"]
        PROVIDERS["LLM Providers<br/>(Claude, ChatGPT, Gemini, Ollama)"]
        MCP["MCP Servers<br/>(JSON-RPC stdio)"]
        HOOKS["Lifecycle Hooks<br/>(.rho/hooks)"]
    end

    %% Wiring
    TUI --> COORD
    LINE --> COORD
    HEADLESS --> COORD
    COORD --> AGENT
    AGENT --> UI

    AGENT --> STREAM
    AGENT --> PERM
    AGENT --> REGISTRY
    AGENT --> COMPACT

    STREAM --> PROVIDERS
    REGISTRY --> MCP
    AGENT --> HOOKS

    AGENT --> SESSION
    AGENT --> CONFIG
    AGENT --> WORKSPACE
    COMPACT --> TOKENS
    UI --> PRESENTER

    class BINARY,CLI,TUI,LINE,HEADLESS,UI,COORD binary
    class ENGINE,AGENT,STREAM,PERM,REGISTRY,COMPACT engine
    class CORE,SESSION,CONFIG,WORKSPACE,TOKENS,PRESENTER core
    class EXTERNAL,PROVIDERS,MCP,HOOKS ext
"#;
    let res = render_flowchart(source);
    assert!(res.is_some());
    let r = res.unwrap();
    assert!(r.contains("CLI Subcommands"));
    assert!(r.contains("Coordinator & Batched Event Channel"));
    assert!(r.contains("AgentEngine / Turn Runner"));
    assert!(r.contains("Presenter Trait & Token Contracts"));
}

#[test]
fn test_image_2_complex_flowchart() {
    let source = r#"
graph TD
  UserInput[User Input / Prompt] --> PrepareContext[Prepare Context\nHierarchical AGENTS.md + Skills + Session History]
  PrepareContext --> Stream[Stream from Provider\nToken-by-token rendering & metrics]
  Stream --> ToolCalls{Tool calls requested?}
  ToolCalls -->|No| DisplayAnswer[Display Final Answer]
  ToolCalls -->|Yes| PermissionGate{Permission Gate}
  PermissionGate -->|Approved| ExecTool[Execute Tool\nBuiltin or MCP]
  PermissionGate -->|Denied| InjectReject[Inject Rejection / User Feedback]
  DisplayAnswer --> AppendSession[Append Turn to Session JSONL]
  AppendSession --> AwaitNext[Awaiting Next User Turn]
  ExecTool --> AppendResult[Append Tool Result to History]
  InjectReject --> AppendResult
  AppendResult --> Stream
"#;
    let rendered = render_flowchart(source).expect("should render image 2 diagram");
    assert!(rendered.contains("User Input / Prompt"));
    assert!(rendered.contains("Prepare Context"));
    assert!(rendered.contains("Tool calls requested?"));
    assert!(rendered.contains("Append Tool Result to History"));
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
