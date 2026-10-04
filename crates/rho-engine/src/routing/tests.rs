use std::collections::BTreeMap;
use std::time::Duration;

use super::*;
use crate::provider::systemone::{ChoiceAnswer, SystemOneAnswer, SystemOneQuestion, SystemOneResponse};
use rho_harness_core::config::Config;

#[test]
fn test_build_routing_request_structure() {
    let req = ModelRouter::build_routing_request("clef-flash", "fix typo in doc");
    assert_eq!(req.model, "clef-flash");
    assert!(req.state.contains("fix typo in doc"));
    let q = req.questions.get("tier").unwrap();
    match q {
        SystemOneQuestion::Choice { instructions, criteria } => {
            assert!(instructions.contains("Select the most appropriate model"));
            assert!(criteria.contains_key("smol"));
            assert!(criteria.contains_key("standard"));
            assert!(criteria.contains_key("slow"));
        }
        _ => panic!("Expected Choice question"),
    }
}

#[test]
fn test_resolve_tier_choice_behavior() {
    let router = ModelRouter::new(
        "http://localhost:11434",
        "clef-flash",
        "claude-3-7-sonnet".to_string(),
        Some("qwen2.5-coder:7b".to_string()),
        Some("o3-mini".to_string()),
    );

    let (tier_smol, model_smol) = router.resolve_tier_choice("smol");
    assert_eq!(tier_smol, ModelTier::Smol);
    assert_eq!(model_smol, "qwen2.5-coder:7b");

    let (tier_slow, model_slow) = router.resolve_tier_choice("slow");
    assert_eq!(tier_slow, ModelTier::Slow);
    assert_eq!(model_slow, "o3-mini");

    let (tier_std, model_std) = router.resolve_tier_choice("standard");
    assert_eq!(tier_std, ModelTier::Standard);
    assert_eq!(model_std, "claude-3-7-sonnet");

    let (tier_unk, model_unk) = router.resolve_tier_choice("unknown");
    assert_eq!(tier_unk, ModelTier::Standard);
    assert_eq!(model_unk, "claude-3-7-sonnet");

    // Without smol or slow configured, fall back to standard
    let router_no_roles = ModelRouter::new(
        "http://localhost:11434",
        "clef-flash",
        "claude-3-7-sonnet".to_string(),
        None,
        None,
    );
    let (t_smol, m_smol) = router_no_roles.resolve_tier_choice("smol");
    assert_eq!(t_smol, ModelTier::Standard);
    assert_eq!(m_smol, "claude-3-7-sonnet");

    let (t_slow, m_slow) = router_no_roles.resolve_tier_choice("slow");
    assert_eq!(t_slow, ModelTier::Standard);
    assert_eq!(m_slow, "claude-3-7-sonnet");
}

#[test]
fn test_parse_verdict() {
    let router = ModelRouter::new(
        "http://localhost:11434",
        "clef-flash",
        "default-model".to_string(),
        Some("smol-model".to_string()),
        None,
    );

    let mut answers = BTreeMap::new();
    answers.insert(
        "tier".to_string(),
        SystemOneAnswer::Choice(ChoiceAnswer {
            choice: "smol".to_string(),
            confidence: Some(0.95),
            probabilities: BTreeMap::new(),
        }),
    );
    let resp = SystemOneResponse { answers };
    let verdict = router.parse_verdict(&resp).unwrap();
    assert_eq!(verdict.tier, ModelTier::Smol);
    assert_eq!(verdict.model, "smol-model");
    assert_eq!(verdict.confidence, Some(0.95));
    assert!(!verdict.fallback);

    let empty_resp = SystemOneResponse {
        answers: BTreeMap::new(),
    };
    assert!(router.parse_verdict(&empty_resp).is_none());
}

#[tokio::test]
async fn test_route_prompt_empty_and_whitespace() {
    let router = ModelRouter::new(
        "http://localhost:11434",
        "clef-flash",
        "default-model".to_string(),
        None,
        None,
    );
    let v1 = router.route_prompt("").await;
    assert_eq!(v1.tier, ModelTier::Standard);
    assert_eq!(v1.model, "default-model");
    assert!(!v1.fallback);

    let v2 = router.route_prompt("   \n\t  ").await;
    assert_eq!(v2.tier, ModelTier::Standard);
    assert!(!v2.fallback);
}

#[tokio::test]
async fn test_route_prompt_unreachable_endpoint_falls_back() {
    let router = ModelRouter::new(
        "http://127.0.0.1:1",
        "clef-flash",
        "default-model".to_string(),
        Some("smol-model".to_string()),
        None,
    )
    .with_timeout(Duration::from_millis(50));

    let v = router.route_prompt("some prompt").await;
    assert_eq!(v.tier, ModelTier::Standard);
    assert_eq!(v.model, "default-model");
    assert!(v.fallback);
}

#[tokio::test]
async fn test_route_prompt_with_loopback_mock_server() {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let server_handle = tokio::task::spawn_blocking(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            let body = r#"{"answers":{"tier":{"type":"choice","choice":"smol","confidence":0.92}}}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    });

    let router = ModelRouter::new(
        &format!("http://{addr}"),
        "clef-flash",
        "standard-model".to_string(),
        Some("smol-model".to_string()),
        None,
    );

    let verdict = router.route_prompt("fix simple typo").await;
    assert_eq!(verdict.tier, ModelTier::Smol);
    assert_eq!(verdict.model, "smol-model");
    assert_eq!(verdict.confidence, Some(0.92));
    assert!(!verdict.fallback);

    let _ = server_handle.await;
}

#[tokio::test]
async fn test_route_prompt_timeout_falls_back() {
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let server_handle = tokio::task::spawn_blocking(move || {
        if let Ok((_stream, _)) = listener.accept() {
            std::thread::sleep(Duration::from_millis(100));
        }
    });

    let router = ModelRouter::new(
        &format!("http://{addr}"),
        "clef-flash",
        "standard-model".to_string(),
        Some("smol-model".to_string()),
        None,
    )
    .with_timeout(Duration::from_millis(20));

    let verdict = router.route_prompt("fix simple typo").await;
    assert_eq!(verdict.tier, ModelTier::Standard);
    assert_eq!(verdict.model, "standard-model");
    assert!(verdict.fallback);

    let _ = server_handle.await;
}

#[test]
fn test_model_tier_display_and_from_config() {
    assert_eq!(ModelTier::Smol.to_string(), "smol");
    assert_eq!(ModelTier::Standard.to_string(), "standard");
    assert_eq!(ModelTier::Slow.to_string(), "slow");

    let mut config = Config::default();
    config
        .models
        .insert("judge".to_string(), "ollama/clef-flash".to_string());
    config
        .models
        .insert("smol".to_string(), "ollama/qwen2.5-coder:7b".to_string());
    config.models.insert("slow".to_string(), "openai/o3-mini".to_string());

    let router = ModelRouter::from_config(&config).unwrap();
    assert_eq!(router.smol.as_deref(), Some("ollama/qwen2.5-coder:7b"));
    assert_eq!(router.slow.as_deref(), Some("openai/o3-mini"));
}
