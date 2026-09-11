//! Give every build of the extension its own identity.
//!
//! The editor plugin publishes the fingerprint the loaded extension reports, and
//! that is how a reload is proven: a fingerprint that moved between builds is
//! evidence the running code changed, and one that did not is evidence of
//! nothing. Without an identity that varies per build there is nothing to move,
//! so the strongest claim available was "the editor has not contradicted us".
//!
//! The identity is derived from the sources this library is built from, not
//! from a timestamp. A timestamp would change on every compile and force a
//! rebuild every time, including when nothing had changed, which would destroy
//! the up-to-date check that keeps the dev loop from reinstalling an identical
//! library. Deriving it from content means it moves exactly when the code does.
//!
//! Only the crates this one is actually built from are read. Hashing the whole
//! workspace would make an unrelated edit elsewhere invalidate this library and
//! rebuild it for no reason, which is the same waste by a slower route.

use std::path::{Path, PathBuf};

/// The crates whose contents can change what this library does.
///
/// Kept in step with the dependencies in `Cargo.toml` by hand, because a build
/// script cannot ask Cargo what it already resolved. Missing one would mean an
/// edit to it produced a stale identity, which the reload report would then
/// describe as "no evidence" rather than as the bug it is.
const SOURCES: [&str; 4] = [".", "../aurum-core", "../aurum-space", "../aurum-vn"];

fn main() {
    for source in SOURCES {
        println!("cargo:rerun-if-changed={source}");
    }

    let mut files = Vec::new();
    for source in SOURCES {
        collect(Path::new(source), &mut files);
    }
    // Sorted so the identifier depends on the set of sources and their
    // contents, not on the order the filesystem happened to hand them over.
    files.sort();

    println!("cargo:rustc-env=AURUM_BUILD_ID={}", identity(&files));
    println!("cargo:rerun-if-changed=build.rs");
}

/// A short, stable name for a particular set of sources.
///
/// FNV-1a, by hand, because this is an identifier and not a security property:
/// nothing is authenticated by it, and the only thing it has to do is differ
/// when the inputs differ and agree when they agree.
fn identity(files: &[PathBuf]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |bytes: &[u8]| {
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };

    for path in files {
        // The path is mixed in as well as the contents, so moving a
        // definition between files counts as a change even when the bytes are
        // identical.
        feed(path.to_string_lossy().as_bytes());
        if let Ok(contents) = std::fs::read(path) {
            feed(&contents);
        }
    }

    format!("build-{hash:016x}")
}

/// Gather the Rust and manifest files under `dir`.
fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            // Build output is not a source, and reading it would make the
            // identity depend on the previous build.
            if name == "target" || name == ".git" || name == "node_modules" {
                continue;
            }
            collect(&path, out);
        } else if matches!(
            path.extension().and_then(|extension| extension.to_str()),
            Some("rs") | Some("toml")
        ) {
            out.push(path);
        }
    }
}
