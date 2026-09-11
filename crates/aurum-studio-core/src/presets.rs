//! Export presets: what a project can be built into.
//!
//! Godot owns this format. `export_presets.cfg` is written by its export
//! dialog, and the fields inside it change between Godot versions and between
//! platforms — a Windows preset and an Android one share almost nothing. So
//! this reads and checks that file rather than generating one. A generator
//! would be a second implementation of somebody else's format, written against
//! a snapshot of it, and the failure mode is the worst kind: a preset that
//! looks right, is accepted by the editor, and produces a broken build.
//!
//! What it is for, then, is telling somebody the truth before they find out at
//! the end of an export. That there are no presets at all, which is what a
//! project carried between machines usually looks like. Which platforms are
//! configured. And whether a preset that cannot possibly work — one naming a
//! platform Godot does not have — is sitting in the file.

use std::path::{Path, PathBuf};

/// Where Godot keeps this file.
pub const PRESETS_FILE: &str = "export_presets.cfg";

/// Why the file could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PresetError {
    Io(String),
}

impl std::fmt::Display for PresetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(detail) => write!(f, "{detail}"),
        }
    }
}

impl std::error::Error for PresetError {}

/// One export preset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preset {
    /// The name a person gave it, e.g. "Windows Desktop".
    pub name: String,
    /// The Godot export platform, e.g. "Windows Desktop".
    pub platform: String,
    /// Whether the editor shows it as runnable.
    pub runnable: bool,
}

/// What a project's presets amount to.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Report {
    pub presets: Vec<Preset>,
    /// Whether the file was there at all. A project with no presets and a
    /// project with no file are different situations and get different advice.
    pub file_present: bool,
    /// Lines the reader could not make sense of, kept so a malformed file is
    /// reported rather than silently read as empty.
    pub unreadable_lines: usize,
}

impl Report {
    pub fn platforms(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.presets.iter().map(|p| p.platform.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        names
    }

    /// Presets that cannot work: a platform Godot does not have.
    ///
    /// Checked against the platforms this engine's Godot version actually
    /// offers. A preset naming something else was written by hand, or by a
    /// different Godot, and will fail at the very end of an export run — which
    /// is the worst moment to find out.
    pub fn impossible(&self, known: &[&str]) -> Vec<&Preset> {
        self.presets
            .iter()
            .filter(|preset| {
                !known
                    .iter()
                    .any(|k| k.eq_ignore_ascii_case(&preset.platform))
            })
            .collect()
    }
}

/// The export platforms Godot 4.7 offers.
///
/// Held as a list because the file being read cannot be trusted to describe
/// itself, and because a name that is not here is exactly the thing worth
/// reporting.
pub const KNOWN_PLATFORMS: &[&str] = &[
    "Windows Desktop",
    "Linux/X11",
    "macOS",
    "Android",
    "iOS",
    "Web",
];

/// Read `export_presets.cfg` from a Godot project directory.
pub fn read(godot_project: &Path) -> Result<Report, PresetError> {
    let path: PathBuf = godot_project.join(PRESETS_FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Report::default());
        }
        Err(error) => return Err(PresetError::Io(format!("'{}': {error}", path.display()))),
    };

    Ok(parse(&text))
}

/// Read the format.
///
/// INI-shaped: `[preset.N]` opens a preset, `[preset.N.options]` opens that
/// preset's settings, and `key=value` lines fill whichever is open. Only the
/// preset headers are interesting here — the options are Godot's business, and
/// reading them would mean tracking a schema that changes between versions for
/// no benefit.
pub fn parse(text: &str) -> Report {
    let mut report = Report {
        file_present: true,
        ..Report::default()
    };

    let mut current: Option<usize> = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') {
            continue;
        }

        if let Some(section) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            // `preset.0` opens a preset; `preset.0.options` is its settings and
            // belongs to the preset already open.
            let section = section.trim();
            current = section
                .strip_prefix("preset.")
                .filter(|rest| !rest.contains('.'))
                .and_then(|index| index.parse::<usize>().ok())
                .map(|index| {
                    // The file numbers presets 0, 1, 2; a gap or a repeat means
                    // somebody edited it by hand, so the index is used only to
                    // keep them apart and never as a position.
                    report.presets.push(Preset {
                        name: String::new(),
                        platform: String::new(),
                        runnable: false,
                    });
                    let _ = index;
                    report.presets.len() - 1
                })
                .or_else(|| {
                    // A section that is neither: leave whatever was open open,
                    // because options sections look like this.
                    if section.starts_with("preset.") {
                        current
                    } else {
                        None
                    }
                });
            continue;
        }

        let Some((key, value)) = line.split_once('=') else {
            report.unreadable_lines += 1;
            continue;
        };
        let Some(index) = current else { continue };

        let value = value.trim().trim_matches('"');
        match key.trim() {
            "name" => report.presets[index].name = value.to_string(),
            "platform" => report.presets[index].platform = value.to_string(),
            "runnable" => report.presets[index].runnable = value.eq_ignore_ascii_case("true"),
            _ => {}
        }
    }

    // A preset with no name is one the editor will not show, so it is dropped
    // rather than reported as an unnamed entry nobody can act on.
    report.presets.retain(|preset| !preset.name.is_empty());
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape Godot actually writes, trimmed to the parts that matter here.
    const SAMPLE: &str = r#"
[preset.0]

name="Windows Desktop"
platform="Windows Desktop"
runnable=true
custom_features=""
export_filter="all_resources"

[preset.0.options]

binary_format/embed_pck=false
texture_format/s3tc_bptc=true

[preset.1]

name="Web"
platform="Web"
runnable=false

[preset.1.options]

variant/extensions_support=false
"#;

    #[test]
    fn a_real_looking_file_is_read() {
        let report = parse(SAMPLE);
        assert!(report.file_present);
        assert_eq!(report.presets.len(), 2);
        assert_eq!(report.presets[0].name, "Windows Desktop");
        assert_eq!(report.presets[0].platform, "Windows Desktop");
        assert!(report.presets[0].runnable);
        assert_eq!(report.presets[1].name, "Web");
        assert!(!report.presets[1].runnable);
    }

    #[test]
    fn options_sections_do_not_start_a_new_preset() {
        // The distinction the whole parser turns on. Counting `[preset.N.options]`
        // as a preset would double the list and invent entries with no name.
        let with_options = "[preset.0]\nname=\"A\"\nplatform=\"Web\"\n\n\
                            [preset.0.options]\nkey=1\nkey2=2\n";
        assert_eq!(parse(with_options).presets.len(), 1);
    }

    #[test]
    fn a_preset_with_no_name_is_not_reported() {
        // The editor will not show it and nobody can act on it, so an entry
        // called "" is noise rather than information.
        let report = parse("[preset.0]\nplatform=\"Web\"\n");
        assert!(report.presets.is_empty());
    }

    #[test]
    fn a_file_with_no_presets_reads_as_empty_rather_than_missing() {
        // "You have no presets" and "there is no file" lead to different
        // advice, so the report has to tell them apart.
        let report = parse("; nothing here yet\n");
        assert!(report.file_present);
        assert!(report.presets.is_empty());
        assert_eq!(report.unreadable_lines, 0, "a comment is not a problem");
    }

    #[test]
    fn a_missing_file_is_reported_as_absent() {
        let root = std::env::temp_dir().join(format!("aurum-presets-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("mkdir");

        let report = read(&root).expect("read");
        assert!(!report.file_present);
        assert!(report.presets.is_empty());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_line_that_is_not_a_setting_is_counted_rather_than_ignored() {
        let report = parse("[preset.0]\nname=\"A\"\nplatform=\"Web\"\nthis is not a setting\n");
        assert_eq!(report.unreadable_lines, 1);
        assert_eq!(report.presets.len(), 1, "the good lines still count");
    }

    #[test]
    fn platforms_are_listed_once_each() {
        let report = parse(
            "[preset.0]\nname=\"A\"\nplatform=\"Web\"\n\n\
             [preset.1]\nname=\"B\"\nplatform=\"Web\"\n\n\
             [preset.2]\nname=\"C\"\nplatform=\"Android\"\n",
        );
        assert_eq!(report.platforms(), vec!["Android", "Web"]);
    }

    #[test]
    fn a_platform_godot_does_not_have_is_reported_as_impossible() {
        // The failure this exists for: a preset written by hand, or by another
        // Godot, that cannot work and says so only at the end of an export.
        let report = parse(
            "[preset.0]\nname=\"Good\"\nplatform=\"Web\"\n\n\
             [preset.1]\nname=\"Made Up\"\nplatform=\"Dreamcast\"\n",
        );
        let impossible = report.impossible(KNOWN_PLATFORMS);
        assert_eq!(impossible.len(), 1);
        assert_eq!(impossible[0].name, "Made Up");
    }

    #[test]
    fn platform_matching_does_not_care_about_case() {
        let report = parse("[preset.0]\nname=\"A\"\nplatform=\"windows desktop\"\n");
        assert!(report.impossible(KNOWN_PLATFORMS).is_empty());
    }

    #[test]
    fn every_known_platform_is_accepted() {
        // A list that rejects a real platform would report every Windows
        // project as broken.
        for platform in KNOWN_PLATFORMS {
            let report = parse(&format!(
                "[preset.0]\nname=\"A\"\nplatform=\"{platform}\"\n"
            ));
            assert!(
                report.impossible(KNOWN_PLATFORMS).is_empty(),
                "'{platform}' should be recognised"
            );
        }
    }

    #[test]
    fn quoted_values_lose_their_quotes() {
        let report = parse("[preset.0]\nname=\"My Export\"\nplatform=\"Web\"\n");
        assert_eq!(report.presets[0].name, "My Export");
    }

    #[test]
    fn runnable_is_read_as_a_boolean_not_as_text() {
        let report = parse(
            "[preset.0]\nname=\"A\"\nplatform=\"Web\"\nrunnable=true\n\n\
             [preset.1]\nname=\"B\"\nplatform=\"Web\"\nrunnable=false\n",
        );
        assert!(report.presets[0].runnable);
        assert!(!report.presets[1].runnable);
    }
}
