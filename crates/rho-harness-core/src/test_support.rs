pub(crate) use rho_test_support::TestDir;

pub(crate) fn temp_dir() -> TestDir {
    TestDir::with_prefix("rho_test_")
}
