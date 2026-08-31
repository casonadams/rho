use super::*;
use crate::tools::AskUserQuestionTool;
use crate::tools::registry::ToolRegistry;
use rig::tool::{Tool, tool_definition};

fn assert_native_definition<T: Tool>(tool: &T, capabilities: &BTreeMap<CapabilityId, Arc<dyn ToolCapability>>) {
    let definition = tool_definition(tool);
    let descriptor = capabilities[&format!("tool:{}", T::NAME).parse().unwrap()].descriptor();
    assert_eq!(definition.parameters, descriptor.argument_schema, "{} schema", T::NAME);
    assert_eq!(
        definition.description,
        descriptor.description,
        "{} description",
        T::NAME
    );
}

#[test]
fn declarations_match_legacy_names_prompts_descriptions_and_schemas() {
    let root = std::env::temp_dir();
    let config = Config::default();
    let catalog = BuiltinToolCatalog::new(&root, &config).unwrap();
    let capabilities = catalog.into_capabilities();
    assert_eq!(capabilities.len(), ToolRegistry::descriptors().len());
    for declaration in DECLARATIONS {
        let legacy = ToolRegistry::descriptor(declaration.name).unwrap();
        let capability = capabilities
            .get(&format!("tool:{}", declaration.name).parse().unwrap())
            .unwrap();
        let descriptor = capability.descriptor();
        assert_eq!(legacy.prompt, descriptor.prompt_guidance);
        assert_eq!(legacy.description, descriptor.description);
        assert_eq!(legacy.capability, declaration.capability);
        assert_eq!(declaration.execution_mode, descriptor.execution_mode);
    }

    let read_desc = capabilities.get(&"tool:read".parse().unwrap()).unwrap().descriptor();
    assert_eq!(read_desc.execution_mode, ExecutionMode::Parallel);

    let write_desc = capabilities.get(&"tool:write".parse().unwrap()).unwrap().descriptor();
    assert_eq!(write_desc.execution_mode, ExecutionMode::Sequential);

    let bash_desc = capabilities.get(&"tool:bash".parse().unwrap()).unwrap().descriptor();
    assert_eq!(bash_desc.execution_mode, ExecutionMode::Sequential);

    assert_native_definition(&ReadTool::new(&root), &capabilities);
    assert_native_definition(&WriteTool::new(&root), &capabilities);
    assert_native_definition(&EditTool::new(&root), &capabilities);
    assert_native_definition(&BashTool::new(&root), &capabilities);
    assert_native_definition(&AskUserTool::new(), &capabilities);
    assert_native_definition(&AskUserQuestionTool::default(), &capabilities);
    let http = HttpClient::new(false).unwrap();
    assert_native_definition(
        &WebSearchTool::new(
            http.clone(),
            SearchRateLimiter::new(0),
            WebSearchConfig {
                region: "wt-wt".to_string(),
                timeout_sec: 1,
            },
        ),
        &capabilities,
    );
    assert_native_definition(
        &WebFetchTool::new(
            http,
            FetchCache::new(60, 4),
            WebFetchConfig {
                timeout_sec: 1,
                max_bytes: 1024,
                default_limit: 20,
            },
        ),
        &capabilities,
    );
}
