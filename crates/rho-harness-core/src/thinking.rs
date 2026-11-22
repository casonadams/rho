//! How much a model reasons before answering, as chosen by `/thinking` and `thinking_level`.

/// Levels in increasing order; each provider maps them onto its own request settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ThinkingLevel {
    Off,
    Minimal,
    Low,
    Medium,
    High,
    XHigh,
    Max,
}

impl ThinkingLevel {
    pub const ALL: [Self; 7] = [
        Self::Off,
        Self::Minimal,
        Self::Low,
        Self::Medium,
        Self::High,
        Self::XHigh,
        Self::Max,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Minimal => "minimal",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::XHigh => "xhigh",
            Self::Max => "max",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Off => "No reasoning",
            Self::Minimal => "Very brief reasoning (~1k tokens)",
            Self::Low => "Light reasoning (~2k tokens)",
            Self::Medium => "Moderate reasoning (~8k tokens)",
            Self::High => "Deep reasoning (~16k tokens)",
            Self::XHigh => "Extra-high reasoning (~32k tokens)",
            Self::Max => "Maximum reasoning",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        let name = name.trim();
        Self::ALL
            .into_iter()
            .find(|level| level.as_str().eq_ignore_ascii_case(name))
    }

    /// The level a stored `thinking_level` setting selects; `None` and unknown values mean off.
    pub fn from_setting(setting: Option<&str>) -> Self {
        setting.and_then(Self::parse).unwrap_or(Self::Off)
    }

    /// The stored form of this level; off is stored explicitly so it is not replaced by the default.
    pub fn setting(self) -> Option<String> {
        Some(self.as_str().to_string())
    }

    /// Validates a user-entered level and returns its stored form.
    pub fn parse_setting(value: &str) -> Result<Option<String>, String> {
        let names: Vec<&str> = Self::ALL.iter().map(|level| level.as_str()).collect();
        Self::parse(value).map(Self::setting).ok_or_else(|| {
            format!(
                "thinking level must be one of {} (got '{}')",
                names.join(", "),
                value.trim()
            )
        })
    }

    /// The next level, wrapping from `Max` to `Off`.
    #[must_use]
    pub fn next(self) -> Self {
        Self::ALL[(self.index() + 1) % Self::ALL.len()]
    }

    /// The previous level, wrapping from `Off` to `Max`.
    #[must_use]
    pub fn prev(self) -> Self {
        Self::ALL[(self.index() + Self::ALL.len() - 1) % Self::ALL.len()]
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|&level| level == self).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::ThinkingLevel;

    #[test]
    fn every_level_round_trips_through_its_name_and_setting() {
        for level in ThinkingLevel::ALL {
            assert_eq!(ThinkingLevel::parse(&level.as_str().to_uppercase()), Some(level));
            assert_eq!(ThinkingLevel::from_setting(level.setting().as_deref()), level);
        }
        assert_eq!(ThinkingLevel::parse("ultra"), None);
        assert_eq!(ThinkingLevel::from_setting(Some("ultra")), ThinkingLevel::Off);
        assert_eq!(ThinkingLevel::from_setting(None), ThinkingLevel::Off);
        assert_eq!(ThinkingLevel::Off.setting().as_deref(), Some("off"));
        assert_eq!(ThinkingLevel::parse_setting(" High "), Ok(Some("high".to_string())));
        assert_eq!(ThinkingLevel::parse_setting("off"), Ok(Some("off".to_string())));
        assert!(ThinkingLevel::parse_setting("hgih").unwrap_err().contains("got 'hgih'"));
    }

    #[test]
    fn cycling_wraps_in_both_directions() {
        assert_eq!(ThinkingLevel::Max.next(), ThinkingLevel::Off);
        assert_eq!(ThinkingLevel::Off.prev(), ThinkingLevel::Max);
        assert_eq!(ThinkingLevel::Low.next(), ThinkingLevel::Medium);
        assert_eq!(ThinkingLevel::Low.prev(), ThinkingLevel::Minimal);
    }
}
