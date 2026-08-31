---
title: Rho Plugin System
---
flowchart TB
    subgraph Host["Host (rho-host / rho-core)"]
        Resolver["CapabilityResolver<br/>(resolver.rs)<br/>merges built-in + external into an active set"]
        ActiveSet["ActiveCapability set<br/>one winner per CapabilityId"]
        Config["config.toml<br/>declares + authorizes external plugins"]
    end

    subgraph SDK["rho-sdk — the contract"]
        CapId["CapabilityId = kind:name<br/>(tool, provider, permission, command,<br/>lifecycle, skill, ui, context)"]
        Traits["Capability traits<br/>ProviderCapability · ToolCapability ·<br/>PermissionCapability · CommandCapability ·<br/>LifecycleCapability · SkillCapability ·<br/>ContextCapability"]
        Plugin["Plugin / PluginBuilder<br/>manifest + BTreeMap<CapabilityId, Arc<dyn Trait>>"]
    end

    subgraph Builtin["rho-plugin-builtin / rho-plugin-providers"]
        BuiltIn["BuiltIn plugins<br/>compiled into the host binary"]
        Tools["builtin tools (bash, read, write, ...)"]
        Providers["provider plugins (openai, mcp, ...)"]
    end

    subgraph External["External plugins (subprocess)"]
        Exe["Executable<br/>PluginOrigin::Configured"]
        Serve["SDK server loop<br/>(runtime.rs)<br/>stdin/stdout line-delimited JSON"]
    end

    Config --> Resolver
    BuiltIn --> Resolver
    Exe --> Resolver
    Resolver --> ActiveSet
    ActiveSet --> Traits
    ActiveSet --> CapId

    Traits --> Plugin
    Plugin --> BuiltIn
    Tools --> BuiltIn
    Providers --> BuiltIn
    Exe --> Serve

    style Host fill:#e6f3ff,stroke:#333
    style SDK fill:#fff4e6,stroke:#333
    style Builtin fill:#e8f5e9,stroke:#333
    style External fill:#fce4ec,stroke:#333
