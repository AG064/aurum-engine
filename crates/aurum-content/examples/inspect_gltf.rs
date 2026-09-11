//! Inspect a glTF or GLB file: summarise what Aurum reads from it.
//!
//! ```pwsh
//! cargo run -p aurum-content --example inspect_gltf -- model.glb
//! ```

use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let Some(argument) = std::env::args().nth(1) else {
        eprintln!("usage: inspect_gltf <file.gltf|file.glb>");
        std::process::exit(2);
    };
    let path = PathBuf::from(&argument);

    let document = aurum_content::gltf_import::import_path(&path)?;

    let triangles: usize = document.meshes.iter().map(|m| m.triangle_count()).sum();
    let vertices: usize = document.meshes.iter().map(|m| m.vertex_count()).sum();

    println!("file       : {}", path.display());
    println!(
        "meshes     : {} ({} vertices, {} triangles)",
        document.meshes.len(),
        vertices,
        triangles
    );
    println!("materials  : {}", document.materials.len());
    println!(
        "nodes      : {} ({} roots)",
        document.scene.len(),
        document.scene.roots.len()
    );
    println!("animations : {}", document.animations.len());

    for animation in document.animations.iter().take(8) {
        println!(
            "  - {} ({} tracks, {:.2}s)",
            animation.name,
            animation.tracks.len(),
            animation.duration()
        );
    }
    for material in document.materials.iter().take(8) {
        println!(
            "  material {}: rgba({:.2}, {:.2}, {:.2}, {:.2}) m={:.2} r={:.2}",
            material.name,
            material.base_color[0],
            material.base_color[1],
            material.base_color[2],
            material.base_color[3],
            material.metallic,
            material.roughness
        );
    }

    let problems = document.validate();
    if problems.is_empty() {
        println!("validate   : clean");
    } else {
        for problem in &problems {
            println!("validate   : {problem}");
        }
    }

    // Re-exporting proves the round trip is lossless enough to be useful.
    match aurum_content::gltf::export(&document) {
        Ok((json, bin)) => println!(
            "re-export  : {} bytes json, {} bytes bin",
            json.len(),
            bin.len()
        ),
        Err(e) => println!("re-export  : FAILED: {e}"),
    }

    Ok(())
}
