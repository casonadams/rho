use super::*;

#[test]
fn test_template_frontmatter() {
    let content = "---\ndescription: Review staged changes\nargument-hint: \"<focus>\"\n---\nReview changes with focus on ${1:-correctness} and all: $@";
    let tmpl = PromptTemplate::parse("review", content, "user");
    assert_eq!(tmpl.metadata.name, "review");
    assert_eq!(tmpl.metadata.description, Some("Review staged changes".to_string()));
    assert_eq!(tmpl.metadata.argument_hint, Some("<focus>".to_string()));
}

#[test]
fn test_template_defaults_and_expansion() {
    let content = "---\ndescription: Review staged changes\nargument-hint: \"<focus>\"\n---\nReview changes with focus on ${1:-correctness} and all: $@";
    let tmpl = PromptTemplate::parse("review", content, "user");
    let expanded = tmpl.expand(&["security"]);
    assert_eq!(expanded, "Review changes with focus on security and all: security");
    let expanded_default = tmpl.expand(&[]);
    assert_eq!(expanded_default, "Review changes with focus on correctness and all: ");
}

#[test]
fn test_positional_and_slice_parameters() {
    let content = "Args: $1, $2, slice: ${@:2} from $@";
    let tmpl = PromptTemplate::parse("args", content, "project");
    let expanded = tmpl.expand(&["one", "two", "three", "four"]);
    assert_eq!(
        expanded,
        "Args: one, two, slice: two three four from one two three four"
    );
}

#[test]
fn expand_resolves_positional_default_and_slice_patterns() {
    let cases: &[(&str, &[&str], &str)] = &[
        ("${1}", &["a", "b"], "a"),
        ("${3}", &["a"], ""),
        ("${@}", &["a", "b"], "a b"),
        ("${ARGUMENTS}", &["a", "b"], "a b"),
        ("${2:-dflt}", &["a"], "dflt"),
        ("${2:-dflt}", &["a", " ", "c"], "dflt"),
        ("${ARGUMENTS:-none}", &[], "none"),
        ("${ARGUMENTS:-none}", &["x", "y"], "x y"),
        ("${2:-dflt}", &["a", "b"], "b"),
        ("${@:2:1}", &["a", "b", "c"], "b"),
        ("${@:5}", &["a"], ""),
        ("$1$2 $ARGUMENTS", &["x", "y"], "xy x y"),
        ("path ${HOME} end", &["a"], "path ${HOME} end"),
        ("${HOME} ${1}", &["a"], "${HOME} a"),
        ("${@:-none}", &[], "none"),
        ("${@:-none}", &["x"], "x"),
        ("open ${1", &["a"], "open ${1"),
    ];
    for (body, args, expected) in cases {
        let tmpl = PromptTemplate::parse("t", body, "user");
        assert_eq!(tmpl.expand(args), *expected, "{body}");
    }
}
