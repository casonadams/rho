//! Plugin platform facade: surfaces `rho-host` platform modules, `rho-sdk` contracts,
//! and `rho-plugin-builtin` components.

pub use rho_host::{
    activation, builtin, external, inspection, loader, permission, process, resolver, safety_floor, tool_dispatch,
};
pub use rho_plugin_builtin::mcp;
pub use rho_plugin_builtin::tools::builtin_tools;
pub use rho_sdk::{capability, contract, protocol, schema, ui};
