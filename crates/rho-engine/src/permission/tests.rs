mod bash;
mod eval;
mod hook;
mod matcher;
pub(crate) mod mock;
mod path;
mod policy;
mod prompt;

use rho_test_support::{EnvVarGuard, TestDir};

/// Points the rho config directory at a fresh scratch directory until the guard drops.
pub(crate) fn config_home() -> (TestDir, EnvVarGuard) {
    let home = TestDir::new();
    let env = EnvVarGuard::set("RHO_HOME", &home.to_string_lossy());
    (home, env)
}
