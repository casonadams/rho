use super::*;

#[test]
fn test_parse_simple_version() {
    let v = SimpleVersion::parse("1.2.3").unwrap();
    assert_eq!(v.major, 1);
    assert_eq!(v.minor, 2);
    assert_eq!(v.patch, 3);
    assert_eq!(v.prerelease, None);

    let v2 = SimpleVersion::parse("v0.3.0").unwrap();
    assert_eq!(v2.major, 0);
    assert_eq!(v2.minor, 3);
    assert_eq!(v2.patch, 0);

    let v3 = SimpleVersion::parse("v1.0.0-rc.1").unwrap();
    assert_eq!(v3.major, 1);
    assert_eq!(v3.minor, 0);
    assert_eq!(v3.patch, 0);
    assert_eq!(v3.prerelease, Some("rc.1".to_string()));
}

#[test]
fn test_is_newer_than() {
    let v1 = SimpleVersion::parse("0.3.0").unwrap();
    let v2 = SimpleVersion::parse("0.3.1").unwrap();
    let v3 = SimpleVersion::parse("0.4.0").unwrap();
    let v4 = SimpleVersion::parse("1.0.0").unwrap();
    let v_rc = SimpleVersion::parse("1.0.0-rc.1").unwrap();

    assert!(v2.is_newer_than(&v1));
    assert!(v3.is_newer_than(&v2));
    assert!(v4.is_newer_than(&v3));
    assert!(v4.is_newer_than(&v_rc));

    assert!(!v1.is_newer_than(&v2));
    assert!(!v1.is_newer_than(&v1));
    assert!(!v_rc.is_newer_than(&v4));
}

#[test]
fn test_is_update_available() {
    assert!(is_update_available("0.3.0", "v0.3.1"));
    assert!(is_update_available("v0.3.0", "0.4.0"));
    assert!(is_update_available("1.0.0-rc.1", "1.0.0"));

    assert!(!is_update_available("0.3.1", "v0.3.1"));
    assert!(!is_update_available("v0.3.1", "0.3.1"));
    assert!(!is_update_available("0.4.0", "0.3.1"));
    assert!(!is_update_available("1.0.0", "1.0.0"));
}

#[test]
fn test_is_update_available_unparseable_fallback() {
    assert!(is_update_available("custom-1", "custom-2"));
    assert!(!is_update_available("same", "same"));
}
