use super::*;

#[test]
fn test_template_frontmatter_and_defaults() {
    let content = "---\ndescription: Review staged changes\nargument-hint: \"<focus>\"\n---\nReview changes with focus on ${1:-correctness} and all: $@";
    let tmpl = PromptTemplate::parse("review", content, "user");
    assert_eq!(tmpl.metadata.name, "review");
    assert_eq!(tmpl.metadata.description, Some("Review staged changes".to_string()));
    assert_eq!(tmpl.metadata.argument_hint, Some("<focus>".to_string()));

    // Test with arg
    let expanded = tmpl.expand(&["security"]);
    assert_eq!(expanded, "Review changes with focus on security and all: security");

    // Test with default
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
