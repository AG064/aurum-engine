//! Decode a PNG and report what was in it.
//!
//! ```pwsh
//! cargo run -p aurum-content --example decode_png -- sprite.png
//! ```

use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let Some(argument) = std::env::args().nth(1) else {
        eprintln!("usage: decode_png <file.png>");
        std::process::exit(2);
    };
    let path = PathBuf::from(&argument);
    let image = aurum_content::png::decode_file(&path)?;

    let mut opaque = 0usize;
    let mut transparent = 0usize;
    let mut sum = [0u64; 4];
    for pixel in image.pixels.chunks_exact(4) {
        if pixel[3] == 0 {
            transparent += 1;
        } else {
            opaque += 1;
        }
        for channel in 0..4 {
            sum[channel] += pixel[channel] as u64;
        }
    }
    let total = (image.width as usize * image.height as usize).max(1);

    println!("file        : {}", path.display());
    println!("size        : {}x{}", image.width, image.height);
    println!("pixels      : {total} ({opaque} opaque, {transparent} transparent)");
    println!(
        "mean rgba   : ({}, {}, {}, {})",
        sum[0] / total as u64,
        sum[1] / total as u64,
        sum[2] / total as u64,
        sum[3] / total as u64
    );

    // Re-encoding proves the decoded pixels are a usable image.
    let png = image.to_png()?;
    println!("re-encode   : {} bytes", png.len());
    Ok(())
}
