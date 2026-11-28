use std::path::Path;

use toml_edit::{DocumentMut, Item, TableLike};

use super::types::FileConfig;
use crate::error::{AppError, Result};

const CONFIG_FILE_MODE: u32 = 0o600;

/// Applies `mutate` to the typed config, then copies only the dotted `keys` it may change into the
/// file's TOML document, so comments, ordering and keys this version does not know survive.
pub(super) fn update_config_file(
    path: &Path,
    keys: &[String],
    mutate: impl FnOnce(&mut FileConfig) -> Result<()>,
) -> Result<()> {
    let original = read_config_text(path)?;
    let mut file_config = parse_file_config(&original)?;
    let mut document: DocumentMut = original
        .parse()
        .map_err(|error| AppError::Config(format!("Failed to parse config file: {error}")))?;
    mutate(&mut file_config)?;
    let updated = serialize(&file_config)?;
    for key in keys {
        let path: Vec<&str> = key.split('.').collect();
        write_key(&mut document, &path, lookup(&updated, &path))?;
    }
    let text = document.to_string();
    parse_file_config(&text)?;
    crate::fs::write_atomic(path, text.as_bytes(), CONFIG_FILE_MODE)?;
    Ok(())
}

fn read_config_text(path: &Path) -> Result<String> {
    match std::fs::read_to_string(path) {
        Ok(content) => Ok(content),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(AppError::Config(format!(
            "Failed to read config file {}: {error}",
            path.display()
        ))),
    }
}

fn parse_file_config(content: &str) -> Result<FileConfig> {
    toml::from_str(content).map_err(|error| AppError::Config(format!("Failed to parse config file: {error}")))
}

fn serialize(file_config: &FileConfig) -> Result<toml::Value> {
    toml::Value::try_from(file_config).map_err(|error| AppError::Config(format!("Failed to serialize config: {error}")))
}

fn lookup<'a>(value: &'a toml::Value, path: &[&str]) -> Option<&'a toml::Value> {
    path.iter().try_fold(value, |value, key| value.get(key))
}

fn write_key(document: &mut DocumentMut, path: &[&str], value: Option<&toml::Value>) -> Result<()> {
    let Some((leaf, parents)) = path.split_last() else {
        return Ok(());
    };
    let Some(table) = parent_table(document.as_table_mut(), parents, value.is_some())? else {
        return Ok(());
    };
    for alias in alias_keys(table, path) {
        table.remove(&alias);
    }
    match value {
        Some(value) => set_value(table, leaf, to_edit_value(value)?),
        None => {
            table.remove(leaf);
        }
    }
    Ok(())
}

fn parent_table<'a>(
    mut table: &'a mut dyn TableLike,
    parents: &[&str],
    create: bool,
) -> Result<Option<&'a mut dyn TableLike>> {
    for key in parents {
        if table.get(key).is_none() {
            if !create {
                return Ok(None);
            }
            let mut child = toml_edit::Table::new();
            child.set_implicit(true);
            table.insert(key, Item::Table(child));
        }
        table = table
            .get_mut(key)
            .and_then(Item::as_table_like_mut)
            .ok_or_else(|| AppError::Config(format!("Config key {key} is not a table")))?;
    }
    Ok(Some(table))
}

/// Sibling keys that deserialize into the same field as `path` (serde aliases such as
/// `thinking_hidden` for `hide_thinking`); keeping one would make the saved file a duplicate field.
fn alias_keys(table: &dyn TableLike, path: &[&str]) -> Vec<String> {
    let Some((leaf, parents)) = path.split_last() else {
        return Vec::new();
    };
    let baseline = probe(parents, None, path).flatten();
    table
        .iter()
        .filter(|&(key, item)| {
            key != *leaf && probe(parents, Some((key, item)), path).is_some_and(|value| value != baseline)
        })
        .map(|(key, _)| key.to_string())
        .collect()
}

/// The value at `path` after deserializing a file holding only `entry` under `parents`, or `None`
/// when that file does not parse.
fn probe(parents: &[&str], entry: Option<(&str, &Item)>, path: &[&str]) -> Option<Option<toml::Value>> {
    let mut document = DocumentMut::new();
    let table = parent_table(document.as_table_mut(), parents, true).ok()??;
    if let Some((key, item)) = entry {
        table.insert(key, item.clone());
    }
    let file_config = parse_file_config(&document.to_string()).ok()?;
    let value = serialize(&file_config).ok()?;
    Some(lookup(&value, path).cloned())
}

fn set_value(table: &mut dyn TableLike, key: &str, value: toml_edit::Value) {
    match table.get_mut(key).and_then(Item::as_value_mut) {
        Some(existing) => {
            let decor = existing.decor().clone();
            *existing = value;
            *existing.decor_mut() = decor;
        }
        None => {
            table.insert(key, Item::Value(value));
        }
    }
}

fn to_edit_value(value: &toml::Value) -> Result<toml_edit::Value> {
    value
        .to_string()
        .parse()
        .map_err(|error| AppError::Config(format!("Failed to serialize config value: {error}")))
}
