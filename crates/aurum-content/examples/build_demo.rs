//! Generate a demo scene as glTF, for eyeballing and for the Godot import
//! check in `scripts/tests/gltf_import.ps1`.
//!
//! ```pwsh
//! cargo run -p aurum-content --example build_demo -- out/
//! ```

use std::path::PathBuf;

use aurum_content::{
    box_mesh, cone, cylinder, plane_mesh, torus, uv_sphere, Animation, Document, Interpolation,
    Mat4, Material, Mesh,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory: PathBuf = std::env::args()
        .nth(1)
        .unwrap_or_else(|| ".".to_string())
        .into();

    let mut document = Document::new("aurum-demo");

    // -- materials ---------------------------------------------------------
    let gold = document.add_material(Material::gold());
    let glass = document.add_material(
        Material::new("Glass")
            .with_color(0.6, 0.8, 1.0)
            .with_alpha(0.35)
            .with_metallic_roughness(0.0, 0.1),
    );
    let ground_mat = document.add_material(
        Material::new("Ground")
            .with_color(0.25, 0.3, 0.28)
            .with_metallic_roughness(0.0, 0.9),
    );
    let neon = document.add_material(
        Material::new("Neon")
            .with_color(0.1, 0.9, 0.7)
            .with_emissive(0.1, 0.9, 0.7)
            .with_metallic_roughness(0.2, 0.4),
    );

    // -- meshes ------------------------------------------------------------
    let cube = document.add_mesh(box_mesh(1.0, 1.0, 1.0));
    let sphere = document.add_mesh(uv_sphere(0.6, 32, 20));
    let pillar = document.add_mesh(cylinder(0.25, 2.0, 20));
    let spike = document.add_mesh(cone(0.4, 1.0, 20));
    let ring = document.add_mesh(torus(0.7, 0.12, 32, 14));
    let ground = document.add_mesh(plane_mesh(12.0, 12.0));

    // A merged mesh, to exercise index offsets and a compound object.
    let mut arch = cylinder(0.15, 1.6, 12);
    arch.merge(&Mesh::transformed_copy(
        &cylinder(0.15, 1.6, 12),
        &Mat4::translation(1.0, 0.0, 0.0),
    ));
    arch.merge(&Mesh::transformed_copy(
        &box_mesh(1.3, 0.2, 0.3),
        &Mat4::translation(0.5, 0.8, 0.0),
    ));
    let arch = document.add_mesh(arch);

    // -- scene -------------------------------------------------------------
    let root = document.scene_mut().add_node("Demo");

    let ground_node = document.scene_mut().add_child(Some(root), "Ground");
    {
        let node = document.scene_mut().node_mut(ground_node).unwrap();
        node.mesh = Some(ground);
        node.material = Some(ground_mat);
        node.scale = [2.0, 1.0, 2.0];
    }

    let hero = document.scene_mut().add_child(Some(root), "Hero");
    {
        let node = document.scene_mut().node_mut(hero).unwrap();
        node.mesh = Some(cube);
        node.material = Some(gold);
        node.translation = [0.0, 0.5, 0.0];
    }

    let orb = document.scene_mut().add_child(Some(hero), "OrbitingOrb");
    {
        let node = document.scene_mut().node_mut(orb).unwrap();
        node.mesh = Some(sphere);
        node.material = Some(glass);
        node.translation = [1.6, 0.4, 0.0];
    }

    let ring_node = document.scene_mut().add_child(Some(hero), "Ring");
    {
        let node = document.scene_mut().node_mut(ring_node).unwrap();
        node.mesh = Some(ring);
        node.material = Some(neon);
        node.translation = [0.0, 1.2, 0.0];
    }

    for (index, x) in [-3.0f32, 3.0].iter().enumerate() {
        let pillar_node = document
            .scene_mut()
            .add_child(Some(root), format!("Pillar{index}"));
        let node = document.scene_mut().node_mut(pillar_node).unwrap();
        node.mesh = Some(pillar);
        node.material = Some(gold);
        node.translation = [*x, 1.0, -3.0];
    }

    let spike_node = document.scene_mut().add_child(Some(root), "Spike");
    {
        let node = document.scene_mut().node_mut(spike_node).unwrap();
        node.mesh = Some(spike);
        node.material = Some(neon);
        node.translation = [0.0, 0.5, 2.5];
    }

    let arch_node = document.scene_mut().add_child(Some(root), "Arch");
    {
        let node = document.scene_mut().node_mut(arch_node).unwrap();
        node.mesh = Some(arch);
        node.material = Some(gold);
        node.translation = [-1.0, 0.8, 3.0];
    }

    // -- animation ---------------------------------------------------------
    // The hero turns; the orb bobs; the ring spins faster.
    let spin = Animation::new("HeroSpin").spin(hero, [0.0, 1.0, 0.0], 1.0, 4.0, 9);
    document.add_animation(spin);

    let bob = Animation::new("OrbBob").translate(
        orb,
        &[0.0, 1.0, 2.0],
        &[[1.6, 0.4, 0.0], [1.6, 1.1, 0.0], [1.6, 0.4, 0.0]],
        Interpolation::Linear,
    );
    document.add_animation(bob);

    let ring_spin = Animation::new("RingSpin").spin(ring_node, [0.0, 0.0, 1.0], 2.0, 4.0, 9);
    document.add_animation(ring_spin);

    // -- export ------------------------------------------------------------
    let problems = document.validate();
    if !problems.is_empty() {
        for problem in &problems {
            eprintln!("invalid: {problem}");
        }
        return Err("document failed validation".into());
    }

    let exported = aurum_content::gltf::write(&document, &directory, "aurum-demo")?;
    let total: usize = document.meshes.iter().map(|m| m.triangle_count()).sum();
    println!(
        "wrote {} ({} bytes) and {}\n{} meshes, {} triangles, {} nodes, {} animations",
        exported.gltf.display(),
        exported.bytes,
        exported.bin.display(),
        document.meshes.len(),
        total,
        document.scene.len(),
        document.animations.len(),
    );
    Ok(())
}
