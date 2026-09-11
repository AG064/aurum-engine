//! The scene graph: nodes with transforms, hierarchy, and materials.
//!
//! Nodes live in an arena and refer to each other by index, which maps
//! directly onto glTF's `nodes` array — no pointer chasing, no recursive
//! serialization, and cycles are cheap to detect.

use serde::{Deserialize, Serialize};

use crate::mesh::Mat4;

/// Identity quaternion, `[x, y, z, w]`.
pub const IDENTITY_ROTATION: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

/// One node in the scene graph.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub name: String,
    pub translation: [f32; 3],
    /// Quaternion `[x, y, z, w]`.
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
    /// Index into [`crate::Document::meshes`].
    pub mesh: Option<usize>,
    /// Index into [`crate::Document::materials`].
    pub material: Option<usize>,
    /// Indices of child nodes.
    pub children: Vec<usize>,
}

impl Default for Node {
    fn default() -> Self {
        Self {
            name: String::new(),
            translation: [0.0; 3],
            rotation: IDENTITY_ROTATION,
            scale: [1.0; 3],
            mesh: None,
            material: None,
            children: Vec::new(),
        }
    }
}

impl Node {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Default::default()
        }
    }

    /// Builder: place the node.
    pub fn at(mut self, x: f32, y: f32, z: f32) -> Self {
        self.translation = [x, y, z];
        self
    }

    /// Builder: rotate about an axis.
    pub fn rotated(mut self, axis: [f32; 3], radians: f32) -> Self {
        let len = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt();
        if len > f32::EPSILON {
            let half = radians * 0.5;
            let s = half.sin() / len;
            self.rotation = [axis[0] * s, axis[1] * s, axis[2] * s, half.cos()];
        }
        self
    }

    /// Builder: scale the node.
    pub fn scaled(mut self, x: f32, y: f32, z: f32) -> Self {
        self.scale = [x, y, z];
        self
    }

    /// Builder: attach a mesh.
    pub fn with_mesh(mut self, mesh: usize) -> Self {
        self.mesh = Some(mesh);
        self
    }

    /// Builder: attach a material.
    pub fn with_material(mut self, material: usize) -> Self {
        self.material = Some(material);
        self
    }

    /// This node's local transform.
    pub fn local_transform(&self) -> Mat4 {
        Mat4::from_trs(self.translation, self.rotation, self.scale)
    }
}

/// A hierarchy of nodes plus the list of roots to instantiate.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Scene {
    pub nodes: Vec<Node>,
    pub roots: Vec<usize>,
}

impl Scene {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a node at the top level and return its index.
    pub fn add_node(&mut self, name: impl Into<String>) -> usize {
        let index = self.nodes.len();
        self.nodes.push(Node::new(name));
        self.roots.push(index);
        index
    }

    /// Add a node under `parent`, or at the top level when `parent` is `None`
    /// or out of range.
    pub fn add_child(&mut self, parent: Option<usize>, name: impl Into<String>) -> usize {
        let index = self.nodes.len();
        self.nodes.push(Node::new(name));
        match parent.filter(|p| *p < index) {
            Some(parent) => self.nodes[parent].children.push(index),
            None => self.roots.push(index),
        }
        index
    }

    pub fn node(&self, index: usize) -> Option<&Node> {
        self.nodes.get(index)
    }

    pub fn node_mut(&mut self, index: usize) -> Option<&mut Node> {
        self.nodes.get_mut(index)
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Whether `ancestor` is `index` or sits above it.
    pub fn is_ancestor_of(&self, ancestor: usize, index: usize) -> bool {
        if ancestor == index {
            return true;
        }
        let Some(node) = self.nodes.get(ancestor) else {
            return false;
        };
        node.children
            .iter()
            .any(|child| self.is_ancestor_of(*child, index))
    }

    /// Re-parent `child` under `parent`.
    ///
    /// Refuses to create a cycle (including making a node its own parent), and
    /// refuses unknown indices. Returns whether the move happened.
    pub fn set_parent(&mut self, child: usize, parent: Option<usize>) -> bool {
        if child >= self.nodes.len() {
            return false;
        }
        if let Some(parent) = parent {
            if parent >= self.nodes.len() || self.is_ancestor_of(child, parent) {
                return false;
            }
        }
        // Detach from the current parent or the root list.
        self.roots.retain(|r| *r != child);
        for node in &mut self.nodes {
            node.children.retain(|c| *c != child);
        }
        match parent {
            Some(parent) => self.nodes[parent].children.push(child),
            None => self.roots.push(child),
        }
        true
    }

    /// The accumulated world transform of `index`, or the identity when the
    /// node is unreachable.
    pub fn world_transform(&self, index: usize) -> Mat4 {
        fn walk(
            scene: &Scene,
            current: usize,
            accumulated: Mat4,
            target: usize,
            found: &mut Option<Mat4>,
        ) {
            if found.is_some() {
                return;
            }
            let Some(node) = scene.nodes.get(current) else {
                return;
            };
            let local = accumulated.multiply(&node.local_transform());
            if current == target {
                *found = Some(local);
                return;
            }
            for child in &node.children {
                walk(scene, *child, local, target, found);
            }
        }

        let mut found = None;
        for root in &self.roots {
            walk(self, *root, Mat4::IDENTITY, index, &mut found);
            if found.is_some() {
                break;
            }
        }
        found.unwrap_or(Mat4::IDENTITY)
    }

    /// Every node with the world transform that places it, in depth-first
    /// order. This is what baking a hierarchy into a single mesh needs.
    pub fn flatten(&self) -> Vec<(usize, Mat4)> {
        fn walk(scene: &Scene, index: usize, parent: Mat4, out: &mut Vec<(usize, Mat4)>) {
            let Some(node) = scene.nodes.get(index) else {
                return;
            };
            let world = parent.multiply(&node.local_transform());
            out.push((index, world));
            for child in &node.children {
                walk(scene, *child, world, out);
            }
        }

        let mut out = Vec::new();
        for root in &self.roots {
            walk(self, *root, Mat4::IDENTITY, &mut out);
        }
        out
    }

    /// Structural problems: dangling child links and cycles.
    pub fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        for (index, node) in self.nodes.iter().enumerate() {
            for child in &node.children {
                if *child >= self.nodes.len() {
                    problems.push(format!("node {index} references unknown child {child}"));
                } else if self.is_ancestor_of(*child, index) {
                    problems.push(format!("nodes {index} and {child} form a cycle"));
                }
            }
        }
        for root in &self.roots {
            if *root >= self.nodes.len() {
                problems.push(format!("unknown root node {root}"));
            }
        }
        problems
    }
}

/// A physically-based material, covering the glTF metallic-roughness model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Material {
    pub name: String,
    /// Linear RGBA, each channel in `0.0..=1.0`.
    pub base_color: [f32; 4],
    pub metallic: f32,
    pub roughness: f32,
    pub emissive: [f32; 3],
    pub double_sided: bool,
}

impl Default for Material {
    fn default() -> Self {
        Self {
            name: String::new(),
            base_color: [1.0, 1.0, 1.0, 1.0],
            metallic: 0.0,
            roughness: 0.5,
            emissive: [0.0; 3],
            double_sided: false,
        }
    }
}

impl Material {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Default::default()
        }
    }

    /// Builder: set the base colour from RGB, opaque.
    pub fn with_color(mut self, r: f32, g: f32, b: f32) -> Self {
        self.base_color = [r, g, b, 1.0];
        self
    }

    pub fn with_alpha(mut self, alpha: f32) -> Self {
        self.base_color[3] = alpha;
        self
    }

    pub fn with_metallic_roughness(mut self, metallic: f32, roughness: f32) -> Self {
        self.metallic = metallic;
        self.roughness = roughness;
        self
    }

    pub fn with_emissive(mut self, r: f32, g: f32, b: f32) -> Self {
        self.emissive = [r, g, b];
        self
    }

    pub fn double_sided(mut self) -> Self {
        self.double_sided = true;
        self
    }

    /// Emit an Unreal-style copper-ish preset, handy for demos and tests.
    pub fn gold() -> Self {
        Self::new("Gold")
            .with_color(1.0, 0.766, 0.336)
            .with_metallic_roughness(1.0, 0.35)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_nodes_default_to_identity() {
        let node = Node::new("N");
        assert_eq!(node.translation, [0.0; 3]);
        assert_eq!(node.rotation, IDENTITY_ROTATION);
        assert_eq!(node.scale, [1.0; 3]);
        assert_eq!(node.local_transform(), Mat4::IDENTITY);
    }

    #[test]
    fn add_node_makes_a_root_and_add_child_does_not() {
        let mut scene = Scene::new();
        let root = scene.add_node("Root");
        let child = scene.add_child(Some(root), "Child");
        assert_eq!(scene.roots, vec![root]);
        assert_eq!(scene.nodes[root].children, vec![child]);
    }

    #[test]
    fn add_child_with_an_unknown_parent_falls_back_to_a_root() {
        let mut scene = Scene::new();
        let orphan = scene.add_child(Some(99), "Orphan");
        assert_eq!(scene.roots, vec![orphan]);
    }

    #[test]
    fn world_transform_accumulates_down_the_chain() {
        let mut scene = Scene::new();
        let root = scene.add_node("Root");
        scene.node_mut(root).unwrap().translation = [10.0, 0.0, 0.0];
        let child = scene.add_child(Some(root), "Child");
        scene.node_mut(child).unwrap().translation = [0.0, 5.0, 0.0];

        let world = scene.world_transform(child);
        let origin = world.transform_point([0.0, 0.0, 0.0]);
        assert!((origin[0] - 10.0).abs() < 1e-5);
        assert!((origin[1] - 5.0).abs() < 1e-5);
    }

    #[test]
    fn parent_scale_propagates_to_children() {
        let mut scene = Scene::new();
        let root = scene.add_node("Root");
        scene.node_mut(root).unwrap().scale = [2.0, 2.0, 2.0];
        let child = scene.add_child(Some(root), "Child");
        scene.node_mut(child).unwrap().translation = [1.0, 0.0, 0.0];

        let world = scene.world_transform(child);
        let origin = world.transform_point([0.0, 0.0, 0.0]);
        assert!((origin[0] - 2.0).abs() < 1e-5, "got {origin:?}");
    }

    #[test]
    fn world_transform_of_an_unreachable_node_is_identity() {
        let mut scene = Scene::new();
        let orphan = scene.add_node("Orphan");
        // Detach it from the roots so it is unreachable.
        scene.roots.clear();
        assert_eq!(scene.world_transform(orphan), Mat4::IDENTITY);
    }

    #[test]
    fn set_parent_moves_between_parents() {
        let mut scene = Scene::new();
        let a = scene.add_node("A");
        let b = scene.add_node("B");
        let child = scene.add_node("Child");

        assert!(scene.set_parent(child, Some(a)));
        assert_eq!(scene.nodes[a].children, vec![child]);
        assert!(!scene.roots.contains(&child));

        assert!(scene.set_parent(child, Some(b)));
        assert!(scene.nodes[a].children.is_empty());
        assert_eq!(scene.nodes[b].children, vec![child]);

        assert!(scene.set_parent(child, None));
        assert!(scene.roots.contains(&child));
    }

    #[test]
    fn set_parent_refuses_cycles_and_unknowns() {
        let mut scene = Scene::new();
        let parent = scene.add_node("Parent");
        let child = scene.add_child(Some(parent), "Child");

        // Making the parent a child of its own descendant would be a cycle.
        assert!(!scene.set_parent(parent, Some(child)));
        assert!(!scene.set_parent(parent, Some(parent)));
        assert!(!scene.set_parent(99, Some(parent)));
        assert!(!scene.set_parent(child, Some(99)));
        assert!(scene.validate().is_empty());
    }

    #[test]
    fn flatten_visits_depth_first_with_world_transforms() {
        let mut scene = Scene::new();
        let root = scene.add_node("Root");
        scene.node_mut(root).unwrap().translation = [1.0, 0.0, 0.0];
        let child = scene.add_child(Some(root), "Child");
        scene.node_mut(child).unwrap().translation = [0.0, 1.0, 0.0];

        let flat = scene.flatten();
        assert_eq!(flat.len(), 2);
        assert_eq!(flat[0].0, root);
        assert_eq!(flat[1].0, child);
        let child_origin = flat[1].1.transform_point([0.0, 0.0, 0.0]);
        assert!((child_origin[0] - 1.0).abs() < 1e-5);
        assert!((child_origin[1] - 1.0).abs() < 1e-5);
    }

    #[test]
    fn validate_reports_dangling_children() {
        let mut scene = Scene::new();
        let node = scene.add_node("A");
        scene.node_mut(node).unwrap().children.push(42);
        assert!(scene
            .validate()
            .iter()
            .any(|p| p.contains("unknown child 42")));
    }

    #[test]
    fn builders_are_chainable() {
        let node = Node::new("N")
            .at(1.0, 2.0, 3.0)
            .scaled(2.0, 2.0, 2.0)
            .with_mesh(0);
        assert_eq!(node.translation, [1.0, 2.0, 3.0]);
        assert_eq!(node.scale, [2.0, 2.0, 2.0]);
        assert_eq!(node.mesh, Some(0));
    }

    #[test]
    fn rotated_builder_normalizes_its_axis() {
        let node = Node::new("N").rotated([0.0, 10.0, 0.0], std::f32::consts::FRAC_PI_2);
        let m = node.local_transform();
        let r = m.transform_point([1.0, 0.0, 0.0]);
        assert!(r[0].abs() < 1e-4 && (r[2] + 1.0).abs() < 1e-4, "got {r:?}");
    }

    #[test]
    fn material_defaults_are_sane_and_chainable() {
        let m = Material::new("M");
        assert_eq!(m.base_color, [1.0, 1.0, 1.0, 1.0]);
        assert_eq!(m.metallic, 0.0);

        let gold = Material::gold();
        assert_eq!(gold.name, "Gold");
        assert_eq!(gold.metallic, 1.0);
        assert!(gold.base_color[3] == 1.0);
    }
}
