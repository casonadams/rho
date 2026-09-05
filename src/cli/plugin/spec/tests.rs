use super::*;

#[test]
fn test_parse_acceptance_criteria_examples() {
    let spec = PluginSpec::parse("foo").unwrap();
    assert_eq!(spec.name, "rho-plugin-foo");
    assert_eq!(spec.owner, DEFAULT_GITHUB_ORG);
    assert_eq!(spec.repo, "rho-plugin-foo");
    assert_eq!(spec.tag, None);

    let spec = PluginSpec::parse("foo@1.0.0").unwrap();
    assert_eq!(spec.name, "rho-plugin-foo");
    assert_eq!(spec.owner, DEFAULT_GITHUB_ORG);
    assert_eq!(spec.repo, "rho-plugin-foo");
    assert_eq!(spec.tag.as_deref(), Some("1.0.0"));

    let spec = PluginSpec::parse("org/repo").unwrap();
    assert_eq!(spec.name, "repo");
    assert_eq!(spec.owner, "org");
    assert_eq!(spec.repo, "repo");
    assert_eq!(spec.tag, None);

    let spec = PluginSpec::parse("org/repo@v1.0.0").unwrap();
    assert_eq!(spec.name, "repo");
    assert_eq!(spec.owner, "org");
    assert_eq!(spec.repo, "repo");
    assert_eq!(spec.tag.as_deref(), Some("v1.0.0"));

    let spec = PluginSpec::parse("https://github.com/org/repo").unwrap();
    assert_eq!(spec.name, "repo");
    assert_eq!(spec.owner, "org");
    assert_eq!(spec.repo, "repo");
    assert_eq!(spec.tag, None);
}

#[test]
fn test_parse_bare_short_name() {
    let spec = PluginSpec::parse("permission").unwrap();
    assert_eq!(spec.name, "rho-plugin-permission");
    assert_eq!(spec.owner, DEFAULT_GITHUB_ORG);
    assert_eq!(spec.repo, "rho-plugin-permission");
    assert_eq!(spec.executable_name, "rho-plugin-permission");
    assert_eq!(spec.tag, None);
    assert_eq!(spec.github_repo(), "casonadams/rho-plugin-permission");
    assert_eq!(spec.short_name(), "permission");
}

#[test]
fn test_parse_bare_prefixed_name() {
    let spec = PluginSpec::parse("rho-plugin-git").unwrap();
    assert_eq!(spec.name, "rho-plugin-git");
    assert_eq!(spec.owner, DEFAULT_GITHUB_ORG);
    assert_eq!(spec.repo, "rho-plugin-git");
    assert_eq!(spec.executable_name, "rho-plugin-git");
    assert_eq!(spec.tag, None);
    assert_eq!(spec.short_name(), "git");
}

#[test]
fn test_parse_pinned_versions() {
    let spec = PluginSpec::parse("permission@0.3.0").unwrap();
    assert_eq!(spec.name, "rho-plugin-permission");
    assert_eq!(spec.tag, Some("0.3.0".to_string()));

    let spec = PluginSpec::parse("rho-plugin-shell@v1.2.3").unwrap();
    assert_eq!(spec.name, "rho-plugin-shell");
    assert_eq!(spec.tag, Some("v1.2.3".to_string()));
}

#[test]
fn test_parse_github_slug() {
    let spec = PluginSpec::parse("casonadams/rho-plugin-permission").unwrap();
    assert_eq!(spec.name, "rho-plugin-permission");
    assert_eq!(spec.owner, "casonadams");
    assert_eq!(spec.repo, "rho-plugin-permission");
    assert_eq!(spec.tag, None);

    let spec = PluginSpec::parse("custom-org/custom-plugin@2.0.0").unwrap();
    assert_eq!(spec.name, "custom-plugin");
    assert_eq!(spec.owner, "custom-org");
    assert_eq!(spec.repo, "custom-plugin");
    assert_eq!(spec.tag, Some("2.0.0".to_string()));

    let spec = PluginSpec::parse("custom-org/custom-plugin/").unwrap();
    assert_eq!(spec.name, "custom-plugin");
    assert_eq!(spec.owner, "custom-org");
    assert_eq!(spec.repo, "custom-plugin");
    assert_eq!(spec.tag, None);
}

#[test]
fn test_parse_github_url() {
    let spec = PluginSpec::parse("https://github.com/casonadams/rho-plugin-permission").unwrap();
    assert_eq!(spec.name, "rho-plugin-permission");
    assert_eq!(spec.owner, "casonadams");
    assert_eq!(spec.repo, "rho-plugin-permission");
    assert_eq!(spec.tag, None);

    let spec = PluginSpec::parse("https://github.com/casonadams/rho-plugin-permission.git").unwrap();
    assert_eq!(spec.repo, "rho-plugin-permission");

    let spec = PluginSpec::parse("https://github.com/org/repo@v1.0.0").unwrap();
    assert_eq!(spec.name, "repo");
    assert_eq!(spec.owner, "org");
    assert_eq!(spec.repo, "repo");
    assert_eq!(spec.tag, Some("v1.0.0".to_string()));
}

#[test]
fn test_parse_invalid_inputs() {
    assert_eq!(PluginSpec::parse(""), Err(PluginSpecError::Empty));
    assert_eq!(PluginSpec::parse("   "), Err(PluginSpecError::Empty));
    assert_eq!(PluginSpec::parse("@1.0.0"), Err(PluginSpecError::Empty));
    assert_eq!(PluginSpec::parse("plugin@"), Err(PluginSpecError::EmptyTag));
    assert!(matches!(
        PluginSpec::parse("http://github.com/org/repo"),
        Err(PluginSpecError::InsecureHttp(_))
    ));
    assert!(matches!(
        PluginSpec::parse("https://gitlab.com/org/repo"),
        Err(PluginSpecError::UnsupportedHost(_))
    ));
    assert!(matches!(
        PluginSpec::parse("org/repo/extra"),
        Err(PluginSpecError::InvalidSlug(_))
    ));
    assert!(matches!(
        PluginSpec::parse("invalid name with spaces"),
        Err(PluginSpecError::InvalidName(_))
    ));
    assert!(matches!(
        PluginSpec::parse("git@github.com:org/repo.git"),
        Err(PluginSpecError::UnsupportedHost(_))
    ));
}

#[test]
fn test_from_str_trait() {
    let spec: PluginSpec = "permission@0.1.0".parse().unwrap();
    assert_eq!(spec.name, "rho-plugin-permission");
    assert_eq!(spec.tag.as_deref(), Some("0.1.0"));
}
