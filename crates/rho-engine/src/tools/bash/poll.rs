use crate::mailbox::Mailbox;
use crate::tools::jobs::JobManager;
use crate::tools::types::ToolResult;

const REFUSAL: &str = "A background job is still running and its result arrives automatically. Call wait to block until it finishes, or end your turn. Do not poll with sleep.";

/// Commands that may follow a poll in the same line without making it real work.
const HARMLESS: &[&str] = &[
    "sleep", "pgrep", "pidof", "ps", "tail", "head", "cat", "echo", "printf", "date", "ls", "jobs", "rg", "grep", "wc",
];

/// Whether `command` only waits or checks on a process: it starts with `sleep`, `pgrep`, `pidof`,
/// `ps`, or `tail -f`, and whatever follows is just printing or filtering. A line that goes on to
/// run anything else (`sleep 1 && make`) is real work.
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

/// The refusal for a poll command while `owner` has a running background job or subagent task.
pub(super) fn refusal(owner: Option<&Mailbox>, command: &str) -> Option<ToolResult> {
    let owner = owner?;
    (is_poll_command(command) && JobManager::global().has_running(owner.id())).then(|| ToolResult::error(REFUSAL))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mailbox::Mailboxes;

    #[test]
    fn only_bare_polls_are_classified_as_polling() {
        let cases = [
            ("sleep 5", true),
            ("sleep 5; echo hi", true),
            ("sleep 30; tail -n 5 build.log", true),
            ("pgrep -f cargo", true),
            ("ps aux | rg x", true),
            ("tail -f log", true),
            ("sleep 1 && make", false),
            ("sleep 5; cargo test", false),
            ("cargo test", false),
            ("echo sleep", false),
            ("tail -n 20 log", false),
            ("ls | rg sleep", false),
            ("", false),
        ];
        for (command, expected) in cases {
            assert_eq!(is_poll_command(command), expected, "{command}");
        }
    }

    #[test]
    fn polls_are_refused_only_while_the_owner_has_running_work() {
        let inbox = Mailboxes::default().open("poll-owner", None);
        let jobs = JobManager::global();

        assert!(
            refusal(Some(&inbox), "sleep 5").is_none(),
            "nothing runs: sleeping is allowed"
        );
        jobs.register("poll-owner", "poll-job".into(), "cargo test".into());

        let refused = refusal(Some(&inbox), "sleep 5").unwrap();
        assert!(refused.is_error);
        assert!(refused.content.contains("Call wait"), "{}", refused.content);
        assert!(refusal(Some(&inbox), "sleep 1 && make").is_none());
        assert!(refusal(None, "sleep 5").is_none());

        jobs.update_status(
            "poll-job",
            crate::tools::jobs::JobStatus::Completed {
                exit_code: 0,
                output: String::new(),
            },
        );
        assert!(refusal(Some(&inbox), "sleep 5").is_none());
    }
}
