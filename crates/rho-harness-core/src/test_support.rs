pub(crate) use rho_test_support::TestDir;

pub(crate) fn temp_dir() -> TestDir {
    TestDir::with_prefix("rho_test_")
}

pub(crate) fn created_temp_dir() -> TestDir {
    temp_dir()
}
