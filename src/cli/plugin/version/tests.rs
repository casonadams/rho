use super::*;

#[test]
fn test_parse_simple_version() {
    let cases = [
        ("1.2.3", (1, 2, 3, None)),
        ("v0.3.0", (0, 3, 0, None)),
        ("v1.0.0-rc.1", (1, 0, 0, Some("rc.1".to_string()))),
    ];
    for (input, (maj, min, pat, pre)) in cases {
        let v = SimpleVersion::parse(input).unwrap();
        assert_eq!((v.major, v.minor, v.patch, v.prerelease), (maj, min, pat, pre));
    }
}

#[test]
fn test_is_newer_than() {
    let cases = [
        ("0.3.1", "0.3.0", true),
        ("0.4.0", "0.3.1", true),
        ("1.0.0", "0.4.0", true),
        ("1.0.0", "1.0.0-rc.1", true),
        ("0.3.0", "0.3.1", false),
        ("0.3.0", "0.3.0", false),
        ("1.0.0-rc.1", "1.0.0", false),
    ];
    for (v_new, v_old, expected) in cases {
        let n = SimpleVersion::parse(v_new).unwrap();
        let o = SimpleVersion::parse(v_old).unwrap();
        assert_eq!(n.is_newer_than(&o), expected);
    }
}

#[test]
fn test_is_update_available() {
    let cases = [
        ("0.3.0", "v0.3.1", true),
        ("v0.3.0", "0.4.0", true),
        ("1.0.0-rc.1", "1.0.0", true),
        ("0.3.1", "v0.3.1", false),
        ("v0.3.1", "0.3.1", false),
        ("0.4.0", "0.3.1", false),
        ("1.0.0", "1.0.0", false),
    ];
    for (current, latest, expected) in cases {
        assert_eq!(is_update_available(current, latest), expected);
    }
}

#[test]
fn test_is_update_available_unparseable_fallback() {
    assert!(is_update_available("custom-1", "custom-2"));
    assert!(!is_update_available("same", "same"));
}
