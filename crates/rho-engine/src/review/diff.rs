//! Parses `git diff` output into the new-side content of each changed file.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    /// 1-based line number in the new file.
    pub no: usize,
    pub text: String,
    pub added: bool,
}

/// A file as it looks after the change; with `-U<large>` context `lines` is the whole file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedFile {
    pub path: String,
    pub is_new: bool,
    pub lines: Vec<DiffLine>,
}

#[derive(Default)]
struct Parser {
    done: Vec<ChangedFile>,
    path: Option<String>,
    is_new: bool,
    skip: bool,
    lines: Vec<DiffLine>,
    in_hunk: bool,
    next_no: usize,
}

/// Files with added or context lines; deleted and binary files are left out.
#[must_use]
pub fn parse(diff: &str) -> Vec<ChangedFile> {
    let mut parser = Parser::default();
    for line in diff.lines() {
        parser.feed(line);
    }
    parser.finish_file();
    parser.done
}

impl Parser {
    fn feed(&mut self, line: &str) {
        if self.in_hunk && self.hunk_line(line) {
            return;
        }
        if line.starts_with("diff --git ") {
            self.finish_file();
        } else if line.starts_with("@@") {
            self.next_no = hunk_start(line);
            self.in_hunk = true;
        } else {
            self.header(line);
        }
    }

    fn hunk_line(&mut self, line: &str) -> bool {
        let (marker, text) = line.split_at(line.chars().next().map_or(0, char::len_utf8));
        match marker {
            "+" | " " | "" => {
                self.lines.push(DiffLine {
                    no: self.next_no,
                    text: text.to_string(),
                    added: marker == "+",
                });
                self.next_no += 1;
                true
            }
            "-" | "\\" => true,
            _ => false,
        }
    }

    fn header(&mut self, line: &str) {
        if line == "new file mode" || line.starts_with("new file mode ") {
            self.is_new = true;
        } else if line.starts_with("deleted file mode")
            || line.starts_with("Binary files ")
            || line == "GIT binary patch"
        {
            self.skip = true;
        } else if let Some(path) = line.strip_prefix("+++ ") {
            self.path = path_after_prefix(path);
            self.skip |= self.path.is_none();
        } else if let Some(path) = line.strip_prefix("rename to ") {
            self.path = Some(unquote(path));
        }
    }

    fn finish_file(&mut self) {
        let lines = std::mem::take(&mut self.lines);
        let path = self.path.take();
        if let (Some(path), false, false) = (path, self.skip, lines.is_empty()) {
            self.done.push(ChangedFile {
                path,
                is_new: self.is_new,
                lines,
            });
        }
        self.is_new = false;
        self.skip = false;
        self.in_hunk = false;
    }
}

fn unquote(path: &str) -> String {
    path.trim_end_matches('\t').trim_matches('"').to_string()
}

fn path_after_prefix(path: &str) -> Option<String> {
    let path = unquote(path);
    if path == "/dev/null" {
        return None;
    }
    Some(path.strip_prefix("b/").map_or(path.clone(), str::to_string))
}

/// The new-side start line of `@@ -a,b +c,d @@`.
fn hunk_start(header: &str) -> usize {
    header
        .split(" +")
        .nth(1)
        .and_then(|rest| rest.split([',', ' ']).next())
        .and_then(|n| n.parse().ok())
        .unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn added(file: &ChangedFile) -> Vec<(usize, &str)> {
        file.lines
            .iter()
            .filter(|l| l.added)
            .map(|l| (l.no, l.text.as_str()))
            .collect()
    }

    #[test]
    fn a_new_file_has_every_line_added() {
        let diff = "diff --git a/n.rs b/n.rs\nnew file mode 100644\nindex 0000000..1111111\n--- /dev/null\n+++ b/n.rs\n@@ -0,0 +1,2 @@\n+fn a() {}\n+fn b() {}\n";
        let files = parse(diff);
        assert_eq!(files.len(), 1);
        assert!(files[0].is_new);
        assert_eq!(files[0].path, "n.rs");
        assert_eq!(added(&files[0]), [(1, "fn a() {}"), (2, "fn b() {}")]);
    }

    #[test]
    fn modified_files_number_added_lines_across_hunks() {
        let diff = "diff --git a/m.rs b/m.rs\n--- a/m.rs\n+++ b/m.rs\n@@ -1,3 +1,4 @@\n one\n-two\n+two!\n+two2\n three\n@@ -20,2 +21,3 @@\n x\n+y\n z\n";
        let files = parse(diff);
        assert!(!files[0].is_new);
        assert_eq!(added(&files[0]), [(2, "two!"), (3, "two2"), (22, "y")]);
        assert_eq!(files[0].lines.last().unwrap().no, 23);
    }

    #[test]
    fn renames_use_the_new_path_and_deletions_and_binaries_are_skipped() {
        let diff = "diff --git a/old.rs b/new.rs\nsimilarity index 90%\nrename from old.rs\nrename to new.rs\n--- a/old.rs\n+++ b/new.rs\n@@ -1 +1,2 @@\n a\n+b\ndiff --git a/gone.rs b/gone.rs\ndeleted file mode 100644\n--- a/gone.rs\n+++ /dev/null\n@@ -1 +0,0 @@\n-x\ndiff --git a/i.png b/i.png\nBinary files a/i.png and b/i.png differ\n";
        let files = parse(diff);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "new.rs");
        assert_eq!(added(&files[0]), [(2, "b")]);
    }

    #[test]
    fn added_lines_that_look_like_headers_and_missing_newlines_are_content() {
        let diff = "diff --git a/a.md b/a.md\nnew file mode 100644\n--- /dev/null\n+++ b/a.md\n@@ -0,0 +1,3 @@\n+--- title\n++++ b/x\n+last\n\\ No newline at end of file\n";
        let files = parse(diff);
        assert_eq!(added(&files[0]), [(1, "--- title"), (2, "+++ b/x"), (3, "last")]);
    }

    #[test]
    fn empty_input_has_no_files() {
        assert!(parse("").is_empty());
    }
}
