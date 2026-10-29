//! Stable, consumer-facing data contracts. No Minecraft or producer-plugin dependency.
use serde::{Deserialize, Serialize};
use std::fmt;

pub const API_VERSION: u32 = 1;

pub type MediaResult<T> = Result<T, MediaError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaError {
    InvalidInput(String),
    Io(String),
    NotFound(String),
    Conflict(String),
}
impl fmt::Display for MediaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(v) => write!(f, "invalid input: {v}"),
            Self::Io(v) => write!(f, "I/O failure: {v}"),
            Self::NotFound(v) => write!(f, "asset not found: {v}"),
            Self::Conflict(v) => write!(f, "conflict: {v}"),
        }
    }
}
impl std::error::Error for MediaError {}

/// Accept only normalized logical asset names, never filesystem paths.
#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct AssetKey {
    pub namespace: String,
    pub name: String,
}
impl AssetKey {
    pub fn new(namespace: impl Into<String>, name: impl Into<String>) -> MediaResult<Self> {
        let namespace = namespace.into();
        let name = name.into();
        if namespace.is_empty() || namespace.len() > 64
            || !namespace.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
        {
            return Err(MediaError::InvalidInput("invalid namespace".into()));
        }
        if name.is_empty() || name.len() > 256
            || name.split('/').any(|part| part.is_empty() || part == "." || part == "..")
            || !name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-' | b'.' | b'/'))
        {
            return Err(MediaError::InvalidInput("invalid asset name".into()));
        }
        Ok(Self { namespace, name })
    }
    pub fn qualified(&self) -> String {
        format!("{}:{}", self.namespace, self.name)
    }
    pub fn validated(&self) -> MediaResult<()> {
        Self::new(&self.namespace, &self.name).map(|_| ())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetKind {
    Sound,
    Image,
    Font,
    Texture,
    Model,
    ResourcePack,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AssetRecord {
    pub key: AssetKey,
    pub kind: AssetKind,
    pub sha256: String,
    pub size: u64,
    pub revision: u64,
}

/// Backend contract used by engines. Producers depend on this API, not vice versa.
pub trait AssetRepository: Send + Sync {
    fn publish(&self, key: AssetKey, kind: AssetKind, bytes: &[u8]) -> MediaResult<AssetRecord>;
    fn metadata(&self, key: &AssetKey) -> MediaResult<AssetRecord>;
    fn read(&self, key: &AssetKey) -> MediaResult<Vec<u8>>;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_identifiers_and_blocks_path_escape() {
        assert!(AssetKey::new("vsrender", "weapons/rifle.png").is_ok());
        assert!(AssetKey::new("my:plugin", "x").is_err());
        assert!(AssetKey::new("good", "../secret").is_err());
        assert!(AssetKey::new("good", "foo//bar").is_err());
        assert!(AssetKey::new("good", "CamelCase").is_err());
    }
}


/// An import item is owned by the caller; the batch is staged before publication.
#[derive(Debug, Clone)]
pub struct AssetImport {
    pub key: AssetKey,
    pub kind: AssetKind,
    pub bytes: Vec<u8>,
}

/// Optional extension for repositories that guarantee an all-or-nothing
/// catalog import. Never silently replace different existing content.
pub trait AtomicAssetRepository: AssetRepository {
    fn import_if_absent_or_identical(
        &self, inputs: &[AssetImport],
    ) -> MediaResult<Vec<AssetRecord>>;
}
