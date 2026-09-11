//! The project registry.
//!
//! Machine-specific state, kept under `%LOCALAPPDATA%\AurumStudio` per the
//! design. Nothing here is project data: the registry remembers *where*
//! projects are, never anything about them, which is why a registered project
//! stays portable and why losing the registry loses only convenience.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// One registered project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// The project's display name, from its `aurum.toml`.
    pub name: String,
    /// The absolute project root.
    pub path: PathBuf,
    /// When it was last opened, as an opaque stamp the caller supplies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_opened: Option<String>,
}

/// The set of known projects.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Registry {
    #[serde(default = "schema_version")]
    pub schema_version: i64,
    #[serde(default)]
    pub projects: Vec<Entry>,
}

fn schema_version() -> i64 {
    1
}

/// Why the registry could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryError {
    Io(String),
    Parse(String),
}

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(m) => write!(f, "{m}"),
            Self::Parse(m) => write!(f, "registry is not valid JSON: {m}"),
        }
    }
}

impl std::error::Error for RegistryError {}

impl Registry {
    /// Where the registry lives on this machine.
    ///
    /// `LOCALAPPDATA` on Windows, `XDG_DATA_HOME` or `~/.local/share`
    /// elsewhere, so the same code works on any machine Studio runs on.
    pub fn default_path() -> Option<PathBuf> {
        let base = if cfg!(windows) {
            std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
        } else {
            std::env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        }?;
        Some(base.join("AurumStudio").join("projects.json"))
    }

    /// The registry path to use, honouring `AURUM_STUDIO_HOME`.
    ///
    /// The override exists so tests and scripts can point at a scratch
    /// registry instead of the user's real one.
    pub fn resolve_path() -> Option<PathBuf> {
        if let Some(home) = std::env::var_os("AURUM_STUDIO_HOME") {
            return Some(PathBuf::from(home).join("projects.json"));
        }
        Self::default_path()
    }

    /// Load a registry, or start an empty one.
    ///
    /// A missing file is not an error — it is a first run. A *corrupt* file
    /// is reported, because silently starting empty would look like every
    /// project had been forgotten.
    pub fn load(path: &Path) -> Result<Self, RegistryError> {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                serde_json::from_str(&text).map_err(|e| RegistryError::Parse(e.to_string()))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self {
                schema_version: schema_version(),
                projects: Vec::new(),
            }),
            Err(e) => Err(RegistryError::Io(format!(
                "could not read '{}': {e}",
                path.display()
            ))),
        }
    }

    /// Write the registry, creating its directory if needed.
    pub fn save(&self, path: &Path) -> Result<(), RegistryError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                RegistryError::Io(format!("could not create '{}': {e}", parent.display()))
            })?;
        }
        let text =
            serde_json::to_string_pretty(self).map_err(|e| RegistryError::Parse(e.to_string()))?;
        std::fs::write(path, text)
            .map_err(|e| RegistryError::Io(format!("could not write '{}': {e}", path.display())))
    }

    /// Add or update a project. Returns whether it was newly added.
    ///
    /// Identity is the canonical path, not the name, so two projects sharing a
    /// name do not overwrite each other in either direction.
    pub fn register(&mut self, name: &str, path: &Path) -> bool {
        let canonical = path
            .canonicalize()
            .map(crate::project::clean_path)
            .unwrap_or_else(|_| path.to_path_buf());
        if let Some(existing) = self
            .projects
            .iter_mut()
            .find(|entry| entry.path == canonical)
        {
            // A renamed project updates in place rather than duplicating.
            existing.name = name.to_string();
            return false;
        }
        self.projects.push(Entry {
            name: name.to_string(),
            path: canonical,
            last_opened: None,
        });
        true
    }

    /// Forget a project by name or path. Returns whether anything was removed.
    pub fn remove(&mut self, name_or_path: &str) -> bool {
        let stored = normalise(name_or_path);
        let before = self.projects.len();
        self.projects
            .retain(|entry| entry.name != name_or_path && Some(&entry.path) != stored.as_ref());
        self.projects.len() != before
    }

    /// Find a project by name or by path.
    ///
    /// A path is normalised the same way registration normalises it, so
    /// looking up a project with the canonical form the filesystem hands back
    /// matches the entry that was stored.
    pub fn find(&self, name_or_path: &str) -> Option<&Entry> {
        let stored = normalise(name_or_path);
        self.projects
            .iter()
            .find(|entry| entry.name == name_or_path || Some(&entry.path) == stored.as_ref())
    }

    /// Entries whose directory no longer exists, so a caller can report them.
    pub fn missing(&self) -> Vec<&Entry> {
        self.projects
            .iter()
            .filter(|entry| !entry.path.is_dir())
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.projects.is_empty()
    }

    pub fn len(&self) -> usize {
        self.projects.len()
    }
}

/// Normalise a lookup key to the form entries are stored in, when it names a
/// path that exists. A name is not a path, so it is left alone.
fn normalise(name_or_path: &str) -> Option<PathBuf> {
    let path = Path::new(name_or_path);
    if name_or_path.is_empty() {
        return None;
    }
    Some(
        path.canonicalize()
            .map(crate::project::clean_path)
            .unwrap_or_else(|_| path.to_path_buf()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("aurum-registry-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_missing_file_is_a_first_run_not_an_error() {
        let dir = temp_dir("missing");
        let registry = Registry::load(&dir.join("nope.json")).unwrap();
        assert!(registry.is_empty());
        assert_eq!(registry.schema_version, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_corrupt_file_is_reported_rather_than_ignored() {
        let dir = temp_dir("corrupt");
        let path = dir.join("projects.json");
        std::fs::write(&path, "{ not json").unwrap();

        let error = Registry::load(&path).unwrap_err();
        assert!(matches!(error, RegistryError::Parse(_)), "{error:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn register_then_reload_round_trips() {
        let dir = temp_dir("roundtrip");
        let project = dir.join("my-project");
        std::fs::create_dir_all(&project).unwrap();
        let path = dir.join("nested/projects.json");

        let mut registry = Registry::load(&path).unwrap();
        assert!(registry.register("my-project", &project), "should be new");
        registry.save(&path).unwrap();

        let reloaded = Registry::load(&path).unwrap();
        assert_eq!(reloaded.len(), 1);
        assert_eq!(reloaded.projects[0].name, "my-project");
        assert_eq!(
            reloaded.projects[0].path,
            crate::project::clean_path(project.canonicalize().unwrap())
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn registering_the_same_path_twice_updates_rather_than_duplicates() {
        let dir = temp_dir("dup");
        let project = dir.join("p");
        std::fs::create_dir_all(&project).unwrap();
        let path = dir.join("projects.json");

        let mut registry = Registry::load(&path).unwrap();
        assert!(registry.register("first", &project));
        // Identity is the path, so a rename updates in place.
        assert!(!registry.register("renamed", &project));
        assert_eq!(registry.len(), 1);
        assert_eq!(registry.projects[0].name, "renamed");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn two_projects_may_share_a_name() {
        let dir = temp_dir("samename");
        let a = dir.join("a");
        let b = dir.join("b");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();

        let mut registry = Registry::default();
        registry.register("same", &a);
        registry.register("same", &b);
        assert_eq!(
            registry.len(),
            2,
            "a shared name must not collapse two projects"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn find_accepts_a_name_or_a_path() {
        let dir = temp_dir("find");
        let project = dir.join("p");
        std::fs::create_dir_all(&project).unwrap();

        let mut registry = Registry::default();
        registry.register("named", &project);
        assert!(registry.find("named").is_some());
        // Both the canonical form the filesystem returns and the cleaned form
        // that gets stored must find the entry.
        assert!(registry
            .find(&project.canonicalize().unwrap().display().to_string())
            .is_some());
        assert!(registry
            .find(
                &crate::project::clean_path(project.canonicalize().unwrap())
                    .display()
                    .to_string()
            )
            .is_some());
        assert!(registry.find("nothing").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn remove_reports_whether_it_did_anything() {
        let dir = temp_dir("remove");
        let project = dir.join("p");
        std::fs::create_dir_all(&project).unwrap();

        let mut registry = Registry::default();
        registry.register("named", &project);
        assert!(registry.remove("named"));
        assert!(
            !registry.remove("named"),
            "removing twice should report nothing done"
        );
        assert!(registry.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_directories_are_reported_but_kept() {
        let dir = temp_dir("missing-dirs");
        let present = dir.join("here");
        std::fs::create_dir_all(&present).unwrap();

        let mut registry = Registry::default();
        registry.register("here", &present);
        registry.register("gone", &dir.join("elsewhere"));
        assert_eq!(registry.missing().len(), 1);
        // Kept, because a project on a disconnected drive should return when
        // the drive does rather than being quietly forgotten.
        assert_eq!(registry.len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_default_path_is_under_the_studio_directory() {
        let path = Registry::default_path().expect("every supported platform has one");
        assert!(path.to_string_lossy().contains("AurumStudio"), "{path:?}");
        assert!(path.ends_with("projects.json"));
    }

    #[test]
    fn saving_creates_the_directory() {
        let dir = temp_dir("mkdir");
        let path = dir.join("a/b/c/projects.json");
        assert!(!path.parent().unwrap().exists());
        Registry::default().save(&path).unwrap();
        assert!(path.is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
