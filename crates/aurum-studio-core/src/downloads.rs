//! Getting a Godot to run, and being sure it is the one that was asked for.
//!
//! Studio can find a Godot that is already on the machine, and that is the
//! case it should keep handling first: most people already have one, and
//! telling them to download a second copy would be rude. This is for the case
//! where there is none, or where the one on PATH is the wrong version for the
//! project — the engine declares a Godot version in `aurum.toml`, and a
//! mismatch is the kind of failure that produces confusing errors a long way
//! from its cause.
//!
//! ## Where the trust comes from
//!
//! Not from the transport. The download is done by a helper the operating
//! system already ships, and a helper is a thing that can be a different
//! version, behind a proxy, or pointed somewhere else entirely. The trust
//! comes from the digest: the expected SHA-256 is written down before the fetch
//! begins, the bytes are hashed after it finishes, and nothing is installed
//! unless the two agree.
//!
//! Three consequences follow, and each is enforced rather than intended:
//!
//! - **A download with no expected digest is refused.** "We will verify it
//!   later" is how an unverified binary gets installed.
//! - **A mismatch leaves nothing behind.** The bytes go to a scratch name
//!   beside the destination, are checked there, and are only moved into place
//!   once they have passed. A failed check leaves the destination exactly as it
//!   was.
//! - **An existing installed copy is never replaced.** Re-downloading the same
//!   version to the same place is work with no purpose and one more chance to
//!   lose a working file.

use std::path::{Path, PathBuf};

use crate::hash::sha256_file;

/// A Godot build that can be fetched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Availability {
    /// The version, as `aurum.toml` writes it.
    pub version: String,
    /// The platform this build is for, e.g. `win64`.
    pub platform: String,
    /// Where it comes from.
    pub url: String,
    /// The SHA-256 the file must have.
    ///
    /// Required, and not optional. A manifest entry without one describes
    /// something this cannot safely install, and accepting it would move the
    /// decision from here to whoever wrote the manifest.
    pub sha256: String,
}

impl Availability {
    /// The file name the URL ends in, which is what gets written.
    pub fn file_name(&self) -> Option<&str> {
        self.url
            .rsplit('/')
            .next()
            .filter(|name| !name.is_empty() && name.contains('.'))
    }
}

/// Why a download did not happen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadError {
    /// The manifest entry has no digest, so nothing about it can be trusted.
    NoDigest { url: String },
    /// The bytes did not hash to what was expected.
    Mismatch {
        path: PathBuf,
        expected: String,
        actual: String,
    },
    /// A verified copy is already installed.
    AlreadyInstalled(PathBuf),
    /// The fetch itself failed.
    Fetch(String),
    /// Writing failed.
    Io(String),
}

impl std::fmt::Display for DownloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoDigest { url } => write!(
                f,
                "'{url}' has no expected digest, so nothing about the download could be checked; \
                 refusing to install it"
            ),
            Self::Mismatch {
                path,
                expected,
                actual,
            } => write!(
                f,
                "'{}' hashed to {actual}, not the expected {expected}; nothing was installed",
                path.display()
            ),
            Self::AlreadyInstalled(path) => {
                write!(f, "'{}' is already installed and verified", path.display())
            }
            Self::Fetch(detail) => write!(f, "the download failed: {detail}"),
            Self::Io(detail) => write!(f, "{detail}"),
        }
    }
}

impl std::error::Error for DownloadError {}

/// How bytes are obtained.
///
/// A trait so the whole install path can be exercised without a network, which
/// is the only way the interesting cases — a mismatch, a truncated file, a
/// destination that already exists — can be tested at all rather than only
/// described.
pub trait Fetcher {
    /// Write the bytes at `url` into `destination`.
    fn fetch(&self, url: &str, destination: &Path) -> Result<(), String>;
}

/// Fetches with the helper the operating system already ships.
///
/// Deliberately not an HTTP client written here: fetching is not the part that
/// makes this safe, the digest is, and a hand-rolled client would be a large
/// amount of new code defending a link in the chain that is already trusted
/// least. `curl` ships with Windows 10 and later and with macOS, and is present
/// on effectively every Linux.
pub struct SystemFetcher;

impl Fetcher for SystemFetcher {
    fn fetch(&self, url: &str, destination: &Path) -> Result<(), String> {
        let command = crate::process::Command::new("curl")
            .arg("--fail")
            .arg("--location")
            .arg("--silent")
            .arg("--show-error")
            .arg("--output")
            .arg(destination.display().to_string())
            .arg(url);
        let outcome = command
            .run(std::time::Duration::from_secs(30 * 60))
            .map_err(|e| format!("could not run curl: {e}"))?;
        if outcome.success() {
            Ok(())
        } else {
            Err(outcome.failure_detail())
        }
    }
}

/// Fetch a build, check it, and put it where it belongs.
///
/// Returns the path it was installed to. See the module documentation for the
/// three rules this keeps.
pub fn install(
    available: &Availability,
    into: &Path,
    fetcher: &dyn Fetcher,
) -> Result<PathBuf, DownloadError> {
    if available.sha256.trim().is_empty() {
        return Err(DownloadError::NoDigest {
            url: available.url.clone(),
        });
    }
    let expected = available.sha256.trim().to_ascii_lowercase();

    let name = available
        .file_name()
        .ok_or_else(|| DownloadError::Fetch(format!("'{}' does not name a file", available.url)))?;
    let destination = into.join(name);

    // An installed, verified copy is left alone. Re-fetching it would be work
    // with no purpose and one more chance to lose a working file.
    if destination.is_file() {
        if let Ok(actual) = sha256_file(&destination) {
            if actual.eq_ignore_ascii_case(&expected) {
                return Err(DownloadError::AlreadyInstalled(destination));
            }
        }
    }

    std::fs::create_dir_all(into)
        .map_err(|e| DownloadError::Io(format!("'{}': {e}", into.display())))?;

    // Downloaded to a scratch name beside the destination, so the bytes never
    // occupy the final path until they have passed. A same-directory rename is
    // atomic, so there is no window in which a half-written file looks
    // installed.
    let scratch = into.join(format!(".{name}.part"));
    let _ = std::fs::remove_file(&scratch);

    if let Err(error) = fetcher.fetch(&available.url, &scratch) {
        let _ = std::fs::remove_file(&scratch);
        return Err(DownloadError::Fetch(error));
    }

    let actual = sha256_file(&scratch).map_err(|e| {
        let _ = std::fs::remove_file(&scratch);
        DownloadError::Io(format!("could not hash '{}': {e}", scratch.display()))
    })?;

    if !actual.eq_ignore_ascii_case(&expected) {
        // Removed rather than kept for inspection. Keeping it would leave
        // something on disk that looks like a Godot build, is not one, and
        // would be found by the next run's discovery.
        let _ = std::fs::remove_file(&scratch);
        return Err(DownloadError::Mismatch {
            path: destination,
            expected,
            actual,
        });
    }

    std::fs::rename(&scratch, &destination).map_err(|e| {
        DownloadError::Io(format!(
            "could not install '{}': {e}",
            destination.display()
        ))
    })?;

    Ok(destination)
}

/// Whether a file on disk is the build described.
pub fn matches(path: &Path, available: &Availability) -> bool {
    let expected = available.sha256.trim();
    if expected.is_empty() {
        return false;
    }
    sha256_file(path)
        .map(|actual| actual.eq_ignore_ascii_case(expected))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// A fetcher that writes whatever it is told to, and remembers what it was
    /// asked for.
    struct Fake {
        contents: Vec<u8>,
        calls: RefCell<Vec<String>>,
        fail: bool,
    }

    impl Fake {
        fn serving(contents: &[u8]) -> Self {
            Self {
                contents: contents.to_vec(),
                calls: RefCell::new(Vec::new()),
                fail: false,
            }
        }

        fn failing() -> Self {
            Self {
                contents: Vec::new(),
                calls: RefCell::new(Vec::new()),
                fail: true,
            }
        }
    }

    impl Fetcher for Fake {
        fn fetch(&self, url: &str, destination: &Path) -> Result<(), String> {
            self.calls.borrow_mut().push(url.to_string());
            if self.fail {
                return Err("the network was not there".to_string());
            }
            std::fs::write(destination, &self.contents).map_err(|e| e.to_string())
        }
    }

    fn unique_dir(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let path =
            std::env::temp_dir().join(format!("aurum-download-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("create temp directory");
        path
    }

    /// The digest of some bytes, computed the same way the check does.
    fn digest_of(bytes: &[u8]) -> String {
        let dir = unique_dir("digest");
        let path = dir.join("payload");
        std::fs::write(&path, bytes).expect("write");
        let digest = sha256_file(&path).expect("hash");
        let _ = std::fs::remove_dir_all(&dir);
        digest
    }

    fn availability(url: &str, sha256: &str) -> Availability {
        Availability {
            version: "4.7".into(),
            platform: "win64".into(),
            url: url.into(),
            sha256: sha256.into(),
        }
    }

    #[test]
    fn a_matching_download_is_installed() {
        let dir = unique_dir("ok");
        let payload = b"a godot build, allegedly";
        let entry = availability("https://example.invalid/Godot.zip", &digest_of(payload));

        let installed = install(&entry, &dir, &Fake::serving(payload)).expect("install");
        assert_eq!(installed, dir.join("Godot.zip"));
        assert_eq!(std::fs::read(&installed).expect("read"), payload);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_download_that_does_not_match_is_refused_and_leaves_nothing() {
        // The whole point. A tampered or truncated download must not reach the
        // destination, and must not be left lying about under a scratch name
        // where the next run's discovery could find it.
        let dir = unique_dir("mismatch");
        let entry = availability(
            "https://example.invalid/Godot.zip",
            &digest_of(b"the real one"),
        );

        let error = install(&entry, &dir, &Fake::serving(b"something else")).unwrap_err();
        match error {
            DownloadError::Mismatch {
                expected, actual, ..
            } => {
                assert_ne!(expected, actual, "the report should show both digests");
            }
            other => panic!("expected a mismatch, got {other:?}"),
        }

        assert!(
            !dir.join("Godot.zip").exists(),
            "nothing should be installed"
        );
        assert!(
            !dir.join(".Godot.zip.part").exists(),
            "the scratch file should not survive a failed check"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_manifest_entry_without_a_digest_is_refused_before_fetching() {
        // "We will verify it later" is how an unverified binary gets installed,
        // so this refuses up front and does not even reach the network.
        let dir = unique_dir("nodigest");
        let entry = availability("https://example.invalid/Godot.zip", "   ");
        let fetcher = Fake::serving(b"anything");

        let error = install(&entry, &dir, &fetcher).unwrap_err();
        assert!(matches!(error, DownloadError::NoDigest { .. }));
        assert!(
            fetcher.calls.borrow().is_empty(),
            "nothing should have been fetched"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failed_fetch_leaves_nothing_behind() {
        let dir = unique_dir("fetchfail");
        let entry = availability("https://example.invalid/Godot.zip", &digest_of(b"payload"));

        assert!(install(&entry, &dir, &Fake::failing()).is_err());
        assert!(!dir.join("Godot.zip").exists());
        assert!(!dir.join(".Godot.zip.part").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_already_installed_copy_is_not_fetched_again() {
        let dir = unique_dir("existing");
        let payload = b"a godot build";
        let entry = availability("https://example.invalid/Godot.zip", &digest_of(payload));

        install(&entry, &dir, &Fake::serving(payload)).expect("first install");

        let second = Fake::serving(payload);
        let error = install(&entry, &dir, &second).unwrap_err();
        assert!(matches!(error, DownloadError::AlreadyInstalled(_)));
        assert!(
            second.calls.borrow().is_empty(),
            "the file was already there and verified, so nothing should have been fetched"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_existing_file_that_does_not_match_is_replaced_rather_than_trusted() {
        // A file at the destination is not evidence that it is the right file.
        let dir = unique_dir("stale");
        let payload = b"the build we want";
        let entry = availability("https://example.invalid/Godot.zip", &digest_of(payload));
        std::fs::write(dir.join("Godot.zip"), b"an older, different build").expect("write");

        let installed = install(&entry, &dir, &Fake::serving(payload)).expect("install");
        assert_eq!(std::fs::read(&installed).expect("read"), payload);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_digest_is_compared_without_caring_about_case() {
        let dir = unique_dir("case");
        let payload = b"payload";
        let upper = digest_of(payload).to_ascii_uppercase();
        let entry = availability("https://example.invalid/Godot.zip", &upper);

        assert!(install(&entry, &dir, &Fake::serving(payload)).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_installed_directory_is_created_if_it_is_not_there() {
        let dir = unique_dir("mkdir").join("nested").join("deeper");
        let payload = b"payload";
        let entry = availability("https://example.invalid/Godot.zip", &digest_of(payload));

        assert!(install(&entry, &dir, &Fake::serving(payload)).is_ok());
        assert!(dir.join("Godot.zip").is_file());
        let _ = std::fs::remove_dir_all(dir.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn a_file_name_is_taken_from_the_end_of_the_url() {
        assert_eq!(
            availability("https://example.invalid/a/b/Godot_v4.7.zip", "x").file_name(),
            Some("Godot_v4.7.zip")
        );
        assert_eq!(
            availability("https://example.invalid/", "x").file_name(),
            None,
            "a url ending in a slash names no file"
        );
        assert_eq!(
            availability("https://example.invalid/Godot", "x").file_name(),
            None,
            "a name with no extension is not a build"
        );
    }

    #[test]
    fn matches_agrees_with_install() {
        let dir = unique_dir("matches");
        let payload = b"payload";
        let entry = availability("https://example.invalid/Godot.zip", &digest_of(payload));
        let installed = install(&entry, &dir, &Fake::serving(payload)).expect("install");

        assert!(matches(&installed, &entry));
        assert!(!matches(
            &installed,
            &availability("https://example.invalid/Godot.zip", &digest_of(b"other"))
        ));
        assert!(
            !matches(
                &installed,
                &availability("https://example.invalid/Godot.zip", "")
            ),
            "an entry with no digest matches nothing"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_refusal_explains_itself() {
        let mismatch = DownloadError::Mismatch {
            path: PathBuf::from("E:/godot.zip"),
            expected: "aaaa".into(),
            actual: "bbbb".into(),
        };
        let text = mismatch.to_string();
        assert!(text.contains("aaaa") && text.contains("bbbb"));
        assert!(text.contains("nothing was installed"));

        let missing = DownloadError::NoDigest {
            url: "https://example.invalid/g.zip".into(),
        };
        assert!(missing.to_string().contains("refusing"));
    }
}
