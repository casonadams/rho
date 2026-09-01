pub mod catalog;
#[cfg(test)]
mod tests;

pub use catalog::{
    BuiltinToolDeclaration, BuiltinToolKind, DECLARATIONS, PROMPT_ASK_USER, PROMPT_BASH, PROMPT_EDIT, PROMPT_READ,
    PROMPT_WEBFETCH, PROMPT_WEBSEARCH, PROMPT_WRITE,
};

use crate::tools::ask_user::{AskUserArgs, AskUserTool};
use crate::tools::bash::{BashArgs, BashTool};
use crate::tools::edit::{EditArgs, EditTool};
use crate::tools::read::{ReadArgs, ReadTool};
use crate::tools::types::{ToolResult, generated_schema, into_dynamic_result};
use crate::tools::web::{
    FetchCache, HttpClient, SearchRateLimiter, WebFetchConfig, WebFetchTool, WebSearchConfig, WebSearchTool,
};
use crate::tools::write::{WriteArgs, WriteTool};
use rho_core::args::{FetchArgs, SearchArgs};
use rho_core::config::Config;
use rho_core::error::Result;
use rig::tool::DynamicTool;
use std::path::Path;
use std::sync::Arc;

fn parse_args<T: serde::de::DeserializeOwned>(args: serde_json::Value) -> std::result::Result<T, ToolResult> {
    serde_json::from_value(args).map_err(|e| ToolResult::error(format!("failed to parse tool arguments: {e}")))
}

pub fn build_builtin_tools(base_dir: &Path, config: &Config) -> Result<Vec<DynamicTool>> {
    let http = HttpClient::new(config.allow_private_network)?;
    let search = WebSearchTool::new(
        http.clone(),
        SearchRateLimiter::new(config.search_min_interval_ms),
        WebSearchConfig {
            region: config.region.clone(),
            timeout_sec: config.search_timeout_sec,
        },
    );
    let fetch = WebFetchTool::new(
        http,
        FetchCache::new(60, 64),
        WebFetchConfig {
            timeout_sec: config.fetch_timeout_sec,
            max_bytes: config.fetch_max_bytes,
            default_limit: config.fetch_limit,
        },
    );
    let read = Arc::new(ReadTool::new(base_dir));
    let write = Arc::new(WriteTool::with_exclusions(
        base_dir,
        [&config.config_dir, &config.sessions_dir],
    ));
    let edit = Arc::new(EditTool::with_exclusions(
        base_dir,
        [&config.config_dir, &config.sessions_dir],
    ));
    let bash = Arc::new(BashTool::new(base_dir));
    let ask = Arc::new(AskUserTool::new());

    let mut tools = Vec::new();

    let r = Arc::clone(&read);
    tools.push(DynamicTool::new(
        "read",
        "Read file contents with line numbering, offset, and limit safeguards.",
        generated_schema::<ReadArgs>(),
        move |_ctx, args| {
            let r = Arc::clone(&r);
            Box::pin(async move {
                let args: ReadArgs = match parse_args(args) {
                    Ok(a) => a,
                    Err(err) => return into_dynamic_result(Ok(err)),
                };
                into_dynamic_result(r.execute(args).await)
            })
        },
    ));

    let w = Arc::clone(&write);
    tools.push(DynamicTool::new(
        "write",
        "Write full content to a file, automatically creating parent directories.",
        generated_schema::<WriteArgs>(),
        move |_ctx, args| {
            let w = Arc::clone(&w);
            Box::pin(async move {
                let args: WriteArgs = match parse_args(args) {
                    Ok(a) => a,
                    Err(err) => return into_dynamic_result(Ok(err)),
                };
                into_dynamic_result(w.execute(args).await)
            })
        },
    ));

    let e = Arc::clone(&edit);
    tools.push(DynamicTool::new(
        "edit",
        "Edit a file by applying exact string replacements. Every oldText must match exactly once.",
        generated_schema::<EditArgs>(),
        move |_ctx, args| {
            let e = Arc::clone(&e);
            Box::pin(async move {
                let args: EditArgs = match parse_args(args) {
                    Ok(a) => a,
                    Err(err) => return into_dynamic_result(Ok(err)),
                };
                into_dynamic_result(e.execute(args).await)
            })
        },
    ));

    let b = Arc::clone(&bash);
    tools.push(DynamicTool::new(
        "bash",
        "Execute a shell command in the current working directory with a timeout. Do not prefix commands with cd.",
        generated_schema::<BashArgs>(),
        move |ctx, args| {
            let b = Arc::clone(&b);
            let stream = ctx.get::<rho_core::presentation::ToolStreamPort>().cloned();
            Box::pin(async move {
                let args: BashArgs = match parse_args(args) {
                    Ok(a) => a,
                    Err(err) => return into_dynamic_result(Ok(err)),
                };
                if let Some(stream_port) = stream {
                    into_dynamic_result(
                        b.execute_streaming(args, move |chunk| stream_port.stream_chunk(chunk))
                            .await,
                    )
                } else {
                    into_dynamic_result(b.execute(args).await)
                }
            })
        },
    ));

    for name in ["search", "websearch", "web_search"] {
        let s = search.clone();
        tools.push(DynamicTool::new(
            name,
            "Search the web and return structured search results with titles, summaries, and URLs.",
            generated_schema::<SearchArgs>(),
            move |_ctx, args| {
                let s = s.clone();
                Box::pin(async move {
                    let args: SearchArgs = match parse_args(args) {
                        Ok(a) => a,
                        Err(err) => return into_dynamic_result(Ok(err)),
                    };
                    into_dynamic_result(s.execute(args).await)
                })
            },
        ));
    }

    for name in ["fetch", "webfetch", "web_fetch"] {
        let f = fetch.clone();
        tools.push(DynamicTool::new(
            name,
            "Fetch and extract readable content from a URL (HTML, JSON, Markdown, RSS/Atom, CSV, PDF).",
            generated_schema::<FetchArgs>(),
            move |_ctx, args| {
                let f = f.clone();
                Box::pin(async move {
                    let args: FetchArgs = match parse_args(args) {
                        Ok(a) => a,
                        Err(err) => return into_dynamic_result(Ok(err)),
                    };
                    into_dynamic_result(f.execute(args).await)
                })
            },
        ));
    }

    for name in ["ask", "ask_user", "ask_user_question"] {
        let a = Arc::clone(&ask);
        tools.push(DynamicTool::new(
            name,
            "Ask the user one or more structured questions to clarify ambiguous requirements, confirm architectural choices, or gather user preferences.",
            generated_schema::<AskUserArgs>(),
            move |ctx, args| {
                let a = Arc::clone(&a);
                let port = ctx.get::<rho_core::presentation::QuestionPort>().cloned();
                Box::pin(async move {
                    let args: AskUserArgs = match parse_args(args) {
                        Ok(a) => a,
                        Err(err) => return into_dynamic_result(Ok(err)),
                    };
                    if let Some(port) = port {
                        into_dynamic_result(a.execute(&port, args).await)
                    } else {
                        into_dynamic_result(Ok(ToolResult::error("Interactive question port is not configured")))
                    }
                })
            },
        ));
    }

    Ok(tools)
}
