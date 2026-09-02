use super::*;

#[test]
fn api_key_serialization_matches_pi_schema() {
    let cred = StoredCredential::api_key("sk-ant-test-1234");
    let json = serde_json::to_string(&cred).unwrap();
    assert_eq!(json, r#"{"type":"api_key","key":"sk-ant-test-1234"}"#);

    let deserialized: StoredCredential = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized, cred);
    assert_eq!(deserialized.raw_secret(), "sk-ant-test-1234");
}

#[test]
fn oauth_credential_serialization_matches_pi_schema() {
    let cred = StoredCredential::oauth("access_123", Some("refresh_456".into()), Some(1725301234000));
    let json = serde_json::to_string(&cred).unwrap();
    assert!(json.contains(r#""type":"oauth""#));
    assert!(json.contains(r#""access":"access_123""#));
    assert!(json.contains(r#""refresh":"refresh_456""#));
    assert!(json.contains(r#""expires":1725301234000"#));

    let deserialized: StoredCredential = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized, cred);
    assert_eq!(deserialized.raw_secret(), "access_123");
}

#[test]
fn expiry_check_respects_threshold() {
    let past = chrono::Utc::now().timestamp_millis() - 10_000;
    let expired_cred = StoredCredential::oauth("tok", None, Some(past));
    assert!(expired_cred.is_expired(0));
    assert!(expired_cred.is_expired(60));

    let future = chrono::Utc::now().timestamp_millis() + 100_000;
    let valid_cred = StoredCredential::oauth("tok", None, Some(future));
    assert!(!valid_cred.is_expired(0));
    assert!(!valid_cred.is_expired(60));
    assert!(valid_cred.is_expired(200));

    let api_key = StoredCredential::api_key("key");
    assert!(!api_key.is_expired(3600));
}

#[test]
fn select_option_builder_sets_fields() {
    let opt = SelectOption::new("openai", "OpenAI").with_description("GPT models");
    assert_eq!(opt.id, "openai");
    assert_eq!(opt.label, "OpenAI");
    assert_eq!(opt.description.as_deref(), Some("GPT models"));
}
