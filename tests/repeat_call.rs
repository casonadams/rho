#[cfg(test)]
mod tests {
    use rho::engine::repeat::{REPEATED_CALL_MESSAGE, RepeatedCallHook};
    use serde_json::json;

    #[test]
    fn third_whitespace_normalized_mutation_is_blocked() {
        let dir = std::env::temp_dir().join(format!("repeat_hook_{}", uuid::Uuid::new_v4()));
        let mut tracker = RepeatedCallHook::new(&dir);
        let commands = [
            "printf x >> marker",
            "  printf   x   >>   marker  ",
            "printf x >> marker",
        ];

        assert!(tracker.check("bash", &json!({"command": commands[0]})).is_ok());
        assert!(tracker.check("bash", &json!({"command": commands[1]})).is_ok());
        assert_eq!(
            tracker.check("bash", &json!({"command": commands[2]})).unwrap_err(),
            REPEATED_CALL_MESSAGE
        );
    }

    #[test]
    fn consecutive_calls_count_toward_the_same_consecutive_threshold() {
        let dir = std::env::temp_dir().join(format!("repeat_consecutive_{}", uuid::Uuid::new_v4()));
        let mut tracker = RepeatedCallHook::new(&dir);
        let arguments = json!({"path": "out.txt", "content": "same"});

        assert!(tracker.check("write", &arguments).is_ok());
        assert!(tracker.check("write", &arguments).is_ok());
        assert_eq!(tracker.check("write", &arguments).unwrap_err(), REPEATED_CALL_MESSAGE);
    }

    #[test]
    fn changed_and_interleaved_calls_reset_consecutive_counter() {
        let dir = std::env::temp_dir().join(format!("repeat_reset_{}", uuid::Uuid::new_v4()));
        let mut tracker = RepeatedCallHook::new(&dir);

        assert!(tracker.check("read", &json!({"path": "missing"})).is_ok());
        assert!(tracker.check("read", &json!({"path": "missing"})).is_ok());
        assert!(tracker.check("read", &json!({"path": "other"})).is_ok());
        assert!(tracker.check("read", &json!({"path": "missing"})).is_ok());
    }
}
