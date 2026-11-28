//! Scalar parsing shared by environment overrides and `rho config`, so both accept the same syntax.

use crate::error::{AppError, Result};

pub(crate) fn parse_bool(name: &str, value: &str) -> Result<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" => Ok(true),
        "0" | "false" | "no" => Ok(false),
        _ => Err(AppError::Config(format!("{name} must be true or false"))),
    }
}

pub(crate) fn parse_positive<T>(name: &str, value: &str) -> Result<T>
where
    T: std::str::FromStr + Default + PartialEq,
{
    let parsed = value
        .trim()
        .parse::<T>()
        .map_err(|_| AppError::Config(format!("{name} must be a positive integer")))?;
    if parsed == T::default() {
        return Err(AppError::Config(format!("{name} must be greater than zero")));
    }
    Ok(parsed)
}

/// `off` and `0` disable session retention, which the config file stores as `0`.
pub(crate) fn parse_retention_days(name: &str, value: &str) -> Result<u32> {
    match value {
        "off" | "0" => Ok(0),
        _ => parse_positive(name, value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_case::test_case;

    #[test_case("true", true; "lowercase true")]
    #[test_case(" TRUE ", true; "padded uppercase true")]
    #[test_case("1", true; "one")]
    #[test_case("yes", true; "yes")]
    #[test_case("false", false; "lowercase false")]
    #[test_case("No", false; "mixed case no")]
    #[test_case("0", false; "zero")]
    fn booleans_accept_one_grammar_everywhere(input: &str, expected: bool) {
        assert_eq!(parse_bool("K", input).unwrap(), expected);
    }

    #[test_case(""; "empty")]
    #[test_case("maybe"; "maybe")]
    #[test_case("2"; "two")]
    #[test_case("on"; "on")]
    fn booleans_reject_other_spellings(input: &str) {
        assert!(parse_bool("K", input).is_err());
    }

    #[test]
    fn positive_integers_are_trimmed_and_zero_is_rejected_with_its_own_message() {
        assert_eq!(parse_positive::<usize>("LIMIT", " 25 ").unwrap(), 25);
        assert!(
            parse_positive::<u64>("LIMIT", "invalid")
                .unwrap_err()
                .to_string()
                .contains("positive integer")
        );
        assert!(
            parse_positive::<usize>("LIMIT", "0")
                .unwrap_err()
                .to_string()
                .contains("greater than zero")
        );
        assert!(parse_positive::<usize>("LIMIT", "-3").is_err());
    }
}
