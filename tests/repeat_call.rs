#[cfg(test)]
mod tests {
    use rho::engine::repeat::{REPEATED_CALL_MESSAGE, RepeatedCallTracker};
    use serde_json::{Value, json};

    type Scenario = (&'static str, Vec<(&'static str, Value)>, Vec<bool>);

    fn scenarios() -> Vec<Scenario> {
        vec![
            (
                "repeat_hook",
                vec![
                    ("bash", json!({"command": "printf x >> marker"})),
                    ("bash", json!({"command": "  printf   x   >>   marker  "})),
                    ("bash", json!({"command": "printf x >> marker"})),
                ],
                vec![true, true, false],
            ),
            (
                "repeat_consecutive",
                vec![
                    ("write", json!({"path": "out.txt", "content": "same"})),
                    ("write", json!({"path": "out.txt", "content": "same"})),
                    ("write", json!({"path": "out.txt", "content": "same"})),
                ],
                vec![true, true, false],
            ),
            (
                "repeat_reset",
                vec![
                    ("read", json!({"path": "missing"})),
                    ("read", json!({"path": "missing"})),
                    ("read", json!({"path": "other"})),
                    ("read", json!({"path": "missing"})),
                ],
                vec![true, true, true, true],
            ),
            (
                "repeat_undo_steps_back",
                vec![
                    ("undo", json!({"path": "out.txt"})),
                    ("undo", json!({"path": "out.txt"})),
                    ("undo", json!({"path": "out.txt"})),
                    ("undo", json!({"path": "out.txt"})),
                ],
                vec![true, true, true, true],
            ),
            (
                "repeat_wait_for_each_job",
                vec![
                    ("wait", json!({})),
                    ("wait", json!({})),
                    ("wait", json!({})),
                    ("wait", json!({})),
                ],
                vec![true, true, true, true],
            ),
        ]
    }

    #[test]
    fn repeated_calls_are_blocked_after_three_consecutive_identical_attempts() {
        for (name, calls, expect_ok) in scenarios() {
            let dir = rho_test_support::TestDir::with_prefix(&format!("{name}_"));
            let mut tracker = RepeatedCallTracker::new(&dir);
            for ((tool, arguments), ok) in calls.iter().zip(expect_ok) {
                let result = tracker.check(tool, arguments);
                if ok {
                    assert!(result.is_ok(), "{name}");
                } else {
                    assert_eq!(result.unwrap_err(), REPEATED_CALL_MESSAGE, "{name}");
                }
            }
        }
    }
}
