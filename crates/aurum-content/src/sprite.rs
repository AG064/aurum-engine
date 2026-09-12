//! Sprite work: atlas packing and a dependency-free PNG encoder.
//!
//! PNG needs a DEFLATE stream, which would normally mean pulling in a
//! compression crate. DEFLATE has a *stored* block type that carries bytes
//! verbatim, so a fully valid PNG can be written with no compressor at all.
//! The cost is file size on disk; the benefit is that sprite support adds no
//! dependency, matching the project's supply-chain policy.
//!
//! The tests include a small decoder for exactly these stored-block streams,
//! so a round trip is verified rather than assumed.

use serde::{Deserialize, Serialize};

/// Something that went wrong building sprite content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpriteError {
    /// The pixel buffer size does not match the stated dimensions.
    SizeMismatch { expected: usize, actual: usize },
    /// A dimension was zero.
    EmptyImage,
    /// An image was too large for the 32-bit PNG field.
    TooLarge,
}

impl std::fmt::Display for SpriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SizeMismatch { expected, actual } => {
                write!(f, "expected {expected} bytes of RGBA, got {actual}")
            }
            Self::EmptyImage => write!(f, "image dimensions must be non-zero"),
            Self::TooLarge => write!(f, "image is too large to encode"),
        }
    }
}

impl std::error::Error for SpriteError {}

/// An integer rectangle in atlas pixel space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    pub fn area(&self) -> u32 {
        self.w * self.h
    }

    /// Normalized texture coordinates, ready for a UV attribute.
    ///
    /// PNG/glTF place the UV origin at the top-left, which is also where the
    /// packer counts from, so no vertical flip is needed.
    pub fn uv(&self, atlas_width: u32, atlas_height: u32) -> [f32; 4] {
        let w = atlas_width.max(1) as f32;
        let h = atlas_height.max(1) as f32;
        [
            self.x as f32 / w,
            self.y as f32 / h,
            (self.x + self.w) as f32 / w,
            (self.y + self.h) as f32 / h,
        ]
    }
}

/// A named sprite placed in an atlas.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AtlasRegion {
    pub name: String,
    pub rect: Rect,
}

/// The layout of a packed sprite atlas.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Atlas {
    pub width: u32,
    pub height: u32,
    pub regions: Vec<AtlasRegion>,
}

impl Atlas {
    pub fn region(&self, name: &str) -> Option<&AtlasRegion> {
        self.regions.iter().find(|r| r.name == name)
    }

    /// Fraction of the atlas covered by sprites. A low ratio means the shelf
    /// packer wasted space and a narrower atlas would do.
    pub fn occupancy(&self) -> f32 {
        let total = (self.width * self.height) as f32;
        if total <= 0.0 {
            return 0.0;
        }
        let used: u32 = self.regions.iter().map(|r| r.rect.area()).sum();
        used as f32 / total
    }
}

/// Pack rectangles using shelf packing: tallest first, rows filled left to
/// right.
///
/// Shelf packing is not optimal, but it is linear, deterministic, and more
/// than adequate for sprite sheets — and determinism matters more here,
/// because an agent regenerating an atlas should get the same layout.
pub fn pack(entries: &[(String, u32, u32)], max_width: u32) -> Atlas {
    let max_width = max_width.max(1);
    let mut sorted: Vec<&(String, u32, u32)> = entries.iter().collect();
    // Tallest first, then widest, then by name so ties are stable.
    sorted.sort_by(|a, b| b.2.cmp(&a.2).then(b.1.cmp(&a.1)).then(a.0.cmp(&b.0)));

    let mut regions = Vec::with_capacity(sorted.len());
    let mut shelf_y = 0u32;
    let mut shelf_height = 0u32;
    let mut cursor_x = 0u32;
    let mut used_width = 0u32;

    for (name, w, h) in sorted {
        let (w, h) = ((*w).max(1), (*h).max(1));
        // Start a new shelf when this sprite will not fit on the current one.
        if cursor_x > 0 && cursor_x + w > max_width {
            shelf_y += shelf_height;
            shelf_height = 0;
            cursor_x = 0;
        }
        regions.push(AtlasRegion {
            name: name.clone(),
            rect: Rect {
                x: cursor_x,
                y: shelf_y,
                w,
                h,
            },
        });
        cursor_x += w;
        used_width = used_width.max(cursor_x);
        shelf_height = shelf_height.max(h);
    }

    // Regions were emitted in sorted order; present them by input order so
    // callers can index the result predictably.
    regions.sort_by_key(|r| {
        entries
            .iter()
            .position(|(name, _, _)| name == &r.name)
            .unwrap_or(usize::MAX)
    });

    Atlas {
        width: used_width.max(1),
        height: (shelf_y + shelf_height).max(1),
        regions,
    }
}

/// A mutable RGBA8 image, used to compose atlases.
#[derive(Debug, Clone, PartialEq)]
pub struct AtlasImage {
    pub width: u32,
    pub height: u32,
    /// Row-major RGBA, four bytes per pixel.
    pub pixels: Vec<u8>,
}

impl AtlasImage {
    /// A fully transparent image.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![0; (width as usize) * (height as usize) * 4],
        }
    }

    /// A solid colour image.
    pub fn filled(width: u32, height: u32, rgba: [u8; 4]) -> Self {
        let mut image = Self::new(width, height);
        for pixel in image.pixels.as_chunks_mut::<4>().0 {
            pixel.copy_from_slice(&rgba);
        }
        image
    }

    /// Copy a source image into this one at `(x, y)`.
    ///
    /// Clips at the edges rather than failing, so an oversized sprite cannot
    /// corrupt neighbouring regions.
    pub fn blit(&mut self, source: &AtlasImage, x: u32, y: u32) {
        for row in 0..source.height {
            let dst_y = y + row;
            if dst_y >= self.height {
                break;
            }
            for column in 0..source.width {
                let dst_x = x + column;
                if dst_x >= self.width {
                    break;
                }
                let src = ((row * source.width + column) * 4) as usize;
                let dst = ((dst_y * self.width + dst_x) * 4) as usize;
                self.pixels[dst..dst + 4].copy_from_slice(&source.pixels[src..src + 4]);
            }
        }
    }

    /// Encode as a PNG.
    pub fn to_png(&self) -> Result<Vec<u8>, SpriteError> {
        encode_png(self.width, self.height, &self.pixels)
    }
}

/// Encode RGBA8 pixels as a PNG.
pub fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, SpriteError> {
    if width == 0 || height == 0 {
        return Err(SpriteError::EmptyImage);
    }
    let expected = (width as usize) * (height as usize) * 4;
    if rgba.len() != expected {
        return Err(SpriteError::SizeMismatch {
            expected,
            actual: rgba.len(),
        });
    }

    // Raw scanlines: a zero filter byte, then the row's bytes.
    let stride = (width as usize) * 4;
    let mut raw = Vec::with_capacity((stride + 1) * height as usize);
    for row in 0..height as usize {
        raw.push(0u8);
        raw.extend_from_slice(&rgba[row * stride..(row + 1) * stride]);
    }

    let mut out = Vec::new();
    out.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.push(8); // bit depth
    ihdr.push(6); // colour type: RGBA
    ihdr.push(0); // compression: deflate
    ihdr.push(0); // filter method
    ihdr.push(0); // interlace: none
    write_chunk(&mut out, b"IHDR", &ihdr);

    write_chunk(&mut out, b"IDAT", &zlib_stored(&raw));
    write_chunk(&mut out, b"IEND", &[]);
    Ok(out)
}

/// Wrap bytes in a zlib stream built entirely from DEFLATE stored blocks.
fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 16);
    // CMF=0x78 (deflate, 32K window), FLG=0x01 makes the header a multiple of 31.
    out.push(0x78);
    out.push(0x01);

    const MAX_BLOCK: usize = 65_535;
    if data.is_empty() {
        out.extend_from_slice(&[0x01, 0x00, 0x00, 0xFF, 0xFF]);
    } else {
        let mut offset = 0;
        while offset < data.len() {
            let len = MAX_BLOCK.min(data.len() - offset);
            let final_block = offset + len >= data.len();
            out.push(if final_block { 0x01 } else { 0x00 });
            let len16 = len as u16;
            out.extend_from_slice(&len16.to_le_bytes());
            out.extend_from_slice(&(!len16).to_le_bytes());
            out.extend_from_slice(&data[offset..offset + len]);
            offset += len;
        }
    }

    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

fn write_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc_input = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(kind);
    crc_input.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

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

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in data {
        a = (a + byte as u32) % 65_521;
        b = (b + a) % 65_521;
    }
    (b << 16) | a
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- a decoder for exactly the stored-block streams we emit ------------

    /// Decode our own PNG back to RGBA, verifying structure as it goes.
    fn decode_our_png(png: &[u8]) -> (u32, u32, Vec<u8>) {
        assert_eq!(
            &png[..8],
            &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A],
            "signature"
        );

        let mut offset = 8;
        let mut width = 0u32;
        let mut height = 0u32;
        let mut idat = Vec::new();

        while offset < png.len() {
            let len = u32::from_be_bytes(png[offset..offset + 4].try_into().unwrap()) as usize;
            let kind = &png[offset + 4..offset + 8];
            let data = &png[offset + 8..offset + 8 + len];
            let stored_crc =
                u32::from_be_bytes(png[offset + 8 + len..offset + 12 + len].try_into().unwrap());

            let mut crc_input = Vec::new();
            crc_input.extend_from_slice(kind);
            crc_input.extend_from_slice(data);
            assert_eq!(crc32(&crc_input), stored_crc, "CRC mismatch in chunk");

            match kind {
                b"IHDR" => {
                    width = u32::from_be_bytes(data[0..4].try_into().unwrap());
                    height = u32::from_be_bytes(data[4..8].try_into().unwrap());
                    assert_eq!(data[8], 8, "bit depth");
                    assert_eq!(data[9], 6, "colour type must be RGBA");
                    assert_eq!(data[12], 0, "interlace must be none");
                }
                b"IDAT" => idat.extend_from_slice(data),
                b"IEND" => break,
                _ => {}
            }
            offset += 12 + len;
        }

        // zlib header, then stored blocks.
        assert_eq!(idat[0], 0x78);
        let header = u16::from_be_bytes([idat[0], idat[1]]);
        assert_eq!(header % 31, 0, "zlib header check bits");

        let mut raw = Vec::new();
        let mut cursor = 2;
        loop {
            let header_byte = idat[cursor];
            let final_block = header_byte & 1 == 1;
            assert_eq!(header_byte & 0b110, 0, "only stored blocks are emitted");
            let len = u16::from_le_bytes([idat[cursor + 1], idat[cursor + 2]]) as usize;
            let nlen = u16::from_le_bytes([idat[cursor + 3], idat[cursor + 4]]);
            assert_eq!(nlen, !(len as u16), "NLEN must be the complement of LEN");
            raw.extend_from_slice(&idat[cursor + 5..cursor + 5 + len]);
            cursor += 5 + len;
            if final_block {
                break;
            }
        }
        let stored_adler = u32::from_be_bytes(idat[cursor..cursor + 4].try_into().unwrap());
        assert_eq!(stored_adler, adler32(&raw), "adler32 mismatch");

        // Undo the per-scanline filter bytes.
        let stride = width as usize * 4;
        let mut pixels = Vec::with_capacity(stride * height as usize);
        for row in 0..height as usize {
            assert_eq!(raw[row * (stride + 1)], 0, "only filter 0 is emitted");
            let start = row * (stride + 1) + 1;
            pixels.extend_from_slice(&raw[start..start + stride]);
        }
        (width, height, pixels)
    }

    #[test]
    fn png_round_trips_through_our_own_decoder() {
        let image = AtlasImage::filled(3, 2, [10, 20, 30, 255]);
        let png = image.to_png().unwrap();
        let (w, h, pixels) = decode_our_png(&png);
        assert_eq!((w, h), (3, 2));
        assert_eq!(pixels, image.pixels);
    }

    #[test]
    fn png_round_trips_a_large_image_across_multiple_blocks() {
        // Larger than one 65535-byte stored block, to exercise the chunking.
        let mut image = AtlasImage::new(200, 120);
        for (i, pixel) in image.pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            pixel.copy_from_slice(&[(i % 251) as u8, (i % 241) as u8, (i % 239) as u8, 255]);
        }
        let png = image.to_png().unwrap();
        let (w, h, pixels) = decode_our_png(&png);
        assert_eq!((w, h), (200, 120));
        assert_eq!(pixels, image.pixels, "multi-block data was altered");
    }

    #[test]
    fn png_rejects_a_mismatched_buffer() {
        let err = encode_png(2, 2, &[0u8; 4]).unwrap_err();
        assert_eq!(
            err,
            SpriteError::SizeMismatch {
                expected: 16,
                actual: 4
            }
        );
    }

    #[test]
    fn png_rejects_empty_dimensions() {
        assert_eq!(encode_png(0, 4, &[]).unwrap_err(), SpriteError::EmptyImage);
    }

    #[test]
    fn blit_places_pixels_and_clips_at_the_edge() {
        let mut canvas = AtlasImage::new(4, 4);
        let sprite = AtlasImage::filled(2, 2, [255, 0, 0, 255]);

        canvas.blit(&sprite, 1, 1);
        // Pixel (1,1) is red, (0,0) is untouched.
        let at = |img: &AtlasImage, x: u32, y: u32| {
            let i = ((y * img.width + x) * 4) as usize;
            [
                img.pixels[i],
                img.pixels[i + 1],
                img.pixels[i + 2],
                img.pixels[i + 3],
            ]
        };
        assert_eq!(at(&canvas, 1, 1), [255, 0, 0, 255]);
        assert_eq!(at(&canvas, 2, 2), [255, 0, 0, 255]);
        assert_eq!(at(&canvas, 0, 0), [0, 0, 0, 0]);
        assert_eq!(at(&canvas, 3, 3), [0, 0, 0, 0]);

        // Partially off-canvas: must clip, not panic or wrap.
        let mut clipped = AtlasImage::new(2, 2);
        clipped.blit(&sprite, 1, 1);
        assert_eq!(at(&clipped, 1, 1), [255, 0, 0, 255]);
    }

    #[test]
    fn pack_places_everything_without_overlap() {
        let entries = vec![
            ("hero".to_string(), 32, 32),
            ("enemy".to_string(), 24, 16),
            ("coin".to_string(), 8, 8),
            ("tile".to_string(), 32, 16),
        ];
        let atlas = pack(&entries, 64);

        assert_eq!(atlas.regions.len(), 4);
        // Input order is preserved in the output.
        assert_eq!(
            atlas
                .regions
                .iter()
                .map(|r| r.name.as_str())
                .collect::<Vec<_>>(),
            vec!["hero", "enemy", "coin", "tile"]
        );

        for region in &atlas.regions {
            assert!(region.rect.x + region.rect.w <= atlas.width);
            assert!(region.rect.y + region.rect.h <= atlas.height);
        }
        for (i, a) in atlas.regions.iter().enumerate() {
            for b in atlas.regions.iter().skip(i + 1) {
                let overlap = a.rect.x < b.rect.x + b.rect.w
                    && b.rect.x < a.rect.x + a.rect.w
                    && a.rect.y < b.rect.y + b.rect.h
                    && b.rect.y < a.rect.y + a.rect.h;
                assert!(!overlap, "{} overlaps {}", a.name, b.name);
            }
        }
        assert!(atlas.occupancy() > 0.0 && atlas.occupancy() <= 1.0);
    }

    #[test]
    fn pack_wraps_to_a_new_shelf_past_max_width() {
        let entries = vec![
            ("a".to_string(), 40, 10),
            ("b".to_string(), 40, 10),
            ("c".to_string(), 40, 10),
        ];
        let atlas = pack(&entries, 50);
        assert!(atlas.height >= 30, "three shelves were expected");
        assert!(atlas.width <= 50);
    }

    #[test]
    fn pack_is_deterministic() {
        let entries = vec![
            ("x".to_string(), 16, 16),
            ("y".to_string(), 16, 16),
            ("z".to_string(), 16, 16),
        ];
        assert_eq!(pack(&entries, 64), pack(&entries, 64));
    }

    #[test]
    fn pack_handles_empty_and_degenerate_input() {
        let empty = pack(&[], 64);
        assert!(empty.regions.is_empty());
        assert!(empty.width >= 1 && empty.height >= 1);

        let zero = pack(&[("z".to_string(), 0, 0)], 64);
        assert_eq!(zero.regions[0].rect.w, 1, "zero sizes clamp to 1");
    }

    #[test]
    fn rect_uv_normalizes_to_the_atlas() {
        let rect = Rect {
            x: 0,
            y: 0,
            w: 50,
            h: 100,
        };
        let uv = rect.uv(100, 100);
        assert_eq!(uv, [0.0, 0.0, 0.5, 1.0]);
    }

    #[test]
    fn atlas_region_lookup_by_name() {
        let atlas = pack(&[("hero".to_string(), 8, 8)], 64);
        assert!(atlas.region("hero").is_some());
        assert!(atlas.region("missing").is_none());
    }

    #[test]
    fn composed_atlas_encodes_and_round_trips() {
        let entries = vec![("hero".to_string(), 4, 4), ("coin".to_string(), 2, 2)];
        let atlas = pack(&entries, 8);
        let mut image = AtlasImage::new(atlas.width, atlas.height);

        let hero = AtlasImage::filled(4, 4, [0, 255, 0, 255]);
        let coin = AtlasImage::filled(2, 2, [255, 255, 0, 255]);
        let hero_rect = atlas.region("hero").unwrap().rect;
        let coin_rect = atlas.region("coin").unwrap().rect;
        image.blit(&hero, hero_rect.x, hero_rect.y);
        image.blit(&coin, coin_rect.x, coin_rect.y);

        let png = image.to_png().unwrap();
        let (w, h, pixels) = decode_our_png(&png);
        assert_eq!((w, h), (atlas.width, atlas.height));
        assert_eq!(pixels, image.pixels);

        // The hero's green really landed inside its own rectangle.
        let i = ((hero_rect.y * w + hero_rect.x) * 4) as usize;
        assert_eq!(&pixels[i..i + 4], &[0, 255, 0, 255]);
    }
}
