pub use rho_harness_core::args::subagent::{SubagentArgs, SubagentRole};

pub const DEFAULT_SUBAGENT_TURNS: usize = 100;

#[must_use]
pub fn resolve_turns(requested: Option<usize>) -> usize {
    requested.unwrap_or(DEFAULT_SUBAGENT_TURNS).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turns_default_to_100_with_no_upper_cap() {
        assert_eq!(resolve_turns(None), DEFAULT_SUBAGENT_TURNS);
        assert_eq!(resolve_turns(Some(0)), 1);
        assert_eq!(resolve_turns(Some(500)), 500);
    }
}
