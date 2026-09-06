use super::*;

#[test]
fn test_parse_acceptance_criteria_examples() {
    let cases = [
        ("foo", ("rho-plugin-foo", DEFAULT_GITHUB_ORG, "rho-plugin-foo", None)),
        (
            "foo@1.0.0",
            ("rho-plugin-foo", DEFAULT_GITHUB_ORG, "rho-plugin-foo", Some("1.0.0")),
        ),
        ("org/repo", ("repo", "org", "repo", None)),
        ("org/repo@v1.0.0", ("repo", "org", "repo", Some("v1.0.0"))),
        ("https://github.com/org/repo", ("repo", "org", "repo", None)),
    ];
    for (input, (name, owner, repo, tag)) in cases {
        let spec = PluginSpec::parse(input).unwrap();
        assert_eq!(
            (
                spec.name.as_str(),
                spec.owner.as_str(),
                spec.repo.as_str(),
                spec.tag.as_deref()
            ),
            (name, owner, repo, tag)
        );
    }
}

#[test]
fn test_parse_bare_short_name() {
    let spec = PluginSpec::parse("permission").unwrap();
    let actual = (
        spec.name.as_str(),
        spec.owner.as_str(),
        spec.repo.as_str(),
        spec.executable_name.as_str(),
        spec.tag.as_deref(),
    );
    assert_eq!(
        actual,
        (
            "rho-plugin-permission",
            DEFAULT_GITHUB_ORG,
            "rho-plugin-permission",
            "rho-plugin-permission",
            None
        )
    );
    assert_eq!(
        (spec.github_repo().as_str(), spec.short_name()),
        ("casonadams/rho-plugin-permission", "permission")
    );
}

#[test]
fn test_parse_bare_prefixed_name() {
    let spec = PluginSpec::parse("rho-plugin-git").unwrap();
    let actual = (
        spec.name.as_str(),
        spec.owner.as_str(),
        spec.repo.as_str(),
        spec.executable_name.as_str(),
        spec.tag.as_deref(),
        spec.short_name(),
    );
    assert_eq!(
        actual,
        (
            "rho-plugin-git",
            DEFAULT_GITHUB_ORG,
            "rho-plugin-git",
            "rho-plugin-git",
            None,
            "git"
        )
    );
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
    let cases = [
        (
            "casonadams/rho-plugin-permission",
            ("rho-plugin-permission", "casonadams", "rho-plugin-permission", None),
        ),
        (
            "custom-org/custom-plugin@2.0.0",
            ("custom-plugin", "custom-org", "custom-plugin", Some("2.0.0")),
        ),
        (
            "custom-org/custom-plugin/",
            ("custom-plugin", "custom-org", "custom-plugin", None),
        ),
    ];
    for (input, (name, owner, repo, tag)) in cases {
        let spec = PluginSpec::parse(input).unwrap();
        let actual = (
            spec.name.as_str(),
            spec.owner.as_str(),
            spec.repo.as_str(),
            spec.tag.as_deref(),
        );
        assert_eq!(actual, (name, owner, repo, tag));
    }
}

#[test]
fn test_parse_github_url_standard() {
    for url in [
        "https://github.com/casonadams/rho-plugin-permission",
        "https://github.com/casonadams/rho-plugin-permission.git",
        "https://github.com/casonadams/rho-plugin-permission/",
    ] {
        let spec = PluginSpec::parse(url).unwrap();
        assert_eq!(
            (spec.name.as_str(), spec.owner.as_str(), spec.repo.as_str()),
            ("rho-plugin-permission", "casonadams", "rho-plugin-permission")
        );
    }
}

#[test]
fn test_parse_github_url_release_and_tree() {
    let cases = [
        ("https://github.com/org/repo@v1.0.0", "v1.0.0"),
        ("https://github.com/org/repo/releases/tag/v1.2.3", "v1.2.3"),
        ("https://github.com/org/repo/tree/v2.0.0", "v2.0.0"),
    ];
    for (url, tag) in cases {
        let spec = PluginSpec::parse(url).unwrap();
        assert_eq!(
            (spec.name.as_str(), spec.owner.as_str(), spec.tag.as_deref()),
            ("repo", "org", Some(tag))
        );
    }
}

#[test]
fn test_parse_invalid_empty_and_tag() {
    for input in ["", "   ", "@1.0.0"] {
        assert_eq!(PluginSpec::parse(input), Err(PluginSpecError::Empty));
    }
    assert_eq!(PluginSpec::parse("plugin@"), Err(PluginSpecError::EmptyTag));
}

#[test]
fn test_parse_invalid_urls_and_names() {
    for host in [
        "http://github.com/org/repo",
        "https://gitlab.com/org/repo",
        "git@github.com:org/repo.git",
        "ftp://github.com/org/repo",
    ] {
        assert!(PluginSpec::parse(host).is_err());
    }
    for invalid in [
        "org/repo/extra",
        "invalid name with spaces",
        "https://github.com/org/repo/extra/path",
        "rho-plugin-",
    ] {
        assert!(PluginSpec::parse(invalid).is_err());
    }
}

#[test]
fn test_from_str_and_display() {
    let spec: PluginSpec = "permission@0.1.0".parse().unwrap();
    assert_eq!(spec.name, "rho-plugin-permission");
    assert_eq!(spec.tag.as_deref(), Some("0.1.0"));
    assert_eq!(format!("{spec}"), "casonadams/rho-plugin-permission@0.1.0");

    let spec_no_tag: PluginSpec = "org/my-plugin".parse().unwrap();
    assert_eq!(format!("{spec_no_tag}"), "org/my-plugin");
}
