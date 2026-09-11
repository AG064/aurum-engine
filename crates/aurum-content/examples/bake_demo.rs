//! Build a small scene, export it as glTF, and write the GDScript that bakes
//! it into a Godot `.tscn` with a script attached.
//!
//! Used by `scripts/tests/bake_scene.ps1`.
//!
//! ```pwsh
//! cargo run -p aurum-content --example bake_demo -- <dir>
//! ```

use std::path::PathBuf;

use aurum_content::gdscript::{bake_scene, BakeOptions};
use aurum_content::{box_mesh, uv_sphere, Animation, Document, Material};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory: PathBuf = std::env::args()
        .nth(1)
        .unwrap_or_else(|| ".".to_string())
        .into();

    let mut document = Document::new("baked");

    let cube = document.add_mesh(box_mesh(1.0, 1.0, 1.0));
    let orb_mesh = document.add_mesh(uv_sphere(0.4, 20, 14));
    let gold = document.add_material(Material::gold());
    let glass = document.add_material(Material::new("Glass").with_alpha(0.5));

    // A deliberately shallow hierarchy, so the bake script's node paths are
    // obvious: Hero at the top level, Orb directly under it.
    let hero = document.scene_mut().add_node("Hero");
    {
        let node = document.scene_mut().node_mut(hero).unwrap();
        node.mesh = Some(cube);
        node.material = Some(gold);
        node.translation = [0.0, 1.0, 0.0];
    }
    let orb = document.scene_mut().add_child(Some(hero), "Orb");
    {
        let node = document.scene_mut().node_mut(orb).unwrap();
        node.mesh = Some(orb_mesh);
        node.material = Some(glass);
        node.translation = [1.5, 0.0, 0.0];
    }

    document.add_animation(Animation::new("HeroSpin").spin(hero, [0.0, 1.0, 0.0], 1.0, 2.0, 9));

    let exported = aurum_content::gltf::write(&document, &directory, "baked")?;

    // The attachment path is relative to the instantiated glTF scene root.
    // Godot wraps our glTF scene, so the node is found by name from the root.
    let options = BakeOptions::new("res://baked.gltf", "res://baked.tscn")
        .with_root_name("BakedScene")
        .attach("Hero", "res://scripts/spin.gd");

    let problems = aurum_content::gdscript::validate(&options);
    if !problems.is_empty() {
        for problem in &problems {
            eprintln!("invalid bake options: {problem}");
        }
        return Err("bake options failed validation".into());
    }

    let script = bake_scene(&document, &options);
    std::fs::write(directory.join("bake.gd"), &script)?;

    println!(
        "wrote {} and {}/bake.gd ({} bytes)",
        exported.gltf.display(),
        directory.display(),
        script.len()
    );
    Ok(())
}
