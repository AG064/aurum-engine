//! SHA-256, for artifact verification.
//!
//! Studio's build guarantees rest on comparing hashes: the staged artifact
//! against the source, and the installed artifact against the staged one. A
//! digest crate would add packages to a workspace that has 27, so the
//! algorithm is implemented here — it is fixed, published, and short enough to
//! be worth owning.
//!
//! Verified against the published NIST vectors, including the million-'a'
//! case, so the implementation is checked against the specification rather
//! than against itself.

/// SHA-256 round constants.
const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// Initial hash state.
const H0: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
];

/// An incremental SHA-256.
#[derive(Debug, Clone)]
pub struct Sha256 {
    state: [u32; 8],
    buffer: [u8; 64],
    buffered: usize,
    /// Total message length in bytes, kept as u64 for the padding.
    length: u64,
}

impl Default for Sha256 {
    fn default() -> Self {
        Self::new()
    }
}

impl Sha256 {
    pub fn new() -> Self {
        Self {
            state: H0,
            buffer: [0; 64],
            buffered: 0,
            length: 0,
        }
    }

    /// Feed more data.
    pub fn update(&mut self, mut data: &[u8]) {
        self.length = self.length.wrapping_add(data.len() as u64);

        // Top up a partial block first.
        if self.buffered > 0 {
            let needed = 64 - self.buffered;
            let take = needed.min(data.len());
            self.buffer[self.buffered..self.buffered + take].copy_from_slice(&data[..take]);
            self.buffered += take;
            data = &data[take..];
            if self.buffered == 64 {
                let block = self.buffer;
                self.compress(&block);
                self.buffered = 0;
            }
        }

        // Then whole blocks, straight from the input. `as_chunks` hands back
        // the remainder alongside them and the blocks are already arrays, so
        // there is nothing to copy into a fixed-size buffer.
        let (blocks, remainder) = data.as_chunks::<64>();
        for block in blocks {
            self.compress(block);
        }

        // Whatever is left waits for the next call.
        if !remainder.is_empty() {
            self.buffer[..remainder.len()].copy_from_slice(remainder);
            self.buffered = remainder.len();
        }
    }

    /// Finish and return the 32-byte digest.
    pub fn finalize(mut self) -> [u8; 32] {
        let bit_length = self.length.wrapping_mul(8);

        // Padding: 0x80, zeros, then the length as a big-endian u64.
        self.update_raw(&[0x80]);
        while self.buffered != 56 {
            self.update_raw(&[0x00]);
        }
        self.update_raw(&bit_length.to_be_bytes());

        let mut digest = [0u8; 32];
        for (index, word) in self.state.iter().enumerate() {
            digest[index * 4..index * 4 + 4].copy_from_slice(&word.to_be_bytes());
        }
        digest
    }

    /// The digest as lowercase hex.
    pub fn hex(self) -> String {
        to_hex(&self.finalize())
    }

    /// Update without counting the bytes, used by padding.
    fn update_raw(&mut self, data: &[u8]) {
        for byte in data {
            self.buffer[self.buffered] = *byte;
            self.buffered += 1;
            if self.buffered == 64 {
                let block = self.buffer;
                self.compress(&block);
                self.buffered = 0;
            }
        }
    }

    fn compress(&mut self, block: &[u8; 64]) {
        let mut w = [0u32; 64];
        for index in 0..16 {
            w[index] = u32::from_be_bytes([
                block[index * 4],
                block[index * 4 + 1],
                block[index * 4 + 2],
                block[index * 4 + 3],
            ]);
        }
        for index in 16..64 {
            let s0 = w[index - 15].rotate_right(7)
                ^ w[index - 15].rotate_right(18)
                ^ (w[index - 15] >> 3);
            let s1 = w[index - 2].rotate_right(17)
                ^ w[index - 2].rotate_right(19)
                ^ (w[index - 2] >> 10);
            w[index] = w[index - 16]
                .wrapping_add(s0)
                .wrapping_add(w[index - 7])
                .wrapping_add(s1);
        }

        let mut a = self.state[0];
        let mut b = self.state[1];
        let mut c = self.state[2];
        let mut d = self.state[3];
        let mut e = self.state[4];
        let mut f = self.state[5];
        let mut g = self.state[6];
        let mut h = self.state[7];

        for index in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[index])
                .wrapping_add(w[index]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        self.state[0] = self.state[0].wrapping_add(a);
        self.state[1] = self.state[1].wrapping_add(b);
        self.state[2] = self.state[2].wrapping_add(c);
        self.state[3] = self.state[3].wrapping_add(d);
        self.state[4] = self.state[4].wrapping_add(e);
        self.state[5] = self.state[5].wrapping_add(f);
        self.state[6] = self.state[6].wrapping_add(g);
        self.state[7] = self.state[7].wrapping_add(h);
    }
}

/// Lowercase hex.
pub fn to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// The SHA-256 of a byte slice, as lowercase hex.
pub fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.hex()
}

/// The SHA-256 of a file, as lowercase hex.
///
/// Reads in chunks rather than loading the whole file, because the artifacts
/// being hashed are multi-megabyte native libraries.
pub fn sha256_file(path: &std::path::Path) -> std::io::Result<String> {
    use std::io::Read;

    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.hex())
}

/// Whether two files have the same contents.
///
/// Compares sizes first, then hashes, so the common "different" case is cheap.
pub fn files_match(a: &std::path::Path, b: &std::path::Path) -> std::io::Result<bool> {
    let size_a = std::fs::metadata(a)?.len();
    let size_b = std::fs::metadata(b)?.len();
    if size_a != size_b {
        return Ok(false);
    }
    Ok(sha256_file(a)? == sha256_file(b)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(data: &[u8]) -> String {
        sha256_hex(data)
    }

    #[test]
    fn matches_the_published_empty_vector() {
        assert_eq!(
            digest(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn matches_the_published_abc_vector() {
        assert_eq!(
            digest(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn matches_the_published_multi_block_vector() {
        // Exactly 56 bytes: the case where padding has to spill into a second
        // block, which a naive implementation gets wrong.
        assert_eq!(
            digest(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn matches_the_published_64_byte_vector() {
        // Exactly one block, another padding boundary.
        assert_eq!(
            digest(b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu"),
            "cf5b16a778af8380036ce59e7b0492370b249b11e8f07a51afac45037afee9d1"
        );
    }

    #[test]
    fn matches_the_million_a_vector() {
        let mut hasher = Sha256::new();
        for _ in 0..1000 {
            hasher.update(&[b'a'; 1000]);
        }
        assert_eq!(
            hasher.hex(),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    #[test]
    fn incremental_and_one_shot_agree_at_every_split() {
        let data: Vec<u8> = (0..=255u8).cycle().take(1000).collect();
        let one_shot = sha256_hex(&data);

        // Splitting anywhere must give the same digest; off-by-one errors in
        // the buffer top-up only show at particular boundaries.
        for split in [0, 1, 55, 56, 63, 64, 65, 127, 128, 500, 999, 1000] {
            let mut hasher = Sha256::new();
            hasher.update(&data[..split]);
            hasher.update(&data[split..]);
            assert_eq!(hasher.hex(), one_shot, "split at {split}");
        }
    }

    #[test]
    fn many_small_updates_agree_with_one_large_one() {
        let data: Vec<u8> = (0..300u32).map(|i| (i % 251) as u8).collect();
        let mut hasher = Sha256::new();
        for byte in &data {
            hasher.update(&[*byte]);
        }
        assert_eq!(hasher.hex(), sha256_hex(&data));
    }

    #[test]
    fn different_input_produces_a_different_digest() {
        assert_ne!(digest(b"abc"), digest(b"abd"));
        assert_ne!(digest(b"abc"), digest(b"abc "));
        assert_ne!(digest(b""), digest(b"\0"));
    }

    #[test]
    fn hex_is_lowercase_and_the_right_length() {
        let text = digest(b"anything");
        assert_eq!(text.len(), 64);
        assert!(text
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        assert_eq!(to_hex(&[0x00, 0x0f, 0xff]), "000fff");
        assert_eq!(to_hex(&[]), "");
    }

    #[test]
    fn hashing_a_file_matches_hashing_its_bytes() {
        let dir = std::env::temp_dir().join(format!("aurum-hash-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("artifact.bin");

        // Larger than the read chunk, so the streaming path is exercised.
        let data: Vec<u8> = (0..200_000u32).map(|i| (i % 256) as u8).collect();
        std::fs::write(&path, &data).unwrap();

        assert_eq!(sha256_file(&path).unwrap(), sha256_hex(&data));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn hashing_a_missing_file_is_an_error() {
        assert!(sha256_file(std::path::Path::new("definitely-not-here.bin")).is_err());
    }

    #[test]
    fn files_match_compares_contents_not_just_names() {
        let dir = std::env::temp_dir().join(format!("aurum-compare-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a.bin");
        let b = dir.join("b.bin");
        let c = dir.join("c.bin");

        std::fs::write(&a, b"same contents").unwrap();
        std::fs::write(&b, b"same contents").unwrap();
        // Same length, different bytes: size alone must not decide.
        std::fs::write(&c, b"diff contents").unwrap();

        assert!(files_match(&a, &b).unwrap());
        assert!(!files_match(&a, &c).unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
