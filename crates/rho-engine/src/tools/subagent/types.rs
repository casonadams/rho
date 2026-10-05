pub use rho_harness_core::args::subagent::{SubagentArgs, SubagentRole};

pub const DEFAULT_SUBAGENT_MAX_TURNS: usize = 8;
pub const MAX_SUBAGENT_TURNS: usize = 15;
pub const MAX_SUBAGENT_DEPTH: usize = 1;

#[must_use]
pub fn clamp_max_turns(requested: Option<usize>) -> usize {
    requested
        .unwrap_or(DEFAULT_SUBAGENT_MAX_TURNS)
        .clamp(1, MAX_SUBAGENT_TURNS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clamp_max_turns() {
        assert_eq!(clamp_max_turns(None), DEFAULT_SUBAGENT_MAX_TURNS);
        assert_eq!(clamp_max_turns(Some(0)), 1);
        assert_eq!(clamp_max_turns(Some(5)), 5);
        assert_eq!(clamp_max_turns(Some(20)), MAX_SUBAGENT_TURNS);
    }
}
