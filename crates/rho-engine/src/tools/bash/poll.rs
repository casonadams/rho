use crate::mailbox::Mailbox;
use crate::tools::types::ToolResult;
use rho_harness_core::args::BashArgs;

const REFUSAL: &str = "A background job is still running and its result arrives automatically. Call wait to block until it finishes, or end your turn. Do not poll with sleep.";

/// Commands that may follow a poll in the same line without making it real work.
const HARMLESS: &[&str] = &[
    "sleep", "pgrep", "pidof", "ps", "tail", "head", "cat", "echo", "printf", "date", "ls", "jobs", "rg", "grep", "wc",
];

/// Whether `command` only waits or checks on a process (`sleep`, `pgrep`, `pidof`, `ps`, `tail -f`);
/// any chained work (`sleep 1 && make`) makes it real work.
pub(super) fn is_poll_command(command: &str) -> bool {
    let mut segments = command
        .split([';', '&', '|', '\n'])
        .map(str::trim)
        .filter(|segment| !segment.is_empty());
    let Some(first) = segments.next() else {
        return false;
    };
    let mut words = first.split_whitespace();
    let polls = match words.next() {
        Some("sleep" | "pgrep" | "pidof" | "ps") => true,
        Some("tail") => words.any(|w| w == "-f" || w == "-F" || w == "--follow"),
        _ => false,
    };
    polls && segments.all(|s| s.split_whitespace().next().is_some_and(|w| HARMLESS.contains(&w)))
}

/// The refusal for a foreground poll command while `owner` has a running background job or subagent task.
pub(super) fn refusal(owner: Option<&Mailbox>, args: &BashArgs) -> Option<ToolResult> {
    let owner = owner?;
    let foreground_poll = args.background != Some(true) && is_poll_command(&args.command);
    (foreground_poll && owner.has_running_jobs()).then(|| ToolResult::error(REFUSAL))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mailbox::Mailboxes;
    use test_case::test_case;

    fn foreground(command: &str) -> BashArgs {
        BashArgs {
            command: command.to_string(),
            timeout: None,
            background: None,
        }
    }

    #[test_case("sleep 5", true; "bare sleep")]
    #[test_case("sleep 5; echo hi", true; "sleep then echo")]
    #[test_case("sleep 30; tail -n 5 build.log", true; "sleep then tail")]
    #[test_case("pgrep -f cargo", true; "pgrep")]
    #[test_case("ps aux | rg x", true; "ps piped")]
    #[test_case("tail -f log", true; "tail follow")]
    #[test_case("sleep 1 && make", false; "sleep then make")]
    #[test_case("sleep 5; cargo test", false; "sleep then cargo test")]
    #[test_case("cargo test", false; "cargo test")]
    #[test_case("echo sleep", false; "echo sleep")]
    #[test_case("tail -n 20 log", false; "tail lines")]
    #[test_case("ls | rg sleep", false; "rg for sleep")]
    #[test_case("", false; "empty")]
    fn only_bare_polls_are_classified_as_polling(command: &str, expected: bool) {
        assert_eq!(is_poll_command(command), expected);
    }

    #[test]
    fn polls_are_refused_only_while_the_owner_has_running_work() {
        let inbox = Mailboxes::default().open(&"poll-owner".into(), None);
        let jobs = inbox.jobs();

        assert!(
            refusal(Some(&inbox), &foreground("sleep 5")).is_none(),
            "nothing runs: sleeping is allowed"
        );
        jobs.register(&"poll-owner".into(), "poll-job".into(), "cargo test".into());

        let refused = refusal(Some(&inbox), &foreground("sleep 5")).unwrap();
        assert!(refused.is_error);
        assert!(refused.content.contains("Call wait"), "{}", refused.content);
        assert!(refusal(Some(&inbox), &foreground("sleep 1 && make")).is_none());
        assert!(refusal(None, &foreground("sleep 5")).is_none());
        let background = BashArgs {
            background: Some(true),
            ..foreground("sleep 1; echo done")
        };
        assert!(
            refusal(Some(&inbox), &background).is_none(),
            "a background job is not a poll"
        );

        jobs.update_status(
            &"poll-job".into(),
            crate::tools::jobs::JobStatus::Completed {
                exit_code: 0,
                output: String::new(),
            },
        );
        assert!(refusal(Some(&inbox), &foreground("sleep 5")).is_none());
    }
}
