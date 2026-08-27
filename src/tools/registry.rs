use crate::tools::policy::ExecutionClass;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCapability {
    ReadOnly,
    WorkspaceMutation,
    Network,
    Interactive,
    Shell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolDescriptor {
    pub name: &'static str,
    pub capability: ToolCapability,
    pub description: &'static str,
}

const DESCRIPTORS: &[ToolDescriptor] = &[
    ToolDescriptor {
        name: "read",
        capability: ToolCapability::ReadOnly,
        description: "Read a file",
    },
    ToolDescriptor {
        name: "write",
        capability: ToolCapability::WorkspaceMutation,
        description: "Write a file",
    },
    ToolDescriptor {
        name: "edit",
        capability: ToolCapability::WorkspaceMutation,
        description: "Edit a file",
    },
    ToolDescriptor {
        name: "bash",
        capability: ToolCapability::Shell,
        description: "Run a shell command",
    },
    ToolDescriptor {
        name: "websearch",
        capability: ToolCapability::Network,
        description: "Search the web",
    },
    ToolDescriptor {
        name: "webfetch",
        capability: ToolCapability::Network,
        description: "Fetch a URL",
    },
    ToolDescriptor {
        name: "ask_user",
        capability: ToolCapability::Interactive,
        description: "Ask the user a question",
    },
    ToolDescriptor {
        name: "ask_user_question",
        capability: ToolCapability::Interactive,
        description: "Ask multiple questions",
    },
];

#[derive(Debug, Clone, Copy, Default)]
pub struct ToolRegistry;

impl ToolRegistry {
    pub fn descriptors() -> &'static [ToolDescriptor] {
        DESCRIPTORS
    }

    pub fn descriptor(name: &str) -> Option<&'static ToolDescriptor> {
        DESCRIPTORS.iter().find(|descriptor| descriptor.name == name)
    }

    pub fn is_known(name: &str) -> bool {
        Self::descriptor(name).is_some()
    }

    pub fn default_class(name: &str) -> Option<ExecutionClass> {
        Self::descriptor(name).map(|descriptor| match descriptor.capability {
            ToolCapability::ReadOnly | ToolCapability::Network | ToolCapability::Interactive => {
                ExecutionClass::ReadOnly
            }
            ToolCapability::WorkspaceMutation => ExecutionClass::WorkspaceMutation,
            ToolCapability::Shell => ExecutionClass::ApprovalRequired {
                tier: crate::tools::bash_ast::RiskTier::Mutating,
                reasons: vec!["Shell commands require safety analysis".to_string()],
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptors_cover_every_registered_tool() {
        let names: Vec<_> = ToolRegistry::descriptors().iter().map(|item| item.name).collect();
        assert_eq!(
            names,
            [
                "read",
                "write",
                "edit",
                "bash",
                "websearch",
                "webfetch",
                "ask_user",
                "ask_user_question"
            ]
        );
    }

    #[test]
    fn unknown_tools_have_no_descriptor() {
        assert!(!ToolRegistry::is_known("not-a-tool"));
    }
}
