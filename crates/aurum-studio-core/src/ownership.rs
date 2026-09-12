//! Process ownership records.
//!
//! Studio launches Godot and its own helpers, and later needs to stop them.
//! The danger is a recycled process identifier: a PID recorded an hour ago can
//! belong to something entirely unrelated by the time `aurum stop` runs.
//!
//! So a record carries enough to *prove* identity — executable path, process
//! start time, and the project it was launched for — and stopping requires all
//! of them to still match. Where a field cannot be established, the answer is
//! no rather than a guess: refusing to stop is recoverable, killing the wrong
//! process is not.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

// Only the Windows path shells out. On Unix the process query reads /proc
// directly, so importing the command runner there leaves an unused import that
// `-D warnings` turns into a build failure — on two of the three platforms CI
// builds, which is why this was invisible from a Windows desk.
#[cfg(windows)]
use std::time::Duration;

#[cfg(windows)]
use crate::process::Command;

/// What a recorded process is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProcessKind {
    /// The Godot editor.
    Editor,
    /// A game launched by the editor bridge.
    Game,
    /// A long-running helper Studio supervises.
    Worker,
}

impl ProcessKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Editor => "editor",
            Self::Game => "game",
            Self::Worker => "worker",
        }
    }
}

/// Everything needed to recognise a process again later.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnershipRecord {
    /// The session that launched it.
    pub session: String,
    /// The executable that was launched.
    pub executable: PathBuf,
    pub pid: u32,
    /// The process start time, as the platform reported it.
    ///
    /// This is the field that defeats PID reuse: two processes can share an
    /// identifier, never a start time.
    pub started: String,
    /// The project it was launched against.
    pub project: PathBuf,
    pub kind: ProcessKind,
}

impl OwnershipRecord {
    /// Write the record into a session directory.
    pub fn write(&self, directory: &Path) -> std::io::Result<PathBuf> {
        std::fs::create_dir_all(directory)?;
        let path = self.path(directory);
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(&path, text)?;
        Ok(path)
    }

    /// Where this record lives in a session directory.
    pub fn path(&self, directory: &Path) -> PathBuf {
        directory.join(format!("{}-{}.json", self.kind.label(), self.pid))
    }

    /// Forget the record, once the process it describes is gone.
    ///
    /// A record left behind for a dead process is not dangerous — the
    /// ownership check refuses to act on it — but it makes every later report
    /// about the session mention a process that no longer exists.
    pub fn remove(&self, directory: &Path) -> std::io::Result<()> {
        std::fs::remove_file(self.path(directory))
    }

    /// Read a record back.
    pub fn read(path: &Path) -> std::io::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        serde_json::from_str(&text)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    /// Every record in a session directory.
    pub fn read_all(directory: &Path) -> Vec<Self> {
        let Ok(entries) = std::fs::read_dir(directory) else {
            return Vec::new();
        };
        let mut records: Vec<Self> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|e| e == "json"))
            .filter_map(|path| Self::read(&path).ok())
            .collect();
        // Stable order, so a caller's decisions do not depend on the
        // filesystem's ordering.
        records.sort_by_key(|record| (record.kind.label(), record.pid));
        records
    }

    /// Whether this record still describes the process at that identifier.
    ///
    /// Every field must match. A missing start time on the live process means
    /// the check cannot be completed, which is a refusal rather than a pass.
    pub fn describes(&self, live: &LiveProcess) -> bool {
        if self.pid != live.pid {
            return false;
        }
        // Paths are compared case-insensitively on Windows, where the same
        // executable is routinely spelled two ways.
        if !paths_equal(&self.executable, &live.executable) {
            return false;
        }
        match &live.started {
            Some(started) => &self.started == started,
            None => false,
        }
    }
}

/// Compare two paths the way the platform does.
pub fn paths_equal(a: &Path, b: &Path) -> bool {
    let (a, b) = (a.to_string_lossy(), b.to_string_lossy());
    if cfg!(windows) {
        a.eq_ignore_ascii_case(&b)
    } else {
        a == b
    }
}

/// What the operating system currently says about a process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveProcess {
    pub pid: u32,
    pub executable: PathBuf,
    /// The start time, when the platform would tell us.
    pub started: Option<String>,
}

impl LiveProcess {
    /// Whether this describes the process the caller is running in.
    pub fn current() -> Option<Self> {
        inspect(std::process::id())
    }
}

/// How long to wait for the platform to answer a process query.
#[cfg(windows)]
const INSPECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Ask the operating system about a process.
///
/// Returns `None` when the process does not exist or could not be described.
/// On Windows the query goes through CIM, which is the only dependency-free
/// way to obtain a start time; `tasklist` reports a name but not a start time,
/// and a name alone cannot defeat PID reuse.
pub fn inspect(pid: u32) -> Option<LiveProcess> {
    // Attribute-based selection, not `cfg!`: the macro evaluates to a bool at
    // compile time but still requires both branches to type-check, and each
    // branch only exists on its own platform.
    #[cfg(windows)]
    {
        inspect_windows(pid)
    }
    #[cfg(not(windows))]
    {
        inspect_unix(pid)
    }
}

#[cfg(windows)]
fn inspect_windows(pid: u32) -> Option<LiveProcess> {
    // Emit one delimited line so the answer parses without a JSON dependency
    // on the shell side.
    let script = format!(
        "$p = Get-CimInstance Win32_Process -Filter \"ProcessId={pid}\" -ErrorAction SilentlyContinue; \
         if ($p) {{ \"$($p.ProcessId)|$($p.ExecutablePath)|$($p.CreationDate.ToString('o'))\" }}"
    );
    let outcome = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .run(INSPECT_TIMEOUT)
        .ok()?;
    if !outcome.success() {
        return None;
    }

    let line = outcome
        .stdout
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())?;
    let mut parts = line.split('|');
    let pid: u32 = parts.next()?.trim().parse().ok()?;
    let executable = parts.next()?.trim();
    let started = parts.next()?.trim();

    if executable.is_empty() {
        // A process we cannot name is one we cannot prove ownership of.
        return None;
    }
    Some(LiveProcess {
        pid,
        executable: PathBuf::from(executable),
        started: if started.is_empty() {
            None
        } else {
            Some(started.to_string())
        },
    })
}

#[cfg(not(windows))]
fn inspect_unix(pid: u32) -> Option<LiveProcess> {
    let executable = std::fs::read_link(format!("/proc/{pid}/exe")).ok()?;
    // Field 22 of /proc/<pid>/stat is the start time in clock ticks; it is
    // fixed for the process's lifetime, which is what makes it useful here.
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let after_name = stat.rsplit_once(')')?.1;
    let started = after_name.split_whitespace().nth(19).map(str::to_string);
    Some(LiveProcess {
        pid,
        executable,
        started,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(pid: u32, executable: &str, started: &str) -> OwnershipRecord {
        OwnershipRecord {
            session: "session-1".into(),
            executable: PathBuf::from(executable),
            pid,
            started: started.into(),
            project: PathBuf::from("A:/project"),
            kind: ProcessKind::Editor,
        }
    }

    fn live(pid: u32, executable: &str, started: Option<&str>) -> LiveProcess {
        LiveProcess {
            pid,
            executable: PathBuf::from(executable),
            started: started.map(str::to_string),
        }
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "aurum-ownership-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    const EXE: &str = "A:/tools/godot.exe";

    #[test]
    fn a_record_describes_the_process_it_was_made_for() {
        let record = record(42, EXE, "2026-09-11T10:00:00Z");
        assert!(record.describes(&live(42, EXE, Some("2026-09-11T10:00:00Z"))));
    }

    #[test]
    fn a_reused_identifier_is_rejected() {
        // The whole point: same PID, different process.
        let record = record(42, EXE, "2026-09-11T10:00:00Z");
        assert!(!record.describes(&live(42, EXE, Some("2026-09-11T11:30:00Z"))));
    }

    #[test]
    fn a_different_executable_is_rejected() {
        let record = record(42, EXE, "2026-09-11T10:00:00Z");
        assert!(!record.describes(&live(
            42,
            "A:/tools/other.exe",
            Some("2026-09-11T10:00:00Z")
        )));
    }

    #[test]
    fn a_different_identifier_is_rejected() {
        let record = record(42, EXE, "2026-09-11T10:00:00Z");
        assert!(!record.describes(&live(43, EXE, Some("2026-09-11T10:00:00Z"))));
    }

    #[test]
    fn an_unknown_start_time_is_a_refusal_not_a_pass() {
        // Without a start time the check is incomplete, and an incomplete
        // check must not authorise terminating a process.
        let record = record(42, EXE, "2026-09-11T10:00:00Z");
        assert!(!record.describes(&live(42, EXE, None)));
    }

    #[cfg(windows)]
    #[test]
    fn windows_paths_compare_case_insensitively() {
        let record = record(42, "A:/Tools/Godot.EXE", "t");
        // The same executable is routinely spelled two ways on Windows.
        assert!(record.describes(&live(42, "a:/tools/godot.exe", Some("t"))));
    }

    #[test]
    fn records_round_trip_through_a_session_directory() {
        let dir = temp_dir("roundtrip");
        let record = record(4242, EXE, "2026-09-11T10:00:00Z");
        let path = record.write(&dir).unwrap();
        assert!(path.is_file(), "the record should exist at {path:?}");
        assert_eq!(path, record.path(&dir));

        let reloaded = OwnershipRecord::read(&path).unwrap();
        assert_eq!(reloaded, record);

        // Forgetting it is how a session stops mentioning a process it has
        // already stopped.
        record.remove(&dir).unwrap();
        assert!(!path.exists());
        assert!(
            record.remove(&dir).is_err(),
            "removing twice is not a promise"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn records_of_different_kinds_do_not_collide() {
        // A game and an editor can share an identifier across a restart, and
        // one must not overwrite the other's record.
        let dir = temp_dir("kinds");
        let editor = record(7, EXE, "t");
        let game = OwnershipRecord {
            kind: ProcessKind::Game,
            ..record(7, EXE, "t")
        };
        assert_ne!(editor.path(&dir), game.path(&dir));
        editor.write(&dir).unwrap();
        game.write(&dir).unwrap();
        assert_eq!(OwnershipRecord::read_all(&dir).len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn every_record_in_a_session_is_readable_in_a_stable_order() {
        let dir = temp_dir("read-all");
        record(1, EXE, "t").write(&dir).unwrap();
        OwnershipRecord {
            kind: ProcessKind::Worker,
            pid: 9,
            ..record(9, "A:/tools/worker.exe", "t")
        }
        .write(&dir)
        .unwrap();
        OwnershipRecord {
            kind: ProcessKind::Game,
            pid: 5,
            ..record(5, EXE, "t")
        }
        .write(&dir)
        .unwrap();

        let all = OwnershipRecord::read_all(&dir);
        assert_eq!(all.len(), 3);
        assert_eq!(
            all.iter().map(|r| r.kind).collect::<Vec<_>>(),
            vec![ProcessKind::Editor, ProcessKind::Game, ProcessKind::Worker],
            "records should come back in a deterministic order"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_or_broken_record_does_not_break_discovery() {
        let dir = temp_dir("broken");
        record(1, EXE, "t").write(&dir).unwrap();
        std::fs::write(dir.join("broken-2.json"), "{ not json").unwrap();
        std::fs::write(dir.join("ignored.txt"), "not a record").unwrap();

        // A corrupt record is skipped rather than failing the whole read.
        let all = OwnershipRecord::read_all(&dir);
        assert_eq!(all.len(), 1);
        assert_eq!(OwnershipRecord::read_all(&dir.join("missing")).len(), 0);
        assert!(OwnershipRecord::read(&dir.join("broken-2.json")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_current_process_can_be_described() {
        // Exercises the platform query for real, on the process running it.
        let me = LiveProcess::current().expect("the current process should be inspectable");
        assert_eq!(me.pid, std::process::id());
        assert!(
            me.started.is_some(),
            "a start time is required to defeat identifier reuse"
        );
        assert!(!me.executable.as_os_str().is_empty());
    }

    #[test]
    fn a_process_that_does_not_exist_is_not_invented() {
        // A PID far outside any plausible range.
        assert_eq!(inspect(u32::MAX - 3), None);
    }

    #[test]
    fn a_record_about_the_current_process_matches_it() {
        let me = LiveProcess::current().unwrap();
        let record = OwnershipRecord {
            session: "s".into(),
            executable: me.executable.clone(),
            pid: me.pid,
            started: me.started.clone().unwrap(),
            project: PathBuf::from("A:/p"),
            kind: ProcessKind::Worker,
        };
        assert!(
            record.describes(&me),
            "a record about this process should match it"
        );
    }

    #[test]
    fn kinds_have_distinct_labels() {
        assert_eq!(ProcessKind::Editor.label(), "editor");
        assert_eq!(ProcessKind::Game.label(), "game");
        assert_eq!(ProcessKind::Worker.label(), "worker");
    }
}
