//! Declarative agent workflows: XML step trees that the engine executes deterministically.

pub mod bundle;
pub mod parse;
pub mod template;
pub mod types;
pub mod xml;

pub use bundle::{WorkflowBundle, WorkflowCatalog, WorkflowOrigin};
pub use parse::{parse_workflow, split_command};
pub use types::{
    AgentNode, CallNode, ChooseNode, EachNode, InputSpec, Items, LoopNode, ModelRole, Node, NodeKind, OutputFormat,
    RunNode, Workflow, WriteNode,
};
