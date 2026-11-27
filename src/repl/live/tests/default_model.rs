use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rho_harness_core::config::Config;

fn extract_desc_marks(desc: &str) -> (&str, &str) {
    let mut parts = desc.split('\t');
    let _prov = parts.next();
    (parts.next().unwrap_or(""), parts.next().unwrap_or(""))
}

fn setup_default_model_session(config_dir: std::path::PathBuf) -> crate::repl::ReplSession {
    let config = Config {
        config_dir,
        model: "claude-3-5-haiku-20241022".into(),
        provider: "anthropic".into(),
        default_model: Some("anthropic/claude-3-7-sonnet-20250219".into()),
        ..Config::default()
    };
    let mut session = crate::repl::ReplSession::new(config, crate::auth::AuthStore::default(), None);
    session.catalog = crate::repl::interactive::models::catalog_with(
        "anthropic",
        &["claude-3-7-sonnet-20250219", "claude-3-5-haiku-20241022"],
    );
    session
}

#[test]
fn model_selector_marks_default_model_separately_from_active() {
    let temp = tempfile::tempdir().unwrap();
    let session = setup_default_model_session(temp.path().to_path_buf());
    let mut controller = crate::repl::test_controller();
    super::super::modal::open_model_selector(&session, &mut controller);
    let modal = controller.state().active_modal().unwrap();

    let active = modal
        .options
        .iter()
        .find(|o| o.label == "anthropic/claude-3-5-haiku-20241022")
        .unwrap();
    assert_eq!(extract_desc_marks(active.description.as_deref().unwrap()), ("✓", ""));

    let default = modal
        .options
        .iter()
        .find(|o| o.label == "anthropic/claude-3-7-sonnet-20250219")
        .unwrap();
    assert_eq!(
        extract_desc_marks(default.description.as_deref().unwrap()),
        ("", "default")
    );
}

#[test]
fn ctrl_s_key_saves_selected_model_as_default() {
    let temp = tempfile::tempdir().unwrap();
    let session = setup_default_model_session(temp.path().to_path_buf());
    let mut controller = crate::repl::test_controller();

    super::super::modal::open_model_selector(&session, &mut controller);

    let ctrl_s = KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL);
    let res = super::super::modal::handle_modal_key(&mut controller, ctrl_s, &mut None).unwrap();

    match res {
        super::super::modal::ModalKeyResult::ModelSelected {
            model: _,
            provider: _,
            save_as_default,
        } => {
            assert!(save_as_default, "Ctrl+S must trigger save_as_default");
        }
        _ => panic!("expected ModelSelected result"),
    }
}
