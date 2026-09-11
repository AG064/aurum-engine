//! Modelling operations: extrusion, lathing, and polygon triangulation.
//!
//! These turn authored profiles into solids, which is the difference between
//! placing primitives and actually modelling. Everything here produces closed,
//! outward-facing triangle meshes — the tests verify that with a manifold
//! edge check and a signed-volume check rather than by eyeballing normals.

use crate::mesh::{normalize3, Mesh};

/// Signed area of a 2D polygon. Positive means counter-clockwise.
pub fn signed_area(profile: &[[f32; 2]]) -> f32 {
    let n = profile.len();
    if n < 3 {
        return 0.0;
    }
    let mut total = 0.0;
    for i in 0..n {
        let a = profile[i];
        let b = profile[(i + 1) % n];
        total += a[0] * b[1] - b[0] * a[1];
    }
    total * 0.5
}

/// Whether a triangle's vertices wind counter-clockwise.
fn cross2(a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> f32 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

fn point_in_triangle(p: [f32; 2], a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> bool {
    // Strictly inside: points exactly on an edge are not "inside", so a
    // collinear neighbour does not block an otherwise valid ear.
    let d1 = cross2(a, b, p);
    let d2 = cross2(b, c, p);
    let d3 = cross2(c, a, p);
    let has_negative = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let has_positive = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(has_negative && has_positive)
}

/// Triangulate a simple polygon by ear clipping.
///
/// Works for concave profiles, which a triangle fan does not — fan
/// triangulation silently produces overlapping geometry on anything but a
/// convex outline. Returns index triples into `profile`, wound
/// counter-clockwise.
///
/// A self-intersecting outline is not a polygon, and this stops rather than
/// looping forever; callers get whatever was clipped cleanly.
pub fn triangulate(profile: &[[f32; 2]]) -> Vec<[usize; 3]> {
    let n = profile.len();
    if n < 3 {
        return Vec::new();
    }

    // Work counter-clockwise so the convexity test is a simple sign check.
    let mut remaining: Vec<usize> = (0..n).collect();
    if signed_area(profile) < 0.0 {
        remaining.reverse();
    }

    let mut triangles = Vec::with_capacity(n.saturating_sub(2));
    let mut guard = 0usize;
    let limit = n * n + 16;

    while remaining.len() > 3 {
        guard += 1;
        if guard > limit {
            break; // Degenerate or self-intersecting outline.
        }

        let count = remaining.len();
        let mut clipped = false;
        for i in 0..count {
            let prev = remaining[(i + count - 1) % count];
            let current = remaining[i];
            let next = remaining[(i + 1) % count];

            let a = profile[prev];
            let b = profile[current];
            let c = profile[next];

            // A convex corner for a counter-clockwise polygon turns left.
            if cross2(a, b, c) <= 0.0 {
                continue;
            }
            // The ear is only valid when no other vertex falls inside it.
            let blocked = remaining.iter().any(|&other| {
                other != prev
                    && other != current
                    && other != next
                    && point_in_triangle(profile[other], a, b, c)
            });
            if blocked {
                continue;
            }

            triangles.push([prev, current, next]);
            remaining.remove(i);
            clipped = true;
            break;
        }

        if !clipped {
            break; // No ear found: bail rather than spin.
        }
    }

    if remaining.len() == 3 {
        triangles.push([remaining[0], remaining[1], remaining[2]]);
    }
    triangles
}

/// Extrude a 2D profile along Z, centred on the origin.
///
/// The profile is a closed outline in the XY plane. Caps get their own
/// vertices so their normals stay flat, and each side quad gets its own so the
/// silhouette stays crisp.
pub fn extrude(profile: &[[f32; 2]], depth: f32) -> Mesh {
    let mut mesh = Mesh::new("Extrusion");
    if profile.len() < 3 || depth.abs() <= f32::EPSILON {
        return mesh;
    }

    // Normalize to counter-clockwise up front. Branching on winding at each
    // cap and wall instead is how the clockwise case ends up inside-out, since
    // `triangulate` normalizes the caps on its own.
    let normalized: Vec<[f32; 2]>;
    let profile: &[[f32; 2]] = if signed_area(profile) < 0.0 {
        let mut reversed = profile.to_vec();
        reversed.reverse();
        normalized = reversed;
        &normalized
    } else {
        profile
    };

    let n = profile.len();
    let half = depth * 0.5;
    let cap_triangles = triangulate(profile);

    // -- bottom cap, facing -Z --------------------------------------------
    let bottom_base = mesh.positions.len() as u32;
    for point in profile {
        mesh.positions.push([point[0], point[1], -half]);
        mesh.normals.push([0.0, 0.0, -1.0]);
        mesh.uvs.push([point[0], point[1]]);
    }
    // `triangulate` returns counter-clockwise triangles, so the bottom cap is
    // simply the reverse and faces -Z.
    for triangle in &cap_triangles {
        mesh.indices.extend_from_slice(&[
            bottom_base + triangle[0] as u32,
            bottom_base + triangle[2] as u32,
            bottom_base + triangle[1] as u32,
        ]);
    }

    // -- top cap, facing +Z ------------------------------------------------
    let top_base = mesh.positions.len() as u32;
    for point in profile {
        mesh.positions.push([point[0], point[1], half]);
        mesh.normals.push([0.0, 0.0, 1.0]);
        mesh.uvs.push([point[0], point[1]]);
    }
    for triangle in &cap_triangles {
        mesh.indices.extend_from_slice(&[
            top_base + triangle[0] as u32,
            top_base + triangle[1] as u32,
            top_base + triangle[2] as u32,
        ]);
    }

    // -- side walls --------------------------------------------------------
    for i in 0..n {
        let a = profile[i];
        let b = profile[(i + 1) % n];
        let d = [b[0] - a[0], b[1] - a[1]];
        let length = (d[0] * d[0] + d[1] * d[1]).sqrt();
        if length <= f32::EPSILON {
            continue; // Repeated point: no wall to build.
        }
        // For a counter-clockwise outline the outward normal is (dy, -dx).
        let normal = [d[1] / length, -d[0] / length, 0.0];
        let u0 = i as f32 / n as f32;
        let u1 = (i + 1) as f32 / n as f32;

        let base = mesh.positions.len() as u32;
        for (point, u, z) in [(a, u0, -half), (b, u1, -half), (b, u1, half), (a, u0, half)] {
            mesh.positions.push([point[0], point[1], z]);
            mesh.normals.push(normal);
            mesh.uvs.push([u, if z < 0.0 { 1.0 } else { 0.0 }]);
        }
        mesh.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    mesh
}

/// Revolve a profile around the Y axis.
///
/// The profile is `[radius, height]` pairs. A radius of zero puts a point on
/// the axis, which closes the shape into a solid of revolution and is handled
/// by dropping the degenerate half of those quads.
///
/// `segments` is the angular resolution; anything under 3 is raised to 3.
/// `arc_degrees` below 360 produces an open shell — the cut faces are not
/// capped, which is the useful behaviour for something like a half-vase.
pub fn lathe(profile: &[[f32; 2]], segments: u32, arc_degrees: f32) -> Mesh {
    let segments = segments.max(3);
    let mut mesh = Mesh::new("Lathe");
    if profile.len() < 2 {
        return mesh;
    }

    let sweep = arc_degrees.clamp(1.0, 360.0).to_radians();
    let steps = segments.max(1);
    // Both cases use the same column count: for a full revolve the last column
    // ends at 2pi, which is the same place as 0, so the seam closes on its own.
    let columns = steps;
    let angle = |j: u32| sweep * j as f32 / steps as f32;

    for i in 0..profile.len() - 1 {
        let (r0, y0) = (profile[i][0], profile[i][1]);
        let (r1, y1) = (profile[i + 1][0], profile[i + 1][1]);

        let dr = r1 - r0;
        let dy = y1 - y0;
        let length = (dr * dr + dy * dy).sqrt();
        if length <= f32::EPSILON {
            continue; // Repeated profile point.
        }
        // Outward normal of the profile, revolved: (dy, -dr) in (radius, Y).
        let (nr, ny) = (dy / length, -dr / length);

        for j in 0..columns {
            let theta0 = angle(j);
            let theta1 = angle(j + 1);

            let corner = |radius: f32, height: f32| {
                [
                    [theta0.cos() * radius, height, theta0.sin() * radius],
                    [theta1.cos() * radius, height, theta1.sin() * radius],
                ]
            };

            let at = |theta: f32| normalize3([nr * theta.cos(), ny, nr * theta.sin()]);

            let (p00, p01) = {
                let c = corner(r0, y0);
                (c[0], c[1])
            };
            let (p10, p11) = {
                let c = corner(r1, y1);
                (c[0], c[1])
            };
            let (n0, n1) = (at(theta0), at(theta1));

            // Skip quads that are degenerate because both edges sit on the axis.
            let degenerate_bottom = r0 <= f32::EPSILON;
            let degenerate_top = r1 <= f32::EPSILON;
            if degenerate_bottom && degenerate_top {
                continue;
            }

            let base = mesh.positions.len() as u32;

            if degenerate_bottom {
                // A triangle at the axis: p00 and p01 coincide.
                mesh.positions.push(p00);
                mesh.normals.push(n0);
                mesh.uvs.push([j as f32 / columns as f32, 1.0]);
                mesh.positions.push(p10);
                mesh.normals.push(n0);
                mesh.uvs.push([j as f32 / columns as f32, 0.0]);
                mesh.positions.push(p11);
                mesh.normals.push(n1);
                mesh.uvs.push([(j + 1) as f32 / columns as f32, 0.0]);
                mesh.indices.extend_from_slice(&[base, base + 1, base + 2]);
            } else if degenerate_top {
                mesh.positions.push(p00);
                mesh.normals.push(n0);
                mesh.uvs.push([j as f32 / columns as f32, 1.0]);
                mesh.positions.push(p10);
                mesh.normals.push(n0);
                mesh.uvs.push([j as f32 / columns as f32, 0.0]);
                mesh.positions.push(p01);
                mesh.normals.push(n1);
                mesh.uvs.push([(j + 1) as f32 / columns as f32, 1.0]);
                mesh.indices.extend_from_slice(&[base, base + 1, base + 2]);
            } else {
                mesh.positions.push(p00);
                mesh.normals.push(n0);
                mesh.uvs.push([j as f32 / columns as f32, 1.0]);

                mesh.positions.push(p10);
                mesh.normals.push(n0);
                mesh.uvs.push([j as f32 / columns as f32, 0.0]);

                mesh.positions.push(p11);
                mesh.normals.push(n1);
                mesh.uvs.push([(j + 1) as f32 / columns as f32, 0.0]);

                mesh.positions.push(p01);
                mesh.normals.push(n1);
                mesh.uvs.push([(j + 1) as f32 / columns as f32, 1.0]);

                mesh.indices.extend_from_slice(&[
                    base,
                    base + 1,
                    base + 2,
                    base,
                    base + 2,
                    base + 3,
                ]);
            }
        }
    }

    mesh
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A square, counter-clockwise.
    fn square() -> Vec<[f32; 2]> {
        vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]
    }

    /// An L shape, which is concave — a triangle fan gets this wrong.
    fn l_shape() -> Vec<[f32; 2]> {
        vec![
            [0.0, 0.0],
            [2.0, 0.0],
            [2.0, 1.0],
            [1.0, 1.0],
            [1.0, 2.0],
            [0.0, 2.0],
        ]
    }

    fn face_normal(mesh: &Mesh, triangle: usize) -> [f32; 3] {
        let i = triangle * 3;
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

    /// Signed volume via the divergence theorem. Positive means the mesh faces
    /// outwards; a negative result is an inside-out solid.
    fn signed_volume(mesh: &Mesh) -> f32 {
        let mut total = 0.0;
        for triangle in 0..mesh.triangle_count() {
            let i = triangle * 3;
            let a = mesh.positions[mesh.indices[i] as usize];
            let b = mesh.positions[mesh.indices[i + 1] as usize];
            let c = mesh.positions[mesh.indices[i + 2] as usize];
            let cross = [
                b[1] * c[2] - b[2] * c[1],
                b[2] * c[0] - b[0] * c[2],
                b[0] * c[1] - b[1] * c[0],
            ];
            total += a[0] * cross[0] + a[1] * cross[1] + a[2] * cross[2];
        }
        total / 6.0
    }

    /// Every directed edge must appear exactly once, and its reverse too, for
    /// the surface to be closed and consistently wound.
    ///
    /// Vertices are welded by position first. These meshes duplicate vertices
    /// on purpose so flat faces keep hard normals, so comparing raw indices
    /// would report every shared edge as unmatched.
    fn assert_closed_manifold(mesh: &Mesh, label: &str) {
        use std::collections::HashMap;

        let mut welded: HashMap<[i64; 3], u32> = HashMap::new();
        let mut canonical = Vec::with_capacity(mesh.positions.len());
        for position in &mesh.positions {
            let key = [
                (position[0] * 10_000.0).round() as i64,
                (position[1] * 10_000.0).round() as i64,
                (position[2] * 10_000.0).round() as i64,
            ];
            let next = welded.len() as u32;
            canonical.push(*welded.entry(key).or_insert(next));
        }

        let mut edges: HashMap<(u32, u32), i32> = HashMap::new();
        for triangle in 0..mesh.triangle_count() {
            let i = triangle * 3;
            let v = [
                canonical[mesh.indices[i] as usize],
                canonical[mesh.indices[i + 1] as usize],
                canonical[mesh.indices[i + 2] as usize],
            ];
            // A triangle that collapses after welding has no edges to pair.
            if v[0] == v[1] || v[1] == v[2] || v[0] == v[2] {
                continue;
            }
            for k in 0..3 {
                *edges.entry((v[k], v[(k + 1) % 3])).or_insert(0) += 1;
            }
        }
        for (&(a, b), &count) in &edges {
            assert_eq!(
                count, 1,
                "{label}: directed edge {a}->{b} appears {count} times"
            );
            assert_eq!(
                edges.get(&(b, a)).copied().unwrap_or(0),
                1,
                "{label}: edge {a}<->{b} has no matching opposite"
            );
        }
    }

    #[test]
    fn signed_area_detects_winding() {
        assert!(signed_area(&square()) > 0.0);
        let mut reversed = square();
        reversed.reverse();
        assert!(signed_area(&reversed) < 0.0);
        assert_eq!(signed_area(&[[0.0, 0.0], [1.0, 1.0]]), 0.0);
    }

    #[test]
    fn triangulate_handles_a_convex_quad() {
        let triangles = triangulate(&square());
        assert_eq!(triangles.len(), 2);
        // Every index is used, and each triangle winds counter-clockwise.
        let mut seen: Vec<usize> = triangles.iter().flatten().copied().collect();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen, vec![0, 1, 2, 3]);
        for t in &triangles {
            assert!(cross2(square()[t[0]], square()[t[1]], square()[t[2]]) > 0.0);
        }
    }

    #[test]
    fn triangulate_handles_a_concave_polygon() {
        let shape = l_shape();
        let triangles = triangulate(&shape);
        // n - 2 triangles, as Euler's formula requires for a simple polygon.
        assert_eq!(triangles.len(), shape.len() - 2);

        // The total area of the triangles must equal the polygon's area, which
        // a fan triangulation of a concave shape would get wrong.
        let area: f32 = triangles
            .iter()
            .map(|t| cross2(shape[t[0]], shape[t[1]], shape[t[2]]) * 0.5)
            .sum();
        assert!(
            (area - signed_area(&shape)).abs() < 1e-4,
            "triangulated area {area} does not match polygon area {}",
            signed_area(&shape)
        );
    }

    #[test]
    fn triangulate_is_winding_independent() {
        let mut reversed = l_shape();
        reversed.reverse();
        let triangles = triangulate(&reversed);
        assert_eq!(triangles.len(), reversed.len() - 2);
        // Output is always counter-clockwise, whatever the input winding.
        for t in &triangles {
            assert!(cross2(reversed[t[0]], reversed[t[1]], reversed[t[2]]) > 0.0);
        }
    }

    #[test]
    fn triangulate_survives_degenerate_input() {
        assert!(triangulate(&[]).is_empty());
        assert!(triangulate(&[[0.0, 0.0], [1.0, 0.0]]).is_empty());
        // A fully collinear outline has no ears; it must terminate, not hang.
        let line = vec![[0.0, 0.0], [1.0, 0.0], [2.0, 0.0], [3.0, 0.0]];
        let _ = triangulate(&line);
    }

    #[test]
    fn extrude_a_square_makes_a_box() {
        let mesh = extrude(&square(), 1.0);
        assert_eq!(mesh.triangle_count(), 12, "4 side quads plus 2 caps");
        assert!(mesh.validate().is_empty(), "{:?}", mesh.validate());
        assert_closed_manifold(&mesh, "extruded square");

        // Unit square by unit depth is a unit cube.
        let volume = signed_volume(&mesh);
        assert!((volume - 1.0).abs() < 1e-4, "volume was {volume}");
    }

    #[test]
    fn extrude_is_centred_on_the_origin() {
        let mesh = extrude(&square(), 2.0);
        let (min, max) = mesh.bounds().unwrap();
        assert!((min[0] - 0.0).abs() < 1e-6);
        assert!((max[0] - 1.0).abs() < 1e-6);
        assert!((min[2] + 1.0).abs() < 1e-6, "bottom should be at -depth/2");
        assert!((max[2] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn extrude_a_concave_profile_is_still_solid() {
        let mesh = extrude(&l_shape(), 1.0);
        assert_closed_manifold(&mesh, "extruded L");
        // The L is three unit squares.
        let volume = signed_volume(&mesh);
        assert!((volume - 3.0).abs() < 1e-4, "volume was {volume}");
    }

    #[test]
    fn extrude_accepts_clockwise_profiles() {
        let mut reversed = square();
        reversed.reverse();
        let mesh = extrude(&reversed, 1.0);
        assert_closed_manifold(&mesh, "clockwise extrusion");
        let volume = signed_volume(&mesh);
        assert!(volume > 0.0, "a clockwise profile must still face outwards");
    }

    #[test]
    fn extrude_rejects_degenerate_input() {
        assert!(extrude(&square(), 0.0).is_empty());
        assert!(extrude(&[[0.0, 0.0], [1.0, 1.0]], 1.0).is_empty());
    }

    #[test]
    fn lathe_a_vertical_profile_makes_a_cylinder() {
        // Radius 1, from y=-1 to y=1, closed at both ends by zero-radius points.
        let profile = vec![[0.0, -1.0], [1.0, -1.0], [1.0, 1.0], [0.0, 1.0]];
        let mesh = lathe(&profile, 32, 360.0);

        assert!(mesh.validate().is_empty(), "{:?}", mesh.validate());
        assert_closed_manifold(&mesh, "lathed cylinder");

        // Volume of a unit-radius, height-2 cylinder is 2*pi.
        let volume = signed_volume(&mesh);
        let expected = std::f32::consts::PI * 2.0;
        assert!(
            (volume - expected).abs() / expected < 0.02,
            "volume {volume} is not close to {expected}"
        );
    }

    #[test]
    fn lathe_a_sphere_profile_makes_a_sphere() {
        // A half-circle profile revolved gives a sphere.
        let steps = 24;
        let mut profile = Vec::new();
        for i in 0..=steps {
            let t = std::f32::consts::PI * i as f32 / steps as f32;
            profile.push([t.sin(), -t.cos()]);
        }
        let mesh = lathe(&profile, 32, 360.0);
        assert_closed_manifold(&mesh, "lathed sphere");

        let volume = signed_volume(&mesh);
        let expected = 4.0 / 3.0 * std::f32::consts::PI;
        assert!(
            (volume - expected).abs() / expected < 0.05,
            "sphere volume {volume} is not close to {expected}"
        );
    }

    #[test]
    fn lathe_a_cone_tapers() {
        let profile = vec![[0.0, 0.0], [1.0, 0.0], [0.0, 2.0]];
        let mesh = lathe(&profile, 32, 360.0);
        assert_closed_manifold(&mesh, "lathed cone");
        // Cone volume is pi r^2 h / 3.
        let expected = std::f32::consts::PI * 2.0 / 3.0;
        let volume = signed_volume(&mesh);
        assert!(
            (volume - expected).abs() / expected < 0.03,
            "cone volume {volume} is not close to {expected}"
        );
    }

    #[test]
    fn lathe_an_open_profile_is_not_closed_but_still_faces_outwards() {
        // A tube: no zero-radius points, so both ends are open.
        let profile = vec![[1.0, -1.0], [1.0, 1.0]];
        let mesh = lathe(&profile, 16, 360.0);
        assert!(mesh.triangle_count() > 0);
        // Positive volume contribution means the outward direction is right.
        for triangle in 0..mesh.triangle_count() {
            let normal = normalize3(face_normal(&mesh, triangle));
            let centre = {
                let i = triangle * 3;
                let mut c = [0.0f32; 3];
                for k in 0..3 {
                    let p = mesh.positions[mesh.indices[i + k] as usize];
                    for axis in 0..3 {
                        c[axis] += p[axis] / 3.0;
                    }
                }
                c
            };
            // Radial part of the normal must point away from the Y axis.
            let radial = [centre[0], 0.0, centre[2]];
            let dot = normal[0] * radial[0] + normal[2] * radial[2];
            assert!(dot > 0.0, "triangle {triangle} faces inward (dot {dot})");
        }
    }

    #[test]
    fn lathe_arc_under_360_sweeps_less_than_a_full_turn() {
        let profile = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]];
        let full = lathe(&profile, 16, 360.0);
        let half = lathe(&profile, 16, 180.0);

        // Both use the same column count, so the difference is angular span,
        // not triangle count: a full turn reaches both sides of the axis.
        let (full_min, full_max) = full.bounds().unwrap();
        assert!(
            full_max[2] > 0.5 && full_min[2] < -0.5,
            "a full turn wraps the axis"
        );

        let (half_min, half_max) = half.bounds().unwrap();
        assert!(
            half_min[2] >= -1e-4,
            "a 180 degree revolve should stay on one side, got min z {}",
            half_min[2]
        );
        assert!(half_max[2] > 0.5);
    }

    #[test]
    fn lathe_raises_degenerate_segment_counts() {
        let profile = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        assert!(lathe(&profile, 0, 360.0).triangle_count() > 0);
        assert!(lathe(&profile, 1, 360.0).triangle_count() > 0);
        assert!(lathe(&[[1.0, 0.0]], 16, 360.0).is_empty());
    }

    #[test]
    fn lathed_output_re_exports_cleanly() {
        // The mesh must be structurally valid enough for glTF export.
        let profile = vec![[0.0, 0.0], [0.8, 0.0], [1.0, 0.5], [0.6, 1.2], [0.0, 1.5]];
        let mesh = lathe(&profile, 20, 360.0);
        assert!(mesh.validate().is_empty(), "{:?}", mesh.validate());
        assert_closed_manifold(&mesh, "vase");
    }
}
