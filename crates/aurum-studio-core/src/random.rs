//! Randomness for session identifiers and bridge tokens.
//!
//! The design requires a cryptographically random token per session, passed to
//! the child process through its environment. A randomness crate would add
//! packages, so this reaches the operating system's own generator directly:
//! `BCryptGenRandom` on Windows, `/dev/urandom` elsewhere.
//!
//! There is a fallback for the case where neither is available. It is seeded
//! from `RandomState`, which the standard library seeds from the OS, and it is
//! **reported as a fallback** rather than passed off as equivalent — a caller
//! that needs a guarantee should be able to see whether it got one.

/// Where a batch of random bytes came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The operating system's generator.
    OperatingSystem,
    /// A userspace generator seeded from the OS.
    ///
    /// Adequate for an identifier that only has to be unique, weaker than the
    /// system generator for anything defending against prediction.
    SeededFallback,
}

/// Random bytes, and where they came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RandomBytes {
    pub bytes: Vec<u8>,
    pub source: Source,
}

/// Fill a buffer from the operating system's generator.
pub fn fill(buffer: &mut [u8]) -> Source {
    #[cfg(windows)]
    {
        if windows::fill(buffer) {
            return Source::OperatingSystem;
        }
    }
    #[cfg(not(windows))]
    {
        if unix::fill(buffer) {
            return Source::OperatingSystem;
        }
    }
    seeded_fallback(buffer);
    Source::SeededFallback
}

/// `length` random bytes.
pub fn bytes(length: usize) -> RandomBytes {
    let mut buffer = vec![0u8; length];
    let source = fill(&mut buffer);
    RandomBytes {
        bytes: buffer,
        source,
    }
}

/// A lowercase hex string of `byte_length` random bytes.
pub fn hex(byte_length: usize) -> String {
    let random = bytes(byte_length);
    let mut out = String::with_capacity(random.bytes.len() * 2);
    for byte in &random.bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// A session identifier: 16 random bytes, hex encoded.
pub fn session_id() -> String {
    hex(16)
}

/// A bridge token: 32 random bytes, hex encoded, matching the 64-character
/// token the Phase 0 editor bridge already uses.
pub fn token() -> String {
    hex(32)
}

/// A userspace fallback, seeded from the standard library's OS-seeded hasher.
fn seeded_fallback(buffer: &mut [u8]) {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};

    // Each RandomState draws a fresh OS-seeded key, so mixing several and
    // advancing a counter between them spreads the entropy across the buffer.
    let mut written = 0;
    let mut counter: u64 = 0;
    while written < buffer.len() {
        let state = RandomState::new();
        let mut hasher = state.build_hasher();
        hasher.write_u64(counter);
        hasher.write_u64(std::process::id() as u64);
        if let Ok(now) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
            hasher.write_u128(now.as_nanos());
        }
        let value = hasher.finish().to_le_bytes();
        let take = value.len().min(buffer.len() - written);
        buffer[written..written + take].copy_from_slice(&value[..take]);
        written += take;
        counter += 1;
    }
}

#[cfg(windows)]
mod windows {
    // `BCRYPT_USE_SYSTEM_PREFERRED_RNG` asks for the system generator and
    // permits a null algorithm handle.
    const BCRYPT_USE_SYSTEM_PREFERRED_RNG: u32 = 0x0000_0002;

    #[link(name = "bcrypt")]
    unsafe extern "system" {
        fn BCryptGenRandom(
            algorithm: *mut core::ffi::c_void,
            buffer: *mut u8,
            length: u32,
            flags: u32,
        ) -> i32;
    }

    pub fn fill(buffer: &mut [u8]) -> bool {
        if buffer.is_empty() {
            return true;
        }
        // SAFETY: the pointer and length describe a live, writable slice, the
        // algorithm handle is null because the system generator was requested,
        // and the call neither retains the pointer nor expects it to outlive
        // the call.
        let status = unsafe {
            BCryptGenRandom(
                core::ptr::null_mut(),
                buffer.as_mut_ptr(),
                buffer.len() as u32,
                BCRYPT_USE_SYSTEM_PREFERRED_RNG,
            )
        };
        // NTSTATUS success is zero.
        status == 0
    }
}

#[cfg(not(windows))]
mod unix {
    use std::io::Read;

    pub fn fill(buffer: &mut [u8]) -> bool {
        let Ok(mut file) = std::fs::File::open("/dev/urandom") else {
            return false;
        };
        file.read_exact(buffer).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn the_operating_system_supplies_the_bytes() {
        let random = bytes(32);
        assert_eq!(random.bytes.len(), 32);
        assert_eq!(
            random.source,
            Source::OperatingSystem,
            "the platform generator should be available"
        );
    }

    #[test]
    fn successive_calls_differ() {
        // A generator that returns a constant would pass every length check
        // and defeat the entire point of a session token.
        let first = bytes(32);
        let second = bytes(32);
        assert_ne!(first.bytes, second.bytes);
    }

    #[test]
    fn many_draws_do_not_repeat() {
        let mut seen = HashSet::new();
        for _ in 0..256 {
            assert!(
                seen.insert(bytes(16).bytes),
                "a duplicate draw suggests a broken generator"
            );
        }
    }

    #[test]
    fn bytes_are_not_all_zero() {
        // A silently failing FFI call tends to produce zeros.
        let random = bytes(64);
        assert!(
            random.bytes.iter().any(|b| *b != 0),
            "an all-zero buffer suggests the generator did not run"
        );
    }

    #[test]
    fn hex_is_lowercase_and_the_right_length() {
        let text = hex(16);
        assert_eq!(text.len(), 32);
        assert!(text
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }

    #[test]
    fn session_ids_and_tokens_have_the_documented_shapes() {
        // The editor bridge expects a 64-character token; Phase 0 already
        // publishes one of that length, so the two must agree.
        assert_eq!(session_id().len(), 32);
        assert_eq!(token().len(), 64);
        assert_ne!(session_id(), session_id());
    }

    #[test]
    fn an_empty_buffer_needs_no_randomness() {
        let mut buffer: [u8; 0] = [];
        assert_eq!(fill(&mut buffer), Source::OperatingSystem);
    }

    #[test]
    fn the_fallback_fills_the_whole_buffer_and_varies() {
        let mut first = [0u8; 64];
        let mut second = [0u8; 64];
        seeded_fallback(&mut first);
        seeded_fallback(&mut second);
        assert!(first.iter().any(|b| *b != 0));
        assert_ne!(
            first, second,
            "the fallback should still vary between calls"
        );
    }

    #[test]
    fn the_fallback_handles_odd_lengths() {
        for length in [1usize, 7, 8, 9, 33] {
            let mut buffer = vec![0u8; length];
            seeded_fallback(&mut buffer);
            assert_eq!(buffer.len(), length);
        }
    }
}
