//! `doctor`: what is wrong with this project, with evidence.
//!
//! Every finding carries the reason it was raised, so the report can be acted
//! on rather than merely read. Three levels, and the distinction matters:
//!
//! - **Healthy** — nothing to do.
//! - **Warning** — work can continue, but something is missing or unexpected.
//! - **Blocked** — a command that was asked for cannot run at all.
//!
//! The overall verdict is the worst finding, because one blocker is not
//! offset by ten healthy checks.

use std::path::{Path, PathBuf};

use crate::project::Project;
use crate::toolchain::{version_satisfies, Toolchain};

/// How serious a finding is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Health {
    Healthy,
    Warning,
    Blocked,
}

impl Health {
    pub fn label(self) -> &'static str {
        match self {
            Self::Healthy => "ok",
            Self::Warning => "warning",
            Self::Blocked => "blocked",
        }
    }

    /// A single character for compact reports.
    pub fn mark(self) -> char {
        match self {
            Self::Healthy => '+',
            Self::Warning => '!',
            Self::Blocked => 'x',
        }
    }
}

/// One check's result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// A stable identifier, so a script can match on it.
    pub id: &'static str,
    pub health: Health,
    pub summary: String,
    /// The fact behind the verdict: a path, a version, an exit code.
    pub evidence: Option<String>,
    /// What to do about it, when there is something to do.
    pub remedy: Option<String>,
}

impl Finding {
    fn new(id: &'static str, health: Health, summary: impl Into<String>) -> Self {
        Self {
            id,
            health,
            summary: summary.into(),
            evidence: None,
            remedy: None,
        }
    }

    fn ok(id: &'static str, summary: impl Into<String>) -> Self {
        Self::new(id, Health::Healthy, summary)
    }

    fn warning(id: &'static str, summary: impl Into<String>) -> Self {
        Self::new(id, Health::Warning, summary)
    }

    fn blocked(id: &'static str, summary: impl Into<String>) -> Self {
        Self::new(id, Health::Blocked, summary)
    }

    fn with_evidence(mut self, evidence: impl Into<String>) -> Self {
        self.evidence = Some(evidence.into());
        self
    }

    fn with_remedy(mut self, remedy: impl Into<String>) -> Self {
        self.remedy = Some(remedy.into());
        self
    }
}

/// The full picture of a project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub project_root: PathBuf,
    pub name: String,
    pub findings: Vec<Finding>,
}

impl Report {
    /// The worst finding decides the verdict.
    pub fn health(&self) -> Health {
        self.findings
            .iter()
            .map(|finding| finding.health)
            .max()
            .unwrap_or(Health::Healthy)
    }

    pub fn count(&self, health: Health) -> usize {
        self.findings
            .iter()
            .filter(|finding| finding.health == health)
            .count()
    }

    pub fn find(&self, id: &str) -> Option<&Finding> {
        self.findings.iter().find(|finding| finding.id == id)
    }

    /// A human-readable report.
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("project : {}\n", self.name));
        out.push_str(&format!("root    : {}\n", self.project_root.display()));
        out.push_str(&format!(
            "verdict : {} ({} ok, {} warning, {} blocked)\n\n",
            self.health().label().to_uppercase(),
            self.count(Health::Healthy),
            self.count(Health::Warning),
            self.count(Health::Blocked),
        ));

        for finding in &self.findings {
            out.push_str(&format!(
                "  [{}] {:<22} {}\n",
                finding.health.mark(),
                finding.id,
                finding.summary
            ));
            if let Some(evidence) = &finding.evidence {
                out.push_str(&format!("        evidence: {evidence}\n"));
            }
            if let Some(remedy) = &finding.remedy {
                out.push_str(&format!("        remedy  : {remedy}\n"));
            }
        }
        out
    }

    /// The report as JSON, for scripts and agents.
    ///
    /// The CLI is meant to be driven by automation, so every report has a
    /// machine-readable form rather than only a rendered one.
    pub fn to_json(&self) -> String {
        let findings: Vec<serde_json::Value> = self
            .findings
            .iter()
            .map(|finding| {
                serde_json::json!({
                    "id": finding.id,
                    "health": finding.health.label(),
                    "summary": finding.summary,
                    "evidence": finding.evidence,
                    "remedy": finding.remedy,
                })
            })
            .collect();

        serde_json::json!({
            "project": self.project_root.display().to_string(),
            "name": self.name,
            "health": self.health().label(),
            "counts": {
                "ok": self.count(Health::Healthy),
                "warning": self.count(Health::Warning),
                "blocked": self.count(Health::Blocked),
            },
            "findings": findings,
        })
        .to_string()
    }

    /// The findings that need attention, worst first.
    pub fn problems(&self) -> Vec<&Finding> {
        let mut problems: Vec<&Finding> = self
            .findings
            .iter()
            .filter(|finding| finding.health != Health::Healthy)
            .collect();
        // Worst first. sort_by_key needs the key owned, and a reversed
        // Health reads more clearly than a negation.
        problems.sort_by_key(|finding| std::cmp::Reverse(finding.health));
        problems
    }
}

/// Diagnose a project.
///
/// Read-only: nothing here creates, modifies, or deletes anything, so running
/// `doctor` on a project is always safe and always repeatable.
pub fn diagnose(project: &Project, toolchain: &Toolchain, godot_hint: Option<&Path>) -> Report {
    let mut findings = Vec::new();

    // ----- project layout -------------------------------------------------

    findings.push(
        Finding::ok("config", "aurum.toml parsed").with_evidence(format!(
            "schema {}, name '{}'",
            project.config.schema_version, project.config.name
        )),
    );

    match &project.layout.godot_project {
        Some(path) => findings.push(
            Finding::ok("godot_project", "project.godot found")
                .with_evidence(path.display().to_string()),
        ),
        None => findings.push(
            Finding::blocked("godot_project", "no project.godot anywhere under the root")
                .with_remedy("point Studio at the directory containing project.godot"),
        ),
    }

    match &project.layout.cargo_manifest {
        Some(path) => findings.push(
            Finding::ok("cargo_manifest", "Cargo manifest found")
                .with_evidence(path.display().to_string()),
        ),
        None => findings.push(
            Finding::blocked("cargo_manifest", "no Cargo.toml at the project root").with_remedy(
                "the GDExtension is built with Cargo, so the workspace manifest is required",
            ),
        ),
    }

    match &project.layout.addon_directory {
        Some(path) => findings.push(
            Finding::ok("addon", "Aurum add-on directory found")
                .with_evidence(path.display().to_string()),
        ),
        None => findings.push(
            Finding::warning("addon", "no Aurum add-on directory found")
                .with_remedy("set addon_destination in aurum.toml, or build the add-on first"),
        ),
    }

    match &project.layout.gdextension {
        Some(path) => findings.push(
            Finding::ok("gdextension", "GDExtension manifest found")
                .with_evidence(path.display().to_string()),
        ),
        None => findings.push(
            Finding::warning(
                "gdextension",
                "no .gdextension manifest in the add-on's bin directory",
            )
            .with_remedy("the native extension cannot load until a manifest exists"),
        ),
    }

    match &project.layout.installed_library {
        Some(path) => {
            let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
            findings.push(
                Finding::ok("installed_library", "debug extension is installed")
                    .with_evidence(format!("{} ({size} bytes)", path.display())),
            );
        }
        None => findings.push(
            Finding::warning("installed_library", "no installed debug extension")
                .with_remedy("run `aurum build` to produce one"),
        ),
    }

    // ----- toolchain ------------------------------------------------------

    match &toolchain.cargo {
        Some(tool) => findings.push(Finding::ok("cargo", "Cargo available").with_evidence(
            format!(
                "{} ({})",
                tool.path.display(),
                tool.version.as_deref().unwrap_or("version unknown")
            ),
        )),
        None => findings.push(
            Finding::blocked("cargo", "Cargo was not found on PATH")
                .with_remedy("install Rust from https://rustup.rs"),
        ),
    }

    match &toolchain.rustc {
        Some(tool) => findings.push(Finding::ok("rustc", "rustc available").with_evidence(
            format!(
                "{} ({})",
                tool.path.display(),
                tool.version.as_deref().unwrap_or("version unknown")
            ),
        )),
        None => findings.push(
            Finding::warning("rustc", "rustc was not found on PATH")
                .with_remedy("Cargo usually ships with rustc; check the Rust installation"),
        ),
    }

    match &toolchain.godot {
        Some(tool) => {
            findings.push(
                Finding::ok("godot", "Godot available").with_evidence(format!(
                    "{} ({})",
                    tool.path.display(),
                    tool.version.as_deref().unwrap_or("version unknown")
                )),
            );

            // A pinned version that does not match is worth knowing about
            // before an editor is launched against it.
            if let Some(required) = &project.config.godot_version {
                let reported = tool.version.as_deref();
                if version_satisfies(reported, required) {
                    findings.push(Finding::ok(
                        "godot_version",
                        format!("Godot {required} satisfied"),
                    ));
                } else {
                    findings.push(
                        Finding::warning(
                            "godot_version",
                            format!(
                                "project expects Godot {required}, found {}",
                                reported.unwrap_or("an unreadable version")
                            ),
                        )
                        .with_remedy("point --godot at a matching build, or update godot_version in aurum.toml"),
                    );
                }
            }
        }
        None => findings.push(
            Finding::warning("godot", "Godot was not found")
                .with_evidence(match godot_hint {
                    Some(hint) => format!(
                        "searched PATH, the hint '{}', and nearby directories",
                        hint.display()
                    ),
                    None => "searched PATH and directories near the project".to_string(),
                })
                .with_remedy("pass --godot <path to the Godot executable>"),
        ),
    }

    // ----- engine relationship -------------------------------------------

    if let Some(hint) = &project.config.engine_path_hint {
        let resolved = resolve_hint(hint, &project.root);
        match resolved {
            Some(path) if path.is_dir() => findings.push(
                Finding::ok("engine_path", "engine path hint resolves")
                    .with_evidence(path.display().to_string()),
            ),
            _ => findings.push(
                Finding::warning(
                    "engine_path",
                    format!("engine path hint '{hint}' does not resolve to a directory"),
                )
                .with_remedy("fix engine.path_hint in aurum.toml, or remove it"),
            ),
        }
    }

    Report {
        project_root: project.root.clone(),
        name: project.config.name.clone(),
        findings,
    }
}

/// Resolve a relative hint against a project root; an absolute hint stands.
fn resolve_hint(hint: &str, project_root: &Path) -> Option<PathBuf> {
    let path = Path::new(hint);
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        project_root.join(path)
    };
    joined.canonicalize().ok().or(Some(joined))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{Layout, ProjectConfig};
    use crate::toolchain::Tool;

    fn project_with(layout: Layout) -> Project {
        Project {
            root: PathBuf::from("/project"),
            config: ProjectConfig {
                name: "test".into(),
                ..Default::default()
            },
            layout,
        }
    }

    fn full_layout() -> Layout {
        Layout {
            godot_project: Some(PathBuf::from("/project/godot/project.godot")),
            cargo_manifest: Some(PathBuf::from("/project/Cargo.toml")),
            gdextension: Some(PathBuf::from(
                "/project/godot/addons/aurum/bin/a.gdextension",
            )),
            addon_directory: Some(PathBuf::from("/project/godot/addons/aurum")),
            installed_library: Some(PathBuf::from("/project/godot/addons/aurum/bin/a.dll")),
            build_script: None,
        }
    }

    fn full_toolchain() -> Toolchain {
        Toolchain {
            cargo: Some(Tool {
                path: PathBuf::from("/tools/cargo"),
                version: Some("1.95.0".into()),
            }),
            rustc: Some(Tool {
                path: PathBuf::from("/tools/rustc"),
                version: Some("1.95.0".into()),
            }),
            godot: Some(Tool {
                path: PathBuf::from("/tools/godot"),
                version: Some("4.7".into()),
            }),
        }
    }

    #[test]
    fn a_complete_project_is_healthy() {
        let report = diagnose(&project_with(full_layout()), &full_toolchain(), None);
        assert_eq!(
            report.health(),
            Health::Healthy,
            "unexpected problems: {:?}",
            report.problems()
        );
        assert_eq!(report.count(Health::Blocked), 0);
    }

    #[test]
    fn a_missing_manifest_blocks_the_build() {
        let mut layout = full_layout();
        layout.cargo_manifest = None;
        let report = diagnose(&project_with(layout), &full_toolchain(), None);

        assert_eq!(report.health(), Health::Blocked);
        let finding = report.find("cargo_manifest").unwrap();
        assert_eq!(finding.health, Health::Blocked);
        assert!(finding.remedy.is_some(), "a blocker should say what to do");
    }

    #[test]
    fn a_missing_project_file_blocks() {
        let mut layout = full_layout();
        layout.godot_project = None;
        let report = diagnose(&project_with(layout), &full_toolchain(), None);
        assert_eq!(report.health(), Health::Blocked);
    }

    #[test]
    fn a_missing_toolchain_blocks_or_warns_by_what_it_prevents() {
        // No Cargo means no build at all, which is blocking.
        let mut toolchain = full_toolchain();
        toolchain.cargo = None;
        let report = diagnose(&project_with(full_layout()), &toolchain, None);
        assert_eq!(report.health(), Health::Blocked);

        // No Godot still permits building, so it only warns.
        let mut toolchain = full_toolchain();
        toolchain.godot = None;
        let report = diagnose(&project_with(full_layout()), &toolchain, None);
        assert_eq!(report.health(), Health::Warning);
        assert_eq!(report.find("godot").unwrap().health, Health::Warning);
        assert!(report.find("godot").unwrap().remedy.is_some());
    }

    #[test]
    fn a_missing_installed_library_warns_rather_than_blocks() {
        let mut layout = full_layout();
        layout.installed_library = None;
        let report = diagnose(&project_with(layout), &full_toolchain(), None);
        assert_eq!(report.health(), Health::Warning);
        assert!(report
            .find("installed_library")
            .unwrap()
            .remedy
            .as_deref()
            .unwrap()
            .contains("aurum build"));
    }

    #[test]
    fn a_pinned_godot_version_is_checked() {
        let mut project = project_with(full_layout());
        project.config.godot_version = Some("4.7".into());
        let report = diagnose(&project, &full_toolchain(), None);
        assert_eq!(
            report.find("godot_version").unwrap().health,
            Health::Healthy
        );

        // A mismatched version warns, and says what was found.
        let mut toolchain = full_toolchain();
        toolchain.godot = Some(Tool {
            path: PathBuf::from("/tools/godot"),
            version: Some("4.2".into()),
        });
        let report = diagnose(&project, &toolchain, None);
        let finding = report.find("godot_version").unwrap();
        assert_eq!(finding.health, Health::Warning);
        assert!(finding.summary.contains("4.2"), "{}", finding.summary);
    }

    #[test]
    fn an_unreadable_godot_version_does_not_satisfy_a_pin() {
        let mut project = project_with(full_layout());
        project.config.godot_version = Some("4.7".into());
        let mut toolchain = full_toolchain();
        toolchain.godot = Some(Tool {
            path: PathBuf::from("/tools/godot"),
            version: None,
        });
        let report = diagnose(&project, &toolchain, None);
        let finding = report.find("godot_version").unwrap();
        assert_eq!(finding.health, Health::Warning);
        assert!(
            finding.summary.contains("unreadable"),
            "{}",
            finding.summary
        );
    }

    #[test]
    fn an_unresolvable_engine_hint_warns_with_evidence() {
        let mut project = project_with(full_layout());
        project.config.engine_path_hint = Some("definitely/not/here".into());
        let report = diagnose(&project, &full_toolchain(), None);
        let finding = report.find("engine_path").unwrap();
        assert_eq!(finding.health, Health::Warning);
        assert!(finding.summary.contains("definitely/not/here"));
    }

    #[test]
    fn findings_carry_evidence_for_both_toolchain_and_paths() {
        let report = diagnose(&project_with(full_layout()), &full_toolchain(), None);
        for id in ["cargo", "godot", "godot_project", "cargo_manifest"] {
            let finding = report.find(id).unwrap_or_else(|| panic!("{id} missing"));
            assert!(finding.evidence.is_some(), "{id} should say what it found");
        }
    }

    #[test]
    fn the_verdict_is_the_worst_finding() {
        // One blocker among many healthy checks is still blocked.
        let mut layout = full_layout();
        layout.cargo_manifest = None;
        let report = diagnose(&project_with(layout), &full_toolchain(), None);
        assert!(report.count(Health::Healthy) > 3);
        assert_eq!(report.health(), Health::Blocked);
    }

    #[test]
    fn problems_are_sorted_worst_first() {
        let mut layout = full_layout();
        layout.cargo_manifest = None;
        layout.installed_library = None;
        let report = diagnose(&project_with(layout), &full_toolchain(), None);
        let problems = report.problems();
        assert!(problems.len() >= 2);
        assert_eq!(problems[0].health, Health::Blocked, "{problems:?}");
    }

    #[test]
    fn rendering_includes_verdicts_evidence_and_remedies() {
        let mut layout = full_layout();
        layout.installed_library = None;
        let report = diagnose(&project_with(layout), &full_toolchain(), None);
        let text = report.render();

        assert!(text.contains("verdict : WARNING"), "{text}");
        assert!(text.contains("installed_library"), "{text}");
        assert!(text.contains("evidence:"), "{text}");
        assert!(text.contains("remedy  :"), "{text}");
    }

    #[test]
    fn json_output_round_trips_and_names_the_verdict() {
        let mut layout = full_layout();
        layout.installed_library = None;
        let report = diagnose(&project_with(layout), &full_toolchain(), None);

        let parsed: serde_json::Value = serde_json::from_str(&report.to_json()).unwrap();
        assert_eq!(parsed["health"], "warning");
        assert_eq!(parsed["counts"]["blocked"], 0);
        assert!(parsed["counts"]["warning"].as_u64().unwrap() >= 1);
        assert!(!parsed["findings"].as_array().unwrap().is_empty());
        assert_eq!(parsed["name"], "test");
    }

    #[test]
    fn health_ordering_puts_blocked_last() {
        assert!(Health::Healthy < Health::Warning);
        assert!(Health::Warning < Health::Blocked);
    }
}
