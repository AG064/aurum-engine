//! # aurum-content
//!
//! Rust-first content authoring for Aurum: procedural meshes, scene graphs,
//! animations, and sprite atlases, exported as **glTF 2.0**.
//!
//! ## Why glTF
//!
//! glTF is the one format that everything already speaks:
//!
//! - Rust can write it with `serde_json` alone, so this crate adds **no
//!   external dependencies**.
//! - Godot imports it natively, so Aurum never hand-writes a fragile `.tscn`.
//! - Blender exports it natively, which is what makes Blender *optional*
//!   rather than required.
//!
//! See `docs/superpowers/specs/2026-09-11-aurum-content-design.md`.
//!
//! ## Shape of the API
//!
//! Build geometry with [`mesh`], assemble it with [`scene`], animate it with
//! [`anim`], then hand the whole [`Document`] to [`gltf::export`]:
//!
//! ```
//! use aurum_content::{box_mesh, Animation, Document, Material, TrackPath};
//!
//! let mut document = Document::new("demo");
//!
//! let cube = document.add_mesh(box_mesh(1.0, 1.0, 1.0));
//! let gold = document.add_material(Material::gold());
//!
//! let node = document.scene_mut().add_node("Cube");
//! let n = document.scene_mut().node_mut(node).unwrap();
//! n.mesh = Some(cube);
//! n.material = Some(gold);
//!
//! document.add_animation(Animation::new("Spin").spin(node, [0.0, 1.0, 0.0], 1.0, 2.0, 5));
//!
//! let (json, binary) = aurum_content::gltf::export(&document).unwrap();
//! assert!(json.contains("\"2.0\""));
//! assert!(!binary.is_empty());
//!
//! // The animation really did make it into the file.
//! let value: serde_json::Value = serde_json::from_str(&json).unwrap();
//! assert_eq!(value["animations"][0]["channels"][0]["target"]["path"], "rotation");
//! assert_eq!(
//!     document.animations[0].track_for(node, TrackPath::Rotation).unwrap().times.len(),
//!     5
//! );
//! ```

pub mod anim;
pub mod gltf;
pub mod gltf_import;
pub mod mesh;
pub mod model;
pub mod png;
pub mod scene;
pub mod sprite;

pub use anim::{Animation, Interpolation, Track, TrackPath};
pub use mesh::{box_mesh, cone, cylinder, plane_mesh, torus, uv_sphere, Mat4, Mesh};
pub use scene::{Material, Node, Scene};
pub use sprite::{Atlas, AtlasRegion, Rect};

/// Every asset kind in one exportable unit.
///
/// A `Document` is self-contained: meshes, materials, a node hierarchy, and
/// animations. Exporting it produces a `.gltf` plus the `.bin` buffer it
/// references.
#[derive(Debug, Clone, Default)]
pub struct Document {
    /// Written into the glTF `asset.generator` field.
    pub name: String,
    pub meshes: Vec<Mesh>,
    pub materials: Vec<Material>,
    pub scene: Scene,
    pub animations: Vec<Animation>,
}

impl Document {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Default::default()
        }
    }

    /// Add a mesh and return its index.
    pub fn add_mesh(&mut self, mesh: Mesh) -> usize {
        self.meshes.push(mesh);
        self.meshes.len() - 1
    }

    /// Add a material and return its index.
    pub fn add_material(&mut self, material: Material) -> usize {
        self.materials.push(material);
        self.materials.len() - 1
    }

    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    pub fn scene_mut(&mut self) -> &mut Scene {
        &mut self.scene
    }

    pub fn add_animation(&mut self, animation: Animation) -> usize {
        self.animations.push(animation);
        self.animations.len() - 1
    }

    /// Merge another document into this one, remapping its indices.
    ///
    /// This is how an imported asset (say, a Blender mesh) joins a scene that
    /// is already being built: mesh, material, node, and animation references
    /// are all shifted so the two documents coexist.
    pub fn append(&mut self, other: Document) {
        let mesh_offset = self.meshes.len();
        let material_offset = self.materials.len();
        let node_offset = self.scene.nodes.len();

        self.meshes.extend(other.meshes);
        self.materials.extend(other.materials);

        for mut node in other.scene.nodes {
            node.mesh = node.mesh.map(|m| m + mesh_offset);
            node.material = node.material.map(|m| m + material_offset);
            node.children = node.children.iter().map(|c| c + node_offset).collect();
            self.scene.nodes.push(node);
        }
        for root in other.scene.roots {
            self.scene.roots.push(root + node_offset);
        }
        for mut animation in other.animations {
            for track in &mut animation.tracks {
                track.node += node_offset;
            }
            self.animations.push(animation);
        }
    }

    /// Structural problems that would make an export invalid or useless.
    pub fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();

        for (index, mesh) in self.meshes.iter().enumerate() {
            for problem in mesh.validate() {
                problems.push(format!("mesh {index} ('{}'): {problem}", mesh.name));
            }
        }

        let node_count = self.scene.nodes.len();
        for (index, node) in self.scene.nodes.iter().enumerate() {
            if let Some(mesh) = node.mesh {
                if mesh >= self.meshes.len() {
                    problems.push(format!("node {index} references unknown mesh {mesh}"));
                }
            }
            if let Some(material) = node.material {
                if material >= self.materials.len() {
                    problems.push(format!(
                        "node {index} references unknown material {material}"
                    ));
                }
            }
            for &child in &node.children {
                if child >= node_count {
                    problems.push(format!("node {index} references unknown child {child}"));
                }
                if child == index {
                    problems.push(format!("node {index} is its own child"));
                }
            }
        }

        for root in &self.scene.roots {
            if *root >= node_count {
                problems.push(format!("scene references unknown root node {root}"));
            }
        }

        for (index, animation) in self.animations.iter().enumerate() {
            for track in &animation.tracks {
                if track.node >= node_count {
                    problems.push(format!(
                        "animation {index} targets unknown node {}",
                        track.node
                    ));
                }
                if track.times.len() != track.value_count() {
                    problems.push(format!(
                        "animation {index} track on node {} has {} times for {} values",
                        track.node,
                        track.times.len(),
                        track.value_count()
                    ));
                }
            }
        }

        problems
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_document_is_valid() {
        assert!(Document::new("empty").validate().is_empty());
    }

    #[test]
    fn validate_catches_dangling_references() {
        let mut document = Document::new("broken");
        let node = document.scene_mut().add_node("Ghost");
        document.scene_mut().node_mut(node).unwrap().mesh = Some(7);
        document.scene_mut().node_mut(node).unwrap().material = Some(3);
        let problems = document.validate();
        assert!(problems.iter().any(|p| p.contains("unknown mesh 7")));
        assert!(problems.iter().any(|p| p.contains("unknown material 3")));
    }

    #[test]
    fn validate_catches_a_mesh_cycle() {
        let mut document = Document::new("cyclic");
        let a = document.scene_mut().add_node("A");
        document.scene_mut().node_mut(a).unwrap().children.push(a);
        assert!(document
            .validate()
            .iter()
            .any(|p| p.contains("its own child")));
    }

    #[test]
    fn append_remaps_every_index() {
        let mut target = Document::new("target");
        let existing = target.add_mesh(box_mesh(1.0, 1.0, 1.0));
        let existing_material = target.add_material(Material::new("Existing"));
        let existing_node = target.scene_mut().add_node("Existing");
        {
            let node = target.scene_mut().node_mut(existing_node).unwrap();
            node.mesh = Some(existing);
            node.material = Some(existing_material);
        }

        let mut incoming = Document::new("incoming");
        let mesh = incoming.add_mesh(box_mesh(2.0, 2.0, 2.0));
        let material = incoming.add_material(Material::new("Incoming"));
        let parent = incoming.scene_mut().add_node("Parent");
        let child = incoming.scene_mut().add_child(Some(parent), "Child");
        for index in [parent, child] {
            let node = incoming.scene_mut().node_mut(index).unwrap();
            node.mesh = Some(mesh);
            node.material = Some(material);
        }
        incoming.add_animation(Animation::new("Spin").spin(parent, [0.0, 1.0, 0.0], 1.0, 1.0, 3));

        target.append(incoming);

        assert_eq!(target.meshes.len(), 2);
        assert_eq!(target.materials.len(), 2);
        assert_eq!(target.scene.len(), 3);
        assert_eq!(
            target.scene.roots.len(),
            2,
            "the incoming root is added too"
        );

        // The appended nodes must point at the appended mesh and material.
        let appended_parent = target.scene.node(1).unwrap();
        assert_eq!(appended_parent.mesh, Some(1), "mesh index was not remapped");
        assert_eq!(
            appended_parent.material,
            Some(1),
            "material index was not remapped"
        );
        assert_eq!(
            appended_parent.children,
            vec![2],
            "child index was not remapped"
        );

        // The animation must follow its node to the new position.
        let track = &target.animations[0].tracks[0];
        assert_eq!(track.node, 1, "animation target was not remapped");

        assert!(target.validate().is_empty(), "{:?}", target.validate());
    }

    #[test]
    fn document_records_generator_name() {
        let document = Document::new("my-scene");
        assert_eq!(document.name, "my-scene");
    }
}
