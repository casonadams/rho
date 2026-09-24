use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeSecret([u8; 32]);

impl NodeSecret {
    pub fn generate() -> Self {
        let mut bytes = [0u8; 32];
        bytes[..16].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
        bytes[16..].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
        Self(bytes)
    }

    pub fn to_bytes(&self) -> [u8; 32] {
        self.0
    }

    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn node_id(&self) -> String {
        let mut s = String::with_capacity(64);
        for b in &self.0 {
            use std::fmt::Write;
            let _ = write!(s, "{b:02x}");
        }
        s
    }
}

pub fn default_secret_key_path() -> Result<PathBuf> {
    let config_dir = dirs::config_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join(".config")))
        .context("could not determine config directory")?;
    Ok(config_dir.join("rho").join("node_secret.key"))
}

pub fn load_or_generate_secret_key(path: &Path) -> Result<NodeSecret> {
    if path.exists() {
        let bytes =
            std::fs::read(path).with_context(|| format!("failed to read secret key from {}", path.display()))?;
        if bytes.len() == 32 {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&bytes);
            return Ok(NodeSecret::from_bytes(arr));
        }
    }

    let secret = NodeSecret::generate();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("failed to create dir {}", parent.display()))?;
    }
    std::fs::write(path, secret.to_bytes())
        .with_context(|| format!("failed to write secret key to {}", path.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }

    Ok(secret)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_and_reload_secret_key() {
        let temp_dir = std::env::temp_dir().join(format!("rho_key_test_{}", uuid::Uuid::new_v4()));
        let key_path = temp_dir.join("test.key");

        let key1 = load_or_generate_secret_key(&key_path).unwrap();
        assert!(key_path.exists());

        let key2 = load_or_generate_secret_key(&key_path).unwrap();
        assert_eq!(key1.node_id(), key2.node_id());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
