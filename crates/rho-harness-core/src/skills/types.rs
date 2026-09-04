use std::fmt;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillMetadata {
    pub name: String,
    pub description: String,
    pub location: String,
}

/// Where a resolved skill came from; `Project` overrides `User`
/// for the same skill name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillOrigin {
    User,
    Project,
}

impl fmt::Display for SkillOrigin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::User => write!(f, "user"),
            Self::Project => write!(f, "project"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSkill {
    pub metadata: SkillMetadata,
    pub origin: SkillOrigin,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SkillResolutionPaths<'a> {
    pub project_dir: Option<&'a Path>,
    pub home_dir: Option<&'a Path>,
}
