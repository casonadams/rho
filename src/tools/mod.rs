//! Tools facade re-exporting from `rho-plugin-builtin`, `rho-core`, and `rho-engine`.

pub use rho_core::approval::{
    ApprovalCapability, ApprovalDecision, ApprovalEventSink, ApprovalHook, ApprovalRequest, DispatchedCall,
    DispatchedResult, ToolEvent, approval_context, authorize_dispatch, enforce_approval,
};
pub use rho_core::bash_ast::{RiskTier, SafetyAnalysis, analyze_command_safety};
pub use rho_core::policy::{ExecutionClass, ToolExecutionPolicy, is_known};
pub use rho_core::presentation::questions::{
    InteractiveQuestionPort, QuestionPort, UserAnswer, UserQuestion, UserQuestionOption,
};
pub use rho_core::workspace::Workspace;
pub use rho_engine::repeat::RepeatedCallHook;
pub use rho_plugin_builtin::tools::{
    AskUserArgs, AskUserQuestionTool, AskUserTool, BashArgs, BashTool, BuiltinToolCatalog, BuiltinToolDeclaration,
    BuiltinToolKind, DECLARATIONS, EditArgs, EditTool, FetchArgs, FetchCache, HttpClient, HttpRequest, PROMPT_TODO,
    ReadArgs, ReadTool, SearchArgs, SearchRateLimiter, TaskStatus, TodoAction, TodoArgs, TodoStore, TodoTask, TodoTool,
    ToolRegistry, ToolResult, WebFetchConfig, WebFetchTool, WebSearchConfig, WebSearchTool, WriteArgs, WriteTool,
    ask_user, bash, builtin_tools, edit, generated_schema, into_rig_result, normalize_schema, read, registry, todo,
    types, web, write,
};
