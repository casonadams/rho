use super::*;

struct FakeCorpus(Vec<(String, String)>);

impl Corpus for FakeCorpus {
    fn files(&self, family: Lang) -> Vec<(String, String)> {
        self.0
            .iter()
            .filter(|(p, _)| Lang::from_path(p) == family)
            .cloned()
            .collect()
    }
}

fn corpus(files: &[(&str, &str)]) -> FakeCorpus {
    FakeCorpus(
        files
            .iter()
            .map(|(p, t)| ((*p).to_string(), (*t).to_string()))
            .collect(),
    )
}

const BLOCK: &str = "let first_value = compute_first_value(input_data, 1);\nlet second_value = compute_second_value(input_data, 2);\nlet merged_values = merge_values(first_value, second_value);\nvalidate_merged_values(&merged_values, &configuration);\nstore_merged_values(&mut storage, merged_values, timestamp);\nnotify_listeners_about_change(&listeners, &storage);\n";

const FUNCTION: &str = "fn NAME(input: &str) -> usize {\n    let trimmed_input = input.trim().to_string();\n    let lowered_input = trimmed_input.to_lowercase();\n    let parts_of_input: Vec<&str> = lowered_input.split(',').collect();\n    parts_of_input.len()\n}\n";

fn run(files: &[(&str, &str)], corpus: &FakeCorpus) -> Vec<String> {
    let sources: Vec<Source> = files.iter().map(|(p, t)| Source::all_added(p, t)).collect();
    find(&sources, corpus)
        .iter()
        .map(|f| match f.end_line {
            Some(end) => format!("{}:{}-{end} {} {}", f.path, f.line, f.rule, f.message),
            None => format!("{}:{} {} {}", f.path, f.line, f.rule, f.message),
        })
        .collect()
}

#[test]
fn added_code_that_exists_in_the_corpus_is_reported_with_both_locations() {
    let added = format!("let a = 1;\n{BLOCK}");
    let existing = format!("// header\n{BLOCK}");

    let found = run(&[("new.rs", &added)], &corpus(&[("old.rs", &existing)]));

    assert_eq!(found, ["new.rs:2-7 duplicate-code duplicates old.rs:2 (6 lines)"]);
}

#[test]
fn formatting_comments_and_blank_lines_do_not_hide_duplicates() {
    let reformatted: String = BLOCK
        .lines()
        .map(|l| format!("        {}\n\n// note\n", l.replace(" = ", "   =   ")))
        .collect();

    let found = run(&[("new.rs", BLOCK)], &corpus(&[("old.rs", &reformatted)]));

    assert_eq!(found, ["new.rs:1-6 duplicate-code duplicates old.rs:1 (6 lines)"]);
}

#[test]
fn duplicates_among_added_files_are_reported_once_at_the_later_location() {
    let found = run(&[("a.rs", BLOCK), ("b.rs", BLOCK)], &corpus(&[]));

    assert_eq!(found, ["b.rs:1-6 duplicate-code duplicates a.rs:1 (6 lines)"]);
}

#[test]
fn a_block_repeated_within_one_file_and_over_existing_lines_is_reported() {
    let text = format!("{BLOCK}let separator = 1;\n{BLOCK}");
    let mut source = Source::all_added("a.rs", &text);
    source.lines.iter_mut().take(7).for_each(|l| l.added = false);

    let found = find(&[source], &corpus(&[]));

    assert_eq!(found.len(), 1);
    assert_eq!((found[0].line, found[0].end_line), (8, Some(13)));
    assert!(found[0].message.contains("a.rs:1 (6 lines)"), "{}", found[0].message);
}

#[test]
fn short_blocks_and_the_changed_file_in_the_corpus_are_not_duplicates() {
    let four: String = BLOCK.lines().take(4).map(|l| format!("{l}\n")).collect();
    let tiny = "let a = 1;\nlet b = 2;\nlet c = 3;\nlet d = 4;\nlet e = 5;\n";

    assert!(run(&[("a.rs", &four), ("b.rs", &four)], &corpus(&[("c.rs", &four)])).is_empty());
    assert!(run(&[("a.rs", tiny)], &corpus(&[("c.rs", tiny)])).is_empty());
    assert!(run(&[("a.rs", BLOCK)], &corpus(&[("a.rs", BLOCK)])).is_empty());
    assert!(run(&[("a.rs", BLOCK)], &corpus(&[("a.py", BLOCK)])).is_empty());
}

#[test]
fn findings_are_capped() {
    let build = |side: &str| -> String {
        (0..60)
            .map(|i| {
                format!(
                    "{}let marker_{i}_{side} = {i};\n",
                    BLOCK.lines().take(5).map(|l| format!("{l}\n")).collect::<String>()
                )
            })
            .collect()
    };
    let (added, existing) = (build("added"), build("other"));

    let found = run(&[("a.rs", &added)], &corpus(&[("b.rs", &existing)]));

    assert_eq!(found.len(), MAX_FINDINGS);
}

#[test]
fn functions_with_identical_bodies_are_reported_even_when_renamed() {
    let first = FUNCTION.replace("NAME", "first_name");
    let second = FUNCTION.replace("NAME", "second_name");
    let third = FUNCTION.replace("NAME", "third_name");

    let among = run(&[("a.rs", &first), ("b.rs", &second)], &corpus(&[]));
    let against = run(&[("c.rs", &third)], &corpus(&[("a.rs", &first)]));

    assert_eq!(
        among,
        ["b.rs:1 duplicate-function `second_name` has the same body as a.rs:1"]
    );
    assert_eq!(
        against,
        ["c.rs:1 duplicate-function `third_name` has the same body as a.rs:1"]
    );
}

#[test]
fn short_function_bodies_are_not_duplicates() {
    let small = "fn NAME(a: u8) -> u8 {\n    let doubled_value = a * 2;\n    doubled_value + 1\n}\n";
    let (a, b) = (small.replace("NAME", "one"), small.replace("NAME", "two"));

    assert!(run(&[("a.rs", &a), ("b.rs", &b)], &corpus(&[])).is_empty());
}
