pub mod parser;
pub mod resolver;
pub mod types;

#[cfg(test)]
mod tests;

pub use parser::{normalize_tool_name, parse_agent_file, parse_tool_list};
pub use resolver::{resolved_agents, resolved_agents_for_paths, resolved_agents_for_paths_async};
pub use types::{AgentMetadata, AgentOrigin, AgentResolutionPaths, ResolvedAgent};
