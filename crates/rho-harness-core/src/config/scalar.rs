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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn booleans_accept_one_grammar_everywhere() {
        for (input, expected) in [
            ("true", true),
            (" TRUE ", true),
            ("1", true),
            ("yes", true),
            ("false", false),
            ("No", false),
            ("0", false),
        ] {
            assert_eq!(parse_bool("K", input).unwrap(), expected, "{input:?}");
        }
        for input in ["", "maybe", "2", "on"] {
            assert!(parse_bool("K", input).is_err(), "{input:?}");
        }
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
