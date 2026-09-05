#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimpleVersion {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub prerelease: Option<String>,
}

impl SimpleVersion {
    pub fn parse(s: &str) -> Option<Self> {
        let trimmed = s.trim().strip_prefix('v').unwrap_or(s.trim());
        let (version_part, prerelease) = match trimmed.split_once('-') {
            Some((v, p)) => (v, Some(p.to_string())),
            None => (trimmed, None),
        };
        let mut parts = version_part.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next().unwrap_or("0").parse().ok()?;
        let patch = parts.next().unwrap_or("0").parse().ok()?;
        Some(Self {
            major,
            minor,
            patch,
            prerelease,
        })
    }

    pub fn is_newer_than(&self, other: &Self) -> bool {
        let self_triple = (self.major, self.minor, self.patch);
        let other_triple = (other.major, other.minor, other.patch);
        if self_triple != other_triple {
            return self_triple > other_triple;
        }
        other.prerelease.is_some() && self.prerelease.is_none()
    }
}

pub fn is_update_available(current: &str, latest: &str) -> bool {
    let cur_trimmed = current.trim();
    let latest_trimmed = latest.trim();
    if cur_trimmed == latest_trimmed {
        return false;
    }
    match (SimpleVersion::parse(cur_trimmed), SimpleVersion::parse(latest_trimmed)) {
        (Some(c), Some(l)) => l.is_newer_than(&c),
        _ => cur_trimmed != latest_trimmed,
    }
}

#[cfg(test)]
#[path = "version/tests.rs"]
mod tests;
