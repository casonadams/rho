use super::*;

fn layer(json: Value) -> Map<String, Value> {
    json.as_object().unwrap().clone()
}

fn server(markers: &[&str]) -> ServerConfig {
    serde_json::from_value(json!({ "command": "x", "extensions": ["rs"], "rootMarkers": markers })).unwrap()
}

#[test]
fn layers_override_defaults_field_by_field_and_can_add_or_disable_servers() {
    let user = layer(json!({
        "rust-analyzer": { "settings": { "rust-analyzer": { "checkOnSave": false } } },
        "gopls": { "disabled": true },
    }));
    let project = layer(json!({
        "rust-analyzer": { "args": ["--log"] },
        "ruff": { "command": "ruff", "args": ["server"], "extensions": ["py"], "linter": true },
        "mylang": { "command": "mylsp", "extensionToLanguage": { ".my": "mylang" } },
    }));
    let servers = resolve_servers(vec![user, project]).unwrap();

    let ra = servers.iter().find(|s| s.name == "rust-analyzer").unwrap();
    assert_eq!(ra.command, "rust-analyzer");
    assert_eq!(ra.args, ["--log"]);
    assert_eq!(ra.root_markers, ["Cargo.toml", "rust-project.json"]);
    assert_eq!(ra.settings, Some(json!({ "rust-analyzer": { "checkOnSave": false } })));
    assert!(servers.iter().all(|s| s.name != "gopls"));

    let mine = servers.iter().find(|s| s.name == "mylang").unwrap();
    assert!(mine.handles(Path::new("a/b.my")));
    assert_eq!(mine.language_id(Path::new("b.my")), "mylang");

    assert_eq!(servers.first().unwrap().name, "rust-analyzer");
    assert_eq!(servers.last().unwrap().name, "ruff", "linters sort after main servers");
    let user_added = servers.iter().position(|s| s.name == "mylang").unwrap();
    let last_default = servers.iter().rposition(|s| s.name == "taplo").unwrap();
    assert!(user_added > last_default, "user servers follow the built-in order");
}

#[test]
fn invalid_server_entries_name_the_server() {
    let error = resolve_servers(vec![layer(json!({ "broken": { "args": [] } }))]).unwrap_err();
    assert!(error.contains("lsp server 'broken'"), "{error}");
}

#[test]
fn layer_files_are_optional_but_must_hold_an_object() {
    let dir = tempfile::tempdir().unwrap();
    assert!(read_layer(&dir.path().join("missing.json")).unwrap().is_empty());
    std::fs::write(dir.path().join("list.json"), "[]").unwrap();
    assert!(
        read_layer(&dir.path().join("list.json"))
            .unwrap_err()
            .contains("JSON object")
    );
}

fn write_layer(path: &Path, json: Value) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, json.to_string()).unwrap();
}

#[test]
fn project_files_apply_only_to_trusted_projects() {
    let dir = tempfile::tempdir().unwrap();
    let (home, ws) = (dir.path().join("home"), dir.path().join("ws"));
    write_layer(&home.join(".agents/lsp.json"), json!({ "gopls": { "args": ["-v"] } }));
    write_layer(
        &ws.join(".lsp.json"),
        json!({ "evil": { "command": "sh", "extensions": ["rs"] } }),
    );

    let untrusted = load_servers(Some(&home), &ws, false).unwrap();
    assert!(untrusted.servers.iter().all(|s| s.name != "evil"));
    assert_eq!(
        untrusted.servers.iter().find(|s| s.name == "gopls").unwrap().args,
        ["-v"]
    );
    let notice = untrusted.notice.unwrap();
    assert!(
        notice.contains(".lsp.json") && notice.contains("RHO_TRUST_PROJECT=1"),
        "{notice}"
    );

    let trusted = load_servers(Some(&home), &ws, true).unwrap();
    assert!(trusted.servers.iter().any(|s| s.name == "evil"));
    assert_eq!(trusted.notice, None);
}

#[test]
fn enabled_false_turns_every_server_off() {
    let dir = tempfile::tempdir().unwrap();
    write_layer(&dir.path().join(".agents/lsp.json"), json!({ "enabled": false }));
    let set = load_servers(Some(dir.path()), &dir.path().join("ws"), false).unwrap();
    assert!(set.servers.is_empty());
    assert!(set.notice.unwrap().contains("turned off"));
}

#[test]
fn language_ids_and_extension_matching() {
    let servers = resolve_servers(Vec::new()).unwrap();
    let ts = servers.iter().find(|s| s.name == "typescript-language-server").unwrap();
    assert!(ts.handles(Path::new("App.TSX")));
    assert!(!ts.handles(Path::new("Makefile")));
    assert_eq!(ts.language_id(Path::new("App.tsx")), "typescriptreact");
    assert_eq!(ts.language_id(Path::new("x.mjs")), "javascript");
}

#[test]
fn roots_prefer_the_workspace_then_the_nearest_marked_ancestor() {
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path();
    let nested = ws.join("tools/cli/src");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(ws.join("tools/cli/Cargo.toml"), "").unwrap();
    std::fs::write(ws.join("tools/app.sln"), "").unwrap();
    let file = nested.join("main.rs");

    assert_eq!(server(&["Cargo.toml"]).root_for(&file, ws), Some(ws.join("tools/cli")));
    assert_eq!(server(&["*.sln"]).root_for(&file, ws), Some(ws.join("tools")));
    assert_eq!(server(&["go.mod"]).root_for(&file, ws), None);
    assert_eq!(server(&[]).root_for(&file, ws), Some(ws.to_path_buf()));

    std::fs::write(ws.join("Cargo.toml"), "").unwrap();
    assert_eq!(server(&["Cargo.toml"]).root_for(&file, ws), Some(ws.to_path_buf()));
}

#[cfg(unix)]
#[test]
fn commands_resolve_only_to_executables_on_the_given_path() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let tool = dir.path().join("mylsp");
    std::fs::write(&tool, "").unwrap();
    let path_var = std::env::join_paths([dir.path()]).unwrap();

    assert_eq!(resolve_command("mylsp", Some(&path_var)), None, "not executable yet");
    std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(resolve_command("mylsp", Some(&path_var)), Some(tool.clone()));
    assert_eq!(resolve_command("mylsp", None), None);
    assert_eq!(resolve_command(tool.to_str().unwrap(), None), Some(tool));
}
