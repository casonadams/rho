use super::*;
use rho_test_support::TestDir;
use test_case::test_case;

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
    let dir = TestDir::new();
    assert!(read_layer(&dir.as_ref().join("missing.json")).unwrap().is_empty());
    std::fs::write(dir.as_ref().join("list.json"), "[]").unwrap();
    assert!(
        read_layer(&dir.as_ref().join("list.json"))
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
    let dir = TestDir::new();
    let (home, ws) = (dir.as_ref().join("home"), dir.as_ref().join("ws"));
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
    let dir = TestDir::new();
    write_layer(&dir.as_ref().join(".agents/lsp.json"), json!({ "enabled": false }));
    let set = load_servers(Some(dir.as_ref()), &dir.as_ref().join("ws"), false).unwrap();
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
fn the_documented_dart_entry_adds_a_server_for_dart_packages() {
    let documented = json!({
        "dart": {
            "command": "dart",
            "args": ["language-server"],
            "extensions": ["dart"],
            "rootMarkers": ["pubspec.yaml"]
        }
    });
    let servers = resolve_servers(vec![layer(documented)]).unwrap();
    let dart = servers
        .iter()
        .find(|s| s.name == "dart")
        .expect("the entry adds a server");
    assert_eq!(
        (dart.command.as_str(), dart.args.as_slice()),
        ("dart", ["language-server".to_string()].as_slice())
    );
    assert!(
        servers.iter().any(|s| s.name == "rust-analyzer"),
        "built-in servers stay"
    );

    let dir = TestDir::new();
    let package = dir.as_ref().join("packages/app");
    std::fs::create_dir_all(package.join("lib")).unwrap();
    std::fs::write(package.join("pubspec.yaml"), "").unwrap();
    let file = package.join("lib/main.dart");
    assert!(dart.handles(&file));
    assert_eq!(
        dart.language_id(&file),
        "dart",
        "the extension is the language id when none is known"
    );
    assert_eq!(dart.root_for(&file, dir.as_ref()), Some(package));
}

#[test]
fn roots_prefer_the_workspace_then_the_nearest_marked_ancestor() {
    let dir = TestDir::new();
    let ws = dir.as_ref();
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
    let dir = TestDir::new();
    let tool = dir.as_ref().join("mylsp");
    std::fs::write(&tool, "").unwrap();
    let path_var = std::env::join_paths([dir.as_ref()]).unwrap();

    let root = Path::new("/");
    assert_eq!(
        resolve_command("mylsp", Some(&path_var), root),
        None,
        "not executable yet"
    );
    std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(resolve_command("mylsp", Some(&path_var), root), Some(tool.clone()));
    assert_eq!(resolve_command("mylsp", None, root), None);
    assert_eq!(resolve_command(tool.to_str().unwrap(), None, root), Some(tool));
}

#[cfg(unix)]
#[test]
fn relative_commands_resolve_to_absolute_paths_so_check_and_spawn_agree() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TestDir::new();
    let tool = dir.as_ref().join("bin/mylsp");
    std::fs::create_dir(dir.as_ref().join("bin")).unwrap();
    std::fs::write(&tool, "").unwrap();
    std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(resolve_command("bin/mylsp", None, dir.as_ref()), Some(tool));
    assert_eq!(resolve_command("bin/mylsp", None, Path::new("/")), None);

    let path_var = std::env::join_paths(["bin"]).unwrap();
    assert_eq!(
        resolve_command("mylsp", Some(&path_var), dir.as_ref()),
        None,
        "relative PATH entries are skipped"
    );
}

#[test]
fn a_malformed_untrusted_project_file_is_a_notice_not_a_failure() {
    let dir = TestDir::new();
    let (home, ws) = (dir.as_ref().join("home"), dir.as_ref().join("ws"));
    write_layer(&home.join(".agents/lsp.json"), json!({ "mine": { "command": "m" } }));
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::write(ws.join(".lsp.json"), "{ not json").unwrap();

    let set = load_servers(Some(&home), &ws, false).unwrap();
    assert!(set.servers.iter().any(|s| s.name == "mine"));
    assert!(set.notice.unwrap().contains(".lsp.json"));
    assert!(
        load_servers(Some(&home), &ws, true).is_err(),
        "a trusted file must parse"
    );
}

#[test_case(json!(5000), Some(5000), false; "unsigned integer")]
#[test_case(json!("5000"), None, true; "string")]
#[test_case(json!(-1), None, true; "negative")]
#[test_case(json!(1.5), None, true; "fraction")]
fn idle_timeouts_must_be_unsigned_integers_and_others_are_ignored_with_a_notice(
    value: Value,
    expected: Option<u64>,
    notice: bool,
) {
    let dir = TestDir::new();
    write_layer(
        &dir.as_ref().join(".agents/lsp.json"),
        json!({ "idleTimeoutMs": value }),
    );
    let set = load_servers(Some(dir.as_ref()), &dir.as_ref().join("ws"), false).unwrap();
    assert_eq!(set.idle_timeout_ms, expected);
    let noticed = set.notice.is_some_and(|n| n.contains("idleTimeoutMs"));
    assert_eq!(noticed, notice);
}
