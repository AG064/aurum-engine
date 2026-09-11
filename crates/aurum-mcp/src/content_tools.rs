//! Content-authoring tools: meshes, materials, nodes, animations, sprites.
//!
//! These wrap [`aurum_content`] so an agent can build a scene and write glTF
//! without Godot or Blender running. Like the runtime tools, every handler is
//! a thin adapter — the geometry, hierarchy, and export semantics all live in
//! the content crate.
//!
//! Two conventions worth knowing:
//!
//! - **Indices are stable.** Removing a node detaches it from the graph but
//!   keeps its slot, so animation targets and existing handles stay valid.
//! - **Content is not session state.** `aurum_reset` does not touch it, and it
//!   is not part of a save file; authored work is exported, not checkpointed.

use aurum_content::anim::{Interpolation, TrackPath};
use aurum_content::{
    box_mesh, cone, cylinder, plane_mesh, torus, uv_sphere, Animation, Document, Mat4, Material,
    Mesh,
};
use serde_json::{json, Value};

use crate::tools::{ok, schema, Args, Tool, ToolContext, ToolError, ToolResult};

// ---------------------------------------------------------------------------
// Parameter helpers
// ---------------------------------------------------------------------------

/// Read an array-of-numbers parameter, falling back to a default.
fn number_array<const N: usize>(
    args: &Args<'_>,
    key: &str,
    default: [f32; N],
) -> Result<[f32; N], ToolError> {
    match args.get(key) {
        None => Ok(default),
        Some(Value::Array(items)) => {
            if items.len() != N {
                return Err(ToolError::Invalid(format!(
                    "'{key}' must have {N} numbers, got {}",
                    items.len()
                )));
            }
            let mut out = [0.0f32; N];
            for (index, item) in items.iter().enumerate() {
                out[index] = item.as_f64().ok_or_else(|| {
                    ToolError::Invalid(format!("'{key}[{index}]' must be a number"))
                })? as f32;
            }
            Ok(out)
        }
        Some(other) => Err(ToolError::Invalid(format!(
            "'{key}' must be an array of {N} numbers, got {}",
            crate::tools::kind_of(other)
        ))),
    }
}

fn f32_or(args: &Args<'_>, key: &str, default: f32) -> Result<f32, ToolError> {
    match args.get(key) {
        None => Ok(default),
        Some(Value::Number(n)) => n
            .as_f64()
            .map(|v| v as f32)
            .ok_or_else(|| ToolError::Invalid(format!("'{key}' must be a number"))),
        Some(other) => Err(ToolError::Invalid(format!(
            "'{key}' must be a number, got {}",
            crate::tools::kind_of(other)
        ))),
    }
}

fn u32_or(args: &Args<'_>, key: &str, default: u32) -> Result<u32, ToolError> {
    match args.get(key) {
        None => Ok(default),
        Some(Value::Number(n)) => n
            .as_u64()
            .map(|v| v.min(u32::MAX as u64) as u32)
            .ok_or_else(|| ToolError::Invalid(format!("'{key}' must be a non-negative integer"))),
        Some(other) => Err(ToolError::Invalid(format!(
            "'{key}' must be a non-negative integer, got {}",
            crate::tools::kind_of(other)
        ))),
    }
}

/// Resolve a mesh index, reporting a useful error when it does not exist.
fn mesh_index(args: &Args<'_>, key: &str, document: &Document) -> Result<usize, ToolError> {
    let index = args.i64(key)?;
    if index < 0 || index as usize >= document.meshes.len() {
        return Err(ToolError::Invalid(format!(
            "'{key}' {index} is not a mesh index; there are {} meshes",
            document.meshes.len()
        )));
    }
    Ok(index as usize)
}

fn node_index(args: &Args<'_>, key: &str, document: &Document) -> Result<usize, ToolError> {
    let index = args.i64(key)?;
    if index < 0 || index as usize >= document.scene.len() {
        return Err(ToolError::Invalid(format!(
            "'{key}' {index} is not a node index; there are {} nodes",
            document.scene.len()
        )));
    }
    Ok(index as usize)
}

/// A compact summary of one mesh.
fn mesh_summary(index: usize, mesh: &Mesh) -> Value {
    json!({
        "index": index,
        "name": mesh.name,
        "vertices": mesh.vertex_count(),
        "triangles": mesh.triangle_count(),
    })
}

// ---------------------------------------------------------------------------
// Read handlers
// ---------------------------------------------------------------------------

fn tool_content_state(ctx: &mut ToolContext<'_>, params: &Value) -> ToolResult {
    let args = Args::new(params)?;
    let include_nodes = args.bool_or("include_nodes", true)?;
    let document = ctx.engine.content();

    let meshes: Vec<Value> = document
        .meshes
        .iter()
        .enumerate()
        .map(|(index, mesh)| mesh_summary(index, mesh))
        .collect();

    let materials: Vec<Value> = document
        .materials
        .iter()
        .enumerate()
        .map(|(index, material)| {
            json!({
                "index": index,
                "name": material.name,
                "base_color": material.base_color,
                "metallic": material.metallic,
                "roughness": material.roughness,
            })
        })
        .collect();

    let animations: Vec<Value> = document
        .animations
        .iter()
        .enumerate()
        .map(|(index, animation)| {
            json!({
                "index": index,
                "name": animation.name,
                "tracks": animation.tracks.len(),
                "duration": animation.duration(),
            })
        })
        .collect();

    let mut out = json!({
        "name": document.name,
        "mesh_count": document.meshes.len(),
        "material_count": document.materials.len(),
        "node_count": document.scene.len(),
        "root_count": document.scene.roots.len(),
        "animation_count": document.animations.len(),
        "total_triangles": document.meshes.iter().map(|m| m.triangle_count()).sum::<usize>(),
        "meshes": meshes,
        "materials": materials,
        "animations": animations,
    });

    if include_nodes {
        let nodes: Vec<Value> = document
            .scene
            .nodes
            .iter()
            .enumerate()
            .map(|(index, node)| {
                json!({
                    "index": index,
                    "name": node.name,
                    "mesh": node.mesh,
                    "material": node.material,
                    "children": node.children,
                    "translation": node.translation,
                    "is_root": document.scene.roots.contains(&index),
                })
            })
            .collect();
        out["nodes"] = Value::Array(nodes);
    }
    ok(out)
}

fn tool_content_validate(ctx: &mut ToolContext<'_>, _params: &Value) -> ToolResult {
    let problems = ctx.engine.content().validate();
    ok(json!({
        "valid": problems.is_empty(),
        "problems": problems,
    }))
}

// ---------------------------------------------------------------------------
// Write handlers
// ---------------------------------------------------------------------------

fn tool_content_reset(ctx: &mut ToolContext<'_>, params: &Value) -> ToolResult {
    let args = Args::new(params)?;
    let name = args.get("name").and_then(Value::as_str).unwrap_or("aurum");
    ctx.engine.reset_content(name);
    ok(json!({ "reset": true, "name": name }))
}

fn tool_mesh_add(ctx: &mut ToolContext<'_>, params: &Value) -> ToolResult {
    let args = Args::new(params)?;
    let kind = args.str("kind")?;
    let name = args.get("name").and_then(Value::as_str).map(str::to_string);

    let mut mesh = match kind {
        "box" => box_mesh(
            f32_or(&args, "width", 1.0)?,
            f32_or(&args, "height", 1.0)?,
            f32_or(&args, "depth", 1.0)?,
        ),
        "plane" => plane_mesh(f32_or(&args, "width", 1.0)?, f32_or(&args, "depth", 1.0)?),
        "sphere" => uv_sphere(
            f32_or(&args, "radius", 0.5)?,
            u32_or(&args, "sectors", 24)?,
            u32_or(&args, "stacks", 16)?,
        ),
        "cylinder" => cylinder(
            f32_or(&args, "radius", 0.5)?,
            f32_or(&args, "height", 1.0)?,
            u32_or(&args, "sectors", 24)?,
        ),
        "cone" => cone(
            f32_or(&args, "radius", 0.5)?,
            f32_or(&args, "height", 1.0)?,
            u32_or(&args, "sectors", 24)?,
        ),
        "torus" => torus(
            f32_or(&args, "radius", 0.5)?,
            f32_or(&args, "tube", 0.15)?,
            u32_or(&args, "segments", 24)?,
            u32_or(&args, "tube_segments", 12)?,
        ),
        other => {
            return Err(ToolError::Invalid(format!(
                "'kind' must be one of box, plane, sphere, cylinder, cone, torus; got '{other}'"
            )))
        }
    };
    if let Some(name) = name {
        mesh.name = name;
    }

    let index = ctx.engine.content_mut().add_mesh(mesh);
    let mesh = &ctx.engine.content().meshes[index];
    let mut summary = mesh_summary(index, mesh);
    summary["name_used"] = json!(mesh.name);
    ok(summary)
}

fn tool_mesh_transform(ctx: &mut ToolContext<'_>, params: &Value) -> ToolResult {
    let args = Args::new(params)?;
    let index = mesh_index(&args, "mesh", ctx.engine.content())?;
    let translation = number_array(&args, "translation", [0.0, 0.0, 0.0])?;
    let rotation = number_array(&args, "rotation", [0.0, 0.0, 0.0, 1.0])?;
    let scale = number_array(&args, "scale", [1.0, 1.0, 1.0])?;

    let matrix = Mat4::from_trs(translation, rotation, scale);
    let mesh = &mut ctx.engine.content_mut().meshes[index];
    mesh.transform(&matrix);
    ok(mesh_summary(index, mesh))
}

fn tool_mesh_merge(ctx: &mut ToolContext<'_>, params: &Value) -> ToolResult {
    let args = Args::new(params)?;
    let into = mesh_index(&args, "into", ctx.engine.content())?;
    let from = mesh_index(&args, "from", ctx.engine.content())?;
    if into == from {
        return Err(ToolError::Invalid(
            "'into' and 'from' must be different meshes".into(),
        ));
    }
    let translation = number_array(&args, "translation", [0.0, 0.0, 0.0])?;
    let rotation = number_array(&args, "rotation", [0.0, 0.0, 0.0, 1.0])?;
    let scale = number_array(&args, "scale", [1.0, 1.0, 1.0])?;

    // Copy first so the borrow of `from` ends before mutating `into`.
    let source = ctx.engine.content().meshes[from].clone();
    let placed = Mesh::transformed_copy(&source, &Mat4::from_trs(translation, rotation, scale));

    let document = ctx.engine.content_mut();
    document.meshes[into].merge(&placed);
    let merged = mesh_summary(into, &document.meshes[into]);
    ok(json!({ "mesh": merged, "merged_from": from }))
}

fn tool_material_add(ctx: &mut ToolContext<'_>, params: &Value) -> ToolResult {
    let args = Args::new(params)?;
    let name = args
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("Material");
    let color = number_array(&args, "color", [1.0, 1.0, 1.0, 1.0])?;
    let emissive = number_array(&args, "emissive", [0.0, 0.0, 0.0])?;

    let mut material = Material::new(name);
    material.base_color = color;
    material.metallic = f32_or(&args, "metallic", 0.0)?.clamp(0.0, 1.0);
    material.roughness = f32_or(&args, "roughness", 0.5)?.clamp(0.0, 1.0);
    material.emissive = emissive;
    material.double_sided = args.bool_or("double_sided", false)?;

    let index = ctx.engine.content_mut().add_material(material);
    let stored = &ctx.engine.content().materials[index];
    ok(json!({
        "index": index,
        "name": stored.name,
        "base_color": stored.base_color,
        "metallic": stored.metallic,
        "roughness": stored.roughness,
    }))
}

fn tool_node_add(ctx: &mut ToolContext<'_>, params: &Value) -> ToolResult {
    let args = Args::new(params)?;
    let name = args.str("name")?;
    let parent = match args.get("parent") {
        Some(_) => Some(node_index(&args, "parent", ctx.engine.content())?),
        None => None,
    };

    let mesh = match args.get("mesh") {
        Some(_) => Some(mesh_index(&args, "mesh", ctx.engine.content())?),
        None => None,
    };
    let material = match args.get("material") {
        Some(_) => {
            let index = args.i64("material")?;
            if index < 0 || index as usize >= ctx.engine.content().materials.len() {
                return Err(ToolError::Invalid(format!(
                    "'material' {index} does not exist; there are {} materials",
                    ctx.engine.content().materials.len()
                )));
            }
            Some(index as usize)
        }
        None => None,
    };

    let document = ctx.engine.content_mut();
    // The document's own scene already starts with a root node; reuse it when
    // the caller wants a top-level object.
    let index = document.scene_mut().add_child(parent, name);
    let node = document.scene_mut().node_mut(index).expect("just added");
    node.mesh = mesh;
    node.material = material;
    node.translation = number_array(&args, "translation", [0.0, 0.0, 0.0])?;
    node.rotation = number_array(&args, "rotation", [0.0, 0.0, 0.0, 1.0])?;
    node.scale = number_array(&args, "scale", [1.0, 1.0, 1.0])?;

    ok(json!({
        "index": index,
        "name": node.name,
        "parent": parent,
        "mesh": node.mesh,
        "material": node.material,
    }))
}

fn tool_node_transform(ctx: &mut ToolContext<'_>, params: &Value) -> ToolResult {
    let args = Args::new(params)?;
    let index = node_index(&args, "node", ctx.engine.content())?;
    let translation = number_array(&args, "translation", [0.0, 0.0, 0.0])?;
    let rotation = number_array(&args, "rotation", [0.0, 0.0, 0.0, 1.0])?;
    let scale = number_array(&args, "scale", [1.0, 1.0, 1.0])?;

    let node = ctx
        .engine
        .content_mut()
        .scene_mut()
        .node_mut(index)
        .expect("index validated");
    node.translation = translation;
    node.rotation = rotation;
    node.scale = scale;
    ok(json!({
        "index": index,
        "name": node.name,
        "translation": node.translation,
        "rotation": node.rotation,
        "scale": node.scale,
    }))
}

fn tool_node_remove(ctx: &mut ToolContext<'_>, params: &Value) -> ToolResult {
    let args = Args::new(params)?;
    let index = node_index(&args, "node", ctx.engine.content())?;

    // Detach rather than delete: indices are animation targets, so compacting
    // the array would silently retarget every track.
    let detached = ctx.engine.content_mut().scene_mut().set_parent(index, None);
    if detached {
        ctx.engine
            .content_mut()
            .scene_mut()
            .roots
            .retain(|r| *r != index);
    }
    ok(json!({
        "index": index,
        "detached": detached,
        "note": "the slot is retained so existing indices stay valid",
    }))
}

fn tool_animation_spin(ctx: &mut ToolContext<'_>, params: &Value) -> ToolResult {
    let args = Args::new(params)?;
    let node = node_index(&args, "node", ctx.engine.content())?;
    let name = args
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("Spin")
        .to_string();
    let axis = number_array(&args, "axis", [0.0, 1.0, 0.0])?;
    let turns = f32_or(&args, "turns", 1.0)?;
    let duration = f32_or(&args, "duration", 2.0)?;
    let keys = u32_or(&args, "keys", 9)?.max(2) as usize;

    if duration <= 0.0 {
        return Err(ToolError::Invalid(
            "'duration' must be greater than 0".into(),
        ));
    }

    let animation = Animation::new(name).spin(node, axis, turns, duration, keys);
    let index = ctx.engine.content_mut().add_animation(animation);
    let stored = &ctx.engine.content().animations[index];
    ok(json!({
        "index": index,
        "name": stored.name,
        "node": node,
        "duration": stored.duration(),
        "keys": keys,
    }))
}

fn tool_animation_add(ctx: &mut ToolContext<'_>, params: &Value) -> ToolResult {
    let args = Args::new(params)?;
    let node = node_index(&args, "node", ctx.engine.content())?;
    let name = args.str("name")?.to_string();
    let path = match args.str("path")? {
        "translation" => TrackPath::Translation,
        "rotation" => TrackPath::Rotation,
        "scale" => TrackPath::Scale,
        other => {
            return Err(ToolError::Invalid(format!(
                "'path' must be translation, rotation, or scale; got '{other}'"
            )))
        }
    };
    let interpolation = match args.get("interpolation").and_then(Value::as_str) {
        None | Some("linear") => Interpolation::Linear,
        Some("step") => Interpolation::Step,
        Some(other) => {
            return Err(ToolError::Invalid(format!(
                "'interpolation' must be linear or step; got '{other}'"
            )))
        }
    };

    let times_value = args.value("times")?;
    let times: Vec<f32> = times_value
        .as_array()
        .ok_or_else(|| ToolError::Invalid("'times' must be an array of numbers".into()))?
        .iter()
        .map(|v| {
            v.as_f64()
                .map(|f| f as f32)
                .ok_or_else(|| ToolError::Invalid("'times' must contain only numbers".into()))
        })
        .collect::<Result<_, _>>()?;

    let values_value = args.value("values")?;
    let flat: Vec<f32> = values_value
        .as_array()
        .ok_or_else(|| ToolError::Invalid("'values' must be an array of numbers".into()))?
        .iter()
        .map(|v| {
            v.as_f64()
                .map(|f| f as f32)
                .ok_or_else(|| ToolError::Invalid("'values' must contain only numbers".into()))
        })
        .collect::<Result<_, _>>()?;

    let components = path.components();
    if flat.len() != times.len() * components {
        return Err(ToolError::Invalid(format!(
            "'values' must hold {} numbers for {} keyframes of a {components}-component track, got {}",
            times.len() * components,
            times.len(),
            flat.len()
        )));
    }

    let mut track = aurum_content::Track::new(node, path).with_interpolation(interpolation);
    for (i, time) in times.iter().enumerate() {
        let start = i * components;
        if !track.keyframe(*time, &flat[start..start + components]) {
            return Err(ToolError::Invalid(format!(
                "keyframe {i} is invalid: time must be finite and non-negative, values finite"
            )));
        }
    }
    if !track.is_valid() {
        track.sort_by_time();
    }

    let mut animation = Animation::new(name);
    animation.add_track(track);
    let index = ctx.engine.content_mut().add_animation(animation);
    let stored = &ctx.engine.content().animations[index];
    ok(json!({
        "index": index,
        "name": stored.name,
        "node": node,
        "keys": stored.tracks[0].times.len(),
        "duration": stored.duration(),
    }))
}

fn tool_sprite_atlas(ctx: &mut ToolContext<'_>, params: &Value) -> ToolResult {
    let args = Args::new(params)?;
    let max_width = u32_or(&args, "max_width", 1024)?;

    let sprites = args.value("sprites")?;
    let entries = sprites
        .as_array()
        .ok_or_else(|| ToolError::Invalid("'sprites' must be an array".into()))?;
    if entries.is_empty() {
        return Err(ToolError::Invalid("'sprites' must not be empty".into()));
    }

    let mut sizes: Vec<(String, u32, u32)> = Vec::with_capacity(entries.len());
    let mut colors: Vec<Option<[u8; 4]>> = Vec::with_capacity(entries.len());

    for (position, entry) in entries.iter().enumerate() {
        let name = entry
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ToolError::Invalid(format!("'sprites[{position}].name' must be a string"))
            })?
            .to_string();
        let width = entry.get("width").and_then(Value::as_u64).unwrap_or(16) as u32;
        let height = entry.get("height").and_then(Value::as_u64).unwrap_or(16) as u32;
        sizes.push((name, width.max(1), height.max(1)));

        colors.push(match entry.get("color") {
            None | Some(Value::Null) => None,
            Some(Value::Array(channels)) => {
                if channels.len() != 4 {
                    return Err(ToolError::Invalid(format!(
                        "'sprites[{position}].color' must have 4 channels"
                    )));
                }
                let mut rgba = [255u8; 4];
                for (i, channel) in channels.iter().enumerate() {
                    rgba[i] = channel.as_u64().unwrap_or(255).min(255) as u8;
                }
                Some(rgba)
            }
            Some(_) => {
                return Err(ToolError::Invalid(format!(
                    "'sprites[{position}].color' must be [r, g, b, a]"
                )))
            }
        });
    }

    let atlas = aurum_content::sprite::pack(&sizes, max_width);

    // Compose a placeholder sheet when colours were supplied: this gives an
    // agent something it can actually look at in Godot while blocking out UI.
    let mut wrote = Vec::new();
    if colors.iter().any(Option::is_some) {
        let mut image = aurum_content::sprite::AtlasImage::new(atlas.width, atlas.height);
        for (region, color) in atlas.regions.iter().zip(&colors) {
            if let Some(rgba) = color {
                let patch =
                    aurum_content::sprite::AtlasImage::filled(region.rect.w, region.rect.h, *rgba);
                image.blit(&patch, region.rect.x, region.rect.y);
            }
        }
        if let Some(Value::String(path)) = args.get("png_path") {
            let target = ctx.paths.resolve(path)?;
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    ToolError::Io(format!("could not create '{}': {e}", parent.display()))
                })?;
            }
            let png = image.to_png().map_err(|e| ToolError::Io(e.to_string()))?;
            std::fs::write(&target, &png).map_err(|e| {
                ToolError::Io(format!("could not write '{}': {e}", target.display()))
            })?;
            wrote.push(json!({ "path": target.display().to_string(), "bytes": png.len() }));
        }
    }

    if let Some(Value::String(path)) = args.get("layout_path") {
        let target = ctx.paths.resolve(path)?;
        let text =
            serde_json::to_string_pretty(&atlas).map_err(|e| ToolError::Io(e.to_string()))?;
        std::fs::write(&target, &text)
            .map_err(|e| ToolError::Io(format!("could not write '{}': {e}", target.display())))?;
        wrote.push(json!({ "path": target.display().to_string(), "bytes": text.len() }));
    }

    ok(json!({
        "width": atlas.width,
        "height": atlas.height,
        "occupancy": atlas.occupancy(),
        "regions": atlas.regions,
        "written": wrote,
    }))
}

fn tool_content_export(ctx: &mut ToolContext<'_>, params: &Value) -> ToolResult {
    let args = Args::new(params)?;
    let path = args.str("path")?;
    let embedded = args.bool_or("embedded", false)?;

    let target = ctx.paths.resolve(path)?;
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| ToolError::Io(format!("could not create '{}': {e}", parent.display())))?;
    }

    let document = ctx.engine.content();
    let problems = document.validate();
    if !problems.is_empty() {
        return Err(ToolError::Engine(format!(
            "content is not exportable: {}",
            problems.join("; ")
        )));
    }

    if embedded {
        let text = aurum_content::gltf::export_embedded(document)
            .map_err(|e| ToolError::Engine(e.to_string()))?;
        std::fs::write(&target, &text)
            .map_err(|e| ToolError::Io(format!("could not write '{}': {e}", target.display())))?;
        return ok(json!({
            "path": target.display().to_string(),
            "email": Value::Null,
            "bytes": text.len(),
            "embedded": true,
        }));
    }

    // Write beside the requested path, with the buffer name the document
    // declares, so the URI always resolves.
    let directory = target.parent().unwrap_or(std::path::Path::new("."));
    let stem = target
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("scene");
    let exported = aurum_content::gltf::write(document, directory, stem)
        .map_err(|e| ToolError::Engine(e.to_string()))?;

    ok(json!({
        "path": exported.gltf.display().to_string(),
        "buffer": exported.bin.display().to_string(),
        "bytes": exported.bytes,
        "embedded": false,
        "mesh_count": document.meshes.len(),
        "node_count": document.scene.len(),
        "animation_count": document.animations.len(),
        "total_triangles": document.meshes.iter().map(|m| m.triangle_count()).sum::<usize>(),
    }))
}

/// Load a glTF or GLB file into the content document.
///
/// This is the Blender path: Blender exports glTF or GLB natively, and this
/// reads it back into the same model the procedural tools build, so an
/// imported asset can be inspected, transformed, merged, and re-exported.
fn tool_content_import(ctx: &mut ToolContext<'_>, params: &Value) -> ToolResult {
    let args = Args::new(params)?;
    let path = ctx.paths.resolve(args.str("path")?)?;
    let mode = args.get("mode").and_then(Value::as_str).unwrap_or("append");
    if mode != "append" && mode != "replace" {
        return Err(ToolError::Invalid(format!(
            "'mode' must be append or replace; got '{mode}'"
        )));
    }

    let imported = aurum_content::gltf_import::import_path(&path)
        .map_err(|e| ToolError::Invalid(e.to_string()))?;

    let summary = json!({
        "source": path.display().to_string(),
        "mesh_count": imported.meshes.len(),
        "material_count": imported.materials.len(),
        "node_count": imported.scene.len(),
        "animation_count": imported.animations.len(),
        "total_triangles": imported.meshes.iter().map(|m| m.triangle_count()).sum::<usize>(),
        "modal": mode,
    });

    match mode {
        "replace" => *ctx.engine.content_mut() = imported,
        _ => ctx.engine.content_mut().append(imported),
    }

    let mut out = summary;
    out["document_mesh_count"] = json!(ctx.engine.content().meshes.len());
    out["document_node_count"] = json!(ctx.engine.content().scene.len());
    ok(out)
}
// ---------------------------------------------------------------------------
// Catalog
// ---------------------------------------------------------------------------

/// The content-authoring tool catalog.
pub fn catalog() -> Vec<Tool> {
    vec![
        Tool {
            name: "aurum_content_state",
            description: "Summarise the authored content: meshes with vertex and triangle counts, \
                          materials, nodes with their transforms and links, and animations. Call \
                          this before editing so you know the indices to pass to other content \
                          tools. Indices are stable for the life of the document.",
            input_schema: schema(
                json!({ "include_nodes": { "type": "boolean", "description": "Include the per-node list (default true)." } }),
                json!([]),
            ),
            read_only: true,
            handler: tool_content_state,
        },
        Tool {
            name: "aurum_content_validate",
            description: "Check the authored content for dangling references, cycles, and broken \
                          meshes. Returns `valid` plus a list of problems.",
            input_schema: schema(json!({}), json!([])),
            read_only: true,
            handler: tool_content_validate,
        },
        Tool {
            name: "aurum_content_reset",
            description: "Discard all authored content and start a fresh document.",
            input_schema: schema(
                json!({ "name": { "type": "string", "description": "Name for the new document." } }),
                json!([]),
            ),
            read_only: false,
            handler: tool_content_reset,
        },
        Tool {
            name: "aurum_mesh_add",
            description: "Create a procedurally generated mesh and return its index. Kinds: box \
                          (width, height, depth), plane (width, depth), sphere (radius, sectors, \
                          stacks), cylinder (radius, height, sectors), cone (radius, height, \
                          sectors), torus (radius, tube, segments, tube_segments).",
            input_schema: schema(
                json!({
                    "kind": { "type": "string", "enum": ["box", "plane", "sphere", "cylinder", "cone", "torus"] },
                    "name": { "type": "string", "description": "Optional name for the mesh." },
                    "width": { "type": "number" },
                    "height": { "type": "number" },
                    "depth": { "type": "number" },
                    "radius": { "type": "number" },
                    "tube": { "type": "number", "description": "Torus tube radius." },
                    "sectors": { "type": "integer", "minimum": 3 },
                    "stacks": { "type": "integer", "minimum": 2 },
                    "segments": { "type": "integer", "minimum": 3 },
                    "tube_segments": { "type": "integer", "minimum": 3 }
                }),
                json!(["kind"]),
            ),
            read_only: false,
            handler: tool_mesh_add,
        },
        Tool {
            name: "aurum_mesh_transform",
            description: "Bake a transform into a mesh's vertices, in place. Use this to build \
                          compound shapes before merging.",
            input_schema: schema(
                json!({
                    "mesh": { "type": "integer", "minimum": 0 },
                    "translation": { "type": "array", "items": { "type": "number" }, "description": "[x, y, z]" },
                    "rotation": { "type": "array", "items": { "type": "number" }, "description": "Quaternion [x, y, z, w]" },
                    "scale": { "type": "array", "items": { "type": "number" }, "description": "[x, y, z]" }
                }),
                json!(["mesh"]),
            ),
            read_only: false,
            handler: tool_mesh_transform,
        },
        Tool {
            name: "aurum_mesh_merge",
            description:
                "Merge one mesh into another at an optional transform, offsetting indices. \
                          This is how a compound object becomes a single mesh. `from` is not \
                          modified.",
            input_schema: schema(
                json!({
                    "into": { "type": "integer", "minimum": 0, "description": "Mesh to merge into." },
                    "from": { "type": "integer", "minimum": 0, "description": "Mesh to copy from." },
                    "translation": { "type": "array", "items": { "type": "number" } },
                    "rotation": { "type": "array", "items": { "type": "number" } },
                    "scale": { "type": "array", "items": { "type": "number" } }
                }),
                json!(["into", "from"]),
            ),
            read_only: false,
            handler: tool_mesh_merge,
        },
        Tool {
            name: "aurum_material_add",
            description: "Create a metallic-roughness material and return its index.",
            input_schema: schema(
                json!({
                    "name": { "type": "string" },
                    "color": { "type": "array", "items": { "type": "number" }, "description": "Linear RGBA, 0..1. Default [1,1,1,1]." },
                    "metallic": { "type": "number", "minimum": 0, "maximum": 1 },
                    "roughness": { "type": "number", "minimum": 0, "maximum": 1 },
                    "emissive": { "type": "array", "items": { "type": "number" }, "description": "RGB, 0..1." },
                    "double_sided": { "type": "boolean" }
                }),
                json!([]),
            ),
            read_only: false,
            handler: tool_material_add,
        },
        Tool {
            name: "aurum_node_add",
            description: "Add a node to the scene graph, optionally parented and carrying a mesh \
                          and material. Returns its index. Omit `parent` for a top-level object.",
            input_schema: schema(
                json!({
                    "name": { "type": "string" },
                    "parent": { "type": "integer", "minimum": 0, "description": "Parent node index." },
                    "mesh": { "type": "integer", "minimum": 0 },
                    "material": { "type": "integer", "minimum": 0 },
                    "translation": { "type": "array", "items": { "type": "number" } },
                    "rotation": { "type": "array", "items": { "type": "number" }, "description": "Quaternion [x, y, z, w]" },
                    "scale": { "type": "array", "items": { "type": "number" } }
                }),
                json!(["name"]),
            ),
            read_only: false,
            handler: tool_node_add,
        },
        Tool {
            name: "aurum_node_transform",
            description: "Set a node's local transform.",
            input_schema: schema(
                json!({
                    "node": { "type": "integer", "minimum": 0 },
                    "translation": { "type": "array", "items": { "type": "number" } },
                    "rotation": { "type": "array", "items": { "type": "number" } },
                    "scale": { "type": "array", "items": { "type": "number" } }
                }),
                json!(["node"]),
            ),
            read_only: false,
            handler: tool_node_transform,
        },
        Tool {
            name: "aurum_node_remove",
            description: "Detach a node from the scene graph so it no longer appears in the \
                          exported scene. Its slot is retained so other indices and animation \
                          targets stay valid.",
            input_schema: schema(
                json!({ "node": { "type": "integer", "minimum": 0 } }),
                json!(["node"]),
            ),
            read_only: false,
            handler: tool_node_remove,
        },
        Tool {
            name: "aurum_animation_spin",
            description: "Author a rotation animation that spins a node about an axis. The \
                          convenience path for the most common animation.",
            input_schema: schema(
                json!({
                    "node": { "type": "integer", "minimum": 0 },
                    "name": { "type": "string" },
                    "axis": { "type": "array", "items": { "type": "number" }, "description": "Rotation axis, default [0,1,0]." },
                    "turns": { "type": "number", "description": "Full turns over the duration (default 1)." },
                    "duration": { "type": "number", "description": "Seconds (default 2)." },
                    "keys": { "type": "integer", "minimum": 2, "description": "Keyframes (default 9)." }
                }),
                json!(["node"]),
            ),
            read_only: false,
            handler: tool_animation_spin,
        },
        Tool {
            name: "aurum_animation_add",
            description: "Author a keyframed animation track on one node property. `values` is a \
                          flat array: 3 numbers per keyframe for translation and scale, 4 for \
                          rotation (a quaternion, normalized for you).",
            input_schema: schema(
                json!({
                    "node": { "type": "integer", "minimum": 0 },
                    "name": { "type": "string" },
                    "path": { "type": "string", "enum": ["translation", "rotation", "scale"] },
                    "times": { "type": "array", "items": { "type": "number" }, "description": "Keyframe times in seconds." },
                    "values": { "type": "array", "items": { "type": "number" }, "description": "Flat keyframe values." },
                    "interpolation": { "type": "string", "enum": ["linear", "step"] }
                }),
                json!(["node", "name", "path", "times", "values"]),
            ),
            read_only: false,
            handler: tool_animation_add,
        },
        Tool {
            name: "aurum_sprite_atlas",
            description: "Pack named sprite rectangles into an atlas and return the layout. \
                          Supply a `color` per sprite to also compose a placeholder sheet as a \
                          PNG, which is useful for blocking out UI before real art exists.",
            input_schema: schema(
                json!({
                    "sprites": {
                        "type": "array",
                        "description": "Sprites to pack.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "name": { "type": "string" },
                                "width": { "type": "integer", "minimum": 1 },
                                "height": { "type": "integer", "minimum": 1 },
                                "color": { "type": "array", "items": { "type": "integer" }, "description": "Optional RGBA 0-255 placeholder colour." }
                            },
                            "required": ["name"]
                        }
                    },
                    "max_width": { "type": "integer", "minimum": 1, "description": "Atlas width budget (default 1024)." },
                    "png_path": { "type": "string", "description": "Where to write the placeholder sheet, when colours are given." },
                    "layout_path": { "type": "string", "description": "Where to write the layout JSON." }
                }),
                json!(["sprites"]),
            ),
            read_only: false,
            handler: tool_sprite_atlas,
        },
        Tool {
            name: "aurum_content_export",
            description: "Write the authored content as glTF 2.0. Godot imports the result \
                          directly. Produces `<name>.gltf` plus a sibling `.bin`, or a single \
                          self-contained file when `embedded` is true. Blender can also open it, \
                          so the same file works with or without Blender installed.",
            input_schema: schema(
                json!({
                    "path": { "type": "string", "description": "Output path inside the server root." },
                    "embedded": { "type": "boolean", "description": "Inline the buffer as base64 for a single file (default false)." }
                }),
                json!(["path"]),
            ),
            read_only: false,
            handler: tool_content_export,
        },
        Tool {
            name: "aurum_content_import",
            description: "Load a glTF or GLB file into the content document. This is how a \
                          Blender-authored asset enters Aurum: Blender exports glTF or GLB \
                          natively, and everything read here can then be transformed, merged, \
                          and re-exported by the other content tools. `mode: append` (the \
                          default) merges it into what you already have and remaps indices; \
                          `replace` discards the current document first.",
            input_schema: schema(
                json!({
                    "path": { "type": "string", "description": "File inside the server root, .gltf or .glb." },
                    "mode": { "type": "string", "enum": ["append", "replace"], "description": "Merge or replace (default append)." }
                }),
                json!(["path"]),
            ),
            read_only: false,
            handler: tool_content_import,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Engine;
    use crate::tools::{find, PathGuard};
    use std::path::PathBuf;

    fn call(engine: &mut Engine, paths: &PathGuard, name: &str, args: Value) -> ToolResult {
        let tool = find(name).expect("tool exists");
        let mut ctx = ToolContext { engine, paths };
        (tool.handler)(&mut ctx, &args)
    }

    fn ctx() -> (Engine, PathGuard) {
        (Engine::new(), PathGuard::new("."))
    }

    fn temp_root(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("aurum-content-tool-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn content_tools_are_prefixed_and_unique_against_the_whole_catalog() {
        let all = crate::tools::catalog();
        let mut names: Vec<&str> = all.iter().map(|t| t.name).collect();
        let before = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(
            before,
            names.len(),
            "the merged catalog has duplicate names"
        );

        for tool in catalog() {
            assert!(
                tool.name.starts_with("aurum_content_")
                    || tool.name.starts_with("aurum_mesh_")
                    || tool.name.starts_with("aurum_node_")
                    || tool.name.starts_with("aurum_material_")
                    || tool.name.starts_with("aurum_animation_")
                    || tool.name.starts_with("aurum_sprite_"),
                "unexpected content tool name: {}",
                tool.name
            );
        }
    }

    #[test]
    fn every_content_tool_has_a_description() {
        for tool in catalog() {
            assert!(
                !tool.description.is_empty(),
                "{} lacks a description",
                tool.name
            );
        }
    }

    #[test]
    fn build_a_scene_from_nothing_and_export_it() {
        let root = temp_root("scene");
        let paths = PathGuard::new(&root);
        let mut engine = Engine::new();

        let mesh = call(
            &mut engine,
            &paths,
            "aurum_mesh_add",
            json!({"kind": "box"}),
        )
        .unwrap();
        let mesh_index = mesh["index"].as_i64().unwrap();
        assert_eq!(mesh["triangles"], 12);

        let material = call(
            &mut engine,
            &paths,
            "aurum_material_add",
            json!({"name": "Red", "color": [1.0, 0.0, 0.0, 1.0]}),
        )
        .unwrap();

        let node = call(
            &mut engine,
            &paths,
            "aurum_node_add",
            json!({
                "name": "Crate",
                "mesh": mesh_index,
                "material": material["index"],
                "translation": [0.0, 1.0, 0.0]
            }),
        )
        .unwrap();
        assert_eq!(node["name"], "Crate");

        call(
            &mut engine,
            &paths,
            "aurum_animation_spin",
            json!({"node": node["index"], "duration": 1.5}),
        )
        .unwrap();

        let exported = call(
            &mut engine,
            &paths,
            "aurum_content_export",
            json!({"path": "out/scene.gltf"}),
        )
        .unwrap();
        assert!(exported["bytes"].as_u64().unwrap() > 0);
        assert!(root.join("out/scene.gltf").exists());
        assert!(root
            .join("out")
            .join(
                exported["buffer"]
                    .as_str()
                    .unwrap()
                    .rsplit(['\\', '/'])
                    .next()
                    .unwrap()
            )
            .exists());

        // The exported file is real glTF with our animation in it.
        let text = std::fs::read_to_string(root.join("out/scene.gltf")).unwrap();
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["asset"]["version"], "2.0");
        assert_eq!(
            value["animations"][0]["channels"][0]["target"]["path"],
            "rotation"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn mesh_transform_and_merge_build_a_compound_shape() {
        let (mut engine, paths) = ctx();
        let post = call(
            &mut engine,
            &paths,
            "aurum_mesh_add",
            json!({"kind": "cylinder", "radius": 0.1, "height": 2.0, "sectors": 8}),
        )
        .unwrap();
        let post_index = post["index"].as_i64().unwrap();
        let triangles_each = post["triangles"].as_i64().unwrap();
        let vertices_each = post["vertices"].as_i64().unwrap();

        let copy = call(
            &mut engine,
            &paths,
            "aurum_mesh_add",
            json!({"kind": "cylinder", "radius": 0.1, "height": 2.0, "sectors": 8}),
        )
        .unwrap();

        let merged = call(
            &mut engine,
            &paths,
            "aurum_mesh_merge",
            json!({"into": post_index, "from": copy["index"], "translation": [1.0, 0.0, 0.0]}),
        )
        .unwrap();
        assert_eq!(merged["mesh"]["triangles"], triangles_each * 2);
        assert_eq!(merged["mesh"]["vertices"], vertices_each * 2);

        // Baking the transform moved the geometry, not just the node.
        let transformed = call(
            &mut engine,
            &paths,
            "aurum_mesh_transform",
            json!({"mesh": post_index, "translation": [0.0, 5.0, 0.0]}),
        )
        .unwrap();
        assert_eq!(transformed["vertices"], vertices_each * 2);
    }

    #[test]
    fn mesh_add_rejects_an_unknown_kind() {
        let (mut engine, paths) = ctx();
        let err = call(
            &mut engine,
            &paths,
            "aurum_mesh_add",
            json!({"kind": "dodecahedron"}),
        )
        .unwrap_err();
        assert!(err.to_string().contains("kind"), "got: {err}");
    }

    #[test]
    fn mesh_transform_rejects_a_malformed_vector() {
        let (mut engine, paths) = ctx();
        let mesh = call(
            &mut engine,
            &paths,
            "aurum_mesh_add",
            json!({"kind": "box"}),
        )
        .unwrap();
        let err = call(
            &mut engine,
            &paths,
            "aurum_mesh_transform",
            json!({"mesh": mesh["index"], "translation": [1.0]}),
        )
        .unwrap_err();
        assert!(err.to_string().contains("3 numbers"), "got: {err}");

        let err = call(
            &mut engine,
            &paths,
            "aurum_mesh_transform",
            json!({"mesh": mesh["index"], "rotation": [0.0, 0.0, 0.0]}),
        )
        .unwrap_err();
        assert!(err.to_string().contains("4 numbers"), "got: {err}");
    }

    #[test]
    fn missing_indices_are_reported_with_the_valid_range() {
        let (mut engine, paths) = ctx();
        let err = call(
            &mut engine,
            &paths,
            "aurum_node_add",
            json!({"name": "X", "mesh": 4}),
        )
        .unwrap_err();
        assert!(err.to_string().contains("there are 0 meshes"), "got: {err}");

        let err = call(
            &mut engine,
            &paths,
            "aurum_node_transform",
            json!({"node": 2}),
        )
        .unwrap_err();
        assert!(err.to_string().contains("0 nodes"), "got: {err}");
    }

    #[test]
    fn hierarchy_is_built_with_parents() {
        let (mut engine, paths) = ctx();
        let parent = call(
            &mut engine,
            &paths,
            "aurum_node_add",
            json!({"name": "Root"}),
        )
        .unwrap();
        let child = call(
            &mut engine,
            &paths,
            "aurum_node_add",
            json!({"name": "Child", "parent": parent["index"]}),
        )
        .unwrap();

        let state = call(&mut engine, &paths, "aurum_content_state", json!({})).unwrap();
        let nodes = state["nodes"].as_array().unwrap();
        let root_node = nodes
            .iter()
            .find(|n| n["index"] == parent["index"])
            .unwrap();
        assert_eq!(root_node["children"], json!([child["index"]]));
        assert_eq!(state["root_count"], 1);
    }

    #[test]
    fn node_remove_detaches_without_shifting_indices() {
        let (mut engine, paths) = ctx();
        let a = call(&mut engine, &paths, "aurum_node_add", json!({"name": "A"})).unwrap();
        let b = call(&mut engine, &paths, "aurum_node_add", json!({"name": "B"})).unwrap();

        call(
            &mut engine,
            &paths,
            "aurum_node_remove",
            json!({"node": a["index"]}),
        )
        .unwrap();

        let state = call(&mut engine, &paths, "aurum_content_state", json!({})).unwrap();
        assert_eq!(state["node_count"], 2, "the slot must be retained");
        assert_eq!(state["root_count"], 1, "only B is still attached");
        // B keeps its index, which is what animation targets depend on.
        let nodes = state["nodes"].as_array().unwrap();
        assert!(nodes.iter().any(|n| n["index"] == b["index"]));
    }

    #[test]
    fn keyframed_animation_validates_value_lengths() {
        let (mut engine, paths) = ctx();
        let node = call(&mut engine, &paths, "aurum_node_add", json!({"name": "N"})).unwrap();

        // Three keyframes of a VEC3 track need nine numbers.
        let err = call(
            &mut engine,
            &paths,
            "aurum_animation_add",
            json!({
                "node": node["index"], "name": "Move", "path": "translation",
                "times": [0.0, 1.0, 2.0], "values": [0.0, 0.0, 0.0, 1.0, 0.0, 0.0]
            }),
        )
        .unwrap_err();
        assert!(err.to_string().contains("9 numbers"), "got: {err}");

        let ok_result = call(
            &mut engine,
            &paths,
            "aurum_animation_add",
            json!({
                "node": node["index"], "name": "Move", "path": "translation",
                "times": [0.0, 1.0, 2.0],
                "values": [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 2.0, 0.0, 0.0]
            }),
        )
        .unwrap();
        assert_eq!(ok_result["keys"], 3);
        assert_eq!(ok_result["duration"], 2.0);
    }

    #[test]
    fn keyframed_animation_sorts_unordered_times() {
        let (mut engine, paths) = ctx();
        let node = call(&mut engine, &paths, "aurum_node_add", json!({"name": "N"})).unwrap();
        let out = call(
            &mut engine,
            &paths,
            "aurum_animation_add",
            json!({
                "node": node["index"], "name": "Unordered", "path": "scale",
                "times": [2.0, 0.0, 1.0],
                "values": [2.0, 2.0, 2.0, 1.0, 1.0, 1.0, 1.5, 1.5, 1.5]
            }),
        )
        .unwrap();
        assert_eq!(out["duration"], 2.0);
        // Exporting requires ascending times, so this must not fail.
        let value = call(&mut engine, &paths, "aurum_content_validate", json!({})).unwrap();
        assert_eq!(value["valid"], true, "{value}");
    }

    #[test]
    fn animation_path_and_interpolation_are_validated() {
        let (mut engine, paths) = ctx();
        let node = call(&mut engine, &paths, "aurum_node_add", json!({"name": "N"})).unwrap();
        let err = call(
            &mut engine,
            &paths,
            "aurum_animation_add",
            json!({"node": node["index"], "name": "X", "path": "opacity", "times": [0.0], "values": [1.0]}),
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("translation, rotation, or scale"),
            "got: {err}"
        );

        let err = call(
            &mut engine,
            &paths,
            "aurum_animation_add",
            json!({"node": node["index"], "name": "X", "path": "scale", "times": [0.0], "values": [1.0, 1.0, 1.0], "interpolation": "bezier"}),
        )
        .unwrap_err();
        assert!(err.to_string().contains("linear or step"), "got: {err}");
    }

    #[test]
    fn spin_rejects_a_nonpositive_duration() {
        let (mut engine, paths) = ctx();
        let node = call(&mut engine, &paths, "aurum_node_add", json!({"name": "N"})).unwrap();
        let err = call(
            &mut engine,
            &paths,
            "aurum_animation_spin",
            json!({"node": node["index"], "duration": 0.0}),
        )
        .unwrap_err();
        assert!(err.to_string().contains("duration"), "got: {err}");
    }

    #[test]
    fn export_refuses_invalid_content() {
        let (mut engine, paths) = ctx();
        // A node pointing at a mesh that does not exist.
        let node = call(
            &mut engine,
            &paths,
            "aurum_node_add",
            json!({"name": "Ghost"}),
        )
        .unwrap();
        engine
            .content_mut()
            .scene_mut()
            .node_mut(node["index"].as_u64().unwrap() as usize)
            .unwrap()
            .mesh = Some(9);

        let err = call(
            &mut engine,
            &paths,
            "aurum_content_export",
            json!({"path": "bad.gltf"}),
        )
        .unwrap_err();
        assert!(err.to_string().contains("not exportable"), "got: {err}");
    }

    #[test]
    fn export_refuses_a_document_with_no_geometry() {
        let (mut engine, paths) = ctx();
        let err = call(
            &mut engine,
            &paths,
            "aurum_content_export",
            json!({"path": "empty.gltf"}),
        )
        .unwrap_err();
        assert!(err.to_string().contains("no meshes"), "got: {err}");
    }

    #[test]
    fn embedded_export_writes_one_self_contained_file() {
        let root = temp_root("embedded");
        let paths = PathGuard::new(&root);
        let mut engine = Engine::new();

        let mesh = call(
            &mut engine,
            &paths,
            "aurum_mesh_add",
            json!({"kind": "sphere"}),
        )
        .unwrap();
        call(
            &mut engine,
            &paths,
            "aurum_node_add",
            json!({"name": "Ball", "mesh": mesh["index"]}),
        )
        .unwrap();
        let out = call(
            &mut engine,
            &paths,
            "aurum_content_export",
            json!({"path": "one.gltf", "embedded": true}),
        )
        .unwrap();
        assert_eq!(out["embedded"], true);

        let text = std::fs::read_to_string(root.join("one.gltf")).unwrap();
        assert!(text.contains("base64"));
        assert!(!root.join("aurum.bin").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn export_is_confined_to_the_root() {
        let root = temp_root("guard");
        let paths = PathGuard::new(&root);
        let mut engine = Engine::new();
        let mesh = call(
            &mut engine,
            &paths,
            "aurum_mesh_add",
            json!({"kind": "box"}),
        )
        .unwrap();
        call(
            &mut engine,
            &paths,
            "aurum_node_add",
            json!({"name": "B", "mesh": mesh["index"]}),
        )
        .unwrap();

        let denied = call(
            &mut engine,
            &paths,
            "aurum_content_export",
            json!({"path": "../escape.gltf"}),
        );
        assert!(matches!(denied, Err(ToolError::Denied(_))));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn content_reset_clears_everything() {
        let (mut engine, paths) = ctx();
        let mesh = call(
            &mut engine,
            &paths,
            "aurum_mesh_add",
            json!({"kind": "box"}),
        )
        .unwrap();
        call(
            &mut engine,
            &paths,
            "aurum_node_add",
            json!({"name": "B", "mesh": mesh["index"]}),
        )
        .unwrap();

        call(
            &mut engine,
            &paths,
            "aurum_content_reset",
            json!({"name": "fresh"}),
        )
        .unwrap();
        let state = call(&mut engine, &paths, "aurum_content_state", json!({})).unwrap();
        assert_eq!(state["mesh_count"], 0);
        assert_eq!(state["node_count"], 0);
        assert_eq!(state["name"], "fresh");
    }

    #[test]
    fn runtime_reset_leaves_authored_content_alone() {
        let (mut engine, paths) = ctx();
        let mesh = call(
            &mut engine,
            &paths,
            "aurum_mesh_add",
            json!({"kind": "box"}),
        )
        .unwrap();
        call(
            &mut engine,
            &paths,
            "aurum_node_add",
            json!({"name": "B", "mesh": mesh["index"]}),
        )
        .unwrap();

        // aurum_reset is a runtime operation; authored content must survive it.
        call(&mut engine, &paths, "aurum_reset", json!({})).unwrap();
        let state = call(&mut engine, &paths, "aurum_content_state", json!({})).unwrap();
        assert_eq!(
            state["mesh_count"], 1,
            "content was lost to a runtime reset"
        );
        assert_eq!(state["node_count"], 1);
    }

    #[test]
    fn content_is_not_part_of_a_save_file() {
        let (mut engine, paths) = ctx();
        let mesh = call(
            &mut engine,
            &paths,
            "aurum_mesh_add",
            json!({"kind": "box"}),
        )
        .unwrap();
        call(
            &mut engine,
            &paths,
            "aurum_node_add",
            json!({"name": "B", "mesh": mesh["index"]}),
        )
        .unwrap();

        let saved = engine.to_save_json();
        assert!(
            saved.get("content").is_none(),
            "content must not be checkpointed"
        );

        let mut restored = Engine::new();
        restored.load_save_json(&saved).unwrap();
        let state = call(&mut restored, &paths, "aurum_content_state", json!({})).unwrap();
        assert_eq!(state["mesh_count"], 0);
    }

    #[test]
    fn import_appends_and_remaps_into_an_existing_document() {
        let root = temp_root("import-append");
        let paths = PathGuard::new(&root);
        let mut engine = Engine::new();

        // Build and export a small scene, then import it back alongside new work.
        let mesh = call(
            &mut engine,
            &paths,
            "aurum_mesh_add",
            json!({"kind": "box"}),
        )
        .unwrap();
        call(
            &mut engine,
            &paths,
            "aurum_node_add",
            json!({"name": "Original", "mesh": mesh["index"]}),
        )
        .unwrap();
        call(
            &mut engine,
            &paths,
            "aurum_animation_spin",
            json!({"node": 0, "name": "Spin"}),
        )
        .unwrap();
        call(
            &mut engine,
            &paths,
            "aurum_content_export",
            json!({"path": "asset.gltf"}),
        )
        .unwrap();

        let imported = call(
            &mut engine,
            &paths,
            "aurum_content_import",
            json!({"path": "asset.gltf", "mode": "append"}),
        )
        .unwrap();

        assert_eq!(imported["mesh_count"], 1);
        assert_eq!(imported["node_count"], 1);
        assert_eq!(imported["animation_count"], 1);
        assert_eq!(
            imported["document_mesh_count"], 2,
            "append must keep the original"
        );
        assert_eq!(imported["document_node_count"], 2);

        // Indices must be remapped, not reused: the document stays valid.
        let valid = call(&mut engine, &paths, "aurum_content_validate", json!({})).unwrap();
        assert_eq!(valid["valid"], true, "{valid}");

        let state = call(&mut engine, &paths, "aurum_content_state", json!({})).unwrap();
        let nodes = state["nodes"].as_array().unwrap();
        let appended = nodes.iter().find(|n| n["index"] == 1).unwrap();
        assert_eq!(appended["mesh"], 1, "the appended node kept mesh index 0");
        assert_eq!(appended["is_root"], true);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn import_replace_discards_the_current_document() {
        let root = temp_root("import-replace");
        let paths = PathGuard::new(&root);
        let mut engine = Engine::new();

        let mesh = call(
            &mut engine,
            &paths,
            "aurum_mesh_add",
            json!({"kind": "sphere"}),
        )
        .unwrap();
        call(
            &mut engine,
            &paths,
            "aurum_node_add",
            json!({"name": "Ball", "mesh": mesh["index"]}),
        )
        .unwrap();
        call(
            &mut engine,
            &paths,
            "aurum_content_export",
            json!({"path": "only.gltf"}),
        )
        .unwrap();

        // Add work that replace should throw away.
        let extra = call(
            &mut engine,
            &paths,
            "aurum_mesh_add",
            json!({"kind": "box"}),
        )
        .unwrap();
        call(
            &mut engine,
            &paths,
            "aurum_node_add",
            json!({"name": "Extra", "mesh": extra["index"]}),
        )
        .unwrap();
        assert_eq!(
            call(&mut engine, &paths, "aurum_content_state", json!({})).unwrap()["mesh_count"],
            2
        );

        let imported = call(
            &mut engine,
            &paths,
            "aurum_content_import",
            json!({"path": "only.gltf", "mode": "replace"}),
        )
        .unwrap();
        assert_eq!(
            imported["document_mesh_count"], 1,
            "replace must discard the extra mesh"
        );
        assert_eq!(imported["modal"], "replace");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn import_reports_missing_files_and_bad_modes() {
        let root = temp_root("import-errors");
        let paths = PathGuard::new(&root);
        let mut engine = Engine::new();

        let missing = call(
            &mut engine,
            &paths,
            "aurum_content_import",
            json!({"path": "nope.gltf"}),
        )
        .unwrap_err();
        assert!(matches!(missing, ToolError::Invalid(_)), "got {missing:?}");

        let bad_mode = call(
            &mut engine,
            &paths,
            "aurum_content_import",
            json!({"path": "x.gltf", "mode": "merge"}),
        )
        .unwrap_err();
        assert!(
            bad_mode.to_string().contains("append or replace"),
            "got: {bad_mode}"
        );

        // The same root guard as every other file tool.
        let denied = call(
            &mut engine,
            &paths,
            "aurum_content_import",
            json!({"path": "../outside.gltf"}),
        );
        assert!(matches!(denied, Err(ToolError::Denied(_))));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn sprite_atlas_packs_and_writes_a_placeholder_sheet() {
        let root = temp_root("atlas");
        let paths = PathGuard::new(&root);
        let mut engine = Engine::new();

        let out = call(
            &mut engine,
            &paths,
            "aurum_sprite_atlas",
            json!({
                "sprites": [
                    {"name": "hero", "width": 32, "height": 32, "color": [255, 0, 0, 255]},
                    {"name": "coin", "width": 16, "height": 16, "color": [255, 255, 0, 255]}
                ],
                "max_width": 64,
                "png_path": "atlas.png",
                "layout_path": "atlas.json"
            }),
        )
        .unwrap();

        assert_eq!(out["regions"].as_array().unwrap().len(), 2);
        assert!(out["width"].as_u64().unwrap() <= 64);
        assert!(out["occupancy"].as_f64().unwrap() > 0.0);

        // A real PNG, and a layout that names both sprites.
        let png = std::fs::read(root.join("atlas.png")).unwrap();
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
        let layout: Value =
            serde_json::from_str(&std::fs::read_to_string(root.join("atlas.json")).unwrap())
                .unwrap();
        assert_eq!(layout["regions"].as_array().unwrap().len(), 2);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn sprite_atlas_writes_no_png_without_colours() {
        let root = temp_root("atlas2");
        let paths = PathGuard::new(&root);
        let mut engine = Engine::new();
        let out = call(
            &mut engine,
            &paths,
            "aurum_sprite_atlas",
            json!({"sprites": [{"name": "a", "width": 8, "height": 8}], "png_path": "none.png"}),
        )
        .unwrap();
        assert!(out["written"].as_array().unwrap().is_empty());
        assert!(!root.join("none.png").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn sprite_atlas_validates_its_input() {
        let (mut engine, paths) = ctx();
        assert!(call(
            &mut engine,
            &paths,
            "aurum_sprite_atlas",
            json!({"sprites": []})
        )
        .is_err());
        assert!(call(
            &mut engine,
            &paths,
            "aurum_sprite_atlas",
            json!({"sprites": [{"width": 4}]})
        )
        .is_err());
        assert!(call(
            &mut engine,
            &paths,
            "aurum_sprite_atlas",
            json!({"sprites": [{"name": "a", "color": [1, 2]}]})
        )
        .is_err());
    }
}
