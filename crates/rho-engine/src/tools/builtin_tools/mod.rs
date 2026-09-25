pub mod catalog;
#[cfg(test)]
mod tests;

pub use catalog::{
    BuiltinToolDeclaration, BuiltinToolKind, DECLARATIONS, PROMPT_BASH, PROMPT_EDIT, PROMPT_FD, PROMPT_READ, PROMPT_RG,
    PROMPT_WEB_FETCH, PROMPT_WEB_SEARCH, PROMPT_WRITE,
};

use crate::tools::bash::{BashArgs, BashTool};
use crate::tools::edit::{EditArgs, EditTool};
use crate::tools::fd::FdTool;
use crate::tools::read::{ReadArgs, ReadTool};
use crate::tools::rg::RgTool;
use crate::tools::types::{ToolResult, generated_schema, into_dynamic_result};
use crate::tools::web::{
    FetchCache, HttpClient, SearchRateLimiter, WebFetchConfig, WebFetchTool, WebSearchConfig, WebSearchTool,
};
use crate::tools::write::{WriteArgs, WriteTool};
use rho_harness_core::args::{FdArgs, RgArgs, WebFetchArgs, WebSearchArgs};
use rho_harness_core::config::Config;
use rho_harness_core::error::Result;
use rig::tool::{DynamicTool, ToolContext};
use std::path::Path;
use std::sync::Arc;

fn parse_args<T: serde::de::DeserializeOwned>(args: serde_json::Value) -> std::result::Result<T, ToolResult> {
    serde_json::from_value(args).map_err(|e| ToolResult::error(format!("failed to parse tool arguments: {e}")))
}

fn dynamic_tool<A, F, Fut>(name: &'static str, description: &'static str, execute: F) -> DynamicTool
where
    A: serde::de::DeserializeOwned + schemars::JsonSchema + Send + 'static,
    F: Fn(&ToolContext, A) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = std::result::Result<ToolResult, rho_harness_core::error::AppError>>
        + Send
        + 'static,
{
    DynamicTool::new(name, description, generated_schema::<A>(), move |ctx, args| {
        let fut = match parse_args::<A>(args) {
            Ok(a) => Ok(execute(ctx, a)),
            Err(err) => Err(err),
        };
        Box::pin(async move {
            match fut {
                Ok(f) => into_dynamic_result(f.await),
                Err(err) => into_dynamic_result(Ok(err)),
            }
        })
    })
}

fn make_read_tool(read: Arc<ReadTool>) -> DynamicTool {
    dynamic_tool(
        "read",
        "Read file contents with line numbering, offset, and limit safeguards. Reads supported images (png, jpeg, gif, webp, bmp) and attaches them to the result.",
        move |_ctx, args: ReadArgs| {
            let r = Arc::clone(&read);
            async move { r.execute(args).await }
        },
    )
}

fn make_write_tool(write: Arc<WriteTool>) -> DynamicTool {
    dynamic_tool(
        "write",
        "Write full content to a file, automatically creating parent directories.",
        move |_ctx, args: WriteArgs| {
            let w = Arc::clone(&write);
            async move { w.execute(args).await }
        },
    )
}

fn make_edit_tool(edit: Arc<EditTool>) -> DynamicTool {
    dynamic_tool(
        "edit",
        "Edit a file by applying exact string replacements. Every oldText must match exactly once.",
        move |_ctx, args: EditArgs| {
            let e = Arc::clone(&edit);
            async move { e.execute(args).await }
        },
    )
}

fn make_bash_tool(bash: Arc<BashTool>) -> DynamicTool {
    dynamic_tool(
        "bash",
        "Execute a shell command in the current working directory with a timeout. Do not prefix commands with cd.",
        move |ctx, args: BashArgs| {
            let b = Arc::clone(&bash);
            let stream = ctx.get::<rho_harness_core::presentation::ToolStreamPort>().cloned();
            async move {
                if let Some(stream_port) = stream {
                    b.execute_streaming(args, move |chunk| stream_port.stream_chunk(chunk))
                        .await
                } else {
                    b.execute(args).await
                }
            }
        },
    )
}

fn make_fd_tool(fd: Arc<FdTool>) -> DynamicTool {
    dynamic_tool(
        "fd",
        "Find files and directories by workspace-relative path with a smart-case regex; gitignore-aware and bounded.",
        move |_ctx, args: FdArgs| {
            let fd_tool = Arc::clone(&fd);
            async move { fd_tool.execute(args).await }
        },
    )
}

fn make_rg_tool(rg: Arc<RgTool>) -> DynamicTool {
    dynamic_tool(
        "rg",
        "Search file contents with a smart-case regex; gitignore-aware, skips binary and large files, bounded.",
        move |_ctx, args: RgArgs| {
            let rg_tool = Arc::clone(&rg);
            async move { rg_tool.execute(args).await }
        },
    )
}

fn build_web_dynamic_tools(config: &Config) -> Result<Vec<DynamicTool>> {
    let mut tools = Vec::new();
    if !config.tools.web.search.enabled && !config.tools.web.fetch.enabled {
        return Ok(tools);
    }
    let http = HttpClient::new(config.allow_private_network)?;

    if config.tools.web.search.enabled {
        let engines = crate::tools::web::search::resolve_engine_chain(
            &config.tools.web.search.default,
            &config.tools.web.search.fallback,
        )?;
        let search = WebSearchTool::new(
            http.clone(),
            SearchRateLimiter::new(config.search_min_interval_ms),
            WebSearchConfig {
                region: config.region.clone(),
                timeout_sec: config.search_timeout_sec,
                engines,
            },
        );
        tools.push(dynamic_tool(
            "web_search",
            "Search the web and return structured search results with titles, summaries, and URLs.",
            move |_ctx, args: WebSearchArgs| {
                let s = search.clone();
                async move { s.execute(args).await }
            },
        ));
    }

    if config.tools.web.fetch.enabled {
        let fetch = WebFetchTool::new(
            http,
            FetchCache::new(60, 64),
            WebFetchConfig {
                timeout_sec: config.fetch_timeout_sec,
                max_bytes: config.fetch_max_bytes,
                pdf_max_bytes: 30 * 1024 * 1024,
                default_limit: config.fetch_limit,
                multimodal: config.tools.web.fetch.multimodal,
                auth_file: Some(config.auth_file.clone()),
            },
        );
        tools.push(dynamic_tool(
            "web_fetch",
            "Fetch and extract readable content from a URL (HTML, JSON, Markdown, RSS/Atom, CSV, PDF, images/diagrams).",
            move |_ctx, args: WebFetchArgs| {
                let f = fetch.clone();
                async move { f.execute(args).await }
            },
        ));
    }

    Ok(tools)
}

fn build_workspace_tools(base_dir: &Path, config: &Config) -> Vec<DynamicTool> {
    let write = Arc::new(WriteTool::with_exclusions(
        base_dir,
        [&config.config_dir, &config.sessions_dir],
    ));
    let edit = Arc::new(EditTool::with_exclusions(
        base_dir,
        [&config.config_dir, &config.sessions_dir],
    ));
    let read = Arc::new(ReadTool::new(base_dir));
    let bash = Arc::new(BashTool::new(base_dir));
    let fd = Arc::new(FdTool::new(base_dir));
    let rg = Arc::new(RgTool::new(base_dir));

    vec![
        make_read_tool(read),
        make_write_tool(write),
        make_edit_tool(edit),
        make_bash_tool(bash),
        make_fd_tool(fd),
        make_rg_tool(rg),
    ]
}

pub fn build_builtin_tools(base_dir: &Path, config: &Config) -> Result<Vec<DynamicTool>> {
    let mut tools = build_workspace_tools(base_dir, config);
    tools.extend(build_web_dynamic_tools(config)?);
    Ok(tools)
}

pub fn build_all_builtin_tools(base_dir: &Path, config: &Config) -> Result<Vec<DynamicTool>> {
    build_builtin_tools(base_dir, config)
}
