pub mod catalog;
#[cfg(test)]
mod tests;

pub use catalog::{
    BuiltinToolDeclaration, BuiltinToolKind, DECLARATIONS, PROMPT_BASH, PROMPT_EDIT, PROMPT_FD, PROMPT_READ, PROMPT_RG,
    PROMPT_SCRIPT, PROMPT_WEB_FETCH, PROMPT_WEB_SEARCH, PROMPT_WRITE,
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
use rig::tool::DynamicTool;
use std::path::Path;
use std::sync::Arc;

fn parse_args<T: serde::de::DeserializeOwned>(args: serde_json::Value) -> std::result::Result<T, ToolResult> {
    serde_json::from_value(args).map_err(|e| ToolResult::error(format!("failed to parse tool arguments: {e}")))
}

fn build_read_dynamic_tool(r: Arc<ReadTool>) -> DynamicTool {
    DynamicTool::new(
        "read",
        "Read file contents with line numbering, offset, and limit safeguards. Reads supported images (png, jpeg, gif, webp, bmp) and attaches them to the result.",
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
    )
}

fn build_write_dynamic_tool(w: Arc<WriteTool>) -> DynamicTool {
    DynamicTool::new(
        "write",
        "Write full content to a file, automatically creating parent directories.",
        generated_schema::<WriteArgs>(),
        move |ctx, args| {
            let w = Arc::clone(&w);
            let stream = ctx.get::<rho_harness_core::presentation::ToolStreamPort>().cloned();
            Box::pin(async move {
                let args: WriteArgs = match parse_args(args) {
                    Ok(a) => a,
                    Err(err) => return into_dynamic_result(Ok(err)),
                };
                if let Some(stream_port) = stream {
                    for line in args.content.lines() {
                        stream_port.stream_chunk(&format!("{line}\n"));
                    }
                }
                into_dynamic_result(w.execute(args).await)
            })
        },
    )
}

fn build_edit_dynamic_tool(e: Arc<EditTool>) -> DynamicTool {
    DynamicTool::new(
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
    )
}

fn build_bash_dynamic_tool(b: Arc<BashTool>) -> DynamicTool {
    DynamicTool::new(
        "bash",
        "Execute a shell command in the current working directory with a timeout. Do not prefix commands with cd.",
        generated_schema::<BashArgs>(),
        move |ctx, args| {
            let b = Arc::clone(&b);
            let stream = ctx.get::<rho_harness_core::presentation::ToolStreamPort>().cloned();
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
    )
}

fn build_fd_dynamic_tool(fd_tool: Arc<FdTool>) -> DynamicTool {
    DynamicTool::new(
        "fd",
        "Find files and directories by workspace-relative path with a smart-case regex; gitignore-aware and bounded.",
        generated_schema::<FdArgs>(),
        move |_ctx, args| {
            let fd_tool = Arc::clone(&fd_tool);
            Box::pin(async move {
                let args: FdArgs = match parse_args(args) {
                    Ok(a) => a,
                    Err(err) => return into_dynamic_result(Ok(err)),
                };
                into_dynamic_result(fd_tool.execute(args).await)
            })
        },
    )
}

fn build_rg_dynamic_tool(rg_tool: Arc<RgTool>) -> DynamicTool {
    DynamicTool::new(
        "rg",
        "Search file contents with a smart-case regex; gitignore-aware, skips binary and large files, bounded.",
        generated_schema::<RgArgs>(),
        move |_ctx, args| {
            let rg_tool = Arc::clone(&rg_tool);
            Box::pin(async move {
                let args: RgArgs = match parse_args(args) {
                    Ok(a) => a,
                    Err(err) => return into_dynamic_result(Ok(err)),
                };
                into_dynamic_result(rg_tool.execute(args).await)
            })
        },
    )
}

fn build_search_dynamic_tool(s: WebSearchTool) -> DynamicTool {
    DynamicTool::new(
        "web_search",
        "Search the web and return structured search results with titles, summaries, and URLs.",
        generated_schema::<WebSearchArgs>(),
        move |_ctx, args| {
            let s = s.clone();
            Box::pin(async move {
                let args: WebSearchArgs = match parse_args(args) {
                    Ok(a) => a,
                    Err(err) => return into_dynamic_result(Ok(err)),
                };
                into_dynamic_result(s.execute(args).await)
            })
        },
    )
}

fn build_fetch_dynamic_tool(f: WebFetchTool) -> DynamicTool {
    DynamicTool::new(
        "web_fetch",
        "Fetch and extract readable content from a URL (HTML, JSON, Markdown, RSS/Atom, CSV, PDF).",
        generated_schema::<WebFetchArgs>(),
        move |_ctx, args| {
            let f = f.clone();
            Box::pin(async move {
                let args: WebFetchArgs = match parse_args(args) {
                    Ok(a) => a,
                    Err(err) => return into_dynamic_result(Ok(err)),
                };
                into_dynamic_result(f.execute(args).await)
            })
        },
    )
}

fn register_file_runners(
    dispatcher: &mut crate::tools::script::ScriptDispatcher,
    read: &Arc<ReadTool>,
    write: &Arc<WriteTool>,
    edit: &Arc<EditTool>,
) {
    let r_c = Arc::clone(read);
    dispatcher.register(
        "read",
        Arc::new(move |args| {
            let r = Arc::clone(&r_c);
            async move {
                let parsed: ReadArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
                let res = r.execute(parsed).await.map_err(|e| e.to_string())?;
                if res.is_error {
                    Err(res.content)
                } else {
                    Ok(res.content)
                }
            }
        }),
    );

    let w_c = Arc::clone(write);
    dispatcher.register(
        "write",
        Arc::new(move |args| {
            let w = Arc::clone(&w_c);
            async move {
                let parsed: WriteArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
                let res = w.execute(parsed).await.map_err(|e| e.to_string())?;
                if res.is_error {
                    Err(res.content)
                } else {
                    Ok(res.content)
                }
            }
        }),
    );

    let e_c = Arc::clone(edit);
    dispatcher.register(
        "edit",
        Arc::new(move |args| {
            let e = Arc::clone(&e_c);
            async move {
                let parsed: EditArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
                let res = e.execute(parsed).await.map_err(|e| e.to_string())?;
                if res.is_error {
                    Err(res.content)
                } else {
                    Ok(res.content)
                }
            }
        }),
    );
}

fn register_exec_and_search_runners(
    dispatcher: &mut crate::tools::script::ScriptDispatcher,
    bash: &Arc<BashTool>,
    fd: &Arc<FdTool>,
    rg: &Arc<RgTool>,
) {
    let b_c = Arc::clone(bash);
    dispatcher.register(
        "bash",
        Arc::new(move |args| {
            let b = Arc::clone(&b_c);
            async move {
                let parsed: BashArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
                let res = b.execute(parsed).await.map_err(|e| e.to_string())?;
                if res.is_error {
                    Err(res.content)
                } else {
                    Ok(res.content)
                }
            }
        }),
    );

    let fd_c = Arc::clone(fd);
    dispatcher.register(
        "fd",
        Arc::new(move |args| {
            let fd = Arc::clone(&fd_c);
            async move {
                let parsed: FdArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
                let res = fd.execute(parsed).await.map_err(|e| e.to_string())?;
                if res.is_error {
                    Err(res.content)
                } else {
                    Ok(res.content)
                }
            }
        }),
    );

    let rg_c = Arc::clone(rg);
    dispatcher.register(
        "rg",
        Arc::new(move |args| {
            let rg = Arc::clone(&rg_c);
            async move {
                let parsed: RgArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
                let res = rg.execute(parsed).await.map_err(|e| e.to_string())?;
                if res.is_error {
                    Err(res.content)
                } else {
                    Ok(res.content)
                }
            }
        }),
    );
}

fn build_web_tools(config: &Config) -> Result<(WebSearchTool, WebFetchTool)> {
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
            pdf_max_bytes: 30 * 1024 * 1024,
            default_limit: config.fetch_limit,
        },
    );
    Ok((search, fetch))
}

fn register_web_runners(
    dispatcher: &mut crate::tools::script::ScriptDispatcher,
    search: &WebSearchTool,
    fetch: &WebFetchTool,
) {
    let s_c = search.clone();
    dispatcher.register(
        "web_search",
        Arc::new(move |args| {
            let s = s_c.clone();
            async move {
                let parsed: WebSearchArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
                let res = s.execute(parsed).await.map_err(|e| e.to_string())?;
                if res.is_error {
                    Err(res.content)
                } else {
                    Ok(res.content)
                }
            }
        }),
    );

    let f_c = fetch.clone();
    dispatcher.register(
        "web_fetch",
        Arc::new(move |args| {
            let f = f_c.clone();
            async move {
                let parsed: WebFetchArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
                let res = f.execute(parsed).await.map_err(|e| e.to_string())?;
                if res.is_error {
                    Err(res.content)
                } else {
                    Ok(res.content)
                }
            }
        }),
    );
}

pub fn build_builtin_tools(base_dir: &Path, config: &Config) -> Result<Vec<DynamicTool>> {
    let (search, fetch) = build_web_tools(config)?;
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

    let mut dispatcher = crate::tools::script::ScriptDispatcher::new();
    register_file_runners(&mut dispatcher, &read, &write, &edit);
    register_exec_and_search_runners(&mut dispatcher, &bash, &fd, &rg);
    register_web_runners(&mut dispatcher, &search, &fetch);

    let script = crate::tools::script::build_script_dynamic_tool(Arc::new(dispatcher), config.output_max_bytes);

    Ok(vec![
        build_read_dynamic_tool(read),
        build_write_dynamic_tool(write),
        build_edit_dynamic_tool(edit),
        build_bash_dynamic_tool(bash),
        build_fd_dynamic_tool(fd),
        build_rg_dynamic_tool(rg),
        build_search_dynamic_tool(search),
        build_fetch_dynamic_tool(fetch),
        script,
    ])
}
