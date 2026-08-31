---
title: Rho Plugin Architecture & Execution Flow
---
flowchart TB
    subgraph Config["Configuration & Discovery"]
        Cfg["config.toml & .rho/config.toml<br/>declares [plugins.<name>] + config"]
        Loader["PluginLoader (loader.rs)<br/>validates & resolves executable paths"]
    end

    subgraph SDK["rho-sdk — Public Contracts & Server"]
        CapId["CapabilityId (kind:name)<br/>tool · context · command · lifecycle · provider · permission · skill"]
        Traits["Capability Traits<br/>ToolCapability · ContextCapability · CommandCapability ·<br/>LifecycleCapability · ProviderCapability · PermissionCapability"]
        Builder["PluginBuilder & Plugin (builder.rs)<br/>registers trait implementations + manifest"]
        ServerLoop["PluginServer / run() (runtime.rs)<br/>stdio JSONL protocol v1 message loop"]
    end

    subgraph External["External Plugin Subprocesses"]
        ExtExe["Plugin Binary (e.g. rho-plugin-docs)<br/>implements Capability Traits"]
        ExtRunner["rho_sdk::server::run(plugin)<br/>handles Handshake, Discovery, Invocations"]
    end

    subgraph Builtins["In-Process Built-ins"]
        BuiltinTools["rho-plugin-builtin<br/>(bash, read, write, edit, websearch, webfetch, ask_user, todo, mcp)"]
        BuiltinProviders["rho-plugin-providers<br/>(anthropic, openai, chatgpt, copilot, gemini, deepseek, ollama, ...)"]
    end

    subgraph Host["Host Platform (rho-host)"]
        Client["PluginProcessClient (client.rs)<br/>supervises subprocess over stdio JSONL"]
        Resolver["CapabilityResolver (resolver.rs)<br/>merges built-in + external capabilities & replaces"]
        ActiveSet["ActiveToolSet (active_set.rs)<br/>Active Map per CapabilityId"]
        Safety["Host Safety Floor (safety_floor.rs)<br/>schema validation · workspace containment · approvals"]
    end

    subgraph Engine["Execution Runtime (rho-engine)"]
        Turn["Turn Orchestrator (turn.rs)<br/>prompt augmentation · tool dispatch · lifecycle events"]
        REPL["REPL Slash Commands (commands.rs)<br/>routes /<command> to CommandCapability"]
    end

    %% Flow connections
    Cfg --> Loader
    Loader --> Client
    Client <== "stdio JSONL (Protocol v1)" ==> ExtRunner

    ExtExe --> Builder
    Builder --> ExtRunner
    Traits --> Builder
    CapId --> Builder

    BuiltinTools --> Resolver
    BuiltinProviders --> Resolver
    Client --> Resolver
    Cfg --> Resolver
    Resolver --> ActiveSet

    ActiveSet --> Safety
    Safety --> Turn
    Turn --> REPL

    ActiveSet -.->|"Context Snippets"| Turn
    ActiveSet -.->|"Tool Execution"| Turn
    ActiveSet -.->|"Lifecycle Events"| Turn
    ActiveSet -.->|"Slash Commands"| REPL

    style Host fill:#e6f3ff,stroke:#333
    style SDK fill:#fff4e6,stroke:#333
    style Builtins fill:#e8f5e9,stroke:#333
    style External fill:#fce4ec,stroke:#333
    style Config fill:#f3e5f5,stroke:#333
    style Engine fill:#e0f2f1,stroke:#333
