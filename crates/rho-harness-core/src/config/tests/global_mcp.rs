use super::super::Config;
use rho_test_support::{EnvVarGuard, TestDir};
use std::path::Path;

/// Points HOME at `home` and RHO_HOME at `rho_home` (or unsets it), restored when the guard drops.
fn set_home_and_rho_home(home: &Path, rho_home: Option<&TestDir>) -> EnvVarGuard {
    let rho_home = rho_home.map(|dir| dir.to_str().unwrap());
    EnvVarGuard::set_all(&[
        ("HOME", Some(home.to_str().unwrap())),
        ("RHO_HOME", rho_home),
        ("RUST_AI_HOME", None),
    ])
}

fn write_server(path: &Path, name: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let content = format!(r#"{{"mcpServers":{{"{name}":{{"command":"echo"}}}}}}"#);
    std::fs::write(path, content).unwrap();
}

#[test]
fn rho_home_global_mcp_file_is_loaded_and_real_home_file_is_not() {
    let home = TestDir::with_prefix("rho_test_home_");
    write_server(&home.join(".claude.json"), "home-only");
    let rho_home = TestDir::with_prefix("rho_test_rho_home_");
    write_server(&rho_home.join(".claude.json"), "rho-home");
    let _env = set_home_and_rho_home(&home, Some(&rho_home));

    let config = Config::load(None).unwrap();

    assert!(config.mcp.servers.contains_key("rho-home"));
    assert!(!config.mcp.servers.contains_key("home-only"));
}

#[test]
fn rho_home_is_the_home_for_every_global_mcp_location() {
    let relative_paths = [
        ".config/mcp/mcp.json",
        ".agents/mcp.json",
        ".claude/mcp.json",
        ".claude.json",
    ];
    for relative in relative_paths {
        let home = TestDir::with_prefix("rho_test_home_");
        write_server(&home.join(relative), "home-only");
        let rho_home = TestDir::with_prefix("rho_test_rho_home_");
        write_server(&rho_home.join(relative), "rho-home");
        let _env = set_home_and_rho_home(&home, Some(&rho_home));

        let config = Config::load(None).unwrap();

        assert!(
            config.mcp.servers.contains_key("rho-home"),
            "{relative} not loaded from RHO_HOME"
        );
        assert!(
            !config.mcp.servers.contains_key("home-only"),
            "{relative} loaded from HOME"
        );
    }
}

#[test]
fn without_rho_home_global_mcp_file_is_read_from_home() {
    let home = TestDir::with_prefix("rho_test_home_");
    write_server(&home.join(".claude.json"), "home-only");
    let _env = set_home_and_rho_home(&home, None);

    let config = Config::load(None).unwrap();

    assert!(config.mcp.servers.contains_key("home-only"));
}
