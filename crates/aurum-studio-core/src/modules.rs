//! The engine modules a project can turn on.
//!
//! `aurum.toml` has carried a `modules` list since the beginning, and until now
//! nothing read it. A setting that is parsed and then ignored is worse than no
//! setting: it looks like configuration and behaves like a comment, and a
//! project that names a module which does not exist gets no warning at all.
//!
//! So this does three things, in increasing order of usefulness. It knows which
//! modules the engine ships. It checks the configured names against that list
//! and against the engine checkout, so a typo is reported rather than silently
//! doing nothing. And it says what each module is for, so the list is a menu
//! rather than a puzzle.
//!
//! Choosing modules does not yet change what gets compiled: the extension is
//! one crate that depends on all of them, and narrowing that means feature
//! flags through the dependency graph. That is a real change and it is not
//! pretended otherwise here.

use std::path::{Path, PathBuf};

/// A module the engine ships.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Module {
    /// The crate name, which is what `aurum.toml` names.
    pub name: &'static str,
    pub description: &'static str,
}

/// Every module, in the order they are offered.
///
/// Held here rather than discovered from the engine checkout, because a project
/// has to be checkable when the checkout is somewhere else or absent — and
/// because a list read from a directory can only say a name exists, never what
/// it is for.
pub const MODULES: &[Module] = &[
    Module {
        name: "aurum-2d",
        description: "sprites, tilemaps, and 2D scene helpers",
    },
    Module {
        name: "aurum-3d",
        description: "3D scene helpers and camera rigs",
    },
    Module {
        name: "aurum-space",
        description: "flight, physics, and sector simulation",
    },
    Module {
        name: "aurum-vn",
        description: "visual-novel story, choices, and variables",
    },
    Module {
        name: "aurum-vr",
        description: "VR rig and interaction helpers",
    },
    Module {
        name: "aurum-text",
        description: "text layout and markup",
    },
    Module {
        name: "aurum-content",
        description: "content authoring: mesh, scene, animation, sprites, glTF",
    },
];

/// Where a configured module stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Named in the project, known to the engine, and present in the checkout.
    Enabled,
    /// Named in the project and known to the engine, but no crate was found.
    /// Only reported when the engine checkout could be read at all.
    Missing,
    /// Known to the engine, not named in the project.
    Available,
    /// Named in the project and not a module the engine ships.
    Unknown,
}

impl State {
    pub fn label(self) -> &'static str {
        match self {
            Self::Enabled => "enabled",
            Self::Missing => "missing",
            Self::Available => "available",
            Self::Unknown => "unknown",
        }
    }
}

/// One line of the report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub description: &'static str,
    pub state: State,
}

/// What a project's module list amounts to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub entries: Vec<Entry>,
    /// Whether the engine checkout could be read, which decides whether
    /// `Missing` is a claim this report is entitled to make.
    pub checkout_read: bool,
}

impl Report {
    pub fn with_state(&self, state: State) -> Vec<&Entry> {
        self.entries.iter().filter(|e| e.state == state).collect()
    }

    /// Names that are not modules the engine ships.
    pub fn unknown(&self) -> Vec<&str> {
        self.with_state(State::Unknown)
            .into_iter()
            .map(|entry| entry.name.as_str())
            .collect()
    }

    /// The closest known name to an unknown one, when there is one.
    ///
    /// A typo is the whole reason unknown names get reported, and a list of
    /// six candidates is not an answer to "did you mean". This is a plain edit
    /// distance, which is enough for names this short.
    pub fn suggestion(&self, name: &str) -> Option<&'static str> {
        MODULES
            .iter()
            .map(|module| (module.name, distance(name, module.name)))
            .filter(|(_, d)| *d <= 2)
            .min_by_key(|(_, d)| *d)
            .map(|(name, _)| name)
    }
}

/// Look at a project's module list.
pub fn report(configured: &[String], engine: Option<&Path>) -> Report {
    let checkout = engine.map(|root| root.join("crates"));
    let checkout_read = checkout.as_ref().is_some_and(|crates| crates.is_dir());

    let mut entries = Vec::new();
    for module in MODULES {
        let named = configured.iter().any(|name| name == module.name);
        let crate_present = checkout
            .as_ref()
            .filter(|_| checkout_read)
            .map(|crates| crates.join(module.name).join("Cargo.toml").is_file());

        let state = match (named, crate_present) {
            (true, Some(false)) => State::Missing,
            (true, _) => State::Enabled,
            (false, _) => State::Available,
        };
        entries.push(Entry {
            name: module.name.to_string(),
            description: module.description,
            state,
        });
    }

    // Names the engine does not ship, kept in the order the project listed
    // them so the report reads like the file.
    for name in configured {
        if !MODULES.iter().any(|module| module.name == name) {
            entries.push(Entry {
                name: name.clone(),
                description: "not a module this engine ships",
                state: State::Unknown,
            });
        }
    }

    Report {
        entries,
        checkout_read,
    }
}

/// The crate directory a module would live in.
pub fn crate_path(engine: &Path, name: &str) -> PathBuf {
    engine.join("crates").join(name)
}

/// Levenshtein distance, for suggesting what a typo meant.
///
/// Written out rather than taken from a crate: it is twenty lines, it runs on
/// names that are at most a dozen characters, and a dependency for this would
/// be a poor trade in a workspace that counts them.
fn distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    let mut current = vec![0usize; b.len() + 1];

    for (i, left) in a.iter().enumerate() {
        current[0] = i + 1;
        for (j, right) in b.iter().enumerate() {
            let substitution = previous[j] + usize::from(left != right);
            current[j + 1] = substitution.min(previous[j + 1] + 1).min(current[j] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    #[test]
    fn nothing_configured_means_everything_is_available() {
        let report = report(&[], None);
        assert_eq!(report.entries.len(), MODULES.len());
        assert!(report.entries.iter().all(|e| e.state == State::Available));
        assert!(report.unknown().is_empty());
    }

    #[test]
    fn a_configured_module_is_enabled() {
        let report = report(&names(&["aurum-2d"]), None);
        let entry = report
            .entries
            .iter()
            .find(|e| e.name == "aurum-2d")
            .expect("the module should be listed");
        assert_eq!(entry.state, State::Enabled);
        assert!(!entry.description.is_empty(), "a menu needs descriptions");
    }

    #[test]
    fn a_name_the_engine_does_not_ship_is_reported_as_unknown() {
        // The bug this exists for: a typo used to be accepted in silence and
        // behave as though the module were on.
        let report = report(&names(&["aurum-2dd"]), None);
        assert_eq!(report.unknown(), vec!["aurum-2dd"]);
        assert_eq!(report.suggestion("aurum-2dd"), Some("aurum-2d"));
    }

    #[test]
    fn unknown_names_are_listed_in_the_order_the_project_named_them() {
        let report = report(&names(&["zebra", "alpha"]), None);
        assert_eq!(report.unknown(), vec!["zebra", "alpha"]);
    }

    #[test]
    fn a_suggestion_is_only_offered_when_it_is_close() {
        let report = report(&[], None);
        assert_eq!(report.suggestion("aurum-2d"), Some("aurum-2d"));
        assert_eq!(report.suggestion("aurum-2dd"), Some("aurum-2d"));
        assert_eq!(
            report.suggestion("something-else-entirely"),
            None,
            "a distant name has no useful suggestion, and guessing would be worse than silence"
        );
    }

    #[test]
    fn a_module_with_no_crate_in_the_checkout_is_missing() {
        let root = std::env::temp_dir().join(format!("aurum-modules-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("crates").join("aurum-2d")).expect("mkdir");
        std::fs::write(
            root.join("crates").join("aurum-2d").join("Cargo.toml"),
            "[package]\nname = \"aurum-2d\"\n",
        )
        .expect("write");

        let report = report(&names(&["aurum-2d", "aurum-vn"]), Some(&root));
        assert!(report.checkout_read, "the checkout should have been read");

        let present = report
            .entries
            .iter()
            .find(|e| e.name == "aurum-2d")
            .unwrap();
        assert_eq!(present.state, State::Enabled);

        let absent = report
            .entries
            .iter()
            .find(|e| e.name == "aurum-vn")
            .unwrap();
        assert_eq!(
            absent.state,
            State::Missing,
            "a module named by the project but absent from the checkout should be reported"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_unreadable_checkout_makes_no_claim_about_what_is_present() {
        // Without a checkout to look at, "missing" would be a guess. A module
        // that is merely unverifiable is reported as enabled, and the report
        // says the checkout was not read so a caller can tell the difference.
        let report = report(&names(&["aurum-2d"]), None);
        assert!(!report.checkout_read);
        assert!(
            report.with_state(State::Missing).is_empty(),
            "nothing should be claimed missing without a checkout to check"
        );
        assert_eq!(report.with_state(State::Enabled).len(), 1);
    }

    #[test]
    fn a_checkout_path_that_is_not_there_is_not_an_error() {
        let report = report(&names(&["aurum-2d"]), Some(Path::new("E:/nowhere")));
        assert!(!report.checkout_read);
        assert_eq!(report.with_state(State::Enabled).len(), 1);
    }

    #[test]
    fn every_module_has_a_name_and_a_description() {
        for module in MODULES {
            assert!(module.name.starts_with("aurum-"), "{}", module.name);
            assert!(!module.description.is_empty(), "{}", module.name);
        }
    }

    #[test]
    fn module_names_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for module in MODULES {
            assert!(seen.insert(module.name), "{} is listed twice", module.name);
        }
    }

    #[test]
    fn states_are_distinguishable() {
        let labels: Vec<&str> = [
            State::Enabled,
            State::Missing,
            State::Available,
            State::Unknown,
        ]
        .iter()
        .map(|s| s.label())
        .collect();
        let unique: std::collections::HashSet<&&str> = labels.iter().collect();
        assert_eq!(unique.len(), labels.len());
    }

    #[test]
    fn distance_is_the_expected_edit_distance() {
        assert_eq!(distance("", ""), 0);
        assert_eq!(distance("abc", "abc"), 0);
        assert_eq!(distance("abc", "abd"), 1);
        assert_eq!(distance("abc", "ab"), 1);
        assert_eq!(distance("", "abc"), 3);
        assert_eq!(distance("kitten", "sitting"), 3);
    }
}
