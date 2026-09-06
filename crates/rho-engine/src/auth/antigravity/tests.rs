use super::*;

#[test]
fn stable_project_id_is_deterministic() {
    let a = stable_project_id("user@example.com");
    let b = stable_project_id("user@example.com");
    let c = stable_project_id("other@example.com");
    assert_eq!(a, b);
    assert_ne!(a, c);
}

#[test]
fn stable_project_id_is_uuid_shaped() {
    let a = stable_project_id("user@example.com");
    let parts: Vec<_> = a.split('-').collect();
    assert_eq!((parts.len(), parts[0].len(), parts[1].len()), (5, 8, 4));
}
