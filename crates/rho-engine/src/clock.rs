pub(crate) fn unix_now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

pub(crate) fn unix_now_secs() -> u64 {
    u64::try_from(chrono::Utc::now().timestamp()).unwrap_or(0)
}

pub(crate) fn expires_at_ms(expires_in_secs: i64, margin_ms: i64) -> i64 {
    unix_now_ms() + expires_in_secs * 1000 - margin_ms
}
