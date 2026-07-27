//! Resource handles and hot-reload hooks.
//!
//! In the Godot project, "assets" are files on disk (sprites, sounds, scenes).
//! The Rust side does not need to know about them — the GDScript shim
//! handles loading. This module is reserved for future work:
//!
//! - Cached resource metadata (size, last-modified, hash) so the GDScript
//!   side can show "this file changed" in the dev console.
//! - Stable handles that survive reloads (a path-based id, not a pointer).
//!
//! For now, this module is a placeholder that exposes a small utility.

use std::path::PathBuf;

/// Stable id for an asset, based on its `res://`-style path.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AssetId(pub String);

impl AssetId {
    pub fn new(path: impl Into<String>) -> Self {
        Self(path.into())
    }
    pub fn path(&self) -> &str {
        &self.0
    }
}

impl From<&str> for AssetId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}
impl From<String> for AssetId {
    fn from(s: String) -> Self {
        Self(s)
    }
}
impl From<PathBuf> for AssetId {
    fn from(p: PathBuf) -> Self {
        Self(p.to_string_lossy().into_owned())
    }
}

/// Metadata about an asset, useful for the dev console.
#[derive(Debug, Clone)]
pub struct AssetInfo {
    pub id: AssetId,
    pub size_bytes: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_id_round_trip() {
        let id = AssetId::new("res://sprites/hero.png");
        assert_eq!(id.path(), "res://sprites/hero.png");
    }
}
