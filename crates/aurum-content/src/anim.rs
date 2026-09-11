//! Animation: tracks of keyframes targeting scene nodes.
//!
//! The model is deliberately glTF-shaped — a track targets one node, one
//! property, and one interpolation mode, carrying parallel time and value
//! arrays. That keeps export a direct translation rather than a conversion.
//!
//! Rotation values are normalized to unit quaternions on insert, because glTF
//! requires it and a denormalized quaternion produces a subtly wrong pose
//! rather than an error.

use serde::{Deserialize, Serialize};

/// How values are interpolated between keyframes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Interpolation {
    /// Straight lines between keys.
    Linear,
    /// Hold each value until the next key.
    Step,
}

impl Interpolation {
    /// The glTF spelling.
    pub fn as_gltf(self) -> &'static str {
        match self {
            Self::Linear => "LINEAR",
            Self::Step => "STEP",
        }
    }
}

/// Which property of a node a track drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrackPath {
    Translation,
    Rotation,
    Scale,
}

impl TrackPath {
    /// The glTF spelling.
    pub fn as_gltf(self) -> &'static str {
        match self {
            Self::Translation => "translation",
            Self::Rotation => "rotation",
            Self::Scale => "scale",
        }
    }

    /// How many floats each keyframe carries.
    pub fn components(self) -> usize {
        match self {
            Self::Translation | Self::Scale => 3,
            Self::Rotation => 4,
        }
    }
}

/// One animated property of one node.
#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    pub node: usize,
    pub path: TrackPath,
    pub interpolation: Interpolation,
    /// Keyframe times in seconds, ascending.
    pub times: Vec<f32>,
    /// Flattened values: `times.len() * path.components()` floats.
    pub values: Vec<f32>,
}

impl Track {
    pub fn new(node: usize, path: TrackPath) -> Self {
        Self {
            node,
            path,
            interpolation: Interpolation::Linear,
            times: Vec::new(),
            values: Vec::new(),
        }
    }

    pub fn with_interpolation(mut self, interpolation: Interpolation) -> Self {
        self.interpolation = interpolation;
        self
    }

    /// Number of components each keyframe carries.
    pub fn components(&self) -> usize {
        self.path.components()
    }

    /// How many complete keyframes the value array holds.
    pub fn value_count(&self) -> usize {
        if self.components() == 0 {
            0
        } else {
            self.values.len() / self.components()
        }
    }

    pub fn is_empty(&self) -> bool {
        self.times.is_empty()
    }

    /// Append a keyframe.
    ///
    /// Returns `false` when `value` has the wrong number of components or
    /// contains a non-finite number. Rotation values are normalized in place.
    pub fn keyframe(&mut self, time: f32, value: &[f32]) -> bool {
        if !time.is_finite() || time < 0.0 {
            return false;
        }
        if value.len() != self.components() || value.iter().any(|v| !v.is_finite()) {
            return false;
        }
        self.times.push(time);
        if self.path == TrackPath::Rotation {
            self.values.extend_from_slice(&normalize_quat(value));
        } else {
            self.values.extend_from_slice(value);
        }
        true
    }

    /// Time of the last keyframe, or `0.0` when empty.
    pub fn duration(&self) -> f32 {
        self.times.last().copied().unwrap_or(0.0)
    }

    /// The keyframe value at `index`, as a slice.
    pub fn value_at(&self, index: usize) -> Option<&[f32]> {
        let n = self.components();
        let start = index.checked_mul(n)?;
        self.values.get(start..start + n)
    }

    /// Sort keyframes by time, keeping values aligned.
    ///
    /// glTF requires ascending times; authors naturally append out of order.
    pub fn sort_by_time(&mut self) {
        let n = self.components();
        if n == 0 {
            return;
        }
        let mut keyframes: Vec<(f32, Vec<f32>)> = self
            .times
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let start = i * n;
                (*t, self.values[start..start + n].to_vec())
            })
            .collect();
        keyframes.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

        self.times = keyframes.iter().map(|(t, _)| *t).collect();
        self.values = keyframes.into_iter().flat_map(|(_, v)| v).collect();
    }

    /// Whether every keyframe is structurally valid for export.
    pub fn is_valid(&self) -> bool {
        self.times.len() == self.value_count()
            && self.times.iter().all(|t| t.is_finite() && *t >= 0.0)
            && self.times.windows(2).all(|w| w[0] <= w[1])
            && self.values.iter().all(|v| v.is_finite())
    }
}

/// A named group of tracks — one glTF animation.
#[derive(Debug, Clone, PartialEq)]
pub struct Animation {
    pub name: String,
    pub tracks: Vec<Track>,
}

impl Animation {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            tracks: Vec::new(),
        }
    }

    pub fn add_track(&mut self, track: Track) -> usize {
        self.tracks.push(track);
        self.tracks.len() - 1
    }

    /// Builder: add a translation track from parallel arrays.
    pub fn translate(
        mut self,
        node: usize,
        times: &[f32],
        values: &[[f32; 3]],
        interpolation: Interpolation,
    ) -> Self {
        let mut track = Track::new(node, TrackPath::Translation).with_interpolation(interpolation);
        for (t, v) in times.iter().zip(values) {
            track.keyframe(*t, v);
        }
        self.add_track(track);
        self
    }

    /// Builder: add a rotation track. Values are `[x, y, z, w]` quaternions.
    pub fn rotate(
        mut self,
        node: usize,
        times: &[f32],
        values: &[[f32; 4]],
        interpolation: Interpolation,
    ) -> Self {
        let mut track = Track::new(node, TrackPath::Rotation).with_interpolation(interpolation);
        for (t, v) in times.iter().zip(values) {
            track.keyframe(*t, v);
        }
        self.add_track(track);
        self
    }

    /// Builder: add a scale track from parallel arrays.
    pub fn scale(
        mut self,
        node: usize,
        times: &[f32],
        values: &[[f32; 3]],
        interpolation: Interpolation,
    ) -> Self {
        let mut track = Track::new(node, TrackPath::Scale).with_interpolation(interpolation);
        for (t, v) in times.iter().zip(values) {
            track.keyframe(*t, v);
        }
        self.add_track(track);
        self
    }

    /// A spin about an axis — the single most common animation, so it gets a
    /// builder rather than making every caller spell out quaternions.
    pub fn spin(self, node: usize, axis: [f32; 3], turns: f32, duration: f32, keys: usize) -> Self {
        let keys = keys.max(2);
        let mut times = Vec::with_capacity(keys);
        let mut values = Vec::with_capacity(keys);
        for i in 0..keys {
            let t = i as f32 / (keys - 1) as f32;
            times.push(t * duration);
            values.push(quat_from_axis_angle(
                axis,
                t * turns * std::f32::consts::TAU,
            ));
        }
        self.rotate(node, &times, &values, Interpolation::Linear)
    }

    /// The last keyframe time across every track.
    pub fn duration(&self) -> f32 {
        self.tracks
            .iter()
            .map(Track::duration)
            .fold(0.0f32, f32::max)
    }

    /// The first track driving `path` on `node`.
    pub fn track_for(&self, node: usize, path: TrackPath) -> Option<&Track> {
        self.tracks
            .iter()
            .find(|t| t.node == node && t.path == path)
    }

    pub fn is_empty(&self) -> bool {
        self.tracks.iter().all(Track::is_empty)
    }

    /// Sort every track and report whether they are all exportable.
    pub fn is_valid(&self) -> bool {
        self.tracks.iter().all(Track::is_valid)
    }
}

/// A unit quaternion from an axis and angle.
pub fn quat_from_axis_angle(axis: [f32; 3], radians: f32) -> [f32; 4] {
    let len = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt();
    if len <= f32::EPSILON {
        return [0.0, 0.0, 0.0, 1.0];
    }
    let half = radians * 0.5;
    let s = half.sin() / len;
    [axis[0] * s, axis[1] * s, axis[2] * s, half.cos()]
}

/// Normalize a quaternion, falling back to identity when degenerate.
pub fn normalize_quat(q: &[f32]) -> [f32; 4] {
    if q.len() < 4 {
        return [0.0, 0.0, 0.0, 1.0];
    }
    let len = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    if len <= f32::EPSILON {
        return [0.0, 0.0, 0.0, 1.0];
    }
    [q[0] / len, q[1] / len, q[2] / len, q[3] / len]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyframes_accumulate_with_matching_lengths() {
        let mut track = Track::new(0, TrackPath::Translation);
        assert!(track.keyframe(0.0, &[0.0, 0.0, 0.0]));
        assert!(track.keyframe(1.0, &[1.0, 0.0, 0.0]));
        assert_eq!(track.value_count(), 2);
        assert_eq!(track.times.len(), 2);
        assert_eq!(track.duration(), 1.0);
        assert!(track.is_valid());
    }

    #[test]
    fn keyframe_rejects_wrong_component_counts() {
        let mut track = Track::new(0, TrackPath::Translation);
        assert!(!track.keyframe(0.0, &[0.0, 0.0]), "2 components for a VEC3");
        assert!(!track.keyframe(0.0, &[0.0, 0.0, 0.0, 0.0]));
        assert!(track.is_empty());

        let mut rotation = Track::new(0, TrackPath::Rotation);
        assert!(!rotation.keyframe(0.0, &[0.0, 0.0, 0.0]), "3 for a VEC4");
        assert!(rotation.keyframe(0.0, &[0.0, 0.0, 0.0, 1.0]));
    }

    #[test]
    fn keyframe_rejects_bad_times_and_values() {
        let mut track = Track::new(0, TrackPath::Scale);
        assert!(!track.keyframe(-1.0, &[1.0, 1.0, 1.0]), "negative time");
        assert!(!track.keyframe(f32::NAN, &[1.0, 1.0, 1.0]), "NaN time");
        assert!(!track.keyframe(0.0, &[f32::NAN, 1.0, 1.0]), "NaN value");
        assert!(!track.keyframe(0.0, &[f32::INFINITY, 1.0, 1.0]));
        assert!(track.is_empty());
    }

    #[test]
    fn rotation_keyframes_are_normalized_on_insert() {
        let mut track = Track::new(0, TrackPath::Rotation);
        assert!(track.keyframe(0.0, &[0.0, 0.0, 0.0, 5.0]));
        let v = track.value_at(0).unwrap();
        let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2] + v[3] * v[3]).sqrt();
        assert!(
            (len - 1.0).abs() < 1e-5,
            "quaternion not unit length: {len}"
        );
    }

    #[test]
    fn degenerate_quaternion_becomes_identity() {
        let mut track = Track::new(0, TrackPath::Rotation);
        assert!(track.keyframe(0.0, &[0.0, 0.0, 0.0, 0.0]));
        assert_eq!(track.value_at(0).unwrap(), &[0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn sort_by_time_keeps_values_aligned() {
        let mut track = Track::new(0, TrackPath::Translation);
        track.keyframe(2.0, &[2.0, 0.0, 0.0]);
        track.keyframe(0.0, &[0.0, 0.0, 0.0]);
        track.keyframe(1.0, &[1.0, 0.0, 0.0]);
        assert!(!track.is_valid(), "out of order before sorting");

        track.sort_by_time();
        assert_eq!(track.times, vec![0.0, 1.0, 2.0]);
        assert_eq!(track.value_at(1).unwrap(), &[1.0, 0.0, 0.0]);
        assert_eq!(track.value_at(2).unwrap(), &[2.0, 0.0, 0.0]);
        assert!(track.is_valid());
    }

    #[test]
    fn spin_produces_a_full_rotation_track() {
        let animation = Animation::new("Spin").spin(3, [0.0, 1.0, 0.0], 1.0, 2.0, 5);
        let track = animation.track_for(3, TrackPath::Rotation).unwrap();
        assert_eq!(track.times.len(), 5);
        assert_eq!(track.duration(), 2.0);
        assert!(animation.is_valid());

        // A full turn returns to the start orientation.
        let first = track.value_at(0).unwrap();
        let last = track.value_at(4).unwrap();
        for i in 0..4 {
            assert!((first[i] - last[i]).abs() < 1e-4 || (first[i] + last[i]).abs() < 1e-4);
        }
    }

    #[test]
    fn builders_attach_tracks_to_the_right_node_and_path() {
        let animation = Animation::new("Move")
            .translate(
                1,
                &[0.0, 1.0],
                &[[0.0; 3], [1.0, 0.0, 0.0]],
                Interpolation::Linear,
            )
            .scale(2, &[0.0, 1.0], &[[1.0; 3], [2.0; 3]], Interpolation::Step);

        assert_eq!(animation.tracks.len(), 2);
        assert_eq!(animation.duration(), 1.0);
        assert!(animation.track_for(1, TrackPath::Translation).is_some());
        assert!(animation.track_for(2, TrackPath::Scale).is_some());
        assert!(animation.track_for(1, TrackPath::Scale).is_none());
        assert_eq!(
            animation
                .track_for(2, TrackPath::Scale)
                .unwrap()
                .interpolation,
            Interpolation::Step
        );
    }

    #[test]
    fn gltf_spellings_match_the_spec() {
        assert_eq!(Interpolation::Linear.as_gltf(), "LINEAR");
        assert_eq!(Interpolation::Step.as_gltf(), "STEP");
        assert_eq!(TrackPath::Translation.as_gltf(), "translation");
        assert_eq!(TrackPath::Rotation.as_gltf(), "rotation");
        assert_eq!(TrackPath::Scale.as_gltf(), "scale");
        assert_eq!(TrackPath::Rotation.components(), 4);
        assert_eq!(TrackPath::Scale.components(), 3);
    }

    #[test]
    fn empty_animation_is_reported_empty_but_valid() {
        let animation = Animation::new("Nothing");
        assert!(animation.is_empty());
        assert!(animation.is_valid());
        assert_eq!(animation.duration(), 0.0);
    }

    #[test]
    fn quat_from_axis_angle_matches_known_rotations() {
        let q = quat_from_axis_angle([0.0, 1.0, 0.0], std::f32::consts::FRAC_PI_2);
        // 90° about Y: w = cos(45°) = 0.7071, y = sin(45°) = 0.7071
        assert!((q[3] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-5);
        assert!((q[1] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-5);

        assert_eq!(
            quat_from_axis_angle([0.0, 0.0, 0.0], 1.0),
            [0.0, 0.0, 0.0, 1.0]
        );
    }
}
