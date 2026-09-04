use super::Theme;
use super::builtin::builtin_themes;
use super::definition::ThemeDef;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeMetadata {
    pub name: String,
    pub description: String,
    pub is_light: bool,
    pub is_custom: bool,
}

#[derive(Debug, Clone)]
pub struct ThemeRegistry {
    themes: BTreeMap<String, (ThemeMetadata, Theme)>,
}

impl Default for ThemeRegistry {
    fn default() -> Self {
        Self::new(None)
    }
}

impl ThemeRegistry {
    pub fn new(config_dir: Option<&Path>) -> Self {
        let mut registry = Self {
            themes: BTreeMap::new(),
        };

        for builtin in builtin_themes() {
            let theme = builtin.to_theme();
            let meta = ThemeMetadata {
                name: builtin.name.to_string(),
                description: builtin.description.to_string(),
                is_light: builtin.is_light,
                is_custom: false,
            };
            registry.themes.insert(builtin.name.to_string(), (meta, theme));
        }

        if let Some((default_meta, default_theme)) = registry.themes.get("default").cloned() {
            let mut ansi_meta = default_meta;
            ansi_meta.name = "ansi".to_string();
            registry.themes.insert("ansi".to_string(), (ansi_meta, default_theme));
        }

        if let Some((cat_meta, cat_theme)) = registry.themes.get("catppuccin").cloned() {
            let mut mocha_meta = cat_meta;
            mocha_meta.name = "catppuccin-mocha".to_string();
            registry
                .themes
                .insert("catppuccin-mocha".to_string(), (mocha_meta, cat_theme));
        }

        if let Some(dir) = config_dir {
            registry.load_custom_themes(&dir.join("themes"));
        }

        registry
    }

    fn load_custom_themes(&mut self, themes_dir: &Path) {
        let Ok(entries) = std::fs::read_dir(themes_dir) else {
            return;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
                continue;
            }

            let Some(file_stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };

            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };

            let Ok(def) = toml::from_str::<ThemeDef>(&content) else {
                continue;
            };

            let name = def.name.clone().unwrap_or_else(|| file_stem.to_string());
            let description = def
                .description
                .clone()
                .unwrap_or_else(|| format!("Custom theme ({file_stem})"));
            let is_light = def.is_light;
            let theme = def.into_theme(&name);

            let meta = ThemeMetadata {
                name: name.clone(),
                description,
                is_light,
                is_custom: true,
            };

            self.themes.insert(name, (meta, theme));
        }
    }

    pub fn get(&self, name: &str) -> Option<&Theme> {
        let normalized = name.trim().to_ascii_lowercase();
        self.themes.get(&normalized).map(|(_, theme)| theme)
    }

    pub fn metadata(&self, name: &str) -> Option<&ThemeMetadata> {
        let normalized = name.trim().to_ascii_lowercase();
        self.themes.get(&normalized).map(|(meta, _)| meta)
    }

    pub fn list(&self) -> Vec<&ThemeMetadata> {
        let mut list = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for name in [
            "default",
            "catppuccin",
            "nord",
            "tokyo-night",
            "dracula",
            "gruvbox",
            "monokai",
            "one-dark",
            "solarized-dark",
            "catppuccin-latte",
        ] {
            if let Some((meta, _)) = self.themes.get(name) {
                list.push(meta);
                seen.insert(name.to_string());
            }
        }

        for (name, (meta, _)) in &self.themes {
            if !seen.contains(name) && name != "ansi" && name != "catppuccin-mocha" {
                list.push(meta);
                seen.insert(name.clone());
            }
        }

        list
    }

    pub fn contains(&self, name: &str) -> bool {
        let normalized = name.trim().to_ascii_lowercase();
        self.themes.contains_key(&normalized)
    }
}
