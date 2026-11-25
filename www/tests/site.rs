use nsh_core::element::Root;
use nsh_core::geometry::{Size, px};
use nsh_engine::EngineConfig;
use nsh_testing::Headless;

fn open(width: f32) -> Headless {
    let mut config = EngineConfig::new(Root::of::<rho_www::App>());
    config.viewport = Size::new(px(width), px(900.0));
    let mut app = Headless::start(config).expect("engine starts");
    app.wait_for("label:rho home").expect("header");
    app
}

#[test]
fn the_docs_link_opens_the_overview_and_its_neighbors() {
    let mut app = open(1280.0);
    app.press("text:Read the docs").unwrap();
    app.wait_for("text:Built-in Tools").expect("the README as the overview");

    app.press("text:Permissions & privacy").unwrap();
    app.wait_for("text:Guard Model").expect("the permissions doc");
}

#[test]
fn the_settings_search_keeps_only_matching_settings() {
    let mut app = open(1280.0);
    app.press("text:Configuration >> nth=0").unwrap();
    app.wait_for("text:permission.guard_audit_log")
        .expect("the config page");
    assert!(app.shows("max_output_tokens"));

    app.type_text("id:settings-search", "guard jsonl").unwrap();
    assert!(app.shows("permission.guard_audit_log"));
    assert!(!app.shows("max_output_tokens"));
    assert!(app.shows("1 of 86 settings"));

    app.type_text("id:settings-search", " nothing-like-this").unwrap();
    assert!(app.shows("No settings match."));
}

#[test]
fn the_ecosystem_filter_shows_one_kind() {
    let mut app = open(1280.0);
    app.press("text:Ecosystem >> nth=0").unwrap();
    app.wait_for("text:Bash command guard").expect("the ecosystem page");
    assert!(app.shows("Playwright"));

    app.press("text:Hooks").unwrap();
    assert!(app.shows("Bash command guard"));
    assert!(!app.shows("Playwright"));
}

#[test]
fn phones_get_a_menu_instead_of_inline_links() {
    let mut app = open(390.0);
    assert_eq!(app.screen().enabled("label:Open menu"), Ok(true));
    assert!(!app.shows("GitHub ↗"));

    app.press("label:Open menu").unwrap();
    assert!(app.shows("GitHub ↗"));
    app.press("text:Configuration >> nth=0").unwrap();
    app.wait_for("text:permission.guard_audit_log")
        .expect("the config page");
    assert!(!app.shows("GitHub ↗"), "the menu closes after a choice");
}

#[test]
fn desktops_show_the_navigation_inline() {
    let app = open(1280.0);
    assert!(app.screen().enabled("label:Open menu").is_err());
    assert!(app.shows("Docs"));
}
