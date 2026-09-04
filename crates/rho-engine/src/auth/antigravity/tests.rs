use super::*;

#[test]
fn stable_project_id_is_uuid_shaped_and_deterministic() {
    let a = stable_project_id("user@example.com");
    let b = stable_project_id("user@example.com");
    let c = stable_project_id("other@example.com");
    assert_eq!(a, b);
    assert_ne!(a, c);
    let parts: Vec<_> = a.split('-').collect();
    assert_eq!(parts.len(), 5);
    assert_eq!(parts[0].len(), 8);
    assert_eq!(parts[1].len(), 4);
}
