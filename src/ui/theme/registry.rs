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
    fn insert_builtins(&mut self) {
        for builtin in builtin_themes() {
            let theme = builtin.to_theme();
            let meta = ThemeMetadata {
                name: builtin.name.to_string(),
                description: builtin.description.to_string(),
                is_light: builtin.is_light,
                is_custom: false,
            };
            self.themes.insert(builtin.name.to_string(), (meta, theme));
        }
        self.alias_theme("default", "ansi");
        self.alias_theme("catppuccin", "catppuccin-mocha");
    }

    fn alias_theme(&mut self, source: &str, target: &str) {
        if let Some((mut meta, theme)) = self.themes.get(source).cloned() {
            meta.name = target.to_string();
            self.themes.insert(target.to_string(), (meta, theme));
        }
    }

    pub fn new(config_dir: Option<&Path>) -> Self {
        let mut registry = Self {
            themes: BTreeMap::new(),
        };
        registry.insert_builtins();
        if let Some(dir) = config_dir {
            registry.load_custom_themes(&dir.join("themes"));
        }
        registry
    }

    fn try_load_theme_file(&mut self, path: &Path) {
        if path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
            return;
        }
        let Some(file_stem) = path.file_stem().and_then(|s| s.to_str()) else {
            return;
        };
        let Ok(content) = std::fs::read_to_string(path) else {
            return;
        };
        let Ok(def) = toml::from_str::<ThemeDef>(&content) else {
            return;
        };
        let name = def.name.clone().unwrap_or_else(|| file_stem.to_string());
        let description = def
            .description
            .clone()
            .unwrap_or_else(|| format!("Custom theme ({file_stem})"));
        let meta = ThemeMetadata {
            name: name.clone(),
            description,
            is_light: def.is_light,
            is_custom: true,
        };
        let theme = def.into_theme(&name);
        self.themes.insert(name, (meta, theme));
    }

    fn load_custom_themes(&mut self, themes_dir: &Path) {
        let Ok(entries) = std::fs::read_dir(themes_dir) else {
            return;
        };
        for entry in entries.flatten() {
            self.try_load_theme_file(&entry.path());
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
        let mut list = self.builtin_order();
        let seen: std::collections::HashSet<String> = list.iter().map(|meta| meta.name.clone()).collect();
        self.push_custom_themes(&mut list, &seen);
        list
    }

    fn builtin_order(&self) -> Vec<&ThemeMetadata> {
        const ORDERED_BUILTINS: [&str; 10] = [
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
        ];
        ORDERED_BUILTINS
            .iter()
            .filter_map(|name| self.themes.get(*name))
            .map(|(meta, _)| meta)
            .collect()
    }

    fn push_custom_themes<'a>(&'a self, list: &mut Vec<&'a ThemeMetadata>, seen: &std::collections::HashSet<String>) {
        for (name, (meta, _)) in &self.themes {
            if !seen.contains(name.as_str()) && name != "ansi" && name != "catppuccin-mocha" {
                list.push(meta);
            }
        }
    }

    pub fn contains(&self, name: &str) -> bool {
        let normalized = name.trim().to_ascii_lowercase();
        self.themes.contains_key(&normalized)
    }
}
