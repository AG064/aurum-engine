//! `aurum` subcommands, implemented over `aurum-studio-core`.
//!
//! The CLI is the automation surface for the whole pipeline: tests, scripts,
//! and AI agents drive Studio through here. That is why every command is
//! deterministic, every report has a machine-readable form, and exit codes
//! distinguish the states a caller has to branch on.

use std::path::PathBuf;
use std::process::ExitCode;

use aurum_studio_core::doctor::{diagnose, Health};
use aurum_studio_core::registry::Registry;
use aurum_studio_core::toolchain::discover;
use aurum_studio_core::Project;

/// Exit codes callers branch on. Distinguishing "unhealthy" from "broken
/// invocation" is what lets a script act on the result rather than parse text.
pub mod exit {
    pub const OK: u8 = 0;
    pub const WARNING: u8 = 1;
    pub const BLOCKED: u8 = 2;
    pub const USAGE: u8 = 64;
    pub const FAILED: u8 = 70;
}

/// Parsed common arguments.
#[derive(Debug, Default)]
struct Options {
    project: Option<PathBuf>,
    godot: Option<PathBuf>,
    json: bool,
    positional: Vec<String>,
}

/// Parse the flag shape every command shares.
fn parse(args: &[String]) -> Result<Options, String> {
    let mut options = Options::default();
    let mut index = 0;
    while index < args.len() {
        let argument = args[index].as_str();
        match argument {
            "--json" => options.json = true,
            "--godot" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| "--godot requires a path".to_string())?;
                options.godot = Some(PathBuf::from(value));
            }
            "--project" | "-p" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| "--project requires a path".to_string())?;
                options.project = Some(PathBuf::from(value));
            }
            "-h" | "--help" => return Err("help".into()),
            other => {
                if let Some(value) = other.strip_prefix("--godot=") {
                    options.godot = Some(PathBuf::from(value));
                } else if let Some(value) = other.strip_prefix("--project=") {
                    options.project = Some(PathBuf::from(value));
                } else if other.starts_with('-') {
                    return Err(format!("unknown option '{other}'"));
                } else {
                    options.positional.push(other.to_string());
                }
            }
        }
        index += 1;
    }
    Ok(options)
}

/// Where the project is: the flag, else the first positional, else the cwd.
fn target(options: &Options) -> PathBuf {
    options
        .project
        .clone()
        .or_else(|| options.positional.first().map(PathBuf::from))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

/// `aurum doctor [project]`
pub fn doctor(args: &[String]) -> ExitCode {
    let options = match parse(args) {
        Ok(options) => options,
        Err(message) => return usage("doctor", &message),
    };
    let path = target(&options);

    let project = match Project::open(&path) {
        Ok(project) => project,
        Err(error) => {
            let message = error.to_string();
            if options.json {
                println!(
                    "{}",
                    serde_json::json!({
                        "project": path.display().to_string(),
                        "health": "blocked",
                        "error": message,
                    })
                );
            } else {
                eprintln!("aurum doctor: {message}");
            }
            // A project that cannot even be opened is blocked, not a crash.
            return ExitCode::from(exit::BLOCKED);
        }
    };

    let toolchain = discover(&project, options.godot.as_deref());
    let report = diagnose(&project, &toolchain, options.godot.as_deref());

    if options.json {
        println!("{}", report.to_json());
    } else {
        print!("{}", report.render());
    }

    match report.health() {
        Health::Healthy => ExitCode::from(exit::OK),
        Health::Warning => ExitCode::from(exit::WARNING),
        Health::Blocked => ExitCode::from(exit::BLOCKED),
    }
}

/// `aurum projects`
pub fn projects(args: &[String]) -> ExitCode {
    let options = match parse(args) {
        Ok(options) => options,
        Err(message) => return usage("projects", &message),
    };

    let Some(path) = Registry::resolve_path() else {
        eprintln!("aurum projects: no home directory to keep a registry in");
        return ExitCode::from(exit::FAILED);
    };
    let registry = match Registry::load(&path) {
        Ok(registry) => registry,
        Err(error) => {
            eprintln!("aurum projects: {error}");
            return ExitCode::from(exit::FAILED);
        }
    };

    if options.json {
        let entries: Vec<serde_json::Value> = registry
            .projects
            .iter()
            .map(|entry| {
                serde_json::json!({
                    "name": entry.name,
                    "path": entry.path.display().to_string(),
                    "present": entry.path.is_dir(),
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::json!({ "registry": path.display().to_string(), "projects": entries })
        );
        return ExitCode::from(exit::OK);
    }

    println!("registry: {}", path.display());
    if registry.is_empty() {
        println!("no projects registered; add one with `aurum import <path>`");
        return ExitCode::from(exit::OK);
    }
    for entry in &registry.projects {
        // A registered project whose directory is gone is shown, not hidden:
        // it may just be on a drive that is not mounted right now.
        let mark = if entry.path.is_dir() { ' ' } else { '!' };
        println!("  [{mark}] {:<24} {}", entry.name, entry.path.display());
    }
    ExitCode::from(exit::OK)
}

/// `aurum import <path>`
///
/// Read-only against the project: it opens and validates, then records the
/// path. Nothing in the project is written without being asked.
pub fn import(args: &[String]) -> ExitCode {
    let options = match parse(args) {
        Ok(options) => options,
        Err(message) => return usage("import", &message),
    };
    let path = target(&options);

    let project = match Project::open(&path) {
        Ok(project) => project,
        Err(error) => {
            eprintln!("aurum import: {error}");
            eprintln!("hint: point at the directory containing aurum.toml");
            return ExitCode::from(exit::BLOCKED);
        }
    };

    let Some(registry_path) = Registry::resolve_path() else {
        eprintln!("aurum import: no home directory to keep a registry in");
        return ExitCode::from(exit::FAILED);
    };
    let mut registry = match Registry::load(&registry_path) {
        Ok(registry) => registry,
        Err(error) => {
            eprintln!("aurum import: {error}");
            return ExitCode::from(exit::FAILED);
        }
    };

    let added = registry.register(&project.config.name, &project.root);
    if let Err(error) = registry.save(&registry_path) {
        eprintln!("aurum import: {error}");
        return ExitCode::from(exit::FAILED);
    }

    if options.json {
        println!(
            "{}",
            serde_json::json!({
                "name": project.config.name,
                "path": project.root.display().to_string(),
                "added": added,
                "registry": registry_path.display().to_string(),
            })
        );
    } else {
        println!(
            "{} '{}' at {}",
            if added { "imported" } else { "updated" },
            project.config.name,
            project.root.display()
        );
    }
    ExitCode::from(exit::OK)
}

/// `aurum forget <name|path>`
pub fn forget(args: &[String]) -> ExitCode {
    let options = match parse(args) {
        Ok(options) => options,
        Err(message) => return usage("forget", &message),
    };
    let Some(needle) = options.positional.first() else {
        return usage("forget", "a project name or path is required");
    };

    let Some(registry_path) = Registry::resolve_path() else {
        eprintln!("aurum forget: no home directory to keep a registry in");
        return ExitCode::from(exit::FAILED);
    };
    let mut registry = match Registry::load(&registry_path) {
        Ok(registry) => registry,
        Err(error) => {
            eprintln!("aurum forget: {error}");
            return ExitCode::from(exit::FAILED);
        }
    };

    // Removing from the registry never touches the project on disk.
    if !registry.remove(needle) {
        eprintln!("aurum forget: '{needle}' is not registered");
        return ExitCode::from(exit::WARNING);
    }
    if let Err(error) = registry.save(&registry_path) {
        eprintln!("aurum forget: {error}");
        return ExitCode::from(exit::FAILED);
    }

    if options.json {
        println!("{}", serde_json::json!({ "removed": needle }));
    } else {
        println!("forgot '{needle}' (the project itself is untouched)");
    }
    ExitCode::from(exit::OK)
}

/// Print a usage error for a command.
fn usage(command: &str, message: &str) -> ExitCode {
    if message != "help" {
        eprintln!("aurum {command}: {message}");
    }
    eprintln!("\n{}", command_usage(command));
    ExitCode::from(if message == "help" {
        exit::OK
    } else {
        exit::USAGE
    })
}

fn command_usage(command: &str) -> &'static str {
    match command {
        "doctor" => {
            "usage: aurum doctor [project] [--godot <path>] [--json]\n\
             \n\
             Reports healthy, warning, or blocked state with evidence.\n\
             exit 0 healthy, 1 warning, 2 blocked"
        }
        "projects" => "usage: aurum projects [--json]",
        "import" => "usage: aurum import <project-path> [--json]",
        "forget" => "usage: aurum forget <name-or-path>",
        _ => "usage: aurum <command>",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_the_shared_flag_shape() {
        let options = parse(&args(&["--json", "--godot", "A:/g.exe", "--project", "p"])).unwrap();
        assert!(options.json);
        assert_eq!(options.godot, Some(PathBuf::from("A:/g.exe")));
        assert_eq!(options.project, Some(PathBuf::from("p")));
    }

    #[test]
    fn accepts_equals_forms_and_short_project() {
        let options = parse(&args(&["--godot=A:/g.exe", "-p", "p"])).unwrap();
        assert_eq!(options.godot, Some(PathBuf::from("A:/g.exe")));
        assert_eq!(options.project, Some(PathBuf::from("p")));
    }

    #[test]
    fn positionals_are_collected_and_flags_are_separated() {
        let options = parse(&args(&["some/path", "--json"])).unwrap();
        assert_eq!(options.positional, vec!["some/path".to_string()]);
        assert!(options.json);
    }

    #[test]
    fn unknown_options_and_missing_values_are_refused() {
        assert!(parse(&args(&["--nope"])).is_err());
        assert!(parse(&args(&["--godot"])).is_err());
        assert!(parse(&args(&["--project"])).is_err());
        // A single dash is a negative-looking positional, not an option.
        assert!(parse(&args(&["-x"])).is_err());
    }

    #[test]
    fn help_is_signalled_distinctly_from_an_error() {
        assert_eq!(parse(&args(&["--help"])).unwrap_err(), "help");
    }

    #[test]
    fn the_target_prefers_the_flag_then_the_positional_then_cwd() {
        let mut options = Options {
            project: Some(PathBuf::from("flagged")),
            ..Default::default()
        };
        options.positional.push("positional".into());
        assert_eq!(target(&options), PathBuf::from("flagged"));

        let options = Options {
            positional: vec!["positional".into()],
            ..Default::default()
        };
        assert_eq!(target(&options), PathBuf::from("positional"));

        let options = Options::default();
        assert_eq!(target(&options), std::env::current_dir().unwrap());
    }

    #[test]
    fn exit_codes_are_distinct_per_state() {
        // Callers branch on these, so collisions would be a bug.
        let codes = [
            exit::OK,
            exit::WARNING,
            exit::BLOCKED,
            exit::USAGE,
            exit::FAILED,
        ];
        let mut sorted = codes.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), codes.len());
        assert_eq!(exit::OK, 0, "success must be zero");
    }
}
