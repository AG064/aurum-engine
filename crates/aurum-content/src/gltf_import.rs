//! glTF 2.0 import.
//!
//! Reads `.gltf` (external `.bin` or an embedded base64 data URI) and `.glb`
//! (the binary container Blender exports by default) back into a
//! [`crate::Document`].
//!
//! This is what makes Blender *optional* rather than merely unused: a mesh can
//! come from Blender, be loaded here, be transformed and merged in Rust, and
//! be exported again — all through one format.
//!
//! ## What is handled
//!
//! - Both containers, and buffers referenced three ways (GLB chunk, data URI,
//!   relative file path).
//! - Interleaved vertex data via `bufferView.byteStride`. Blender writes this
//!   routinely, and ignoring it produces plausible-looking garbage.
//! - Normalized integer attributes, dequantized to float.
//! - Every index component type (u8, u16, u32) and float/normalized attribute
//!   component types.
//! - Multi-primitive meshes, by splitting them into child nodes so per-
//!   primitive materials survive.
//! - `matrix` node transforms, decomposed into translation/rotation/scale.
//!
//! ## What is refused, loudly
//!
//! Draco or meshopt compression is reported as unsupported rather than
//! silently producing an empty mesh. A clear error beats a wrong model.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::anim::{Animation, Interpolation, Track, TrackPath};
use crate::mesh::Mesh;
use crate::scene::{Material, Node};
use crate::Document;

/// glTF component types, as they appear in the JSON.
mod component {
    pub const BYTE: u32 = 5120;
    pub const UNSIGNED_BYTE: u32 = 5121;
    pub const SHORT: u32 = 5122;
    pub const UNSIGNED_SHORT: u32 = 5123;
    pub const UNSIGNED_INT: u32 = 5125;
    pub const FLOAT: u32 = 5126;
}

fn component_size(component_type: u32) -> Option<usize> {
    match component_type {
        component::BYTE | component::UNSIGNED_BYTE => Some(1),
        component::SHORT | component::UNSIGNED_SHORT => Some(2),
        component::UNSIGNED_INT | component::FLOAT => Some(4),
        _ => None,
    }
}

fn component_count(kind: &str) -> Option<usize> {
    match kind {
        "SCALAR" => Some(1),
        "VEC2" => Some(2),
        "VEC3" => Some(3),
        "VEC4" => Some(4),
        "MAT4" => Some(16),
        _ => None,
    }
}

/// Why an import failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportError {
    Io(String),
    Json(String),
    /// The file uses a feature this importer does not implement.
    Unsupported(String),
    /// The file is malformed.
    Malformed(String),
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(m) => write!(f, "{m}"),
            Self::Json(m) => write!(f, "invalid glTF JSON: {m}"),
            Self::Unsupported(m) => write!(f, "unsupported glTF feature: {m}"),
            Self::Malformed(m) => write!(f, "malformed glTF: {m}"),
        }
    }
}

impl std::error::Error for ImportError {}

// ---------------------------------------------------------------------------
// Wire types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Root {
    #[serde(default)]
    scene: Option<usize>,
    #[serde(default)]
    scenes: Vec<SceneDef>,
    #[serde(default)]
    nodes: Vec<NodeDef>,
    #[serde(default)]
    meshes: Vec<MeshDef>,
    #[serde(default)]
    materials: Vec<MaterialDef>,
    #[serde(default)]
    accessors: Vec<AccessorDef>,
    #[serde(default)]
    buffer_views: Vec<BufferViewDef>,
    #[serde(default)]
    buffers: Vec<BufferDef>,
    #[serde(default)]
    animations: Vec<AnimationDef>,
    #[serde(default)]
    extensions_required: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct SceneDef {
    #[serde(default)]
    nodes: Vec<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NodeDef {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    mesh: Option<usize>,
    #[serde(default)]
    children: Vec<usize>,
    #[serde(default)]
    translation: Option<[f32; 3]>,
    #[serde(default)]
    rotation: Option<[f32; 4]>,
    #[serde(default)]
    scale: Option<[f32; 3]>,
    #[serde(default)]
    matrix: Option<[f32; 16]>,
}

#[derive(Debug, Deserialize)]
struct MeshDef {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    primitives: Vec<PrimitiveDef>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PrimitiveDef {
    #[serde(default)]
    attributes: HashMap<String, usize>,
    #[serde(default)]
    indices: Option<usize>,
    #[serde(default)]
    material: Option<usize>,
    #[serde(default = "default_mode")]
    mode: u32,
    #[serde(default)]
    extensions: Option<serde_json::Value>,
}

fn default_mode() -> u32 {
    4
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MaterialDef {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    pbr_metallic_roughness: Option<PbrDef>,
    #[serde(default)]
    emissive_factor: Option<[f32; 3]>,
    #[serde(default)]
    double_sided: bool,
    #[serde(default)]
    alpha_mode: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PbrDef {
    #[serde(default)]
    base_color_factor: Option<[f32; 4]>,
    #[serde(default)]
    metallic_factor: Option<f32>,
    #[serde(default)]
    roughness_factor: Option<f32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccessorDef {
    #[serde(default)]
    buffer_view: Option<usize>,
    #[serde(default)]
    byte_offset: usize,
    component_type: u32,
    count: usize,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    normalized: bool,
    #[serde(default)]
    sparse: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BufferViewDef {
    buffer: usize,
    #[serde(default)]
    byte_offset: usize,
    byte_length: usize,
    #[serde(default)]
    byte_stride: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BufferDef {
    #[serde(default)]
    uri: Option<String>,
    #[serde(default)]
    byte_length: usize,
}

#[derive(Debug, Deserialize)]
struct AnimationDef {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    samplers: Vec<SamplerDef>,
    #[serde(default)]
    channels: Vec<ChannelDef>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SamplerDef {
    input: usize,
    output: usize,
    #[serde(default)]
    interpolation: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChannelDef {
    sampler: usize,
    target: TargetDef,
}

#[derive(Debug, Deserialize)]
struct TargetDef {
    #[serde(default)]
    node: Option<usize>,
    #[serde(default)]
    path: String,
}

// ---------------------------------------------------------------------------
// Entry points
// ---------------------------------------------------------------------------

/// Read a `.gltf` or `.glb` file into a document.
pub fn import_path(path: &Path) -> Result<Document, ImportError> {
    let bytes = std::fs::read(path)
        .map_err(|e| ImportError::Io(format!("could not read '{}': {e}", path.display())))?;

    // Trust the magic over the extension: a mislabelled file should still work.
    if bytes.len() >= 4 && &bytes[..4] == b"glTF" {
        return import_glb(&bytes);
    }

    let text = String::from_utf8(bytes)
        .map_err(|e| ImportError::Malformed(format!("not UTF-8 and not a GLB: {e}")))?;
    let base = path.parent().map(Path::to_path_buf);
    import_gltf_json(&text, base.as_deref())
}

/// Read a `.gltf` JSON document, resolving relative buffer URIs against
/// `base_dir`.
pub fn import_gltf_json(text: &str, base_dir: Option<&Path>) -> Result<Document, ImportError> {
    let root: Root = serde_json::from_str(text).map_err(|e| ImportError::Json(e.to_string()))?;
    reject_unsupported(&root)?;

    let mut buffers = Vec::with_capacity(root.buffers.len());
    for buffer in &root.buffers {
        buffers.push(resolve_buffer(buffer, base_dir)?);
    }
    build(root, buffers)
}

/// Read a `.glb` container.
pub fn import_glb(bytes: &[u8]) -> Result<Document, ImportError> {
    const HEADER: usize = 12;
    if bytes.len() < HEADER {
        return Err(ImportError::Malformed(
            "GLB is shorter than its header".into(),
        ));
    }
    if &bytes[..4] != b"glTF" {
        return Err(ImportError::Malformed("GLB magic is not 'glTF'".into()));
    }
    let version = u32::from_le_bytes(bytes[4..8].try_into().expect("4 bytes"));
    if version != 2 {
        return Err(ImportError::Unsupported(format!("GLB version {version}")));
    }

    let mut json_chunk: Option<&[u8]> = None;
    let mut bin_chunk: Option<&[u8]> = None;
    let mut offset = HEADER;
    while offset + 8 <= bytes.len() {
        let length = u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("4")) as usize;
        let kind = &bytes[offset + 4..offset + 8];
        let start = offset + 8;
        let end = start.saturating_add(length).min(bytes.len());
        match kind {
            b"JSON" => json_chunk = Some(&bytes[start..end]),
            b"BIN\0" => bin_chunk = Some(&bytes[start..end]),
            _ => {} // Unknown chunks are skippable by specification.
        }
        // Chunks are padded to four bytes.
        offset = start + length + (4 - (length % 4)) % 4;
    }

    let json = json_chunk.ok_or_else(|| ImportError::Malformed("GLB has no JSON chunk".into()))?;
    let text = std::str::from_utf8(json)
        .map_err(|e| ImportError::Malformed(format!("GLB JSON is not UTF-8: {e}")))?;

    let root: Root = serde_json::from_str(text).map_err(|e| ImportError::Json(e.to_string()))?;
    reject_unsupported(&root)?;

    let mut buffers = Vec::with_capacity(root.buffers.len());
    for (index, buffer) in root.buffers.iter().enumerate() {
        buffers.push(match &buffer.uri {
            // A GLB's first buffer with no URI is the BIN chunk.
            None => bin_chunk
                .map(|b| b.to_vec())
                .ok_or_else(|| ImportError::Malformed("GLB has no BIN chunk".into()))?,
            Some(uri) => resolve_uri(uri, None)
                .map_err(|e| ImportError::Malformed(format!("buffer {index}: {e}")))?,
        });
    }
    build(root, buffers)
}

fn reject_unsupported(root: &Root) -> Result<(), ImportError> {
    const HANDLED: &[&str] = &[
        "KHR_materials_unlit",
        "KHR_materials_emissive_strength",
        "KHR_texture_transform",
        "KHR_materials_specular",
        "KHR_materials_ior",
        "KHR_materials_clearcoat",
        "KHR_materials_volume",
        "KHR_materials_sheen",
        "KHR_materials_transmission",
    ];
    for required in &root.extensions_required {
        if !HANDLED.contains(&required.as_str()) {
            return Err(ImportError::Unsupported(format!(
                "'{required}' is required by this file. Compressed geometry such as \
                 KHR_draco_mesh_compression or EXT_meshopt_compression cannot be read; \
                 re-export without compression."
            )));
        }
    }
    Ok(())
}

fn resolve_buffer(buffer: &BufferDef, base_dir: Option<&Path>) -> Result<Vec<u8>, ImportError> {
    match &buffer.uri {
        None => Err(ImportError::Malformed(
            "a buffer with no URI is only valid inside a GLB".into(),
        )),
        Some(uri) => resolve_uri(uri, base_dir),
    }
}

fn resolve_uri(uri: &str, base_dir: Option<&Path>) -> Result<Vec<u8>, ImportError> {
    if let Some(rest) = uri.strip_prefix("data:") {
        let (meta, payload) = rest
            .split_once(',')
            .ok_or_else(|| ImportError::Malformed("malformed data URI".into()))?;
        if !meta.contains("base64") {
            return Err(ImportError::Unsupported(
                "only base64 data URIs are supported".into(),
            ));
        }
        return base64_decode(payload)
            .ok_or_else(|| ImportError::Malformed("invalid base64 in data URI".into()));
    }

    // Percent-encoded URIs are legal; decode the common cases.
    let decoded = percent_decode(uri);
    let path = match base_dir {
        Some(base) => base.join(decoded),
        None => PathBuf::from(decoded),
    };
    std::fs::read(&path)
        .map_err(|e| ImportError::Io(format!("could not read buffer '{}': {e}", path.display())))
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(value) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(value);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

// ---------------------------------------------------------------------------
// Building the document
// ---------------------------------------------------------------------------

struct Reader<'a> {
    root: &'a Root,
    buffers: Vec<Vec<u8>>,
}

impl<'a> Reader<'a> {
    /// Raw bytes for one accessor element, honouring byte stride.
    fn element(&self, accessor_index: usize, element: usize) -> Result<&[u8], ImportError> {
        let accessor =
            self.root.accessors.get(accessor_index).ok_or_else(|| {
                ImportError::Malformed(format!("accessor {accessor_index} missing"))
            })?;

        let size = component_size(accessor.component_type).ok_or_else(|| {
            ImportError::Unsupported(format!("component type {}", accessor.component_type))
        })?;
        let components = component_count(&accessor.kind).ok_or_else(|| {
            ImportError::Unsupported(format!("accessor type '{}'", accessor.kind))
        })?;
        let element_size = size * components;

        let view =
            self.root
                .buffer_views
                .get(accessor.buffer_view.ok_or_else(|| {
                    ImportError::Unsupported("accessors without a bufferView".into())
                })?)
                .ok_or_else(|| ImportError::Malformed("bufferView missing".into()))?;

        let buffer = self
            .buffers
            .get(view.buffer)
            .ok_or_else(|| ImportError::Malformed(format!("buffer {} missing", view.buffer)))?;

        // Tightly packed unless the view says otherwise. Getting this wrong is
        // the classic interleaved-buffer bug: readable, plausible, wrong.
        let stride = view.byte_stride.unwrap_or(element_size);
        let start = view.byte_offset + accessor.byte_offset + element * stride;
        let end = start + element_size;
        // Check against the declared view length first: it gives a precise
        // message, and catches a view that overruns its buffer.
        if end > view.byte_offset + view.byte_length {
            return Err(ImportError::Malformed(format!(
                "accessor {accessor_index} element {element} runs past bufferView bounds"
            )));
        }
        if end > buffer.len() {
            return Err(ImportError::Malformed(format!(
                "accessor {accessor_index} element {element} runs past its buffer"
            )));
        }
        Ok(&buffer[start..end])
    }

    /// An accessor as floats, dequantizing normalized integers.
    fn floats(&self, accessor_index: usize) -> Result<(Vec<f32>, usize), ImportError> {
        let accessor =
            self.root.accessors.get(accessor_index).ok_or_else(|| {
                ImportError::Malformed(format!("accessor {accessor_index} missing"))
            })?;
        let components = component_count(&accessor.kind).ok_or_else(|| {
            ImportError::Unsupported(format!("accessor type '{}'", accessor.kind))
        })?;

        let mut out = Vec::with_capacity(accessor.count * components);
        for element in 0..accessor.count {
            let bytes = self.element(accessor_index, element)?;
            for c in 0..components {
                out.push(decode_component(
                    bytes,
                    c,
                    accessor.component_type,
                    accessor.normalized,
                )?);
            }
        }
        Ok((out, components))
    }

    /// An accessor as u32 indices.
    fn indices(&self, accessor_index: usize) -> Result<Vec<u32>, ImportError> {
        let accessor =
            self.root.accessors.get(accessor_index).ok_or_else(|| {
                ImportError::Malformed(format!("accessor {accessor_index} missing"))
            })?;
        let mut out = Vec::with_capacity(accessor.count);
        for element in 0..accessor.count {
            let bytes = self.element(accessor_index, element)?;
            let value = match accessor.component_type {
                component::UNSIGNED_BYTE => bytes[0] as u32,
                component::UNSIGNED_SHORT => {
                    u16::from_le_bytes(bytes[..2].try_into().expect("2")) as u32
                }
                component::UNSIGNED_INT => u32::from_le_bytes(bytes[..4].try_into().expect("4")),
                other => {
                    return Err(ImportError::Unsupported(format!(
                        "index component type {other}"
                    )))
                }
            };
            out.push(value);
        }
        Ok(out)
    }

    fn vec3s(&self, accessor_index: usize) -> Result<Vec<[f32; 3]>, ImportError> {
        let (data, components) = self.floats(accessor_index)?;
        if components < 3 {
            return Err(ImportError::Malformed(format!(
                "accessor {accessor_index} has {components} components, expected 3"
            )));
        }
        Ok(data
            .chunks_exact(components)
            .map(|c| [c[0], c[1], c[2]])
            .collect())
    }

    fn vec2s(&self, accessor_index: usize) -> Result<Vec<[f32; 2]>, ImportError> {
        let (data, components) = self.floats(accessor_index)?;
        if components < 2 {
            return Err(ImportError::Malformed(format!(
                "accessor {accessor_index} has {components} components, expected 2"
            )));
        }
        Ok(data
            .chunks_exact(components)
            .map(|c| [c[0], c[1]])
            .collect())
    }
}

/// Read component `index` of an element as f32.
fn decode_component(
    bytes: &[u8],
    index: usize,
    component_type: u32,
    normalized: bool,
) -> Result<f32, ImportError> {
    let size = component_size(component_type)
        .ok_or_else(|| ImportError::Unsupported(format!("component type {component_type}")))?;
    let start = index * size;
    let slice = bytes
        .get(start..start + size)
        .ok_or_else(|| ImportError::Malformed("element is short".into()))?;

    Ok(match component_type {
        component::FLOAT => f32::from_le_bytes(slice.try_into().expect("4")),
        component::UNSIGNED_BYTE => {
            let v = slice[0] as f32;
            if normalized {
                v / 255.0
            } else {
                v
            }
        }
        component::BYTE => {
            let v = slice[0] as i8 as f32;
            if normalized {
                (v / 127.0).max(-1.0)
            } else {
                v
            }
        }
        component::UNSIGNED_SHORT => {
            let v = u16::from_le_bytes(slice.try_into().expect("2")) as f32;
            if normalized {
                v / 65535.0
            } else {
                v
            }
        }
        component::SHORT => {
            let v = i16::from_le_bytes(slice.try_into().expect("2")) as f32;
            if normalized {
                (v / 32767.0).max(-1.0)
            } else {
                v
            }
        }
        component::UNSIGNED_INT => u32::from_le_bytes(slice.try_into().expect("4")) as f32,
        other => return Err(ImportError::Unsupported(format!("component type {other}"))),
    })
}

/// Decompose a column-major 4x4 into translation, rotation, and scale.
///
/// Shear cannot be represented as TRS and is dropped; glTF only allows a
/// `matrix` when TRS is insufficient, so this is a lossy fallback rather than
/// the normal path.
fn decompose(matrix: &[f32; 16]) -> ([f32; 3], [f32; 4], [f32; 3]) {
    let translation = [matrix[12], matrix[13], matrix[14]];

    let mut basis = [
        [matrix[0], matrix[1], matrix[2]],
        [matrix[4], matrix[5], matrix[6]],
        [matrix[8], matrix[9], matrix[10]],
    ];
    let mut scale = [0.0f32; 3];
    for axis in 0..3 {
        scale[axis] = (basis[axis][0] * basis[axis][0]
            + basis[axis][1] * basis[axis][1]
            + basis[axis][2] * basis[axis][2])
            .sqrt();
        if scale[axis] > f32::EPSILON {
            for value in basis[axis].iter_mut() {
                *value /= scale[axis];
            }
        }
    }

    // Negate a mirrored axis so the rotation stays a proper rotation.
    let determinant = basis[0][0] * (basis[1][1] * basis[2][2] - basis[1][2] * basis[2][1])
        - basis[0][1] * (basis[1][0] * basis[2][2] - basis[1][2] * basis[2][0])
        + basis[0][2] * (basis[1][0] * basis[2][1] - basis[1][1] * basis[2][0]);
    if determinant < 0.0 {
        for row in basis.iter_mut() {
            row[0] = -row[0];
        }
        scale[0] = -scale[0];
    }

    // Standard rotation-matrix to quaternion.
    let trace = basis[0][0] + basis[1][1] + basis[2][2];
    let rotation = if trace > 0.0 {
        let s = (trace + 1.0).sqrt() * 2.0;
        [
            (basis[1][2] - basis[2][1]) / s,
            (basis[2][0] - basis[0][2]) / s,
            (basis[0][1] - basis[1][0]) / s,
            0.25 * s,
        ]
    } else if basis[0][0] > basis[1][1] && basis[0][0] > basis[2][2] {
        let s = (1.0 + basis[0][0] - basis[1][1] - basis[2][2]).sqrt() * 2.0;
        [
            0.25 * s,
            (basis[1][0] + basis[0][1]) / s,
            (basis[2][0] + basis[0][2]) / s,
            (basis[1][2] - basis[2][1]) / s,
        ]
    } else if basis[1][1] > basis[2][2] {
        let s = (1.0 + basis[1][1] - basis[0][0] - basis[2][2]).sqrt() * 2.0;
        [
            (basis[1][0] + basis[0][1]) / s,
            0.25 * s,
            (basis[2][1] + basis[1][2]) / s,
            (basis[2][0] - basis[0][2]) / s,
        ]
    } else {
        let s = (1.0 + basis[2][2] - basis[0][0] - basis[1][1]).sqrt() * 2.0;
        [
            (basis[2][0] + basis[0][2]) / s,
            (basis[2][1] + basis[1][2]) / s,
            0.25 * s,
            (basis[0][1] - basis[1][0]) / s,
        ]
    };

    (translation, crate::anim::normalize_quat(&rotation), scale)
}

fn build(root: Root, buffers: Vec<Vec<u8>>) -> Result<Document, ImportError> {
    // A buffer shorter than it claims means a truncated download or a
    // half-written file; fail now rather than reading zeros later.
    for (index, (declared, resolved)) in root.buffers.iter().zip(&buffers).enumerate() {
        if resolved.len() < declared.byte_length {
            return Err(ImportError::Malformed(format!(
                "buffer {index} declares {} bytes but only {} were read",
                declared.byte_length,
                resolved.len()
            )));
        }
    }

    // Sparse accessors substitute individual elements. Ignoring the
    // substitution would yield data that looks valid and is wrong, so refuse.
    for (index, accessor) in root.accessors.iter().enumerate() {
        if accessor.sparse.is_some() {
            return Err(ImportError::Unsupported(format!(
                "accessor {index} is sparse; sparse substitution is not implemented"
            )));
        }
    }

    let reader = Reader {
        root: &root,
        buffers,
    };

    let mut document = Document::new("imported");

    // -- materials ---------------------------------------------------------
    for material in &root.materials {
        let pbr = material.pbr_metallic_roughness.as_ref();
        let mut out = Material::new(material.name.clone().unwrap_or_else(|| "Material".into()));
        out.base_color = pbr
            .and_then(|p| p.base_color_factor)
            .unwrap_or([1.0, 1.0, 1.0, 1.0]);
        out.metallic = pbr.and_then(|p| p.metallic_factor).unwrap_or(1.0);
        out.roughness = pbr.and_then(|p| p.roughness_factor).unwrap_or(1.0);
        out.emissive = material.emissive_factor.unwrap_or([0.0; 3]);
        // glTF defaults to OPAQUE, where alpha in the base colour is ignored.
        // Honouring it keeps a round trip faithful in both directions.
        let opaque = !matches!(material.alpha_mode.as_deref(), Some("BLEND") | Some("MASK"));
        if opaque {
            out.base_color[3] = 1.0;
        }
        out.double_sided = material.double_sided;
        document.add_material(out);
    }

    // -- meshes ------------------------------------------------------------
    // One document mesh per glTF primitive, remembering the mapping so nodes
    // can be rebuilt with the right material.
    let mut primitive_mesh: Vec<Vec<usize>> = Vec::with_capacity(root.meshes.len());

    for mesh_def in &root.meshes {
        let mut indices = Vec::with_capacity(mesh_def.primitives.len());
        for (primitive_index, primitive) in mesh_def.primitives.iter().enumerate() {
            if primitive.mode != 4 {
                return Err(ImportError::Unsupported(format!(
                    "primitive mode {} (only triangles, mode 4, are supported)",
                    primitive.mode
                )));
            }
            if primitive
                .extensions
                .as_ref()
                .is_some_and(|e| e.get("KHR_draco_mesh_compression").is_some())
            {
                return Err(ImportError::Unsupported(
                    "KHR_draco_mesh_compression cannot be read; re-export without compression"
                        .into(),
                ));
            }

            let base_name = mesh_def.name.clone().unwrap_or_else(|| "Mesh".into());
            let name = if mesh_def.primitives.len() > 1 {
                format!("{base_name}_{primitive_index}")
            } else {
                base_name
            };
            let mut mesh = Mesh::new(name);

            if let Some(position) = primitive.attributes.get("POSITION") {
                mesh.positions = reader.vec3s(*position)?;
            } else {
                return Err(ImportError::Malformed(format!(
                    "primitive {primitive_index} has no POSITION attribute"
                )));
            }
            if let Some(normal) = primitive.attributes.get("NORMAL") {
                mesh.normals = reader.vec3s(*normal)?;
            }
            if let Some(uv) = primitive.attributes.get("TEXCOORD_0") {
                mesh.uvs = reader.vec2s(*uv)?;
            }
            if let Some(index_accessor) = primitive.indices {
                mesh.indices = reader.indices(index_accessor)?;
            } else {
                // Non-indexed geometry is legal; synthesize a trivial index run.
                mesh.indices = (0..mesh.positions.len() as u32).collect();
            }

            // Fill gaps so the mesh is exportable straight back out.
            if mesh.normals.len() != mesh.positions.len() {
                mesh.compute_normals();
            }
            mesh.ensure_uvs();

            indices.push(document.add_mesh(mesh));
        }
        primitive_mesh.push(indices);
    }

    // -- nodes -------------------------------------------------------------
    let mut parent_of: HashMap<usize, usize> = HashMap::new();
    for (index, node) in root.nodes.iter().enumerate() {
        for child in &node.children {
            parent_of.insert(*child, index);
        }
    }

    // Document node indices 0..n map one-to-one onto glTF node indices, so
    // animation channels keep pointing at the right node. Children synthesized
    // for multi-primitive meshes are appended after, never interleaved.
    let mut nodes: Vec<Node> = Vec::with_capacity(root.nodes.len());
    let mut synthesized: Vec<Node> = Vec::new();

    for (index, node_def) in root.nodes.iter().enumerate() {
        let mut node = Node::new(
            node_def
                .name
                .clone()
                .unwrap_or_else(|| format!("Node{index}")),
        );

        if let Some(matrix) = node_def.matrix {
            let (t, r, s) = decompose(&matrix);
            node.translation = t;
            node.rotation = r;
            node.scale = s;
        } else {
            node.translation = node_def.translation.unwrap_or([0.0; 3]);
            node.rotation = node_def.rotation.unwrap_or(crate::scene::IDENTITY_ROTATION);
            node.scale = node_def.scale.unwrap_or([1.0; 3]);
        }
        node.children = node_def.children.clone();

        if let Some(mesh_index) = node_def.mesh {
            let definition = root.meshes.get(mesh_index).ok_or_else(|| {
                ImportError::Malformed(format!("node {index} references missing mesh {mesh_index}"))
            })?;
            let document_meshes = primitive_mesh
                .get(mesh_index)
                .ok_or_else(|| ImportError::Malformed(format!("mesh {mesh_index} was not read")))?;

            match document_meshes.as_slice() {
                [] => {}
                // The common case, including everything Aurum itself writes.
                [only] => {
                    node.mesh = Some(*only);
                    node.material = definition.primitives.first().and_then(|p| p.material);
                }
                // glTF carries the material per primitive, but a node carries
                // one mesh, so each primitive becomes a child node instead.
                many => {
                    for (slot, mesh) in many.iter().enumerate() {
                        let child_index = root.nodes.len() + synthesized.len();
                        let mut child = Node::new(format!("{}_p{slot}", node.name));
                        child.mesh = Some(*mesh);
                        child.material = definition.primitives.get(slot).and_then(|p| p.material);
                        node.children.push(child_index);
                        synthesized.push(child);
                    }
                }
            }
        }

        nodes.push(node);
    }

    nodes.extend(synthesized);

    document.scene.nodes = nodes;
    document.scene.roots = root
        .scene
        .and_then(|s| root.scenes.get(s))
        .map(|s| s.nodes.clone())
        .unwrap_or_else(|| {
            // No scene declared: every node without a parent is a root.
            (0..root.nodes.len())
                .filter(|i| !parent_of.contains_key(i))
                .collect()
        });
    // -- animations --------------------------------------------------------
    for animation in &root.animations {
        let mut out = Animation::new(animation.name.clone().unwrap_or_else(|| "Animation".into()));
        for channel in &animation.channels {
            let Some(node) = channel.target.node else {
                continue; // Channels may target the whole scene; not modelled.
            };
            let path = match channel.target.path.as_str() {
                "translation" => TrackPath::Translation,
                "rotation" => TrackPath::Rotation,
                "scale" => TrackPath::Scale,
                _ => continue, // "weights" morph targets are not modelled.
            };
            let Some(sampler) = animation.samplers.get(channel.sampler) else {
                continue;
            };

            let (times, _) = reader.floats(sampler.input)?;
            let (values, components) = reader.floats(sampler.output)?;
            if components != path.components() {
                return Err(ImportError::Malformed(format!(
                    "animation channel for '{path:?}' has {components} components per key"
                )));
            }

            let interpolation = match sampler.interpolation.as_deref() {
                Some("STEP") => Interpolation::Step,
                _ => Interpolation::Linear,
            };

            let mut track = Track::new(node, path).with_interpolation(interpolation);
            for (slot, time) in times.iter().enumerate() {
                let start = slot * components;
                if start + components > values.len() {
                    break;
                }
                track.keyframe(*time, &values[start..start + components]);
            }
            if !track.is_empty() {
                out.add_track(track);
            }
        }
        if !out.is_empty() {
            document.add_animation(out);
        }
    }

    Ok(document)
}

// ---------------------------------------------------------------------------
// Base64
// ---------------------------------------------------------------------------

/// Decode standard base64, tolerating whitespace. Returns `None` on bad input.
pub fn base64_decode(input: &str) -> Option<Vec<u8>> {
    fn value(byte: u8) -> Option<u32> {
        match byte {
            b'A'..=b'Z' => Some((byte - b'A') as u32),
            b'a'..=b'z' => Some((byte - b'a') as u32 + 26),
            b'0'..=b'9' => Some((byte - b'0') as u32 + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }

    let mut out = Vec::with_capacity(input.len() / 4 * 3);
    let mut accumulator = 0u32;
    let mut bits = 0u32;

    for byte in input.bytes() {
        if byte == b'=' {
            break;
        }
        if byte.is_ascii_whitespace() {
            continue;
        }
        let v = value(byte)?;
        accumulator = (accumulator << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((accumulator >> bits) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gltf;
    use crate::mesh::{box_mesh, uv_sphere};
    use crate::scene::Material;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("aurum-import-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Pack JSON and a binary buffer into a GLB container.
    fn to_glb(json: &str, bin: &[u8]) -> Vec<u8> {
        let mut json_bytes = json.as_bytes().to_vec();
        while !json_bytes.len().is_multiple_of(4) {
            json_bytes.push(b' ');
        }
        let mut bin_bytes = bin.to_vec();
        while !bin_bytes.len().is_multiple_of(4) {
            bin_bytes.push(0);
        }

        let total = 12 + 8 + json_bytes.len() + 8 + bin_bytes.len();
        let mut out = Vec::with_capacity(total);
        out.extend_from_slice(b"glTF");
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&(total as u32).to_le_bytes());
        out.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(b"JSON");
        out.extend_from_slice(&json_bytes);
        out.extend_from_slice(&(bin_bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(b"BIN\0");
        out.extend_from_slice(&bin_bytes);
        out
    }

    /// A document with geometry, materials, hierarchy, and animation.
    fn sample() -> Document {
        let mut document = Document::new("round-trip");
        let cube = document.add_mesh(box_mesh(2.0, 1.0, 0.5));
        let sphere = document.add_mesh(uv_sphere(0.5, 12, 8));
        let gold = document.add_material(Material::gold());
        let glass = document.add_material(
            Material::new("Glass")
                .with_color(0.5, 0.7, 1.0)
                .with_alpha(0.25),
        );

        let root = document.scene_mut().add_node("Root");
        {
            let node = document.scene_mut().node_mut(root).unwrap();
            node.mesh = Some(cube);
            node.material = Some(gold);
            node.translation = [1.0, 2.0, 3.0];
        }
        let child = document.scene_mut().add_child(Some(root), "Orb");
        {
            let node = document.scene_mut().node_mut(child).unwrap();
            node.mesh = Some(sphere);
            node.material = Some(glass);
            node.translation = [0.0, 1.0, 0.0];
        }
        document.add_animation(Animation::new("Spin").spin(root, [0.0, 1.0, 0.0], 1.0, 2.0, 5));
        document
    }

    #[test]
    fn base64_decode_matches_known_vectors() {
        assert_eq!(base64_decode("").unwrap(), Vec::<u8>::new());
        assert_eq!(base64_decode("Zg==").unwrap(), b"f");
        assert_eq!(base64_decode("Zm8=").unwrap(), b"fo");
        assert_eq!(base64_decode("Zm9v").unwrap(), b"foo");
        assert_eq!(base64_decode("Zm9vYmFy").unwrap(), b"foobar");
        assert_eq!(
            base64_decode("Zm9v YmFy").unwrap(),
            b"foobar",
            "whitespace is skipped"
        );
        assert!(base64_decode("!!!!").is_none());

        // Round trip through the encoder used by the exporter.
        let data: Vec<u8> = (0..=255u8).collect();
        let encoded = crate::gltf::base64(&data);
        assert_eq!(base64_decode(&encoded).unwrap(), data);
    }

    #[test]
    fn round_trip_through_gltf_files() {
        let dir = temp_dir("gltf");
        let original = sample();
        gltf::write(&original, &dir, "scene").unwrap();

        let imported = import_path(&dir.join("scene.gltf")).unwrap();

        assert_eq!(imported.meshes.len(), original.meshes.len());
        assert_eq!(imported.materials.len(), original.materials.len());
        assert_eq!(imported.scene.nodes.len(), original.scene.nodes.len());
        assert_eq!(imported.animations.len(), 1);

        // Geometry survives exactly.
        for (a, b) in original.meshes.iter().zip(&imported.meshes) {
            assert_eq!(a.positions, b.positions, "positions changed");
            assert_eq!(a.indices, b.indices, "indices changed");
            assert_eq!(a.uvs.len(), b.uvs.len());
            assert_eq!(a.normals.len(), b.normals.len());
        }

        // Hierarchy and transforms survive.
        assert_eq!(imported.scene.roots.len(), 1);
        let root = imported.scene.node(imported.scene.roots[0]).unwrap();
        assert_eq!(root.name, "Root");
        assert_eq!(root.translation, [1.0, 2.0, 3.0]);
        assert_eq!(root.children.len(), 1);
        let child = imported.scene.node(root.children[0]).unwrap();
        assert_eq!(child.name, "Orb");

        // The animation still targets the right node with the right keys.
        let track = imported.animations[0]
            .track_for(imported.scene.roots[0], TrackPath::Rotation)
            .expect("rotation track survived");
        assert_eq!(track.times.len(), 5);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn round_trip_through_glb() {
        let original = sample();
        let (json, bin) = gltf::export(&original).unwrap();

        // A GLB's buffer has no URI; it is the BIN chunk.
        let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
        value["buffers"][0].as_object_mut().unwrap().remove("uri");
        let glb = to_glb(&serde_json::to_string(&value).unwrap(), &bin);

        let imported = import_glb(&glb).unwrap();
        assert_eq!(imported.meshes.len(), original.meshes.len());
        assert_eq!(imported.scene.nodes.len(), original.scene.nodes.len());
        assert_eq!(imported.animations.len(), 1);
        assert_eq!(
            imported.meshes[0].positions, original.meshes[0].positions,
            "GLB geometry should be identical"
        );
    }

    #[test]
    fn glb_is_detected_by_magic_not_extension() {
        let dir = temp_dir("magic");
        let original = sample();
        let (json, bin) = gltf::export(&original).unwrap();
        let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
        value["buffers"][0].as_object_mut().unwrap().remove("uri");
        let glb = to_glb(&serde_json::to_string(&value).unwrap(), &bin);

        // Deliberately the wrong extension.
        let path = dir.join("actually-binary.gltf");
        std::fs::write(&path, &glb).unwrap();
        let imported = import_path(&path).unwrap();
        assert_eq!(imported.meshes.len(), original.meshes.len());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn interleaved_attributes_honour_byte_stride() {
        // Three vertices, each { position: vec3, normal: vec3 } interleaved at
        // a 24-byte stride, then three u16 indices. Reading this without
        // honouring byteStride yields plausible-looking garbage.
        let dir = temp_dir("interleaved");

        let mut buffer = Vec::new();
        let vertices: [([f32; 3], [f32; 3]); 3] = [
            ([0.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            ([0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
        ];
        for (position, normal) in vertices {
            for v in position {
                buffer.extend_from_slice(&v.to_le_bytes());
            }
            for v in normal {
                buffer.extend_from_slice(&v.to_le_bytes());
            }
        }
        let index_offset = buffer.len();
        for i in [0u16, 1, 2] {
            buffer.extend_from_slice(&i.to_le_bytes());
        }
        std::fs::write(dir.join("data.bin"), &buffer).unwrap();

        let json = format!(
            r#"{{
  "asset": {{ "version": "2.0" }},
  "scene": 0,
  "scenes": [{{ "nodes": [0] }}],
  "nodes": [{{ "name": "Tri", "mesh": 0 }}],
  "meshes": [{{ "name": "Tri", "primitives": [{{
      "attributes": {{ "POSITION": 0, "NORMAL": 1 }}, "indices": 2
  }}] }}],
  "accessors": [
    {{ "bufferView": 0, "byteOffset": 0,  "componentType": 5126, "count": 3, "type": "VEC3" }},
    {{ "bufferView": 0, "byteOffset": 12, "componentType": 5126, "count": 3, "type": "VEC3" }},
    {{ "bufferView": 1, "componentType": 5123, "count": 3, "type": "SCALAR" }}
  ],
  "bufferViews": [
    {{ "buffer": 0, "byteOffset": 0, "byteLength": {index_offset}, "byteStride": 24 }},
    {{ "buffer": 0, "byteOffset": {index_offset}, "byteLength": 6 }}
  ],
  "buffers": [{{ "uri": "data.bin", "byteLength": {total} }}]
}}"#,
            total = buffer.len()
        );
        std::fs::write(dir.join("interleaved.gltf"), &json).unwrap();

        let imported = import_path(&dir.join("interleaved.gltf")).unwrap();
        assert_eq!(imported.meshes.len(), 1);
        let mesh = &imported.meshes[0];
        assert_eq!(mesh.positions.len(), 3);
        assert_eq!(mesh.positions[0], [0.0, 0.0, 0.0]);
        assert_eq!(mesh.positions[1], [1.0, 0.0, 0.0]);
        assert_eq!(mesh.positions[2], [0.0, 0.0, 1.0]);
        // Stride ignored would have read vertex 1's normal as its position.
        assert_eq!(mesh.normals[0], [0.0, 1.0, 0.0]);
        assert_eq!(mesh.normals[2], [0.0, 1.0, 0.0]);
        assert_eq!(mesh.indices, vec![0, 1, 2]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn normalized_byte_attributes_are_dequantized() {
        let dir = temp_dir("normalized");

        // Three positions (f32) then three UVs as normalized u8 pairs.
        let mut buffer = Vec::new();
        for p in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]] {
            for v in p {
                buffer.extend_from_slice(&v.to_le_bytes());
            }
        }
        let uv_offset = buffer.len();
        buffer.extend_from_slice(&[0u8, 0, 255, 0, 0, 255]);

        std::fs::write(dir.join("data.bin"), &buffer).unwrap();
        let json = format!(
            r#"{{
  "asset": {{ "version": "2.0" }},
  "scene": 0,
  "scenes": [{{ "nodes": [0] }}],
  "nodes": [{{ "mesh": 0 }}],
  "meshes": [{{ "primitives": [{{ "attributes": {{ "POSITION": 0, "TEXCOORD_0": 1 }} }}] }}],
  "accessors": [
    {{ "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3" }},
    {{ "bufferView": 1, "componentType": 5121, "count": 3, "type": "VEC2", "normalized": true }}
  ],
  "bufferViews": [
    {{ "buffer": 0, "byteOffset": 0, "byteLength": {uv_offset} }},
    {{ "buffer": 0, "byteOffset": {uv_offset}, "byteLength": 6 }}
  ],
  "buffers": [{{ "uri": "data.bin", "byteLength": {total} }}]
}}"#,
            total = buffer.len()
        );
        std::fs::write(dir.join("norm.gltf"), &json).unwrap();

        let imported = import_path(&dir.join("norm.gltf")).unwrap();
        let uvs = &imported.meshes[0].uvs;
        assert_eq!(uvs.len(), 3);
        assert!((uvs[1][0] - 1.0).abs() < 1e-6, "255 must dequantize to 1.0");
        assert_eq!(uvs[0], [0.0, 0.0]);
        assert!((uvs[2][1] - 1.0).abs() < 1e-6);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn embedded_data_uri_round_trip() {
        let dir = temp_dir("embedded");
        let original = sample();
        let text = gltf::export_embedded(&original).unwrap();
        std::fs::write(dir.join("one.gltf"), &text).unwrap();

        let imported = import_path(&dir.join("one.gltf")).unwrap();
        assert_eq!(imported.meshes.len(), original.meshes.len());
        assert_eq!(
            imported.meshes[0].positions, original.meshes[0].positions,
            "data URI geometry should be identical"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn multi_primitive_meshes_become_child_nodes() {
        let dir = temp_dir("multiprim");

        let mut buffer = Vec::new();
        for p in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]] {
            for v in p {
                buffer.extend_from_slice(&v.to_le_bytes());
            }
        }
        std::fs::write(dir.join("data.bin"), &buffer).unwrap();

        // One glTF mesh, two primitives, each with its own material.
        let json = format!(
            r#"{{
  "asset": {{ "version": "2.0" }},
  "scene": 0,
  "scenes": [{{ "nodes": [0] }}],
  "nodes": [{{ "name": "Multi", "mesh": 0, "translation": [0, 5, 0] }}],
  "meshes": [{{ "name": "Multi", "primitives": [
    {{ "attributes": {{ "POSITION": 0 }}, "material": 0 }},
    {{ "attributes": {{ "POSITION": 0 }}, "material": 1 }}
  ] }}],
  "materials": [{{ "name": "A" }}, {{ "name": "B" }}],
  "accessors": [{{ "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3" }}],
  "bufferViews": [{{ "buffer": 0, "byteOffset": 0, "byteLength": {total} }}],
  "buffers": [{{ "uri": "data.bin", "byteLength": {total} }}]
}}"#,
            total = buffer.len()
        );
        std::fs::write(dir.join("multi.gltf"), &json).unwrap();

        let imported = import_path(&dir.join("multi.gltf")).unwrap();
        assert_eq!(imported.meshes.len(), 2, "one document mesh per primitive");

        // The node keeps its transform and gains two children carrying the
        // meshes, so both materials survive.
        let node = imported.scene.node(0).unwrap();
        assert_eq!(node.translation, [0.0, 5.0, 0.0]);
        assert_eq!(node.mesh, None);
        assert_eq!(node.children.len(), 2);
        assert_eq!(node.children, vec![1, 2]);

        let first = imported.scene.node(1).unwrap();
        let second = imported.scene.node(2).unwrap();
        assert_eq!(first.material, Some(0));
        assert_eq!(second.material, Some(1));
        assert_ne!(first.mesh, second.mesh);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn matrix_transforms_are_decomposed() {
        let dir = temp_dir("matrix");

        let mut buffer = Vec::new();
        for p in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
            for v in p {
                buffer.extend_from_slice(&v.to_le_bytes());
            }
        }
        std::fs::write(dir.join("data.bin"), &buffer).unwrap();

        // Scale 2 on each axis, then a 90 degree turn about Y, then translate.
        let expected = crate::mesh::Mat4::from_trs(
            [4.0, 5.0, 6.0],
            crate::anim::quat_from_axis_angle([0.0, 1.0, 0.0], std::f32::consts::FRAC_PI_2),
            [2.0, 2.0, 2.0],
        );
        let matrix: Vec<String> = expected.0.iter().map(|v| format!("{v}")).collect();

        let json = format!(
            r#"{{
  "asset": {{ "version": "2.0" }},
  "scene": 0,
  "scenes": [{{ "nodes": [0] }}],
  "nodes": [{{ "name": "M", "mesh": 0, "matrix": [{}] }}],
  "meshes": [{{ "primitives": [{{ "attributes": {{ "POSITION": 0 }} }}] }}],
  "accessors": [{{ "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3" }}],
  "bufferViews": [{{ "buffer": 0, "byteOffset": 0, "byteLength": {total} }}],
  "buffers": [{{ "uri": "data.bin", "byteLength": {total} }}]
}}"#,
            matrix.join(", "),
            total = buffer.len()
        );
        std::fs::write(dir.join("matrix.gltf"), &json).unwrap();

        let imported = import_path(&dir.join("matrix.gltf")).unwrap();
        let node = imported.scene.node(0).unwrap();

        assert!((node.translation[0] - 4.0).abs() < 1e-4);
        assert!((node.translation[1] - 5.0).abs() < 1e-4);
        assert!((node.scale[0] - 2.0).abs() < 1e-4);

        // The decomposed TRS must rebuild the original transform.
        let rebuilt = node.local_transform();
        let point = rebuilt.transform_point([1.0, 0.0, 0.0]);
        let reference = expected.transform_point([1.0, 0.0, 0.0]);
        for axis in 0..3 {
            assert!(
                (point[axis] - reference[axis]).abs() < 1e-3,
                "axis {axis}: {point:?} vs {reference:?}"
            );
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sparse_accessors_are_refused() {
        let json = r#"{
  "asset": { "version": "2.0" },
  "accessors": [{
    "componentType": 5126, "count": 3, "type": "VEC3",
    "sparse": { "count": 1, "indices": { "bufferView": 0, "componentType": 5123 }, "values": { "bufferView": 1 } }
  }],
  "bufferViews": [
    { "buffer": 0, "byteOffset": 0, "byteLength": 2 },
    { "buffer": 0, "byteOffset": 4, "byteLength": 12 }
  ],
  "buffers": [{ "uri": "data:application/octet-stream;base64,AAAAAAAAAAAAAAAAAAAAAA==", "byteLength": 16 }]
}"#;
        match import_gltf_json(json, None) {
            Err(ImportError::Unsupported(message)) => assert!(message.contains("sparse")),
            other => panic!("expected a sparse rejection, got {other:?}"),
        }
    }

    #[test]
    fn draco_compression_is_refused_with_a_useful_message() {
        let json = r#"{
  "asset": { "version": "2.0" },
  "extensionsRequired": ["KHR_draco_mesh_compression"],
  "extensionsUsed": ["KHR_draco_mesh_compression"]
}"#;
        match import_gltf_json(json, None) {
            Err(ImportError::Unsupported(message)) => {
                assert!(
                    message.contains("draco") || message.contains("Draco"),
                    "{message}"
                );
                assert!(
                    message.contains("re-export"),
                    "the message should say what to do"
                );
            }
            other => panic!("expected an unsupported error, got {other:?}"),
        }
    }

    #[test]
    fn harmless_required_extensions_are_accepted() {
        let json = r#"{
  "asset": { "version": "2.0" },
  "extensionsRequired": ["KHR_materials_unlit"]
}"#;
        assert!(import_gltf_json(json, None).is_ok());
    }

    #[test]
    fn non_triangle_modes_are_refused() {
        let json = r#"{
  "asset": { "version": "2.0" },
  "meshes": [{ "primitives": [{ "attributes": { "POSITION": 0 }, "mode": 1 }] }],
  "accessors": [{ "componentType": 5126, "count": 3, "type": "VEC3" }]
}"#;
        match import_gltf_json(json, None) {
            Err(ImportError::Unsupported(message)) => assert!(message.contains("mode 1")),
            other => panic!("expected a mode rejection, got {other:?}"),
        }
    }

    #[test]
    fn truncated_buffers_are_refused() {
        let json = r#"{
  "asset": { "version": "2.0" },
  "buffers": [{ "uri": "data:application/octet-stream;base64,AAAAAA==", "byteLength": 4096 }]
}"#;
        match import_gltf_json(json, None) {
            Err(ImportError::Malformed(message)) => {
                assert!(message.contains("declares 4096"), "{message}")
            }
            other => panic!("expected a truncation error, got {other:?}"),
        }
    }

    #[test]
    fn a_primitive_without_positions_is_refused() {
        let json = r#"{
  "asset": { "version": "2.0" },
  "scene": 0,
  "scenes": [{ "nodes": [0] }],
  "nodes": [{ "mesh": 0 }],
  "meshes": [{ "primitives": [{ "attributes": { "NORMAL": 0 } }] }],
  "accessors": [{ "componentType": 5126, "count": 3, "type": "VEC3" }]
}"#;
        match import_gltf_json(json, None) {
            Err(ImportError::Malformed(message)) => assert!(message.contains("POSITION")),
            other => panic!("expected a POSITION error, got {other:?}"),
        }
    }

    #[test]
    fn non_indexed_geometry_gains_a_trivial_index_run() {
        let dir = temp_dir("nonindexed");
        let mut buffer = Vec::new();
        for p in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
            for v in p {
                buffer.extend_from_slice(&v.to_le_bytes());
            }
        }
        std::fs::write(dir.join("data.bin"), &buffer).unwrap();

        let json = format!(
            r#"{{
  "asset": {{ "version": "2.0" }},
  "scene": 0,
  "scenes": [{{ "nodes": [0] }}],
  "nodes": [{{ "mesh": 0 }}],
  "meshes": [{{ "primitives": [{{ "attributes": {{ "POSITION": 0 }} }}] }}],
  "accessors": [{{ "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3" }}],
  "bufferViews": [{{ "buffer": 0, "byteOffset": 0, "byteLength": {total} }}],
  "buffers": [{{ "uri": "data.bin", "byteLength": {total} }}]
}}"#,
            total = buffer.len()
        );
        std::fs::write(dir.join("plain.gltf"), &json).unwrap();

        let imported = import_path(&dir.join("plain.gltf")).unwrap();
        let mesh = &imported.meshes[0];
        assert_eq!(mesh.indices, vec![0, 1, 2]);
        // Missing normals and uvs are filled so the mesh re-exports cleanly.
        assert!(mesh.validate().is_empty(), "{:?}", mesh.validate());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_file_is_an_io_error() {
        let result = import_path(Path::new("definitely-not-here.gltf"));
        assert!(matches!(result, Err(ImportError::Io(_))));
    }

    #[test]
    fn malformed_glb_is_refused() {
        assert!(matches!(import_glb(&[]), Err(ImportError::Malformed(_))));
        assert!(matches!(
            import_glb(b"nope"),
            Err(ImportError::Malformed(_))
        ));
        // Correct magic, wrong version.
        let mut bad = b"glTF".to_vec();
        bad.extend_from_slice(&1u32.to_le_bytes());
        bad.extend_from_slice(&12u32.to_le_bytes());
        assert!(matches!(import_glb(&bad), Err(ImportError::Unsupported(_))));
    }
}
