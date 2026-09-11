//! glTF 2.0 export.
//!
//! Writes a [`crate::Document`] as glTF 2.0 JSON plus the binary buffer it
//! references. The output is deliberately conservative: one attribute per
//! buffer view, four-byte alignment between views, min/max on every accessor
//! the specification requires them for, and no extensions.
//!
//! That conservatism is the point. Godot's importer and Blender's exporter
//! both accept far more than the minimum, but a file that stays inside the
//! core specification imports everywhere without a compatibility matrix.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use crate::anim::Animation;
use crate::Document;

/// glTF component types.
const FLOAT: u32 = 5126;
const UNSIGNED_SHORT: u32 = 5123;
const UNSIGNED_INT: u32 = 5125;

/// glTF buffer view targets.
const ARRAY_BUFFER: u32 = 34962;
const ELEMENT_ARRAY_BUFFER: u32 = 34963;

/// Something that prevented an export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GltfError {
    /// The document is structurally invalid; these are the problems.
    Invalid(Vec<String>),
    /// A file could not be written.
    Io(String),
    /// A mesh had no geometry to export.
    EmptyDocument,
}

impl std::fmt::Display for GltfError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(problems) => {
                write!(f, "document is not exportable: {}", problems.join("; "))
            }
            Self::Io(message) => write!(f, "{message}"),
            Self::EmptyDocument => {
                write!(f, "document has no meshes with geometry; nothing to export")
            }
        }
    }
}

impl std::error::Error for GltfError {}

/// Where an export was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exported {
    pub gltf: PathBuf,
    pub bin: PathBuf,
    pub bytes: usize,
}

/// Accumulates the binary buffer alongside the JSON that indexes it.
#[derive(Default)]
struct Builder {
    bin: Vec<u8>,
    buffer_views: Vec<Value>,
    accessors: Vec<Value>,
}

impl Builder {
    /// Append bytes, four-byte aligned, and register a buffer view.
    fn view(&mut self, bytes: &[u8], target: Option<u32>) -> usize {
        while !self.bin.len().is_multiple_of(4) {
            self.bin.push(0);
        }
        let offset = self.bin.len();
        self.bin.extend_from_slice(bytes);
        let mut view = Map::new();
        view.insert("buffer".into(), json!(0));
        view.insert("byteOffset".into(), json!(offset));
        view.insert("byteLength".into(), json!(bytes.len()));
        if let Some(target) = target {
            view.insert("target".into(), json!(target));
        }
        self.buffer_views.push(Value::Object(view));
        self.buffer_views.len() - 1
    }

    fn f32_view(&mut self, data: &[f32], target: Option<u32>) -> usize {
        let mut bytes = Vec::with_capacity(data.len() * 4);
        for value in data {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        self.view(&bytes, target)
    }

    fn u32_view(&mut self, data: &[u32], target: Option<u32>) -> usize {
        let mut bytes = Vec::with_capacity(data.len() * 4);
        for value in data {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        self.view(&bytes, target)
    }

    fn u16_view(&mut self, data: &[u16], target: Option<u32>) -> usize {
        let mut bytes = Vec::with_capacity(data.len() * 2);
        for value in data {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        self.view(&bytes, target)
    }

    fn accessor(
        &mut self,
        buffer_view: usize,
        component_type: u32,
        count: usize,
        kind: &str,
        min_max: Option<(Value, Value)>,
    ) -> usize {
        let mut accessor = Map::new();
        accessor.insert("bufferView".into(), json!(buffer_view));
        accessor.insert("componentType".into(), json!(component_type));
        accessor.insert("count".into(), json!(count));
        accessor.insert("type".into(), json!(kind));
        if let Some((min, max)) = min_max {
            accessor.insert("min".into(), min);
            accessor.insert("max".into(), max);
        }
        self.accessors.push(Value::Object(accessor));
        self.accessors.len() - 1
    }
}

/// Per-axis minimum and maximum of interleaved vectors, as glTF wants them.
fn min_max(values: &[f32], components: usize) -> (Value, Value) {
    let mut min = vec![f32::INFINITY; components];
    let mut max = vec![f32::NEG_INFINITY; components];
    for chunk in values.chunks_exact(components) {
        for axis in 0..components {
            min[axis] = min[axis].min(chunk[axis]);
            max[axis] = max[axis].max(chunk[axis]);
        }
    }
    if values.is_empty() {
        min = vec![0.0; components];
        max = vec![0.0; components];
    }
    (
        Value::Array(min.into_iter().map(|v| json!(v)).collect()),
        Value::Array(max.into_iter().map(|v| json!(v)).collect()),
    )
}

/// Export a document as glTF JSON plus a sibling binary buffer.
///
/// The JSON's buffer URI is `<name>.bin`, matching [`write`].
pub fn export(document: &Document) -> Result<(String, Vec<u8>), GltfError> {
    let problems = document.validate();
    if !problems.is_empty() {
        return Err(GltfError::Invalid(problems));
    }
    if document.meshes.iter().all(|m| m.is_empty()) {
        return Err(GltfError::EmptyDocument);
    }

    let mut builder = Builder::default();
    let bin_name = buffer_name(document);

    // -- materials ---------------------------------------------------------
    let materials: Vec<Value> = document
        .materials
        .iter()
        .map(|material| {
            json!({
                "name": material.name,
                "pbrMetallicRoughness": {
                    "baseColorFactor": material.base_color,
                    "metallicFactor": material.metallic,
                    "roughnessFactor": material.roughness,
                },
                "emissiveFactor": material.emissive,
                "doubleSided": material.double_sided,
                "alphaMode": if material.base_color[3] < 1.0 { "BLEND" } else { "OPAQUE" },
            })
        })
        .collect();

    // -- meshes ------------------------------------------------------------
    // glTF carries the material on the primitive, not the node, so a mesh is
    // emitted once per (mesh, material) pair and shared between nodes.
    let mut mesh_cache: HashMap<(usize, Option<usize>), usize> = HashMap::new();
    let mut gltf_meshes: Vec<Value> = Vec::new();

    for node in document.scene.nodes.iter() {
        let Some(mesh_index) = node.mesh else {
            continue;
        };
        let key = (mesh_index, node.material);
        if mesh_cache.contains_key(&key) {
            continue;
        }
        let mesh = &document.meshes[mesh_index];
        if mesh.is_empty() {
            // An empty mesh is legal to reference but produces no primitive;
            // skip it rather than emit one with a zero count.
            continue;
        }

        let position_view = builder.f32_view(&flatten3(&mesh.positions), Some(ARRAY_BUFFER));
        let (pmin, pmax) = min_max(&flatten3(&mesh.positions), 3);
        let position = builder.accessor(
            position_view,
            FLOAT,
            mesh.positions.len(),
            "VEC3",
            Some((pmin, pmax)),
        );

        let mut attributes = Map::new();
        attributes.insert("POSITION".into(), json!(position));

        if mesh.normals.len() == mesh.positions.len() {
            let view = builder.f32_view(&flatten3(&mesh.normals), Some(ARRAY_BUFFER));
            let accessor = builder.accessor(view, FLOAT, mesh.normals.len(), "VEC3", None);
            attributes.insert("NORMAL".into(), json!(accessor));
        }
        if mesh.uvs.len() == mesh.positions.len() {
            let view = builder.f32_view(&flatten2(&mesh.uvs), Some(ARRAY_BUFFER));
            let accessor = builder.accessor(view, FLOAT, mesh.uvs.len(), "VEC2", None);
            attributes.insert("TEXCOORD_0".into(), json!(accessor));
        }

        // Use 16-bit indices when the vertex count allows it: smaller files,
        // and some importers are happier with them.
        let max_index = mesh.indices.iter().copied().max().unwrap_or(0);
        let index_accessor = if max_index < u16::MAX as u32 {
            let narrowed: Vec<u16> = mesh.indices.iter().map(|i| *i as u16).collect();
            let view = builder.u16_view(&narrowed, Some(ELEMENT_ARRAY_BUFFER));
            builder.accessor(view, UNSIGNED_SHORT, narrowed.len(), "SCALAR", None)
        } else {
            let view = builder.u32_view(&mesh.indices, Some(ELEMENT_ARRAY_BUFFER));
            builder.accessor(view, UNSIGNED_INT, mesh.indices.len(), "SCALAR", None)
        };

        let mut primitive = Map::new();
        primitive.insert("attributes".into(), Value::Object(attributes));
        primitive.insert("indices".into(), json!(index_accessor));
        primitive.insert("mode".into(), json!(4)); // TRIANGLES
        if let Some(material) = node.material {
            primitive.insert("material".into(), json!(material));
        }

        gltf_meshes.push(json!({
            "name": mesh.name,
            "primitives": [Value::Object(primitive)],
        }));
        mesh_cache.insert(key, gltf_meshes.len() - 1);
    }

    // -- nodes -------------------------------------------------------------
    let nodes: Vec<Value> = document
        .scene
        .nodes
        .iter()
        .map(|node| {
            let mut out = Map::new();
            out.insert("name".into(), json!(node.name));
            // Emit only non-identity TRS so the file stays readable.
            if node.translation != [0.0, 0.0, 0.0] {
                out.insert("translation".into(), json!(node.translation));
            }
            if node.rotation != crate::scene::IDENTITY_ROTATION {
                out.insert("rotation".into(), json!(node.rotation));
            }
            if node.scale != [1.0, 1.0, 1.0] {
                out.insert("scale".into(), json!(node.scale));
            }
            if !node.children.is_empty() {
                out.insert("children".into(), json!(node.children));
            }
            if let Some(index) = mesh_cache.get(&(node.mesh.unwrap_or(usize::MAX), node.material)) {
                out.insert("mesh".into(), json!(index));
            }
            Value::Object(out)
        })
        .collect();

    // -- animations --------------------------------------------------------
    let animations: Vec<Value> = document
        .animations
        .iter()
        .filter(|a| !a.is_empty())
        .map(|animation| export_animation(animation, &mut builder))
        .collect();

    // -- assemble ----------------------------------------------------------
    let mut root = Map::new();
    root.insert(
        "asset".into(),
        json!({
            "version": "2.0",
            "generator": if document.name.is_empty() {
                "aurum-content".to_string()
            } else {
                format!("aurum-content ({})", document.name)
            },
        }),
    );
    root.insert("scene".into(), json!(0));
    root.insert(
        "scenes".into(),
        json!([{ "name": document.name, "nodes": document.scene.roots }]),
    );
    root.insert("nodes".into(), Value::Array(nodes));
    if !gltf_meshes.is_empty() {
        root.insert("meshes".into(), Value::Array(gltf_meshes));
    }
    if !materials.is_empty() {
        root.insert("materials".into(), Value::Array(materials));
    }
    if !animations.is_empty() {
        root.insert("animations".into(), Value::Array(animations));
    }
    root.insert("accessors".into(), Value::Array(builder.accessors));
    root.insert("bufferViews".into(), Value::Array(builder.buffer_views));
    root.insert(
        "buffers".into(),
        json!([{ "byteLength": builder.bin.len(), "uri": bin_name }]),
    );

    let json_text = serde_json::to_string_pretty(&Value::Object(root))
        .map_err(|e| GltfError::Io(e.to_string()))?;
    Ok((json_text, builder.bin))
}

fn export_animation(animation: &Animation, builder: &mut Builder) -> Value {
    let mut samplers = Vec::with_capacity(animation.tracks.len());
    let mut channels = Vec::with_capacity(animation.tracks.len());

    for track in &animation.tracks {
        if track.is_empty() {
            continue;
        }
        let components = track.components();

        let input_view = builder.f32_view(&track.times, None);
        let (tmin, tmax) = min_max(&track.times, 1);
        let input = builder.accessor(
            input_view,
            FLOAT,
            track.times.len(),
            "SCALAR",
            Some((tmin, tmax)),
        );

        let output_view = builder.f32_view(&track.values, None);
        let kind = if components == 4 { "VEC4" } else { "VEC3" };
        let output = builder.accessor(output_view, FLOAT, track.value_count(), kind, None);

        samplers.push(json!({
            "input": input,
            "output": output,
            "interpolation": track.interpolation.as_gltf(),
        }));
        channels.push(json!({
            "sampler": samplers.len() - 1,
            "target": { "node": track.node, "path": track.path.as_gltf() },
        }));
    }

    json!({ "name": animation.name, "samplers": samplers, "channels": channels })
}

fn buffer_name(document: &Document) -> String {
    let base: String = document
        .name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let base = base.trim_matches('_');
    if base.is_empty() {
        "buffer.bin".to_string()
    } else {
        format!("{base}.bin")
    }
}

fn flatten3(values: &[[f32; 3]]) -> Vec<f32> {
    values.iter().flat_map(|v| v.iter().copied()).collect()
}

fn flatten2(values: &[[f32; 2]]) -> Vec<f32> {
    values.iter().flat_map(|v| v.iter().copied()).collect()
}

/// Export as a single self-contained `.gltf` with the buffer inlined as a
/// base64 data URI.
///
/// Handy when the consumer expects one file, at the cost of about a third
/// more bytes.
pub fn export_embedded(document: &Document) -> Result<String, GltfError> {
    let (json_text, bin) = export(document)?;
    let mut value: Value =
        serde_json::from_str(&json_text).map_err(|e| GltfError::Io(e.to_string()))?;
    let uri = format!("data:application/octet-stream;base64,{}", base64(&bin));
    if let Some(buffer) = value
        .get_mut("buffers")
        .and_then(|b| b.as_array_mut())
        .and_then(|b| b.first_mut())
    {
        buffer["uri"] = json!(uri);
    }
    serde_json::to_string_pretty(&value).map_err(|e| GltfError::Io(e.to_string()))
}

/// Write `<stem>.gltf` and `<stem>.bin` into `directory`.
pub fn write(document: &Document, directory: &Path, stem: &str) -> Result<Exported, GltfError> {
    let (json_text, bin) = export(document)?;
    std::fs::create_dir_all(directory)
        .map_err(|e| GltfError::Io(format!("could not create '{}': {e}", directory.display())))?;

    let gltf_path = directory.join(format!("{stem}.gltf"));
    let bin_path = directory.join(buffer_name(document));

    std::fs::write(&bin_path, &bin)
        .map_err(|e| GltfError::Io(format!("could not write '{}': {e}", bin_path.display())))?;
    std::fs::write(&gltf_path, &json_text)
        .map_err(|e| GltfError::Io(format!("could not write '{}': {e}", gltf_path.display())))?;

    Ok(Exported {
        gltf: gltf_path,
        bin: bin_path,
        bytes: json_text.len() + bin.len(),
    })
}

/// Standard base64, with padding. Small enough not to warrant a crate.
fn base64(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim::{Animation, Interpolation};
    use crate::mesh::{box_mesh, uv_sphere, Mat4, Mesh};
    use crate::scene::Material;

    /// A document with one textured-material cube, one child, and a spin.
    fn sample() -> Document {
        let mut document = Document::new("sample");
        let cube = document.add_mesh(box_mesh(1.0, 1.0, 1.0));
        let material = document.add_material(Material::gold());

        let root = document.scene_mut().add_node("Cube");
        {
            let node = document.scene_mut().node_mut(root).unwrap();
            node.mesh = Some(cube);
            node.material = Some(material);
            node.translation = [0.0, 1.0, 0.0];
        }
        let child = document.scene_mut().add_child(Some(root), "Child");
        document.scene_mut().node_mut(child).unwrap().mesh = Some(cube);

        document.add_animation(Animation::new("Spin").spin(root, [0.0, 1.0, 0.0], 1.0, 2.0, 5));
        document
    }

    fn parse(document: &Document) -> Value {
        let (json_text, _) = export(document).unwrap();
        serde_json::from_str(&json_text).unwrap()
    }

    #[test]
    fn export_produces_the_required_top_level_sections() {
        let value = parse(&sample());
        assert_eq!(value["asset"]["version"], "2.0");
        assert!(value["asset"]["generator"]
            .as_str()
            .unwrap()
            .contains("aurum-content"));
        assert_eq!(value["scene"], 0);
        assert!(value["scenes"][0]["nodes"].is_array());
        assert!(value["nodes"].is_array());
        assert!(value["meshes"].is_array());
        assert!(value["accessors"].is_array());
        assert!(value["bufferViews"].is_array());
        assert_eq!(value["buffers"][0]["uri"], "sample.bin");
    }

    #[test]
    fn buffer_views_are_four_byte_aligned_and_inside_the_buffer() {
        let (json_text, bin) = export(&sample()).unwrap();
        let value: Value = serde_json::from_str(&json_text).unwrap();
        let declared = value["buffers"][0]["byteLength"].as_u64().unwrap() as usize;
        assert_eq!(declared, bin.len(), "declared length must match the buffer");

        for view in value["bufferViews"].as_array().unwrap() {
            let offset = view["byteOffset"].as_u64().unwrap();
            let length = view["byteLength"].as_u64().unwrap();
            assert_eq!(
                offset % 4,
                0,
                "bufferView offset {offset} is not 4-byte aligned"
            );
            assert!(
                offset + length <= bin.len() as u64,
                "bufferView runs past the buffer"
            );
        }
    }

    #[test]
    fn positions_carry_min_and_max_as_the_spec_requires() {
        let value = parse(&sample());
        let accessors = value["accessors"].as_array().unwrap();
        let position = accessors
            .iter()
            .find(|a| a["type"] == "VEC3" && a["min"].is_array())
            .expect("a POSITION accessor with min/max");

        let min = position["min"].as_array().unwrap();
        let max = position["max"].as_array().unwrap();
        assert_eq!(min.len(), 3);
        for axis in 0..3 {
            let lo = min[axis].as_f64().unwrap();
            let hi = max[axis].as_f64().unwrap();
            assert!(lo <= hi);
            assert!((lo + 0.5).abs() < 1e-5, "box min should be -0.5");
            assert!((hi - 0.5).abs() < 1e-5, "box max should be +0.5");
        }
    }

    #[test]
    fn every_accessor_stays_inside_its_buffer_view() {
        let (json_text, bin) = export(&sample()).unwrap();
        let value: Value = serde_json::from_str(&json_text).unwrap();
        let views = value["bufferViews"].as_array().unwrap();
        let component_size = |ty: u64| match ty {
            5126 | 5125 => 4u64,
            5123 => 2,
            _ => 1,
        };
        let components = |kind: &str| match kind {
            "SCALAR" => 1u64,
            "VEC2" => 2,
            "VEC3" => 3,
            "VEC4" => 4,
            _ => 1,
        };

        for accessor in value["accessors"].as_array().unwrap() {
            let view = &views[accessor["bufferView"].as_u64().unwrap() as usize];
            let available = view["byteLength"].as_u64().unwrap();
            let needed = accessor["count"].as_u64().unwrap()
                * component_size(accessor["componentType"].as_u64().unwrap())
                * components(accessor["type"].as_str().unwrap());
            assert!(
                needed <= available,
                "accessor needs {needed} bytes but its view has {available}"
            );
        }
        assert!(!bin.is_empty());
    }

    #[test]
    fn index_accessors_choose_the_narrowest_type() {
        let mut document = Document::new("small");
        let cube = document.add_mesh(box_mesh(1.0, 1.0, 1.0));
        let node = document.scene_mut().add_node("Cube");
        document.scene_mut().node_mut(node).unwrap().mesh = Some(cube);

        let value = parse(&document);
        let index = value["accessors"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["type"] == "SCALAR")
            .unwrap();
        assert_eq!(
            index["componentType"], UNSIGNED_SHORT,
            "24 verts fit in u16"
        );

        // A dense sphere exceeds 65535 vertices and must fall back to u32.
        let mut big = Document::new("big");
        let sphere = big.add_mesh(uv_sphere(1.0, 300, 300));
        let node = big.scene_mut().add_node("Sphere");
        big.scene_mut().node_mut(node).unwrap().mesh = Some(sphere);
        assert!(big.meshes[0].vertex_count() > u16::MAX as usize);
        let value = parse(&big);
        let index = value["accessors"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["type"] == "SCALAR")
            .unwrap();
        assert_eq!(index["componentType"], UNSIGNED_INT);
    }

    #[test]
    fn nodes_carry_trs_and_children() {
        let value = parse(&sample());
        let nodes = value["nodes"].as_array().unwrap();
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0]["name"], "Cube");
        assert_eq!(nodes[0]["translation"], json!([0.0, 1.0, 0.0]));
        assert_eq!(nodes[0]["children"], json!([1]));
        // Identity TRS is omitted rather than written out.
        assert!(nodes[0].get("scale").is_none());
        assert!(nodes[0].get("rotation").is_none());
    }

    #[test]
    fn a_mesh_shared_by_two_nodes_with_the_same_material_is_emitted_once() {
        let mut document = Document::new("shared");
        let cube = document.add_mesh(box_mesh(1.0, 1.0, 1.0));
        let material = document.add_material(Material::new("Shared"));

        let root = document.scene_mut().add_node("Cube");
        {
            let node = document.scene_mut().node_mut(root).unwrap();
            node.mesh = Some(cube);
            node.material = Some(material);
        }
        let child = document.scene_mut().add_child(Some(root), "Child");
        {
            let node = document.scene_mut().node_mut(child).unwrap();
            node.mesh = Some(cube);
            node.material = Some(material);
        }

        let value = parse(&document);
        assert_eq!(
            value["meshes"].as_array().unwrap().len(),
            1,
            "identical mesh and material must be deduplicated"
        );
        assert_eq!(value["nodes"][0]["mesh"], 0);
        assert_eq!(value["nodes"][1]["mesh"], 0);
    }

    #[test]
    fn the_same_mesh_with_a_different_material_gets_its_own_primitive() {
        let mut document = Document::new("variants");
        let cube = document.add_mesh(box_mesh(1.0, 1.0, 1.0));
        let gold = document.add_material(Material::gold());
        let glass = document.add_material(Material::new("Glass").with_alpha(0.5));

        let a = document.scene_mut().add_node("Solid");
        {
            let node = document.scene_mut().node_mut(a).unwrap();
            node.mesh = Some(cube);
            node.material = Some(gold);
        }
        let b = document.scene_mut().add_node("Ghost");
        {
            let node = document.scene_mut().node_mut(b).unwrap();
            node.mesh = Some(cube);
            node.material = Some(glass);
        }

        let value = parse(&document);
        // glTF carries the material on the primitive, not the node, so these
        // must be two meshes pointing at different materials.
        assert_eq!(value["meshes"].as_array().unwrap().len(), 2);
        assert_eq!(value["meshes"][0]["primitives"][0]["material"], 0);
        assert_eq!(value["meshes"][1]["primitives"][0]["material"], 1);
    }

    #[test]
    fn materials_export_as_metallic_roughness() {
        let value = parse(&sample());
        let material = &value["materials"][0];
        assert_eq!(material["name"], "Gold");
        assert_eq!(material["pbrMetallicRoughness"]["metallicFactor"], 1.0);
        assert_eq!(material["alphaMode"], "OPAQUE");
        assert_eq!(material["doubleSided"], false);
    }

    #[test]
    fn transparency_selects_blend_mode() {
        let mut document = Document::new("glass");
        let material = document.add_material(
            Material::new("Glass")
                .with_color(1.0, 1.0, 1.0)
                .with_alpha(0.4),
        );
        let cube = document.add_mesh(box_mesh(1.0, 1.0, 1.0));
        let node = document.scene_mut().add_node("Pane");
        {
            let n = document.scene_mut().node_mut(node).unwrap();
            n.mesh = Some(cube);
            n.material = Some(material);
        }
        let value = parse(&document);
        assert_eq!(value["materials"][0]["alphaMode"], "BLEND");
        // Compare through f32: the value round-trips as f32 -> f64 and is not
        // exactly 0.4 in decimal.
        let alpha = value["materials"][0]["pbrMetallicRoughness"]["baseColorFactor"][3]
            .as_f64()
            .unwrap() as f32;
        assert!((alpha - 0.4).abs() < 1e-6, "alpha was {alpha}");
    }

    #[test]
    fn animations_export_channels_and_samplers() {
        let value = parse(&sample());
        let animation = &value["animations"][0];
        assert_eq!(animation["name"], "Spin");
        assert_eq!(animation["channels"].as_array().unwrap().len(), 1);
        assert_eq!(animation["channels"][0]["target"]["path"], "rotation");
        assert_eq!(animation["channels"][0]["target"]["node"], 0);
        assert_eq!(animation["samplers"][0]["interpolation"], "LINEAR");

        // The time accessor is a SCALAR with min/max, as required.
        let input_index = animation["samplers"][0]["input"].as_u64().unwrap() as usize;
        let input = &value["accessors"][input_index];
        assert_eq!(input["type"], "SCALAR");
        assert!(input["min"].is_array() && input["max"].is_array());
        assert_eq!(input["count"], 5);

        // The rotation output is VEC4.
        let output_index = animation["samplers"][0]["output"].as_u64().unwrap() as usize;
        assert_eq!(value["accessors"][output_index]["type"], "VEC4");
    }

    #[test]
    fn empty_animations_are_dropped() {
        let mut document = Document::new("quiet");
        let cube = document.add_mesh(box_mesh(1.0, 1.0, 1.0));
        let node = document.scene_mut().add_node("Cube");
        document.scene_mut().node_mut(node).unwrap().mesh = Some(cube);
        document.add_animation(Animation::new("Nothing"));
        let value = parse(&document);
        assert!(value.get("animations").is_none());
    }

    #[test]
    fn export_refuses_an_invalid_document() {
        let mut document = Document::new("broken");
        let node = document.scene_mut().add_node("Ghost");
        document.scene_mut().node_mut(node).unwrap().mesh = Some(9);
        match export(&document) {
            Err(GltfError::Invalid(problems)) => {
                assert!(problems.iter().any(|p| p.contains("unknown mesh 9")))
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn export_refuses_a_document_with_no_geometry() {
        let mut document = Document::new("hollow");
        document.add_mesh(Mesh::new("Empty"));
        assert_eq!(export(&document), Err(GltfError::EmptyDocument));
    }

    #[test]
    fn embedded_export_inlines_the_buffer() {
        let text = export_embedded(&sample()).unwrap();
        assert!(text.contains("data:application/octet-stream;base64,"));
        assert!(!text.contains(".bin"));
        let value: Value = serde_json::from_str(&text).unwrap();
        assert!(value["buffers"][0]["uri"].as_str().unwrap().len() > 40);
    }

    #[test]
    fn write_creates_both_files_and_the_uri_matches() {
        let dir = std::env::temp_dir().join(format!("aurum-content-gltf-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let document = sample();
        let exported = write(&document, &dir, "scene").unwrap();
        assert!(exported.gltf.exists());
        assert!(exported.bin.exists());
        assert!(exported.bytes > 0);

        let text = std::fs::read_to_string(&exported.gltf).unwrap();
        let value: Value = serde_json::from_str(&text).unwrap();
        let uri = value["buffers"][0]["uri"].as_str().unwrap();
        assert_eq!(uri, "sample.bin", "URI must match the file we wrote");
        assert!(dir.join(uri).exists(), "the referenced buffer must exist");
        assert_eq!(
            std::fs::metadata(dir.join(uri)).unwrap().len() as u64,
            value["buffers"][0]["byteLength"].as_u64().unwrap()
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_transformed_merge_exports_correctly() {
        // Two boxes merged into one mesh, exercising offset indices in glTF.
        let mut combined = box_mesh(1.0, 1.0, 1.0);
        combined.merge(&Mesh::transformed_copy(
            &box_mesh(1.0, 1.0, 1.0),
            &Mat4::translation(3.0, 0.0, 0.0),
        ));

        let mut document = Document::new("merged");
        let mesh = document.add_mesh(combined);
        let node = document.scene_mut().add_node("Pair");
        document.scene_mut().node_mut(node).unwrap().mesh = Some(mesh);

        let value = parse(&document);
        let position = value["accessors"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["type"] == "VEC3" && a["min"].is_array())
            .unwrap();
        assert_eq!(position["count"], 48);
        assert!((position["max"][0].as_f64().unwrap() - 3.5).abs() < 1e-5);
    }

    #[test]
    fn step_interpolation_is_preserved() {
        let mut document = Document::new("step");
        let cube = document.add_mesh(box_mesh(1.0, 1.0, 1.0));
        let node = document.scene_mut().add_node("Cube");
        document.scene_mut().node_mut(node).unwrap().mesh = Some(cube);
        document.add_animation(Animation::new("Blink").scale(
            node,
            &[0.0, 0.5, 1.0],
            &[[1.0; 3], [0.0; 3], [1.0; 3]],
            Interpolation::Step,
        ));

        let value = parse(&document);
        assert_eq!(
            value["animations"][0]["samplers"][0]["interpolation"],
            "STEP"
        );
        assert_eq!(
            value["animations"][0]["channels"][0]["target"]["path"],
            "scale"
        );
    }

    #[test]
    fn base64_matches_known_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }
}
