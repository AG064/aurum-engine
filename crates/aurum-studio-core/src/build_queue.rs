//! One build at a time, and the newest request wins.
//!
//! The design states the rule plainly:
//!
//! > Only one build may write an Aurum artifact for a project at a time. A
//! > newer filesystem event may supersede a queued build, but Studio does not
//! > terminate an active Cargo process unless the user explicitly cancels it.
//!
//! That is three separate promises, and they need three separate mechanisms.
//!
//! **One build at a time** is not something a single-threaded worker can
//! guarantee on its own. `aurum dev`, the Studio shell, and a bare
//! `aurum build` are three processes a person can easily have open at once, and
//! the supervisor's worker thread only serializes the commands *inside* one of
//! them. Two processes racing to install the same library is exactly the state
//! [`install`](crate::build::install) takes such care to recover from, so the
//! cheaper answer is not to reach it: [`BuildLock`] is an operating-system
//! lock on a per-project file, which a second process cannot take and cannot
//! accidentally steal from a process that is still alive.
//!
//! **A newer event supersedes a queued build** is [`BuildQueue`]. While one
//! build runs there is at most one waiting, and a fresh request replaces it
//! rather than joining a line. Building the intermediate state would produce an
//! artifact nobody asked for, from source that no longer exists.
//!
//! **A running build is never terminated** is the absence of a method. There is
//! no `cancel`, no `interrupt`, and no timeout here; stopping a build is a
//! decision the user makes through [`crate::process`], explicitly.

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

use crate::build::BuildRequest;

/// Why a build could not be started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockError {
    /// Another process is building this project right now.
    Held { by: Option<u32> },
    /// The lock could not be created or examined.
    Io(String),
}

impl std::fmt::Display for LockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Held { by: Some(pid) } => write!(
                f,
                "another Studio process (pid {pid}) is already building this project"
            ),
            Self::Held { by: None } => {
                write!(f, "another Studio process is already building this project")
            }
            Self::Io(detail) => write!(f, "could not take the build lock: {detail}"),
        }
    }
}

impl std::error::Error for LockError {}

/// An exclusive claim on a project's build.
///
/// Held for as long as the value lives. Dropping it — including by unwinding,
/// or by the process dying — releases the claim, because it is the operating
/// system's lock on an open handle rather than a flag in a file. That
/// distinction matters: a flag survives a crash and wedges the project until
/// somebody deletes it by hand, which is the failure mode every stale lock file
/// in every build system has taught people to expect.
///
/// The file lives under the temporary directory rather than in the project, so
/// taking a lock never writes anything into a user's repository.
#[derive(Debug)]
pub struct BuildLock {
    path: PathBuf,
    /// Where the holder's identifier is recorded.
    ///
    /// Kept in a second file because the lock file itself cannot be read while
    /// the lock is held: the exclusivity that makes the lock work is exactly
    /// what stops anyone opening it to ask who has it. Writing the holder into
    /// the locked file made the answer to "who holds this" permanently "I
    /// cannot tell" — which a test caught, because the assertion looked
    /// reasonable and the mechanism quietly could not support it.
    owner: PathBuf,
    /// The open handle. The lock *is* this value being alive.
    _file: File,
}

impl BuildLock {
    /// Where the lock for a project lives.
    ///
    /// Under the temporary directory, named by a hash of the canonical project
    /// root, so two processes that name the same project differently — a
    /// relative path, a trailing separator, a different drive letter case —
    /// still contend for the same lock.
    pub fn path_for(project_root: &Path) -> PathBuf {
        lock_directory().join(format!("{}.lock", lock_key(project_root)))
    }

    /// Where the holder is recorded.
    pub fn owner_path_for(project_root: &Path) -> PathBuf {
        lock_directory().join(format!("{}.owner", lock_key(project_root)))
    }

    /// Take the lock, or report who holds it.
    pub fn try_acquire(project_root: &Path) -> Result<Self, LockError> {
        let path = Self::path_for(project_root);
        let owner = Self::owner_path_for(project_root);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| LockError::Io(format!("'{}': {error}", parent.display())))?;
        }

        let file = open_exclusive(&path).map_err(|error| {
            // A sharing violation on Windows is the lock being held, which is
            // an ordinary answer rather than a failure.
            if is_sharing_violation(&error) {
                LockError::Held {
                    by: read_holder(&owner),
                }
            } else {
                LockError::Io(format!("'{}': {error}", path.display()))
            }
        })?;

        // Recorded for whoever is refused next, so the message can name a
        // process rather than say "something". While this lock is held the
        // holder is alive by definition, so the record cannot be stale at the
        // moment it is read.
        let _ = std::fs::write(&owner, std::process::id().to_string());

        Ok(Self {
            path,
            owner,
            _file: file,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// The directory locks live in.
fn lock_directory() -> PathBuf {
    std::env::temp_dir().join("aurum-studio-locks")
}

/// A stable name for a project's lock.
fn lock_key(project_root: &Path) -> String {
    let canonical = project_root
        .canonicalize()
        .map(crate::project::clean_path)
        .unwrap_or_else(|_| project_root.to_path_buf());

    let digest = crate::hash::sha256_hex(canonical.to_string_lossy().as_bytes());
    // Sixteen hex characters is 64 bits of a name that only has to be distinct
    // among the projects on one machine.
    digest[..16].to_string()
}

/// The process named in an owner file, when it can be read.
fn read_holder(owner: &Path) -> Option<u32> {
    std::fs::read_to_string(owner)
        .ok()?
        .trim()
        .parse::<u32>()
        .ok()
}

/// Open a file that no other handle may share.
///
/// On Windows this is exact: `share_mode(0)` makes a second open fail with a
/// sharing violation for as long as the first handle is open, and the system
/// closes that handle when the holder exits for any reason. There is no stale
/// state to clean up, because there is no state — only a handle.
#[cfg(windows)]
fn open_exclusive(path: &Path) -> std::io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        // The contents are never read or written: the lock is the open handle,
        // and the holder is recorded in a separate file precisely because this
        // one cannot be opened while it is held. Truncation is stated rather
        // than left to a default, which is what clippy is asking for.
        .truncate(false)
        .share_mode(0)
        .open(path)
}

/// The best `std` alone offers elsewhere.
///
/// Without `flock` or a lock-file crate there is no advisory lock in the
/// standard library, so this creates the file exclusively and removes it on
/// drop. It is honest about its weakness: a process killed between creating
/// the file and removing it leaves a stale lock. Windows is the platform this
/// shells ships for, and there the real mechanism above is exact.
#[cfg(not(windows))]
fn open_exclusive(path: &Path) -> std::io::Result<File> {
    match OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(file) => Ok(file),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Err(error),
        Err(error) => Err(error),
    }
}

#[cfg(windows)]
fn is_sharing_violation(error: &std::io::Error) -> bool {
    // ERROR_SHARING_VIOLATION (32) and ERROR_LOCK_VIOLATION (33).
    matches!(error.raw_os_error(), Some(32) | Some(33))
}

#[cfg(not(windows))]
fn is_sharing_violation(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::AlreadyExists
}

impl Drop for BuildLock {
    fn drop(&mut self) {
        // On Windows the lock *is* the open handle, so the system releases it
        // and there is nothing to remove. Elsewhere the file is the lock.
        #[cfg(not(windows))]
        let _ = std::fs::remove_file(&self.path);

        // The holder record only means anything while the lock is held. On
        // Windows a process dying without this leaves the file behind, which
        // is harmless: a refused caller only reads it while somebody holds the
        // lock, and that somebody rewrote it on the way in.
        let _ = std::fs::remove_file(&self.owner);
    }
}

/// What became of a submitted build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Submission {
    /// Nothing was running, so this is the next build.
    Ready,
    /// A build is running; this one waits behind it.
    Queued,
    /// A build is running and something was already waiting. This replaced it.
    Superseded,
}

impl Submission {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Queued => "queued",
            Self::Superseded => "superseded the queued build",
        }
    }
}

/// A serialized queue of builds for one project.
///
/// Deliberately holds requests rather than running them: the caller owns the
/// thread and the timeout, so this stays a policy object that can be tested
/// without a compiler, a project, or a clock.
#[derive(Debug, Default)]
pub struct BuildQueue {
    state: std::sync::Mutex<QueueState>,
}

#[derive(Debug, Default)]
struct QueueState {
    running: bool,
    pending: Option<BuildRequest>,
    /// How many waiting requests were replaced before they ever ran.
    superseded: u64,
    /// How many builds have been handed out.
    started: u64,
}

/// What the queue is doing, for a log line or a status panel.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Snapshot {
    pub running: bool,
    pub pending: Option<String>,
    pub superseded: u64,
    pub started: u64,
}

impl BuildQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// Offer a build.
    ///
    /// Returns what happened to it, so a caller can say "queued behind the
    /// running build" rather than leaving a person guessing why nothing is
    /// happening.
    pub fn submit(&self, request: BuildRequest) -> Submission {
        let mut state = self.lock();
        if !state.running {
            state.pending = Some(request);
            return Submission::Ready;
        }
        match state.pending.replace(request) {
            Some(_) => {
                state.superseded += 1;
                Submission::Superseded
            }
            None => Submission::Queued,
        }
    }

    /// Take the next build to run, if the queue is free.
    ///
    /// `None` while a build is running, which is what makes this serialized:
    /// there is no way to obtain a second request until [`BuildQueue::finish`]
    /// has been called for the first.
    pub fn begin_next(&self) -> Option<BuildRequest> {
        let mut state = self.lock();
        if state.running {
            return None;
        }
        let request = state.pending.take()?;
        state.running = true;
        state.started += 1;
        Some(request)
    }

    /// Mark the running build finished, whatever it produced.
    ///
    /// A failed build frees the queue exactly as a successful one does.
    /// Leaving it blocked on failure would mean one bad edit stops the project
    /// rebuilding until Studio restarts.
    pub fn finish(&self) -> bool {
        let mut state = self.lock();
        let was_running = state.running;
        state.running = false;
        was_running
    }

    pub fn is_running(&self) -> bool {
        self.lock().running
    }

    /// Whether anything is waiting. Used by tests and by the dev loop to decide
    /// whether another pass is needed.
    pub fn has_pending(&self) -> bool {
        self.lock().pending.is_some()
    }

    pub fn snapshot(&self) -> Snapshot {
        let state = self.lock();
        Snapshot {
            running: state.running,
            pending: state
                .pending
                .as_ref()
                .map(|request| format!("{} ({:?})", request.package, request.profile)),
            superseded: state.superseded,
            started: state.started,
        }
    }

    /// A line for the session log, when anything was collapsed.
    ///
    /// Silent when nothing was superseded, because a line saying "0 builds
    /// superseded" on every build is noise in the one place a person looks for
    /// signal.
    pub fn coalesce_report(&self) -> Option<String> {
        let superseded = self.lock().superseded;
        (superseded > 0).then(|| {
            format!(
                "{superseded} queued build{} superseded by newer changes",
                if superseded == 1 { " was" } else { "s were" }
            )
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, QueueState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::Profile;

    fn request(package: &str) -> BuildRequest {
        BuildRequest::new(
            PathBuf::from("workspace"),
            package,
            Profile::Debug,
            PathBuf::from("addons/aurum/bin/aurum.debug.dll"),
            PathBuf::from("cargo"),
        )
    }

    fn unique_root(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let path =
            std::env::temp_dir().join(format!("aurum-lock-{tag}-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&path).expect("create temp project");
        path
    }

    // -- serialization -----------------------------------------------------

    #[test]
    fn the_first_build_runs_immediately() {
        let queue = BuildQueue::new();
        assert_eq!(queue.submit(request("first")), Submission::Ready);
        assert!(!queue.is_running());

        let begun = queue.begin_next().expect("the first build should begin");
        assert_eq!(begun.package, "first");
        assert!(queue.is_running());
    }

    #[test]
    fn a_second_build_cannot_start_while_one_is_running() {
        // This is the whole promise: one build writes the artifact at a time.
        let queue = BuildQueue::new();
        queue.submit(request("first"));
        queue.begin_next();

        assert_eq!(queue.submit(request("second")), Submission::Queued);
        assert!(
            queue.begin_next().is_none(),
            "a running build must block the next one"
        );
        assert!(queue.is_running());
    }

    #[test]
    fn a_newer_request_replaces_the_one_waiting() {
        let queue = BuildQueue::new();
        queue.submit(request("running"));
        queue.begin_next();

        assert_eq!(queue.submit(request("old")), Submission::Queued);
        assert_eq!(queue.submit(request("new")), Submission::Superseded);

        queue.finish();
        let begun = queue.begin_next().expect("the newer build should run");
        assert_eq!(
            begun.package, "new",
            "the superseded request must not run: its source no longer exists"
        );
        assert!(
            queue.begin_next().is_none(),
            "only one build should have been waiting"
        );
    }

    #[test]
    fn a_hundred_edits_while_building_collapse_to_one_rebuild() {
        // The realistic shape: a person saves repeatedly during a long build.
        // Every save is a request, and exactly one rebuild should come of it.
        let queue = BuildQueue::new();
        queue.submit(request("running"));
        queue.begin_next();

        assert_eq!(queue.submit(request("edit-0")), Submission::Queued);
        for index in 1..100 {
            assert_eq!(
                queue.submit(request(&format!("edit-{index}"))),
                Submission::Superseded
            );
        }

        queue.finish();
        let begun = queue.begin_next().expect("one rebuild should run");
        assert_eq!(
            begun.package, "edit-99",
            "the newest edit is the one to build"
        );
        assert!(queue.begin_next().is_none());

        let snapshot = queue.snapshot();
        assert_eq!(
            snapshot.started, 2,
            "one running plus one collapsed rebuild"
        );
        assert_eq!(snapshot.superseded, 99);
        assert_eq!(
            queue.coalesce_report().as_deref(),
            Some("99 queued builds were superseded by newer changes")
        );
    }

    #[test]
    fn a_failed_build_frees_the_queue_like_a_successful_one() {
        // Otherwise one bad edit stops the project rebuilding until restart.
        let queue = BuildQueue::new();
        queue.submit(request("doomed"));
        queue.begin_next();
        queue.submit(request("next"));

        assert!(queue.finish(), "a build was running");
        assert!(!queue.is_running());
        assert!(
            queue.begin_next().is_some(),
            "the queue must keep moving after a failure"
        );
    }

    #[test]
    fn finishing_when_nothing_runs_reports_so() {
        let queue = BuildQueue::new();
        assert!(!queue.finish());
    }

    #[test]
    fn an_idle_queue_hands_out_nothing() {
        let queue = BuildQueue::new();
        assert!(queue.begin_next().is_none());
        assert!(!queue.is_running());
        assert!(!queue.has_pending());
    }

    #[test]
    fn a_report_is_only_offered_when_something_was_collapsed() {
        let queue = BuildQueue::new();
        queue.submit(request("only"));
        assert_eq!(
            queue.coalesce_report(),
            None,
            "nothing was superseded, so there is nothing to report"
        );

        queue.begin_next();
        queue.submit(request("waiting"));
        queue.submit(request("newer"));
        assert!(queue
            .coalesce_report()
            .unwrap()
            .contains("1 queued build was"));
    }

    #[test]
    fn the_snapshot_describes_the_queue_without_running_it() {
        let queue = BuildQueue::new();
        queue.submit(request("alpha"));
        let snapshot = queue.snapshot();
        assert!(!snapshot.running);
        assert!(snapshot.pending.as_deref().unwrap().contains("alpha"));
        assert_eq!(snapshot.started, 0, "submitting is not starting");
    }

    #[test]
    fn submission_labels_say_what_happened() {
        assert_eq!(Submission::Ready.label(), "ready");
        assert_eq!(Submission::Queued.label(), "queued");
        assert!(Submission::Superseded.label().contains("superseded"));
    }

    #[test]
    fn the_queue_is_shared_safely_across_threads() {
        // The supervisor and the dev loop both reach for this from different
        // threads, so it has to be usable that way.
        let queue = std::sync::Arc::new(BuildQueue::new());
        let mut handles = Vec::new();
        for index in 0..8 {
            let queue = std::sync::Arc::clone(&queue);
            handles.push(std::thread::spawn(move || {
                queue.submit(request(&format!("thread-{index}")));
            }));
        }
        for handle in handles {
            handle.join().expect("no thread should panic");
        }

        // Whatever the interleaving, exactly one build can be running at once.
        let begun = queue.begin_next();
        assert!(begun.is_some());
        assert!(
            queue.begin_next().is_none(),
            "two threads must not both start a build"
        );
    }

    // -- the lock ----------------------------------------------------------

    #[test]
    fn a_project_can_be_locked() {
        let root = unique_root("acquire");
        let lock = BuildLock::try_acquire(&root).expect("the first lock should succeed");
        assert!(lock.path().exists());
        assert!(lock.path().starts_with(std::env::temp_dir()));
    }

    #[test]
    fn a_second_lock_on_the_same_project_is_refused() {
        // The cross-process guarantee, exercised through the same operating
        // system mechanism a second process would hit. Two Studio instances,
        // or Studio and the CLI, must not build one project at once.
        let root = unique_root("contend");
        let _held = BuildLock::try_acquire(&root).expect("the first lock should succeed");

        let refused = BuildLock::try_acquire(&root);
        match refused {
            Err(LockError::Held { by }) => {
                // On Windows the holder's pid is readable; elsewhere a
                // crashed process can leave the file, so it may be unknown.
                let _ = by;
            }
            Err(other) => panic!("expected the lock to be held, got {other:?}"),
            Ok(_) => panic!("a second lock must not be granted"),
        }
    }

    #[test]
    fn a_held_lock_names_the_process_holding_it() {
        let root = unique_root("holder");
        let _held = BuildLock::try_acquire(&root).expect("the first lock should succeed");

        match BuildLock::try_acquire(&root) {
            Err(LockError::Held { by }) => {
                if cfg!(windows) {
                    assert_eq!(
                        by,
                        Some(std::process::id()),
                        "a refusal should be able to name the holder"
                    );
                }
            }
            other => panic!("expected a held lock, got {other:?}"),
        }
    }

    #[test]
    fn releasing_the_lock_lets_the_next_build_through() {
        let root = unique_root("release");
        {
            let _held = BuildLock::try_acquire(&root).expect("the first lock should succeed");
        }
        // Dropping releases it: the lock is a live handle, not a flag, so a
        // crash cannot wedge the project the way a stale file would.
        let again = BuildLock::try_acquire(&root);
        assert!(again.is_ok(), "the lock should be free after release");
    }

    #[test]
    fn two_different_projects_do_not_contend() {
        // Otherwise building one project would block an unrelated one.
        let first = unique_root("project-a");
        let second = unique_root("project-b");
        let _a = BuildLock::try_acquire(&first).expect("first project");
        let _b = BuildLock::try_acquire(&second).expect("second project");
    }

    #[test]
    fn the_same_project_named_differently_contends_for_one_lock() {
        // A relative path, a redundant separator, and a trailing dot all name
        // the same directory, and all have to reach the same lock.
        let root = unique_root("naming");
        let awkward = root.join(".").join("");
        assert_eq!(
            BuildLock::path_for(&root),
            BuildLock::path_for(&awkward),
            "naming the same project differently must not produce a second lock"
        );
    }

    #[test]
    fn a_lock_never_writes_into_the_project() {
        // Taking a lock must not leave anything in a user's repository.
        let root = unique_root("clean");
        let before: Vec<_> = std::fs::read_dir(&root)
            .expect("read the project")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name())
            .collect();

        let _held = BuildLock::try_acquire(&root).expect("the lock should succeed");

        let after: Vec<_> = std::fs::read_dir(&root)
            .expect("read the project")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name())
            .collect();
        assert_eq!(before, after, "the project directory should be untouched");
    }

    #[test]
    fn a_lock_error_explains_itself() {
        let held = LockError::Held { by: Some(4242) };
        assert!(held.to_string().contains("4242"));
        let anonymous = LockError::Held { by: None };
        assert!(anonymous.to_string().contains("another Studio process"));
    }
}
