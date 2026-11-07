use std::path::PathBuf;

pub(crate) fn temp_dir(label: &str) -> PathBuf {
    tempfile::Builder::new()
        .prefix(&format!("rho-{label}-"))
        .tempdir()
        .unwrap()
        .keep()
}

pub(crate) fn temp_auth_store() -> (tempfile::TempDir, crate::auth::AuthStore) {
    let dir = tempfile::tempdir().unwrap();
    let auth_store = crate::auth::AuthStore::load(dir.path().join("auth.json")).unwrap();
    (dir, auth_store)
}

pub(crate) fn write_file(path: &std::path::Path, contents: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

pub(crate) fn assert_quota_cases(
    cases: &[(&str, &str, Option<&str>, Option<&str>)],
    now: chrono::DateTime<chrono::Utc>,
    parse: impl Fn(&serde_json::Value, Option<&str>, chrono::DateTime<chrono::Utc>) -> Option<String>,
) {
    for (name, json, model, expected) in cases {
        let value: serde_json::Value = serde_json::from_str(json).unwrap();
        assert_eq!(parse(&value, *model, now), expected.map(str::to_string), "{name}");
    }
}
