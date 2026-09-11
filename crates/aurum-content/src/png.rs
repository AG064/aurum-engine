//! PNG decoding, including a complete DEFLATE decompressor.
//!
//! Reading PNG means implementing inflate. This module does that from scratch
//! — stored, fixed-Huffman, and dynamic-Huffman blocks — so sprite work needs
//! no compression crate and the workspace stays dependency-free.
//!
//! ## What is supported
//!
//! - All five scanline filters, including Paeth.
//! - Colour types 0, 2, 3, 4, and 6 (grey, RGB, palette, grey+alpha, RGBA).
//! - Bit depths 1, 2, 4, 8, and 16; everything is returned as RGBA8.
//! - Chunk CRCs are verified, so a truncated or corrupted file is reported
//!   rather than decoded into noise.
//!
//! ## What is refused
//!
//! Adam7 interlacing. It is rare in sprite sheets and in anything Godot or
//! Aseprite exports, and a clear error is better than a scrambled image.

use crate::sprite::AtlasImage;

/// Why a PNG could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PngError {
    /// The file does not start with the PNG signature.
    NotPng,
    /// A chunk was truncated or its CRC did not match.
    Corrupt(String),
    /// The file uses a feature this decoder does not implement.
    Unsupported(String),
    /// The compressed data was not valid DEFLATE.
    Inflate(String),
}

impl std::fmt::Display for PngError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotPng => write!(f, "not a PNG file"),
            Self::Corrupt(m) => write!(f, "corrupt PNG: {m}"),
            Self::Unsupported(m) => write!(f, "unsupported PNG feature: {m}"),
            Self::Inflate(m) => write!(f, "invalid compressed data: {m}"),
        }
    }
}

impl std::error::Error for PngError {}

const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

// ---------------------------------------------------------------------------
// Bit reader
// ---------------------------------------------------------------------------

/// Reads bits least-significant-first, which is what DEFLATE requires.
struct BitReader<'a> {
    data: &'a [u8],
    position: usize,
    bit_count: u32,
    buffer: u32,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            position: 0,
            bit_count: 0,
            buffer: 0,
        }
    }

    fn bits(&mut self, count: u32) -> Result<u32, PngError> {
        debug_assert!(count <= 24);
        while self.bit_count < count {
            if self.position >= self.data.len() {
                return Err(PngError::Inflate("ran out of input".into()));
            }
            self.buffer |= (self.data[self.position] as u32) << self.bit_count;
            self.position += 1;
            self.bit_count += 8;
        }
        let value = self.buffer & ((1u32 << count) - 1);
        self.buffer >>= count;
        self.bit_count -= count;
        Ok(value)
    }

    /// Discard buffered bits and align to the next byte boundary.
    fn align(&mut self) {
        let drop = self.bit_count % 8;
        self.buffer >>= drop;
        self.bit_count -= drop;
    }
}

// ---------------------------------------------------------------------------
// Huffman
// ---------------------------------------------------------------------------

/// A canonical Huffman decoder, built from code lengths.
struct Huffman {
    /// How many codes exist at each length, indexed by length.
    counts: [u16; 16],
    /// Symbols ordered by code.
    symbols: Vec<u16>,
}

impl Huffman {
    fn new(lengths: &[u8]) -> Result<Self, PngError> {
        let mut counts = [0u16; 16];
        for &length in lengths {
            if length as usize >= counts.len() {
                return Err(PngError::Inflate(format!(
                    "code length {length} is invalid"
                )));
            }
            counts[length as usize] += 1;
        }
        // Length zero means "unused", not "one code of length zero".
        counts[0] = 0;

        let mut offsets = [0u16; 16];
        for length in 1..15 {
            offsets[length + 1] = offsets[length] + counts[length];
        }

        let mut symbols = vec![0u16; lengths.len()];
        for (symbol, &length) in lengths.iter().enumerate() {
            if length != 0 {
                symbols[offsets[length as usize] as usize] = symbol as u16;
                offsets[length as usize] += 1;
            }
        }
        Ok(Self { counts, symbols })
    }

    fn decode(&self, reader: &mut BitReader<'_>) -> Result<u16, PngError> {
        let mut code = 0i32;
        let mut first = 0i32;
        let mut index = 0i32;

        for length in 1..=15 {
            code |= reader.bits(1)? as i32;
            let count = self.counts[length] as i32;
            if code - first < count {
                let slot = (index + (code - first)) as usize;
                return self
                    .symbols
                    .get(slot)
                    .copied()
                    .ok_or_else(|| PngError::Inflate("huffman symbol out of range".into()));
            }
            index += count;
            first = (first + count) << 1;
            code <<= 1;
        }
        Err(PngError::Inflate("invalid huffman code".into()))
    }
}

// ---------------------------------------------------------------------------
// DEFLATE
// ---------------------------------------------------------------------------

const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DISTANCE_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DISTANCE_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

/// The order code lengths are stored in for a dynamic block.
const CODE_LENGTH_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

/// Decompress a zlib stream (DEFLATE with a two-byte header and an Adler-32).
pub fn inflate_zlib(data: &[u8]) -> Result<Vec<u8>, PngError> {
    if data.len() < 2 {
        return Err(PngError::Inflate("zlib stream is too short".into()));
    }
    let header = u16::from_be_bytes([data[0], data[1]]);
    if !header.is_multiple_of(31) {
        return Err(PngError::Inflate("zlib header check bits failed".into()));
    }
    if data[0] & 0x0F != 8 {
        return Err(PngError::Unsupported(format!(
            "compression method {} (only deflate is defined)",
            data[0] & 0x0F
        )));
    }
    if data[1] & 0x20 != 0 {
        return Err(PngError::Unsupported("zlib preset dictionaries".into()));
    }
    inflate_deflate(&data[2..])
}

/// Decompress a raw DEFLATE stream.
pub fn inflate_deflate(data: &[u8]) -> Result<Vec<u8>, PngError> {
    let mut reader = BitReader::new(data);
    let mut out = Vec::new();

    loop {
        let final_block = reader.bits(1)? == 1;
        let block_type = reader.bits(2)?;

        match block_type {
            0 => {
                // Stored: align to a byte, then read LEN and the raw bytes.
                reader.align();
                let len = reader.bits(16)? as usize;
                let nlen = reader.bits(16)? as usize;
                if len != (!nlen & 0xFFFF) {
                    return Err(PngError::Inflate(
                        "stored block length and its complement disagree".into(),
                    ));
                }
                for _ in 0..len {
                    out.push(reader.bits(8)? as u8);
                }
            }
            1 => {
                let (literal, distance) = fixed_tables()?;
                inflate_block(&mut reader, &mut out, &literal, &distance)?;
            }
            2 => {
                let (literal, distance) = dynamic_tables(&mut reader)?;
                inflate_block(&mut reader, &mut out, &literal, &distance)?;
            }
            other => {
                return Err(PngError::Inflate(format!("reserved block type {other}")));
            }
        }

        if final_block {
            break;
        }
        // A stream that never sets the final bit would loop forever otherwise.
        if reader.position > data.len() {
            return Err(PngError::Inflate(
                "stream ended without a final block".into(),
            ));
        }
    }

    Ok(out)
}

fn fixed_tables() -> Result<(Huffman, Huffman), PngError> {
    // Literal/length codes 0-143 are 8 bits, 144-255 are 9, 256-279 are 7,
    // and 280-287 are 8 bits.
    let mut literal_lengths = vec![0u8; 288];
    literal_lengths[0..144].fill(8);
    literal_lengths[144..256].fill(9);
    literal_lengths[256..280].fill(7);
    literal_lengths[280..288].fill(8);

    let distance_lengths = vec![5u8; 30];
    Ok((
        Huffman::new(&literal_lengths)?,
        Huffman::new(&distance_lengths)?,
    ))
}

fn dynamic_tables(reader: &mut BitReader<'_>) -> Result<(Huffman, Huffman), PngError> {
    let literal_count = reader.bits(5)? as usize + 257;
    let distance_count = reader.bits(5)? as usize + 1;
    let code_length_count = reader.bits(4)? as usize + 4;

    if literal_count > 286 || distance_count > 30 {
        return Err(PngError::Inflate(
            "dynamic block declares too many codes".into(),
        ));
    }

    let mut code_lengths = [0u8; 19];
    for &slot in CODE_LENGTH_ORDER.iter().take(code_length_count) {
        code_lengths[slot] = reader.bits(3)? as u8;
    }
    let code_length_table = Huffman::new(&code_lengths)?;

    // The literal and distance code lengths are run-length encoded.
    let total = literal_count + distance_count;
    let mut lengths = Vec::with_capacity(total);
    while lengths.len() < total {
        let symbol = code_length_table.decode(reader)?;
        match symbol {
            0..=15 => lengths.push(symbol as u8),
            16 => {
                let previous = *lengths
                    .last()
                    .ok_or_else(|| PngError::Inflate("repeat with no previous length".into()))?;
                let repeat = 3 + reader.bits(2)? as usize;
                lengths.extend(std::iter::repeat_n(previous, repeat));
            }
            17 => {
                let repeat = 3 + reader.bits(3)? as usize;
                lengths.extend(std::iter::repeat_n(0u8, repeat));
            }
            18 => {
                let repeat = 11 + reader.bits(7)? as usize;
                lengths.extend(std::iter::repeat_n(0u8, repeat));
            }
            other => {
                return Err(PngError::Inflate(format!(
                    "invalid code length symbol {other}"
                )))
            }
        }
        if lengths.len() > total {
            return Err(PngError::Inflate(
                "code length run overruns the table".into(),
            ));
        }
    }

    let literal = Huffman::new(&lengths[..literal_count])?;
    let distance = Huffman::new(&lengths[literal_count..])?;
    Ok((literal, distance))
}

fn inflate_block(
    reader: &mut BitReader<'_>,
    out: &mut Vec<u8>,
    literal: &Huffman,
    distance: &Huffman,
) -> Result<(), PngError> {
    loop {
        let symbol = literal.decode(reader)?;
        match symbol {
            0..=255 => out.push(symbol as u8),
            256 => return Ok(()), // End of block.
            257..=285 => {
                let index = symbol as usize - 257;
                let length =
                    LENGTH_BASE[index] as usize + reader.bits(LENGTH_EXTRA[index] as u32)? as usize;

                let distance_symbol = distance.decode(reader)? as usize;
                if distance_symbol >= DISTANCE_BASE.len() {
                    return Err(PngError::Inflate("invalid distance code".into()));
                }
                let back = DISTANCE_BASE[distance_symbol] as usize
                    + reader.bits(DISTANCE_EXTRA[distance_symbol] as u32)? as usize;

                if back > out.len() || back == 0 {
                    return Err(PngError::Inflate(format!(
                        "distance {back} reaches before the start of the output"
                    )));
                }
                // Overlapping copies are legal and must be byte-by-byte.
                let start = out.len() - back;
                for offset in 0..length {
                    let byte = out[start + offset];
                    out.push(byte);
                }
            }
            other => {
                return Err(PngError::Inflate(format!("invalid literal code {other}")));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// PNG
// ---------------------------------------------------------------------------

fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, entry) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
        *entry = c;
    }
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc = table[((crc ^ byte as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

/// Decode a PNG into RGBA8 pixels.
pub fn decode(bytes: &[u8]) -> Result<AtlasImage, PngError> {
    if bytes.len() < 8 || bytes[..8] != SIGNATURE {
        return Err(PngError::NotPng);
    }

    let mut width = 0u32;
    let mut height = 0u32;
    let mut bit_depth = 0u8;
    let mut colour_type = 0u8;
    let mut interlace = 0u8;
    let mut palette: Vec<[u8; 3]> = Vec::new();
    let mut transparency: Option<Vec<u8>> = None;
    let mut compressed = Vec::new();
    let mut seen_header = false;
    let mut seen_end = false;

    let mut offset = 8;
    while offset + 8 <= bytes.len() {
        let length =
            u32::from_be_bytes(bytes[offset..offset + 4].try_into().expect("4 bytes")) as usize;
        let kind = &bytes[offset + 4..offset + 8];
        let start = offset + 8;
        let end = start
            .checked_add(length)
            .ok_or_else(|| PngError::Corrupt("chunk length overflow".into()))?;
        if end + 4 > bytes.len() {
            return Err(PngError::Corrupt(format!(
                "chunk '{}' is truncated",
                String::from_utf8_lossy(kind)
            )));
        }
        let data = &bytes[start..end];
        let stored_crc = u32::from_be_bytes(bytes[end..end + 4].try_into().expect("4 bytes"));

        let mut crc_input = Vec::with_capacity(4 + data.len());
        crc_input.extend_from_slice(kind);
        crc_input.extend_from_slice(data);
        if crc32(&crc_input) != stored_crc {
            return Err(PngError::Corrupt(format!(
                "CRC mismatch in '{}' chunk",
                String::from_utf8_lossy(kind)
            )));
        }

        match kind {
            b"IHDR" => {
                if data.len() != 13 {
                    return Err(PngError::Corrupt("IHDR is not 13 bytes".into()));
                }
                width = u32::from_be_bytes(data[0..4].try_into().expect("4"));
                height = u32::from_be_bytes(data[4..8].try_into().expect("4"));
                bit_depth = data[8];
                colour_type = data[9];
                if data[10] != 0 {
                    return Err(PngError::Unsupported(format!(
                        "compression method {}",
                        data[10]
                    )));
                }
                if data[11] != 0 {
                    return Err(PngError::Unsupported(format!("filter method {}", data[11])));
                }
                interlace = data[12];
                seen_header = true;
                if width == 0 || height == 0 {
                    return Err(PngError::Corrupt("zero-sized image".into()));
                }
            }
            b"PLTE" => {
                palette = data.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect();
            }
            b"tRNS" => transparency = Some(data.to_vec()),
            b"IDAT" => compressed.extend_from_slice(data),
            b"IEND" => {
                seen_end = true;
                break;
            }
            _ => {} // Ancillary chunks are not needed to reconstruct pixels.
        }

        offset = end + 4;
    }

    if !seen_header {
        return Err(PngError::Corrupt("no IHDR chunk".into()));
    }
    // Every valid PNG ends with IEND, so its absence means truncation.
    if !seen_end {
        return Err(PngError::Corrupt(
            "missing IEND chunk; the file is truncated".into(),
        ));
    }
    if interlace != 0 {
        return Err(PngError::Unsupported(
            "Adam7 interlacing; re-export without interlacing".into(),
        ));
    }

    let channels = match colour_type {
        0 => 1, // greyscale
        2 => 3, // RGB
        3 => 1, // palette index
        4 => 2, // greyscale + alpha
        6 => 4, // RGBA
        other => return Err(PngError::Unsupported(format!("colour type {other}"))),
    };
    if !matches!(bit_depth, 1 | 2 | 4 | 8 | 16) {
        return Err(PngError::Unsupported(format!("bit depth {bit_depth}")));
    }
    if bit_depth == 16 && colour_type == 3 {
        return Err(PngError::Corrupt(
            "palette images cannot have a bit depth of 16".into(),
        ));
    }
    if colour_type == 3 && palette.is_empty() {
        return Err(PngError::Corrupt(
            "palette image without a PLTE chunk".into(),
        ));
    }

    let raw = inflate_zlib(&compressed)?;

    // Bits per pixel, used to advance within a scanline.
    let bits_per_pixel = channels as usize * bit_depth as usize;
    let bytes_per_pixel = bits_per_pixel.div_ceil(8).max(1);
    let stride = (width as usize * bits_per_pixel).div_ceil(8);
    let expected = (stride + 1) * height as usize;
    if raw.len() < expected {
        return Err(PngError::Corrupt(format!(
            "expected {expected} bytes of scanline data, got {}",
            raw.len()
        )));
    }

    let mut pixels = vec![0u8; stride * height as usize];
    unfilter(&raw, &mut pixels, stride, height as usize, bytes_per_pixel)?;

    let rgba = to_rgba8(
        &pixels,
        width,
        height,
        bit_depth,
        colour_type,
        &palette,
        transparency.as_deref(),
    )?;

    Ok(AtlasImage {
        width,
        height,
        pixels: rgba,
    })
}

/// Reverse the per-scanline filters, in place into `out`.
fn unfilter(
    raw: &[u8],
    out: &mut [u8],
    stride: usize,
    height: usize,
    bytes_per_pixel: usize,
) -> Result<(), PngError> {
    for row in 0..height {
        let filter = raw[row * (stride + 1)];
        let source = &raw[row * (stride + 1) + 1..row * (stride + 1) + 1 + stride];
        let (previous, current) = out.split_at_mut(row * stride);
        let previous = if row == 0 {
            None
        } else {
            Some(&previous[(row - 1) * stride..])
        };
        let target = &mut current[..stride];

        for x in 0..stride {
            let a = if x >= bytes_per_pixel {
                target[x - bytes_per_pixel]
            } else {
                0
            };
            let b = previous.map(|p| p[x]).unwrap_or(0);
            let c = if x >= bytes_per_pixel {
                previous.map(|p| p[x - bytes_per_pixel]).unwrap_or(0)
            } else {
                0
            };

            let value = match filter {
                0 => source[x],
                1 => source[x].wrapping_add(a),
                2 => source[x].wrapping_add(b),
                3 => source[x].wrapping_add(((a as u16 + b as u16) / 2) as u8),
                4 => source[x].wrapping_add(paeth(a, b, c)),
                other => {
                    return Err(PngError::Corrupt(format!(
                        "unknown filter type {other} on row {row}"
                    )))
                }
            };
            target[x] = value;
        }
    }
    Ok(())
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = a as i16 + b as i16 - c as i16;
    let pa = (p - a as i16).abs();
    let pb = (p - b as i16).abs();
    let pc = (p - c as i16).abs();
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// Read the `index`-th sample of a scanline at the given bit depth.
fn sample(scanline: &[u8], index: usize, bit_depth: u8) -> u16 {
    match bit_depth {
        8 => scanline[index] as u16,
        16 => {
            let at = index * 2;
            u16::from_be_bytes([scanline[at], scanline[at + 1]])
        }
        depth => {
            // Sub-byte samples are packed most-significant-first.
            let per_byte = 8 / depth as usize;
            let byte = scanline[index / per_byte];
            let slot = index % per_byte;
            let shift = 8 - depth as usize * (slot + 1);
            ((byte >> shift) & ((1 << depth) - 1)) as u16
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn to_rgba8(
    pixels: &[u8],
    width: u32,
    height: u32,
    bit_depth: u8,
    colour_type: u8,
    palette: &[[u8; 3]],
    transparency: Option<&[u8]>,
) -> Result<Vec<u8>, PngError> {
    let channels = match colour_type {
        0 => 1,
        2 => 3,
        3 => 1,
        4 => 2,
        _ => 4,
    };
    let bits_per_pixel = channels * bit_depth as usize;
    let stride = (width as usize * bits_per_pixel).div_ceil(8);
    let max_sample = ((1u32 << bit_depth) - 1) as f32;

    // Scale a sample to 0..=255. 16-bit values are narrowed.
    let scale = |value: u16| -> u8 {
        if bit_depth == 16 {
            (value >> 8) as u8
        } else if bit_depth == 8 {
            value as u8
        } else {
            ((value as f32 / max_sample) * 255.0).round() as u8
        }
    };

    let mut out = Vec::with_capacity((width as usize) * (height as usize) * 4);
    for row in 0..height as usize {
        let scanline = &pixels[row * stride..(row + 1) * stride];
        for column in 0..width as usize {
            let base = column * channels;
            let read = |channel: usize| sample(scanline, base + channel, bit_depth);

            let rgba = match colour_type {
                0 => {
                    // Greyscale; tRNS gives a single fully transparent value.
                    let grey = scale(read(0));
                    let raw = read(0);
                    let alpha = match transparency {
                        Some(t) if t.len() >= 2 => {
                            let key = u16::from_be_bytes([t[0], t[1]]);
                            if raw == key {
                                0
                            } else {
                                255
                            }
                        }
                        _ => 255,
                    };
                    [grey, grey, grey, alpha]
                }
                2 => {
                    let alpha = match transparency {
                        Some(t) if t.len() >= 6 => {
                            let key = [
                                u16::from_be_bytes([t[0], t[1]]),
                                u16::from_be_bytes([t[2], t[3]]),
                                u16::from_be_bytes([t[4], t[5]]),
                            ];
                            if read(0) == key[0] && read(1) == key[1] && read(2) == key[2] {
                                0
                            } else {
                                255
                            }
                        }
                        _ => 255,
                    };
                    [scale(read(0)), scale(read(1)), scale(read(2)), alpha]
                }
                3 => {
                    let index = read(0) as usize;
                    let entry = palette.get(index).copied().ok_or_else(|| {
                        PngError::Corrupt(format!("palette index {index} is out of range"))
                    })?;
                    let alpha = transparency
                        .and_then(|t| t.get(index).copied())
                        .unwrap_or(255);
                    [entry[0], entry[1], entry[2], alpha]
                }
                4 => {
                    let grey = scale(read(0));
                    [grey, grey, grey, scale(read(1))]
                }
                _ => [
                    scale(read(0)),
                    scale(read(1)),
                    scale(read(2)),
                    scale(read(3)),
                ],
            };
            out.extend_from_slice(&rgba);
        }
    }
    Ok(out)
}

/// Read and decode a PNG file.
pub fn decode_file(path: &std::path::Path) -> Result<AtlasImage, PngError> {
    let bytes = std::fs::read(path)
        .map_err(|e| PngError::Corrupt(format!("could not read '{}': {e}", path.display())))?;
    decode(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sprite::{encode_png, AtlasImage};

    fn round_trip(width: u32, height: u32, fill: impl Fn(usize) -> [u8; 4]) -> AtlasImage {
        let mut source = AtlasImage::new(width, height);
        for (index, pixel) in source.pixels.chunks_exact_mut(4).enumerate() {
            pixel.copy_from_slice(&fill(index));
        }
        let encoded = encode_png(width, height, &source.pixels).unwrap();
        let decoded = decode(&encoded).unwrap();
        assert_eq!(decoded.width, width);
        assert_eq!(decoded.height, height);
        assert_eq!(
            decoded.pixels, source.pixels,
            "pixels changed in the round trip"
        );
        decoded
    }

    #[test]
    fn inflate_reads_stored_blocks() {
        // Two stored blocks, exercising the non-final then final path.
        let message = b"hello world, this is a test of stored blocks";
        let chunks: Vec<&[u8]> = message.chunks(10).collect();
        let mut stream = vec![0x78, 0x01];
        for (index, chunk) in chunks.iter().enumerate() {
            stream.push(if index == chunks.len() - 1 { 1 } else { 0 });
            let len = chunk.len() as u16;
            stream.extend_from_slice(&len.to_le_bytes());
            stream.extend_from_slice(&(!len).to_le_bytes());
            stream.extend_from_slice(chunk);
        }
        stream.extend_from_slice(&adler(message).to_be_bytes());
        let out = inflate_zlib(&stream).unwrap();
        assert_eq!(out, message);
        assert!(chunks.len() > 1, "the test should span several blocks");
    }

    fn adler(data: &[u8]) -> u32 {
        let (mut a, mut b) = (1u32, 0u32);
        for &byte in data {
            a = (a + byte as u32) % 65521;
            b = (b + a) % 65521;
        }
        (b << 16) | a
    }

    #[test]
    fn inflate_reads_a_fixed_huffman_block() {
        // "abc" compressed as a single fixed-Huffman block, built by hand:
        // literals fit in 8-bit codes and the block ends with symbol 256.
        // Header fields read least-significant-bit first; Huffman codes are
        // packed most-significant-bit first. Mixing the two is the classic
        // way to hand-build a stream that decodes to nonsense.
        let mut bits: Vec<bool> = Vec::new();
        let lsb = |value: u32, count: u32, bits: &mut Vec<bool>| {
            for i in 0..count {
                bits.push((value >> i) & 1 == 1);
            }
        };
        let msb = |value: u32, count: u32, bits: &mut Vec<bool>| {
            for i in (0..count).rev() {
                bits.push((value >> i) & 1 == 1);
            }
        };
        lsb(1, 1, &mut bits); // BFINAL
        lsb(1, 2, &mut bits); // BTYPE = fixed

        // Fixed Huffman: literals 0-143 use code 0x30 + symbol, 8 bits.
        for symbol in [b'a', b'b', b'c'] {
            msb(0x30 + symbol as u32, 8, &mut bits);
        }
        msb(0, 7, &mut bits); // end of block, symbol 256, 7 bits

        // Pack LSB-first into bytes, as DEFLATE requires.
        let mut packed = vec![0u8; bits.len().div_ceil(8)];
        for (index, bit) in bits.iter().enumerate() {
            if *bit {
                packed[index / 8] |= 1 << (index % 8);
            }
        }
        assert_eq!(inflate_deflate(&packed).unwrap(), b"abc");
    }

    #[test]
    fn inflate_reads_a_back_reference() {
        // Fixed Huffman: literal 'a', then a length/distance pair copying it.
        let mut bits: Vec<bool> = Vec::new();
        let lsb = |value: u32, count: u32, bits: &mut Vec<bool>| {
            for i in 0..count {
                bits.push((value >> i) & 1 == 1);
            }
        };
        let msb = |value: u32, count: u32, bits: &mut Vec<bool>| {
            for i in (0..count).rev() {
                bits.push((value >> i) & 1 == 1);
            }
        };
        lsb(1, 1, &mut bits); // BFINAL
        lsb(1, 2, &mut bits); // fixed

        msb(0x30 + b'a' as u32, 8, &mut bits); // literal 'a'
                                               // Length 3 is symbol 257: 7-bit code 0b0000001.
        msb(0b0000001, 7, &mut bits);
        // Distance 1 uses the 5-bit fixed distance code 0.
        msb(0, 5, &mut bits);
        msb(0, 7, &mut bits); // end of block

        let mut packed = vec![0u8; bits.len().div_ceil(8)];
        for (index, bit) in bits.iter().enumerate() {
            if *bit {
                packed[index / 8] |= 1 << (index % 8);
            }
        }
        // "a" plus a 3-byte copy of itself is "aaaa".
        assert_eq!(inflate_deflate(&packed).unwrap(), b"aaaa");
    }

    #[test]
    fn inflate_rejects_corrupt_streams() {
        assert!(inflate_zlib(&[]).is_err());
        // Bad zlib header check bits.
        assert!(inflate_zlib(&[0x78, 0x00]).is_err());
        // A reserved deflate block type.
        assert!(inflate_deflate(&[0b0000_0111]).is_err());
    }

    #[test]
    fn decode_rejects_non_png_input() {
        assert_eq!(decode(b"not a png at all").unwrap_err(), PngError::NotPng);
        assert_eq!(decode(&[]).unwrap_err(), PngError::NotPng);
    }

    #[test]
    fn decode_detects_a_corrupted_chunk() {
        let image = AtlasImage::filled(4, 4, [10, 20, 30, 255]);
        let mut encoded = encode_png(4, 4, &image.pixels).unwrap();
        // Flip a byte inside the IHDR payload.
        encoded[20] ^= 0xFF;
        match decode(&encoded) {
            Err(PngError::Corrupt(message)) => assert!(message.contains("CRC"), "{message}"),
            other => panic!("expected a CRC failure, got {other:?}"),
        }
    }

    #[test]
    fn decode_detects_truncation() {
        let image = AtlasImage::filled(8, 8, [1, 2, 3, 4]);
        let encoded = encode_png(8, 8, &image.pixels).unwrap();
        let truncated = &encoded[..encoded.len() - 12];
        assert!(decode(truncated).is_err());
    }

    #[test]
    fn round_trip_a_solid_image() {
        round_trip(7, 5, |_| [200, 100, 50, 255]);
    }

    #[test]
    fn round_trip_a_gradient() {
        round_trip(16, 16, |i| {
            [
                (i % 256) as u8,
                ((i * 7) % 256) as u8,
                ((i * 13) % 256) as u8,
                255,
            ]
        });
    }

    #[test]
    fn round_trip_an_image_spanning_several_stored_blocks() {
        // Larger than one 65535-byte stored block.
        round_trip(300, 200, |i| {
            [(i % 251) as u8, (i % 241) as u8, (i % 239) as u8, 255]
        });
    }

    #[test]
    fn round_trip_preserves_transparency() {
        let decoded = round_trip(4, 4, |i| [255, 0, 0, if i % 2 == 0 { 0 } else { 255 }]);
        assert_eq!(decoded.pixels[3], 0, "alpha was not preserved");
        assert_eq!(decoded.pixels[7], 255);
    }

    #[test]
    fn paeth_predictor_matches_the_specification() {
        // p = a + b - c = 0, so |p - a| = 10 is the smallest and a wins.
        assert_eq!(paeth(10, 20, 30), 10);
        assert_eq!(paeth(0, 0, 0), 0);
        assert_eq!(paeth(255, 255, 255), 255);
        // p = a + b - c; ties choose a, then b, then c.
        assert_eq!(paeth(1, 1, 1), 1);
        assert_eq!(paeth(200, 100, 100), 200);
    }

    #[test]
    fn unfilter_reverses_each_filter_type() {
        // One row of four bytes, encoded with each filter in turn.
        for filter in 0u8..=4 {
            let original: [u8; 4] = [10, 20, 30, 40];
            // Build a filtered row that should decode back to `original`.
            let mut raw = vec![filter];
            let mut filtered = [0u8; 4];
            for x in 0..4 {
                let a = if x >= 1 { original[x - 1] } else { 0 };
                let b = 0u8; // no previous row
                let c = 0u8;
                filtered[x] = match filter {
                    0 => original[x],
                    1 => original[x].wrapping_sub(a),
                    2 => original[x].wrapping_sub(b),
                    3 => original[x].wrapping_sub(((a as u16 + b as u16) / 2) as u8),
                    _ => original[x].wrapping_sub(paeth(a, b, c)),
                };
            }
            raw.extend_from_slice(&filtered);

            let mut out = vec![0u8; 4];
            unfilter(&raw, &mut out, 4, 1, 1).unwrap();
            assert_eq!(out, original, "filter {filter} did not reverse");
        }
    }

    #[test]
    fn sample_reads_sub_byte_depths_msb_first() {
        // 1-bit: 0b1010_0000 -> 1, 0, 1, 0
        let line = [0b1010_0000u8];
        assert_eq!(sample(&line, 0, 1), 1);
        assert_eq!(sample(&line, 1, 1), 0);
        assert_eq!(sample(&line, 2, 1), 1);
        assert_eq!(sample(&line, 3, 1), 0);

        // 4-bit: 0xAB -> 0xA, 0xB
        let line = [0xABu8];
        assert_eq!(sample(&line, 0, 4), 0xA);
        assert_eq!(sample(&line, 1, 4), 0xB);

        // 8 and 16 bit are straightforward.
        assert_eq!(sample(&[7u8], 0, 8), 7);
        assert_eq!(sample(&[0x12, 0x34], 0, 16), 0x1234);
    }
}
