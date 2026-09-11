//! Classifying a change: what does it cost the running editor?
//!
//! The design's central promise is that routine work never restarts the
//! editor. That promise only means something if something can tell the
//! difference, so every change is classified into one of four verdicts, each
//! with a reason the user can read.
//!
//! The interesting case is Rust. Most Rust changes are implementation changes
//! behind a stable `AurumNode`, which reload; but a change to the *native
//! schema* — a new `#[func]`, a changed signature, a registered class — cannot
//! be migrated live, and pretending otherwise produces a stale extension that
//! looks like it loaded. So Rust sources are inspected for the attributes that
//! alter the Godot-facing surface, and anything found raises the verdict.

use std::path::Path;

/// What a change costs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Verdict {
    /// Nothing has to happen: the file is not part of the running build.
    NoAction,
    /// Reload in place, keeping the editor and the game alive.
    Reload,
    /// The game must restart; the editor stays up.
    GameplayRestart,
    /// Native registration changed; a controlled editor restart is required.
    EditorRestart,
}

impl Verdict {
    pub fn label(self) -> &'static str {
        match self {
            Self::NoAction => "no action",
            Self::Reload => "hot reload",
            Self::GameplayRestart => "gameplay restart",
            Self::EditorRestart => "editor restart",
        }
    }

    /// Whether the editor process can stay alive.
    pub fn keeps_editor(self) -> bool {
        self != Self::EditorRestart
    }
}

/// A classified change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classification {
    pub verdict: Verdict,
    /// Why, in terms the user can act on.
    pub reason: String,
}

impl Classification {
    fn new(verdict: Verdict, reason: impl Into<String>) -> Self {
        Self {
            verdict,
            reason: reason.into(),
        }
    }
}

/// The attributes that change the Godot-facing surface on the Rust side.
///
/// Each one alters native registration, a method signature, or the extension
/// entry, none of which Godot can migrate in a live process. The tuple is
/// (needle, why it matters).
const SCHEMA_MARKERS: &[(&str, &str)] = &[
    ("#[func]", "a Godot-facing method was added or changed"),
    ("#[signal]", "a Godot-facing signal changed"),
    ("#[export", "an exported property changed"),
    ("#[var]", "an exported variable changed"),
    ("#[constant]", "an exported constant changed"),
    ("#[class(", "a native class registration changed"),
    ("#[godot_api]", "the Godot API surface of a class changed"),
    ("#[gdextension", "the extension entry point changed"),
    (
        "#[derive(GodotClass)]",
        "a native class was added or removed",
    ),
];

/// Classify a single file change.
pub fn classify_path(path: &Path) -> Classification {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let extension = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let text = path
        .to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase();

    // The manifest describes how the library is loaded. Changing it changes
    // what Godot was told at startup, which cannot be revised live.
    if extension == "gdextension" {
        return Classification::new(
            Verdict::EditorRestart,
            "the GDExtension manifest changed; library mappings and entry symbols are read at load",
        );
    }

    // Godot-native content reloads in place.
    match extension.as_str() {
        "gd" | "tscn" | "tres" | "gdshader" | "shader" | "import" | "svg" | "png" | "jpg"
        | "jpeg" | "webp" | "ogg" | "wav" | "mp3" | "ttf" => {
            return Classification::new(
                Verdict::Reload,
                format!("Godot reloads .{extension} files in place"),
            )
        }
        "godot" => {
            return Classification::new(
                Verdict::GameplayRestart,
                "project settings are read at startup; the running game must restart",
            )
        }
        _ => {}
    }

    // Documentation, CI, and test scripts do not affect a running editor.
    if matches!(extension.as_str(), "md" | "txt" | "yml" | "yaml")
        || text.contains("/docs/")
        || text.contains("/.github/")
    {
        return Classification::new(
            Verdict::NoAction,
            "documentation and CI do not affect the running build",
        );
    }

    if extension == "ps1" || text.contains("/scripts/tests/") {
        return Classification::new(
            Verdict::NoAction,
            "scripts and tests are not loaded by the running editor",
        );
    }

    if name == "cargo.toml" || name == "cargo.lock" {
        return Classification::new(
            Verdict::Reload,
            "dependency changes rebuild the extension and reload it",
        );
    }

    if extension == "rs" {
        // Decided by content; the caller supplies it. Without content a
        // signature change cannot be ruled out.
        return Classification::new(
            Verdict::Reload,
            "Rust implementation changes reload behind the stable AurumNode API",
        );
    }

    Classification::new(
        Verdict::NoAction,
        format!("'.{extension}' is not part of the running build"),
    )
}

/// Classify a change, using the file's contents where they matter.
///
/// Rust sources are inspected for the attributes that alter native
/// registration. This over-reports on purpose: a comment mentioning `#[func]`
/// raises the verdict. A false "editor restart" costs a restart the user did
/// not need, while a false "reload" produces an extension that appears to
/// update and silently does not.
pub fn classify_change(path: &Path, contents: Option<&str>) -> Classification {
    let base = classify_path(path);

    if base.verdict != Verdict::Reload {
        return base;
    }
    let extension = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if extension == "gdextension" {
        return base;
    }

    let Some(contents) = contents else {
        return base;
    };

    for (marker, reason) in SCHEMA_MARKERS {
        if contents.contains(marker) {
            return Classification::new(
                Verdict::EditorRestart,
                format!("{reason} ({marker}); native registration cannot be migrated live"),
            );
        }
    }

    base
}

/// Whether a change means the native extension has to be rebuilt.
///
/// Godot reloads its own content without help, so a changed scene or script
/// needs nothing from Cargo. Only the Rust side and the manifests that drive
/// it require a build. Rebuilding for a `.gd` edit would be wasted work on
/// every keystroke-sized save.
pub fn rebuild_required(path: &Path) -> bool {
    let extension = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    matches!(extension.as_str(), "rs" | "toml" | "lock")
}

/// Classify a set of changes, keeping the most severe verdict.
pub fn classify_all<'a, I>(changes: I) -> Classification
where
    I: IntoIterator<Item = (&'a Path, Option<&'a str>)>,
{
    let mut worst: Option<Classification> = None;
    let mut count = 0usize;

    for (path, contents) in changes {
        count += 1;
        let classification = classify_change(path, contents);
        match &worst {
            Some(current) if current.verdict >= classification.verdict => {}
            _ => worst = Some(classification),
        }
    }

    match worst {
        // One restart is one restart, however many files caused it.
        Some(classification) if count > 1 => Classification::new(
            classification.verdict,
            format!("{} (worst of {count} changed files)", classification.reason),
        ),
        Some(classification) => classification,
        None => Classification::new(Verdict::NoAction, "nothing changed"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn path(text: &str) -> PathBuf {
        PathBuf::from(text)
    }

    #[test]
    fn godot_content_reloads_in_place() {
        for file in [
            "godot/scripts/aurum_runtime.gd",
            "godot/demos/2d_squares/main.tscn",
            "godot/resources/theme.tres",
            "godot/shaders/water.gdshader",
            "godot/addons/aurum/icon.svg",
            "godot/assets/hero.png",
        ] {
            let classification = classify_path(&path(file));
            assert_eq!(
                classification.verdict,
                Verdict::Reload,
                "{file} should reload, got {classification:?}"
            );
            assert!(classification.verdict.keeps_editor());
        }
    }

    #[test]
    fn the_gdextension_manifest_requires_an_editor_restart() {
        let classification = classify_path(&path("godot/addons/aurum/bin/aurum.gdextension"));
        assert_eq!(classification.verdict, Verdict::EditorRestart);
        assert!(classification.reason.contains("manifest"));
        assert!(!classification.verdict.keeps_editor());
    }

    #[test]
    fn project_settings_restart_the_game_but_not_the_editor() {
        let classification = classify_path(&path("godot/project.godot"));
        assert_eq!(classification.verdict, Verdict::GameplayRestart);
        assert!(classification.verdict.keeps_editor());
    }

    #[test]
    fn plain_rust_changes_reload() {
        let source = "pub fn step(&mut self, dt: f32) { self.x += dt; }";
        let classification = classify_change(&path("crates/aurum-core/src/lib.rs"), Some(source));
        assert_eq!(
            classification.verdict,
            Verdict::Reload,
            "{classification:?}"
        );
    }

    #[test]
    fn every_schema_attribute_raises_the_verdict() {
        for marker in [
            "#[func]",
            "#[signal]",
            "#[export]",
            "#[var]",
            "#[constant]",
            "#[class(base=Node)]",
            "#[godot_api]",
            "#[gdextension(entry_symbol = \"x\")]",
            "#[derive(GodotClass)]",
        ] {
            let source = format!("impl Node {{\n    {marker}\n    fn go(&self) {{}}\n}}");
            let classification =
                classify_change(&path("crates/aurum-godot/src/lib.rs"), Some(&source));
            assert_eq!(
                classification.verdict,
                Verdict::EditorRestart,
                "{marker} should require a restart, got {classification:?}"
            );
            assert!(
                classification.reason.contains("cannot be migrated"),
                "the reason should say why: {}",
                classification.reason
            );
        }
    }

    #[test]
    fn rust_without_contents_is_treated_as_a_reload() {
        // Contents are unavailable during a filesystem event burst; the
        // caller re-classifies once the file can be read.
        let classification = classify_change(&path("crates/aurum-core/src/lib.rs"), None);
        assert_eq!(classification.verdict, Verdict::Reload);
    }

    #[test]
    fn a_schema_attribute_in_a_comment_over_reports_on_purpose() {
        // Documented behaviour: a false restart is cheap, a missed one leaves
        // a stale extension that looks loaded.
        let source = "// see #[func] for the exported surface";
        let classification = classify_change(&path("crates/aurum-godot/src/lib.rs"), Some(source));
        assert_eq!(classification.verdict, Verdict::EditorRestart);
    }

    #[test]
    fn contents_do_not_upgrade_a_non_rust_file() {
        // A markdown file that happens to quote an attribute is still a doc.
        let classification = classify_change(
            &path("docs/HOT_RELOAD.md"),
            Some("adding a #[func] needs a restart"),
        );
        assert_eq!(classification.verdict, Verdict::NoAction);
    }

    #[test]
    fn documentation_and_ci_take_no_action() {
        for file in [
            "README.md",
            "docs/WORKFLOW.md",
            ".github/workflows/ci.yml",
            "CHANGELOG.md",
        ] {
            assert_eq!(
                classify_path(&path(file)).verdict,
                Verdict::NoAction,
                "{file}"
            );
        }
    }

    #[test]
    fn scripts_and_tests_take_no_action() {
        for file in [
            "scripts/build.ps1",
            "scripts/dev.ps1",
            "scripts/tests/phase0_contract.ps1",
        ] {
            assert_eq!(
                classify_path(&path(file)).verdict,
                Verdict::NoAction,
                "{file}"
            );
        }
    }

    #[test]
    fn cargo_manifests_reload() {
        assert_eq!(classify_path(&path("Cargo.toml")).verdict, Verdict::Reload);
        assert_eq!(classify_path(&path("Cargo.lock")).verdict, Verdict::Reload);
    }

    #[test]
    fn windows_separators_classify_the_same_as_posix_ones() {
        let posix = classify_path(&path("godot/scripts/aurum_runtime.gd"));
        let windows = classify_path(&path(r"godot\scripts\aurum_runtime.gd"));
        assert_eq!(posix.verdict, windows.verdict);

        let docs = classify_path(&path(r"repo\docs\NOTES.md"));
        assert_eq!(docs.verdict, Verdict::NoAction);
    }

    #[test]
    fn an_unknown_extension_is_reported_as_unrelated() {
        let classification = classify_path(&path("assets/model.blend"));
        assert_eq!(classification.verdict, Verdict::NoAction);
        assert!(
            classification.reason.contains(".blend"),
            "the reason should name the type: {}",
            classification.reason
        );
    }

    #[test]
    fn a_set_of_changes_reports_the_worst() {
        let changes = [
            (path("README.md"), None),
            (path("godot/scripts/a.gd"), None),
            (path("crates/aurum-core/src/lib.rs"), None),
        ];
        let borrows: Vec<(&Path, Option<&str>)> =
            changes.iter().map(|(p, c)| (p.as_path(), *c)).collect();
        let classification = classify_all(borrows);
        assert_eq!(classification.verdict, Verdict::Reload);
        assert!(
            classification.reason.contains("worst of 3"),
            "{}",
            classification.reason
        );
    }

    #[test]
    fn a_restart_wins_over_everything_else_in_a_set() {
        let restarting = "impl X { #[func] fn y(&self) {} }".to_string();
        let changes = [
            (path("godot/scripts/a.gd"), None),
            (
                path("crates/aurum-godot/src/lib.rs"),
                Some(restarting.as_str()),
            ),
            (path("README.md"), None),
        ];
        let borrows: Vec<(&Path, Option<&str>)> =
            changes.iter().map(|(p, c)| (p.as_path(), *c)).collect();
        assert_eq!(classify_all(borrows).verdict, Verdict::EditorRestart);
    }

    #[test]
    fn one_restart_is_reported_once_however_many_files_caused_it() {
        let source = "impl X { #[func] fn y(&self) {} }".to_string();
        let changes = [
            (path("crates/aurum-godot/src/a.rs"), Some(source.as_str())),
            (path("crates/aurum-godot/src/b.rs"), Some(source.as_str())),
            (path("crates/aurum-godot/src/c.rs"), Some(source.as_str())),
        ];
        let borrows: Vec<(&Path, Option<&str>)> =
            changes.iter().map(|(p, c)| (p.as_path(), *c)).collect();
        let classification = classify_all(borrows);
        assert_eq!(classification.verdict, Verdict::EditorRestart);
        // One explanation, not one per file: the reason names the shared cause
        // once and says how many files were considered.
        assert!(
            classification.reason.contains("worst of 3"),
            "{}",
            classification.reason
        );
        assert_eq!(
            classification.reason.matches("native registration").count(),
            1,
            "the cause should be explained exactly once: {}",
            classification.reason
        );
    }

    #[test]
    fn an_empty_change_set_takes_no_action() {
        let classification = classify_all(Vec::<(&Path, Option<&str>)>::new());
        assert_eq!(classification.verdict, Verdict::NoAction);
        assert!(classification.reason.contains("nothing changed"));
    }

    #[test]
    fn only_rust_and_manifests_need_a_build() {
        // Godot reloads its own content; Cargo is only needed for Rust.
        assert!(rebuild_required(&path("crates/aurum-core/src/lib.rs")));
        assert!(rebuild_required(&path("Cargo.toml")));
        assert!(rebuild_required(&path("Cargo.lock")));

        assert!(!rebuild_required(&path("godot/scripts/a.gd")));
        assert!(!rebuild_required(&path("godot/main.tscn")));
        assert!(!rebuild_required(&path("godot/shaders/water.gdshader")));
        assert!(!rebuild_required(&path("README.md")));
        // Case should not matter on Windows-style paths.
        assert!(rebuild_required(&path("crates/thing/SRC/LIB.RS")));
    }

    #[test]
    fn verdicts_are_ordered_by_cost() {
        assert!(Verdict::NoAction < Verdict::Reload);
        assert!(Verdict::Reload < Verdict::GameplayRestart);
        assert!(Verdict::GameplayRestart < Verdict::EditorRestart);
    }

    #[test]
    fn only_an_editor_restart_gives_up_the_editor() {
        assert!(Verdict::NoAction.keeps_editor());
        assert!(Verdict::Reload.keeps_editor());
        assert!(Verdict::GameplayRestart.keeps_editor());
        assert!(!Verdict::EditorRestart.keeps_editor());
    }

    #[test]
    fn labels_name_the_action() {
        assert_eq!(Verdict::NoAction.label(), "no action");
        assert_eq!(Verdict::Reload.label(), "hot reload");
        assert_eq!(Verdict::GameplayRestart.label(), "gameplay restart");
        assert_eq!(Verdict::EditorRestart.label(), "editor restart");
    }
}
