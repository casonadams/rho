//! Public data shapes returned by [`super::analyze_command_safety`].
//!
//! These types form the contract with the rest of the crate (see
//! [`super::analyzer`]) and with [`crate::tools::policy`], which consumes the
//! analysis to decide whether a shell command needs user approval.

use serde::{Deserialize, Serialize};

pub use rho_sdk::ui::RiskTier;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SafetyAnalysis {
    pub tier: RiskTier,
    pub reasons: Vec<String>,
    pub commands: Vec<String>,
    pub session_patterns: Option<Vec<String>>,
}
