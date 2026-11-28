use super::*;
use test_case::test_case;

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

#[test_case("${1}", &["a", "b"], "a"; "braced positional")]
#[test_case("${3}", &["a"], ""; "missing positional is empty")]
#[test_case("${@}", &["a", "b"], "a b"; "braced all args")]
#[test_case("${ARGUMENTS}", &["a", "b"], "a b"; "braced arguments")]
#[test_case("${2:-dflt}", &["a"], "dflt"; "default for missing positional")]
#[test_case("${2:-dflt}", &["a", " ", "c"], "dflt"; "default for blank positional")]
#[test_case("${ARGUMENTS:-none}", &[], "none"; "default for no arguments")]
#[test_case("${ARGUMENTS:-none}", &["x", "y"], "x y"; "arguments override default")]
#[test_case("${2:-dflt}", &["a", "b"], "b"; "positional overrides default")]
#[test_case("${@:2:1}", &["a", "b", "c"], "b"; "slice with length")]
#[test_case("${@:5}", &["a"], ""; "slice past end is empty")]
#[test_case("$1$2 $ARGUMENTS", &["x", "y"], "xy x y"; "unbraced forms")]
#[test_case("path ${HOME} end", &["a"], "path ${HOME} end"; "unknown variable kept")]
#[test_case("${HOME} ${1}", &["a"], "${HOME} a"; "unknown variable kept beside positional")]
#[test_case("${@:-none}", &[], "none"; "all args default when empty")]
#[test_case("${@:-none}", &["x"], "x"; "all args override default")]
#[test_case("open ${1", &["a"], "open ${1"; "unclosed brace kept")]
fn expand_resolves_positional_default_and_slice_patterns(body: &str, args: &[&str], expected: &str) {
    let tmpl = PromptTemplate::parse("t", body, "user");
    assert_eq!(tmpl.expand(args), expected);
}
