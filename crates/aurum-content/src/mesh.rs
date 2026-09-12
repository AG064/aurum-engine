//! Procedural geometry: the mesh model, transforms, and primitive builders.
//!
//! Everything here is pure computation on `f32` arrays. Nothing allocates a
//! GPU resource or touches Godot — the result is data that [`crate::gltf`]
//! writes out and an engine imports.
//!
//! Winding is counter-clockwise when viewed from outside, which is what glTF
//! treats as front-facing. The primitive tests verify that by computing face
//! normals and checking they point away from the shape's interior, so a
//! regression in winding fails a test rather than producing an inside-out
//! model that still "looks fine" in JSON.

use serde::{Deserialize, Serialize};

/// A 4×4 matrix in **column-major** order, matching glTF's convention.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mat4(pub [f32; 16]);

impl Default for Mat4 {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Mat4 {
    pub const IDENTITY: Mat4 = Mat4([
        1.0, 0.0, 0.0, 0.0, //
        0.0, 1.0, 0.0, 0.0, //
        0.0, 0.0, 1.0, 0.0, //
        0.0, 0.0, 0.0, 1.0,
    ]);

    /// Compose a translation, a quaternion rotation (`[x, y, z, w]`), and a
    /// scale, applied in that order — the TRS convention glTF uses.
    pub fn from_trs(translation: [f32; 3], rotation: [f32; 4], scale: [f32; 3]) -> Self {
        let [x, y, z, w] = rotation;
        // Standard quaternion-to-matrix, then scale the basis columns.
        let (x2, y2, z2) = (x + x, y + y, z + z);
        let (xx, xy, xz) = (x * x2, x * y2, x * z2);
        let (yy, yz, zz) = (y * y2, y * z2, z * z2);
        let (wx, wy, wz) = (w * x2, w * y2, w * z2);

        let [sx, sy, sz] = scale;
        Mat4([
            (1.0 - (yy + zz)) * sx,
            (xy + wz) * sx,
            (xz - wy) * sx,
            0.0,
            (xy - wz) * sy,
            (1.0 - (xx + zz)) * sy,
            (yz + wx) * sy,
            0.0,
            (xz + wy) * sz,
            (yz - wx) * sz,
            (1.0 - (xx + yy)) * sz,
            0.0,
            translation[0],
            translation[1],
            translation[2],
            1.0,
        ])
    }

    pub fn translation(x: f32, y: f32, z: f32) -> Self {
        let mut m = Self::IDENTITY;
        m.0[12] = x;
        m.0[13] = y;
        m.0[14] = z;
        m
    }

    pub fn scale(x: f32, y: f32, z: f32) -> Self {
        let mut m = Self::IDENTITY;
        m.0[0] = x;
        m.0[5] = y;
        m.0[10] = z;
        m
    }

    /// Rotation about an axis by `radians`.
    pub fn rotation(axis: [f32; 3], radians: f32) -> Self {
        let len = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt();
        if len <= f32::EPSILON {
            return Self::IDENTITY;
        }
        let half = radians * 0.5;
        let s = half.sin() / len;
        Self::from_trs(
            [0.0; 3],
            [axis[0] * s, axis[1] * s, axis[2] * s, half.cos()],
            [1.0; 3],
        )
    }

    /// Matrix product `self * rhs`, so `rhs` is applied first.
    pub fn multiply(&self, rhs: &Mat4) -> Mat4 {
        let a = &self.0;
        let b = &rhs.0;
        let mut out = [0.0f32; 16];
        for column in 0..4 {
            for row in 0..4 {
                let mut sum = 0.0;
                for k in 0..4 {
                    sum += a[k * 4 + row] * b[column * 4 + k];
                }
                out[column * 4 + row] = sum;
            }
        }
        Mat4(out)
    }

    /// Transform a point (translation applies).
    pub fn transform_point(&self, p: [f32; 3]) -> [f32; 3] {
        let m = &self.0;
        [
            m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12],
            m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13],
            m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14],
        ]
    }

    /// Transform a direction (translation does not apply).
    pub fn transform_direction(&self, d: [f32; 3]) -> [f32; 3] {
        let m = &self.0;
        [
            m[0] * d[0] + m[4] * d[1] + m[8] * d[2],
            m[1] * d[0] + m[5] * d[1] + m[9] * d[2],
            m[2] * d[0] + m[6] * d[1] + m[10] * d[2],
        ]
    }

    /// The upper-left 3×3 determinant. Negative means the transform mirrors,
    /// which flips triangle winding.
    pub fn determinant3(&self) -> f32 {
        let m = &self.0;
        m[0] * (m[5] * m[10] - m[9] * m[6]) - m[4] * (m[1] * m[10] - m[9] * m[2])
            + m[8] * (m[1] * m[6] - m[5] * m[2])
    }

    /// Inverse-transpose of the upper-left 3×3, for transforming normals
    /// correctly under non-uniform scale. Falls back to the plain basis when
    /// the matrix is singular.
    fn normal_matrix(&self) -> [f32; 9] {
        let m = &self.0;
        let a = [m[0], m[1], m[2]];
        let b = [m[4], m[5], m[6]];
        let c = [m[8], m[9], m[10]];

        // Cofactors give the inverse-transpose directly (adjugate == det * A^-T).
        let r0 = [
            b[1] * c[2] - b[2] * c[1],
            b[2] * c[0] - b[0] * c[2],
            b[0] * c[1] - b[1] * c[0],
        ];
        let r1 = [
            a[2] * c[1] - a[1] * c[2],
            a[0] * c[2] - a[2] * c[0],
            a[1] * c[0] - a[0] * c[1],
        ];
        let r2 = [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ];
        let det = a[0] * r0[0] + a[1] * r0[1] + a[2] * r0[2];
        if det.abs() <= f32::EPSILON {
            return [a[0], a[1], a[2], b[0], b[1], b[2], c[0], c[1], c[2]];
        }
        // Rows above are already the cofactor matrix rows; transpose them.
        [
            r0[0], r1[0], r2[0], //
            r0[1], r1[1], r2[1], //
            r0[2], r1[2], r2[2],
        ]
    }
}

/// Indexed triangle mesh with optional normals and texture coordinates.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Mesh {
    pub name: String,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
}

impl Mesh {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Default::default()
        }
    }

    pub fn vertex_count(&self) -> usize {
        self.positions.len()
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// Whether the mesh has geometry worth exporting.
    pub fn is_empty(&self) -> bool {
        self.positions.is_empty() || self.indices.is_empty()
    }

    /// Fill in uniformly zero UVs when a builder produced none, so every
    /// exported primitive has the same attribute set and can be merged.
    pub fn ensure_uvs(&mut self) {
        if self.uvs.len() != self.positions.len() {
            self.uvs = vec![[0.0, 0.0]; self.positions.len()];
        }
    }

    /// Recompute smooth (area-weighted) vertex normals from the faces.
    ///
    /// Vertices are welded by position for the accumulation, so a mesh built
    /// from shared vertices gets smooth shading while a flat-shaded primitive
    /// keeps its split vertices and stays faceted.
    pub fn compute_normals(&mut self) {
        let mut accum = vec![[0.0f32; 3]; self.positions.len()];
        for tri in self.indices.as_chunks::<3>().0 {
            let (i0, i1, i2) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);
            let (Some(&p0), Some(&p1), Some(&p2)) = (
                self.positions.get(i0),
                self.positions.get(i1),
                self.positions.get(i2),
            ) else {
                continue;
            };
            // Un-normalized cross product is already area-weighted.
            let e1 = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
            let e2 = [p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]];
            let n = [
                e1[1] * e2[2] - e1[2] * e2[1],
                e1[2] * e2[0] - e1[0] * e2[2],
                e1[0] * e2[1] - e1[1] * e2[0],
            ];
            for &index in &[i0, i1, i2] {
                accum[index][0] += n[0];
                accum[index][1] += n[1];
                accum[index][2] += n[2];
            }
        }
        self.normals = accum.into_iter().map(normalize3).collect();
    }

    /// Axis-aligned bounds as `(min, max)`, or `None` when empty.
    pub fn bounds(&self) -> Option<([f32; 3], [f32; 3])> {
        let first = *self.positions.first()?;
        let mut min = first;
        let mut max = first;
        for p in &self.positions {
            for axis in 0..3 {
                min[axis] = min[axis].min(p[axis]);
                max[axis] = max[axis].max(p[axis]);
            }
        }
        Some((min, max))
    }

    /// Apply a transform in place.
    ///
    /// Normals use the inverse-transpose so non-uniform scale does not skew
    /// them, and mirroring transforms flip the winding so the mesh keeps
    /// facing outwards.
    pub fn transform(&mut self, matrix: &Mat4) {
        for p in &mut self.positions {
            *p = matrix.transform_point(*p);
        }
        let nm = matrix.normal_matrix();
        for n in &mut self.normals {
            let transformed = [
                nm[0] * n[0] + nm[1] * n[1] + nm[2] * n[2],
                nm[3] * n[0] + nm[4] * n[1] + nm[5] * n[2],
                nm[6] * n[0] + nm[7] * n[1] + nm[8] * n[2],
            ];
            *n = normalize3(transformed);
        }
        if matrix.determinant3() < 0.0 {
            for tri in self.indices.as_chunks_mut::<3>().0 {
                tri.swap(1, 2);
            }
        }
    }

    /// Append `other` into `self`, offsetting its indices.
    ///
    /// Attribute sets are normalized first, so meshes from different builders
    /// combine cleanly.
    pub fn merge(&mut self, other: &Mesh) {
        self.ensure_uvs();
        let mut other = other.clone();
        other.ensure_uvs();
        if other.normals.len() != other.positions.len() {
            other.compute_normals();
        }

        let base = self.positions.len() as u32;
        self.positions.extend_from_slice(&other.positions);
        self.normals.extend_from_slice(&other.normals);
        self.uvs.extend_from_slice(&other.uvs);
        self.indices.extend(other.indices.iter().map(|i| i + base));
    }

    /// A copy of `other` placed by `matrix`, ready to merge.
    pub fn transformed_copy(other: &Mesh, matrix: &Mat4) -> Mesh {
        let mut copy = other.clone();
        if copy.normals.len() != copy.positions.len() {
            copy.compute_normals();
        }
        copy.transform(matrix);
        copy
    }

    /// Validate structural invariants. Returns a list of problems, empty when
    /// the mesh is sound. Used by tests and by the MCP tools before export.
    pub fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        let n = self.positions.len();
        if !self.indices.len().is_multiple_of(3) {
            problems.push(format!(
                "index count {} is not a multiple of 3",
                self.indices.len()
            ));
        }
        if self.normals.len() != n {
            problems.push(format!("{} normals for {n} positions", self.normals.len()));
        }
        if self.uvs.len() != n {
            problems.push(format!("{} uvs for {n} positions", self.uvs.len()));
        }
        if let Some(&bad) = self.indices.iter().find(|&&i| i as usize >= n) {
            problems.push(format!("index {bad} is out of range for {n} vertices"));
        }
        if self
            .positions
            .iter()
            .any(|p| p.iter().any(|c| !c.is_finite()))
        {
            problems.push("positions contain a non-finite value".into());
        }
        problems
    }
}

/// Normalize a 3-vector, returning `+Y` for a degenerate input so downstream
/// code never sees a NaN normal.
pub fn normalize3(v: [f32; 3]) -> [f32; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len <= f32::EPSILON {
        return [0.0, 1.0, 0.0];
    }
    [v[0] / len, v[1] / len, v[2] / len]
}

// ---------------------------------------------------------------------------
// Primitives
// ---------------------------------------------------------------------------

/// An axis-aligned box centred on the origin, with flat normals.
pub fn box_mesh(width: f32, height: f32, depth: f32) -> Mesh {
    let (hx, hy, hz) = (width * 0.5, height * 0.5, depth * 0.5);
    let mut mesh = Mesh::new("Box");

    // Six faces, each with its own four vertices so normals stay flat.
    let faces: [([f32; 3], [[f32; 3]; 4]); 6] = [
        // +X
        (
            [1.0, 0.0, 0.0],
            [[hx, -hy, hz], [hx, -hy, -hz], [hx, hy, -hz], [hx, hy, hz]],
        ),
        // -X
        (
            [-1.0, 0.0, 0.0],
            [
                [-hx, -hy, -hz],
                [-hx, -hy, hz],
                [-hx, hy, hz],
                [-hx, hy, -hz],
            ],
        ),
        // +Y
        (
            [0.0, 1.0, 0.0],
            [[-hx, hy, hz], [hx, hy, hz], [hx, hy, -hz], [-hx, hy, -hz]],
        ),
        // -Y
        (
            [0.0, -1.0, 0.0],
            [
                [-hx, -hy, -hz],
                [hx, -hy, -hz],
                [hx, -hy, hz],
                [-hx, -hy, hz],
            ],
        ),
        // +Z
        (
            [0.0, 0.0, 1.0],
            [[-hx, -hy, hz], [hx, -hy, hz], [hx, hy, hz], [-hx, hy, hz]],
        ),
        // -Z
        (
            [0.0, 0.0, -1.0],
            [
                [hx, -hy, -hz],
                [-hx, -hy, -hz],
                [-hx, hy, -hz],
                [hx, hy, -hz],
            ],
        ),
    ];

    for (normal, corners) in faces {
        let base = mesh.positions.len() as u32;
        let uv = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
        for (corner, tex) in corners.iter().zip(uv) {
            mesh.positions.push(*corner);
            mesh.normals.push(normal);
            mesh.uvs.push(tex);
        }
        mesh.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    mesh
}

/// A flat quad in the XZ plane, centred on the origin, facing `+Y`.
pub fn plane_mesh(width: f32, depth: f32) -> Mesh {
    let (hx, hz) = (width * 0.5, depth * 0.5);
    let mut mesh = Mesh::new("Plane");
    mesh.positions = vec![
        [-hx, 0.0, hz],
        [hx, 0.0, hz],
        [hx, 0.0, -hz],
        [-hx, 0.0, -hz],
    ];
    mesh.normals = vec![[0.0, 1.0, 0.0]; 4];
    mesh.uvs = vec![[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
    mesh.indices = vec![0, 1, 2, 0, 2, 3];
    mesh
}

/// A UV sphere of `radius`, tessellated by `sectors` (longitude) and `stacks`
/// (latitude).
pub fn uv_sphere(radius: f32, sectors: u32, stacks: u32) -> Mesh {
    let sectors = sectors.max(3);
    let stacks = stacks.max(2);
    let mut mesh = Mesh::new("Sphere");

    for stack in 0..=stacks {
        let phi = std::f32::consts::PI * stack as f32 / stacks as f32;
        let y = phi.cos();
        let ring = phi.sin();
        for sector in 0..=sectors {
            let theta = std::f32::consts::TAU * sector as f32 / sectors as f32;
            let (x, z) = (theta.cos() * ring, theta.sin() * ring);
            mesh.positions.push([x * radius, y * radius, z * radius]);
            mesh.normals.push(normalize3([x, y, z]));
            mesh.uvs
                .push([sector as f32 / sectors as f32, stack as f32 / stacks as f32]);
        }
    }

    let stride = sectors + 1;
    for stack in 0..stacks {
        for sector in 0..sectors {
            let a = stack * stride + sector;
            let b = a + stride;
            // Outward-facing quads: (p00, p01, p10) and (p01, p11, p10). Each
            // degenerates to a line at one pole, so the pole triangles are
            // skipped rather than emitted with zero area.
            if stack != 0 {
                mesh.indices.extend_from_slice(&[a, a + 1, b]);
            }
            if stack != stacks - 1 {
                mesh.indices.extend_from_slice(&[a + 1, b + 1, b]);
            }
        }
    }
    mesh
}

/// A cylinder of `radius` and `height`, centred on the origin, capped.
pub fn cylinder(radius: f32, height: f32, sectors: u32) -> Mesh {
    let sectors = sectors.max(3);
    let half = height * 0.5;
    let mut mesh = Mesh::new("Cylinder");

    // Side wall: split vertices so the seam and the caps keep hard edges.
    for sector in 0..=sectors {
        let theta = std::f32::consts::TAU * sector as f32 / sectors as f32;
        let (x, z) = (theta.cos(), theta.sin());
        let u = sector as f32 / sectors as f32;
        mesh.positions.push([x * radius, -half, z * radius]);
        mesh.normals.push([x, 0.0, z]);
        mesh.uvs.push([u, 1.0]);
        mesh.positions.push([x * radius, half, z * radius]);
        mesh.normals.push([x, 0.0, z]);
        mesh.uvs.push([u, 0.0]);
    }
    for sector in 0..sectors {
        let a = sector * 2;
        let (b, c, d) = (a + 1, a + 2, a + 3);
        // Quad (bottom_s, top_s, bottom_s+1, top_s+1), wound outward.
        mesh.indices.extend_from_slice(&[a, b, c, b, d, c]);
    }

    push_disc(&mut mesh, radius, half, sectors, true);
    push_disc(&mut mesh, radius, -half, sectors, false);
    mesh
}

/// A capped cone with its base at `-height/2` and apex at `+height/2`.
pub fn cone(radius: f32, height: f32, sectors: u32) -> Mesh {
    let sectors = sectors.max(3);
    let half = height * 0.5;
    let mut mesh = Mesh::new("Cone");

    // The apex is duplicated per sector so each side triangle is flat-shaded.
    let slope = (radius / height).atan();
    for sector in 0..sectors {
        let t0 = sector as f32 / sectors as f32;
        let t1 = (sector + 1) as f32 / sectors as f32;
        let a0 = std::f32::consts::TAU * t0;
        let a1 = std::f32::consts::TAU * t1;
        let mid = (a0 + a1) * 0.5;
        let n = normalize3([
            mid.cos() * slope.cos(),
            slope.sin(),
            mid.sin() * slope.cos(),
        ]);

        let base = mesh.positions.len() as u32;
        mesh.positions
            .push([a0.cos() * radius, -half, a0.sin() * radius]);
        mesh.normals.push(n);
        mesh.uvs.push([t0, 1.0]);

        mesh.positions
            .push([a1.cos() * radius, -half, a1.sin() * radius]);
        mesh.normals.push(n);
        mesh.uvs.push([t1, 1.0]);

        mesh.positions.push([0.0, half, 0.0]);
        mesh.normals.push(n);
        mesh.uvs.push([(t0 + t1) * 0.5, 0.0]);

        mesh.indices.extend_from_slice(&[base, base + 2, base + 1]);
    }

    push_disc(&mut mesh, radius, -half, sectors, false);
    mesh
}

/// A torus in the XZ plane around the Y axis.
pub fn torus(
    major_radius: f32,
    minor_radius: f32,
    major_segments: u32,
    minor_segments: u32,
) -> Mesh {
    let major_segments = major_segments.max(3);
    let minor_segments = minor_segments.max(3);
    let mut mesh = Mesh::new("Torus");

    for i in 0..=major_segments {
        let u = i as f32 / major_segments as f32;
        let theta = std::f32::consts::TAU * u;
        let (ct, st) = (theta.cos(), theta.sin());
        for j in 0..=minor_segments {
            let v = j as f32 / minor_segments as f32;
            let phi = std::f32::consts::TAU * v;
            let (cp, sp) = (phi.cos(), phi.sin());

            let normal = [ct * cp, sp, st * cp];
            let center = [ct * major_radius, 0.0, st * major_radius];
            mesh.positions.push([
                center[0] + normal[0] * minor_radius,
                center[1] + normal[1] * minor_radius,
                center[2] + normal[2] * minor_radius,
            ]);
            mesh.normals.push(normal);
            mesh.uvs.push([u, v]);
        }
    }

    let stride = minor_segments + 1;
    for i in 0..major_segments {
        for j in 0..minor_segments {
            let a = i * stride + j;
            let b = a + stride;
            mesh.indices
                .extend_from_slice(&[a, b, a + 1, a + 1, b, b + 1]);
        }
    }
    mesh
}

/// Append a triangle-fan disc cap at `y`, facing `+Y` when `up`.
fn push_disc(mesh: &mut Mesh, radius: f32, y: f32, sectors: u32, up: bool) {
    let normal = if up {
        [0.0, 1.0, 0.0]
    } else {
        [0.0, -1.0, 0.0]
    };
    let base = mesh.positions.len() as u32;
    mesh.positions.push([0.0, y, 0.0]);
    mesh.normals.push(normal);
    mesh.uvs.push([0.5, 0.5]);

    for sector in 0..=sectors {
        let theta = std::f32::consts::TAU * sector as f32 / sectors as f32;
        let (x, z) = (theta.cos(), theta.sin());
        mesh.positions.push([x * radius, y, z * radius]);
        mesh.normals.push(normal);
        mesh.uvs.push([x * 0.5 + 0.5, z * 0.5 + 0.5]);
    }
    for sector in 0..sectors {
        let a = base + 1 + sector;
        let b = a + 1;
        // The ring runs counter-clockwise when seen from +Y, so a cap facing
        // up needs the reverse order to stay outward.
        if up {
            mesh.indices.extend_from_slice(&[base, b, a]);
        } else {
            mesh.indices.extend_from_slice(&[base, a, b]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Face normal of a triangle, un-normalized.
    fn face_normal(mesh: &Mesh, tri: usize) -> [f32; 3] {
        let i = tri * 3;
        let p0 = mesh.positions[mesh.indices[i] as usize];
        let p1 = mesh.positions[mesh.indices[i + 1] as usize];
        let p2 = mesh.positions[mesh.indices[i + 2] as usize];
        let e1 = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
        let e2 = [p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]];
        [
            e1[1] * e2[2] - e1[2] * e2[1],
            e1[2] * e2[0] - e1[0] * e2[2],
            e1[0] * e2[1] - e1[1] * e2[0],
        ]
    }

    fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
        a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
    }

    /// Centroid of a triangle.
    fn centroid(mesh: &Mesh, tri: usize) -> [f32; 3] {
        let i = tri * 3;
        let mut c = [0.0f32; 3];
        for k in 0..3 {
            let p = mesh.positions[mesh.indices[i + k] as usize];
            for axis in 0..3 {
                c[axis] += p[axis] / 3.0;
            }
        }
        c
    }

    /// Every face must point away from the origin for an origin-centred
    /// convex solid. This is the winding check that catches inside-out meshes.
    fn assert_faces_point_outward(mesh: &Mesh, label: &str) {
        for tri in 0..mesh.triangle_count() {
            let c = centroid(mesh, tri);
            let n = face_normal(mesh, tri);
            assert!(
                dot(n, c) > 0.0,
                "{label}: face {tri} points inward (n·c = {})",
                dot(n, c)
            );
        }
    }

    #[test]
    fn primitives_are_structurally_sound() {
        let meshes = [
            box_mesh(1.0, 2.0, 3.0),
            plane_mesh(1.0, 1.0),
            uv_sphere(1.0, 16, 12),
            cylinder(1.0, 2.0, 16),
            cone(1.0, 2.0, 16),
            torus(1.0, 0.25, 16, 12),
        ];
        for mesh in meshes {
            let problems = mesh.validate();
            assert!(problems.is_empty(), "{}: {problems:?}", mesh.name);
            assert!(
                mesh.triangle_count() > 0,
                "{} produced no triangles",
                mesh.name
            );
        }
    }

    #[test]
    fn box_faces_point_outward_and_are_flat() {
        let mesh = box_mesh(2.0, 2.0, 2.0);
        assert_eq!(mesh.vertex_count(), 24, "each face needs its own vertices");
        assert_eq!(mesh.triangle_count(), 12);
        assert_faces_point_outward(&mesh, "box");

        // Each stored normal must equal its face normal: flat shading.
        for tri in 0..mesh.triangle_count() {
            let i = tri * 3;
            let stored = mesh.normals[mesh.indices[i] as usize];
            let computed = normalize3(face_normal(&mesh, tri));
            assert!(
                dot(stored, computed) > 0.99,
                "box face {tri} is not flat-shaded"
            );
        }
    }

    #[test]
    fn sphere_faces_point_outward_with_smooth_normals() {
        let mesh = uv_sphere(1.0, 16, 12);
        assert_faces_point_outward(&mesh, "sphere");
        // Normals equal the normalized position on a unit sphere.
        for (p, n) in mesh.positions.iter().zip(&mesh.normals) {
            assert!(dot(normalize3(*p), *n) > 0.999);
        }
    }

    #[test]
    fn cylinder_and_cone_faces_point_outward() {
        assert_faces_point_outward(&cylinder(1.0, 2.0, 16), "cylinder");
        assert_faces_point_outward(&cone(1.0, 2.0, 16), "cone");
    }

    #[test]
    fn torus_normals_are_radial_from_the_tube_centre() {
        let (major, minor) = (1.0f32, 0.25f32);
        let mesh = torus(major, minor, 16, 12);
        for (p, n) in mesh.positions.iter().zip(&mesh.normals) {
            // The tube centre for this vertex lies on the major circle.
            let horizontal = (p[0] * p[0] + p[2] * p[2]).sqrt();
            let scale = major / horizontal;
            let tube_centre = [p[0] * scale, 0.0, p[2] * scale];
            let radial = normalize3([
                p[0] - tube_centre[0],
                p[1] - tube_centre[1],
                p[2] - tube_centre[2],
            ]);
            assert!(dot(radial, *n) > 0.99, "torus normal is not radial");
        }
    }

    #[test]
    fn plane_faces_up() {
        let mesh = plane_mesh(2.0, 2.0);
        assert!(mesh.normals.iter().all(|n| *n == [0.0, 1.0, 0.0]));
        assert_eq!(mesh.triangle_count(), 2);
        // The outward-from-origin check does not apply here: the plane passes
        // through the origin, so its centroids lie in the plane and n·c is
        // zero. Check the winding directly against the +Y normal instead.
        for tri in 0..mesh.triangle_count() {
            let n = normalize3(face_normal(&mesh, tri));
            assert!(
                dot(n, [0.0, 1.0, 0.0]) > 0.99,
                "triangle {tri} winds downward"
            );
        }
    }

    #[test]
    fn compute_normals_matches_known_geometry() {
        let mut mesh = plane_mesh(2.0, 2.0);
        mesh.normals.clear();
        mesh.compute_normals();
        assert!(mesh.normals.iter().all(|n| dot(*n, [0.0, 1.0, 0.0]) > 0.99));
    }

    #[test]
    fn transform_translates_scales_and_rotates() {
        let mut mesh = box_mesh(1.0, 1.0, 1.0);
        mesh.transform(&Mat4::translation(5.0, 0.0, 0.0));
        let (min, max) = mesh.bounds().unwrap();
        assert!((min[0] - 4.5).abs() < 1e-5);
        assert!((max[0] - 5.5).abs() < 1e-5);

        let mut scaled = box_mesh(1.0, 1.0, 1.0);
        scaled.transform(&Mat4::scale(2.0, 1.0, 1.0));
        let (_, max) = scaled.bounds().unwrap();
        assert!((max[0] - 1.0).abs() < 1e-5);
        assert!((max[1] - 0.5).abs() < 1e-5);

        let mut rotated = plane_mesh(2.0, 2.0);
        rotated.transform(&Mat4::rotation(
            [1.0, 0.0, 0.0],
            std::f32::consts::FRAC_PI_2,
        ));
        // A +Y-facing plane rotated 90° about X now faces -Z or +Z.
        assert!(rotated.normals.iter().all(|n| n[1].abs() < 1e-5));
    }

    #[test]
    fn trs_composition_matches_reference() {
        // Rotate 90° about Y: +X should map to -Z (right-handed, Y up).
        let m = Mat4::from_trs(
            [0.0; 3],
            {
                let half = std::f32::consts::FRAC_PI_2 * 0.5;
                [0.0, half.sin(), 0.0, half.cos()]
            },
            [1.0; 3],
        );
        let r = m.transform_point([1.0, 0.0, 0.0]);
        assert!(r[0].abs() < 1e-5 && (r[2] + 1.0).abs() < 1e-5, "got {r:?}");
    }

    #[test]
    fn multiply_applies_the_right_hand_side_first() {
        let translate = Mat4::translation(10.0, 0.0, 0.0);
        let scale = Mat4::scale(2.0, 2.0, 2.0);
        // scale first, then translate: (1,0,0) -> (2,0,0) -> (12,0,0)
        let m = translate.multiply(&scale);
        let p = m.transform_point([1.0, 0.0, 0.0]);
        assert!((p[0] - 12.0).abs() < 1e-5, "got {p:?}");
    }

    #[test]
    fn mirroring_transform_flips_winding_to_stay_outward() {
        let mut mesh = box_mesh(1.0, 1.0, 1.0);
        let before = mesh.indices.clone();
        let mut mirrored = Mat4::IDENTITY;
        mirrored.0[0] = -1.0; // mirror on X
        mesh.transform(&mirrored);
        assert_ne!(mesh.indices, before, "winding must be flipped");
        assert_faces_point_outward(&mesh, "mirrored box");
    }

    #[test]
    fn merge_offsets_indices_and_keeps_geometry_apart() {
        let mut combined = box_mesh(1.0, 1.0, 1.0);
        let before = combined.vertex_count();
        let shifted =
            Mesh::transformed_copy(&box_mesh(1.0, 1.0, 1.0), &Mat4::translation(10.0, 0.0, 0.0));
        combined.merge(&shifted);

        assert_eq!(combined.vertex_count(), before * 2);
        assert_eq!(combined.triangle_count(), 24);
        assert!(combined.validate().is_empty());
        let (min, max) = combined.bounds().unwrap();
        assert!((min[0] + 0.5).abs() < 1e-5, "first box moved");
        assert!((max[0] - 10.5).abs() < 1e-5, "second box not placed");
    }

    #[test]
    fn merge_fills_missing_attribute_sets() {
        let mut base = box_mesh(1.0, 1.0, 1.0);
        let mut other = plane_mesh(1.0, 1.0);
        other.uvs.clear();
        other.normals.clear();
        base.merge(&other);
        assert!(base.validate().is_empty(), "{:?}", base.validate());
    }

    #[test]
    fn validate_catches_broken_meshes() {
        let mut mesh = plane_mesh(1.0, 1.0);
        mesh.indices.push(999);
        mesh.indices.push(0);
        let problems = mesh.validate();
        assert!(problems.iter().any(|p| p.contains("multiple of 3")));
        assert!(problems.iter().any(|p| p.contains("out of range")));

        let mut no_normals = plane_mesh(1.0, 1.0);
        no_normals.normals.clear();
        assert!(no_normals.validate().iter().any(|p| p.contains("normals")));
    }

    #[test]
    fn normalize3_never_returns_nan() {
        assert_eq!(normalize3([0.0, 0.0, 0.0]), [0.0, 1.0, 0.0]);
        let n = normalize3([0.0, 5.0, 0.0]);
        assert!((n[1] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn degenerate_inputs_are_clamped_not_panicking() {
        assert!(uv_sphere(1.0, 0, 0).triangle_count() > 0);
        assert!(cylinder(1.0, 1.0, 0).triangle_count() > 0);
        assert!(cone(1.0, 1.0, 0).triangle_count() > 0);
        assert!(torus(1.0, 0.1, 0, 0).triangle_count() > 0);
        assert!(box_mesh(0.0, 0.0, 0.0).validate().is_empty());
    }
}
