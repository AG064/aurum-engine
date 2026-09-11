//! Aurum space simulation module.
//!
//! This module owns reusable space-flight state. It is deliberately independent
//! of Godot so it can be tested as a normal Rust library and reused by more
//! than one game. A game supplies input commands and mirrors snapshots into
//! its presentation layer.

use aurum_core::time::FixedTimestep;
use serde::{Deserialize, Serialize};
use std::ops::{Add, AddAssign, Mul, MulAssign, Neg, Sub, SubAssign};

pub const SECTOR_SIZE_M: f32 = 10_000.0;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    pub const ZERO: Self = Self::new(0.0, 0.0, 0.0);

    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    pub fn length_squared(self) -> f32 {
        self.x * self.x + self.y * self.y + self.z * self.z
    }

    pub fn length(self) -> f32 {
        self.length_squared().sqrt()
    }

    pub fn normalized(self) -> Self {
        let length = self.length();
        if length <= f32::EPSILON {
            Self::ZERO
        } else {
            self * (1.0 / length)
        }
    }

    pub fn clamp_length(self, max_length: f32) -> Self {
        if max_length <= 0.0 {
            return Self::ZERO;
        }
        let length = self.length();
        if length > max_length {
            self * (max_length / length)
        } else {
            self
        }
    }

    pub fn dot(self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    pub fn cross(self, other: Self) -> Self {
        Self::new(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x,
        )
    }

    pub fn lerp(self, target: Self, amount: f32) -> Self {
        self + (target - self) * amount.clamp(0.0, 1.0)
    }
}

impl Add for Vec3 {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self::new(self.x + rhs.x, self.y + rhs.y, self.z + rhs.z)
    }
}

impl AddAssign for Vec3 {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl Sub for Vec3 {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self::new(self.x - rhs.x, self.y - rhs.y, self.z - rhs.z)
    }
}

impl SubAssign for Vec3 {
    fn sub_assign(&mut self, rhs: Self) {
        *self = *self - rhs;
    }
}

impl Mul<f32> for Vec3 {
    type Output = Self;

    fn mul(self, rhs: f32) -> Self::Output {
        Self::new(self.x * rhs, self.y * rhs, self.z * rhs)
    }
}

impl MulAssign<f32> for Vec3 {
    fn mul_assign(&mut self, rhs: f32) {
        *self = *self * rhs;
    }
}

impl Neg for Vec3 {
    type Output = Self;

    fn neg(self) -> Self::Output {
        Self::new(-self.x, -self.y, -self.z)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Quat {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

impl Quat {
    pub const IDENTITY: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        w: 1.0,
    };

    pub fn normalized(self) -> Self {
        let length = (self.x * self.x + self.y * self.y + self.z * self.z + self.w * self.w).sqrt();
        if length <= f32::EPSILON {
            Self::IDENTITY
        } else {
            Self {
                x: self.x / length,
                y: self.y / length,
                z: self.z / length,
                w: self.w / length,
            }
        }
    }

    pub fn conjugate(self) -> Self {
        Self {
            x: -self.x,
            y: -self.y,
            z: -self.z,
            w: self.w,
        }
    }

    pub fn from_axis_angle(axis: Vec3, angle: f32) -> Self {
        let half = angle * 0.5;
        let sine = half.sin();
        let axis = axis.normalized();
        Self {
            x: axis.x * sine,
            y: axis.y * sine,
            z: axis.z * sine,
            w: half.cos(),
        }
    }

    pub fn from_local_angular_velocity(angular_velocity: Vec3, dt: f32) -> Self {
        let angle = angular_velocity.length() * dt;
        if angle <= f32::EPSILON {
            Self::IDENTITY
        } else {
            Self::from_axis_angle(angular_velocity, angle)
        }
    }

    pub fn rotate_vector(self, vector: Vec3) -> Vec3 {
        let q_vector = Vec3::new(self.x, self.y, self.z);
        let twice_cross = q_vector.cross(vector) * 2.0;
        vector + twice_cross * self.w + q_vector.cross(twice_cross)
    }

    pub fn inverse_rotate_vector(self, vector: Vec3) -> Vec3 {
        self.conjugate().rotate_vector(vector)
    }

    pub fn forward(self) -> Vec3 {
        self.rotate_vector(Vec3::new(0.0, 0.0, -1.0))
    }
}

impl Default for Quat {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Mul for Quat {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self::Output {
        Self {
            w: self.w * rhs.w - self.x * rhs.x - self.y * rhs.y - self.z * rhs.z,
            x: self.w * rhs.x + self.x * rhs.w + self.y * rhs.z - self.z * rhs.y,
            y: self.w * rhs.y - self.x * rhs.z + self.y * rhs.w + self.z * rhs.x,
            z: self.w * rhs.z + self.x * rhs.y - self.y * rhs.x + self.z * rhs.w,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SectorCoord {
    pub x: i64,
    pub y: i64,
    pub z: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct UniversePosition {
    pub sector: SectorCoord,
    pub local: Vec3,
}

impl UniversePosition {
    pub fn origin() -> Self {
        Self::default()
    }

    /// Translate in universe space and return the sector movement caused by
    /// normalization. Local coordinates remain inside one sector.
    pub fn translate(&mut self, delta: Vec3) -> SectorCoord {
        self.local += delta;
        let half = SECTOR_SIZE_M * 0.5;
        // Field initializers run in written order, matching the per-axis
        // normalization this replaced; each axis is independent.
        SectorCoord {
            x: normalize_axis(&mut self.local.x, &mut self.sector.x, half),
            y: normalize_axis(&mut self.local.y, &mut self.sector.y, half),
            z: normalize_axis(&mut self.local.z, &mut self.sector.z, half),
        }
    }
}

fn normalize_axis(value: &mut f32, sector: &mut i64, half: f32) -> i64 {
    let offset = ((*value + half) / SECTOR_SIZE_M).floor() as i64;
    if offset != 0 {
        *value -= offset as f32 * SECTOR_SIZE_M;
        *sector += offset;
    }
    offset
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FlightConfig {
    pub mass_kg: f32,
    pub thrust_n: f32,
    pub rotation_accel_rad_s2: f32,
    pub max_rotation_rate_rad_s: f32,
    pub max_speed_mps: f32,
    pub boost_multiplier: f32,
    pub boost_fuel_per_s: f32,
    pub boost_heat_per_s: f32,
    pub fuel_capacity: f32,
    pub heat_capacity: f32,
    pub heat_dissipation_per_s: f32,
    pub flight_assist_damping: f32,
    pub dampen_strength: f32,
    pub shield_capacity: f32,
    pub hull_capacity: f32,
}

impl Default for FlightConfig {
    fn default() -> Self {
        Self {
            mass_kg: 1_500.0,
            thrust_n: 180_000.0,
            rotation_accel_rad_s2: 4.0,
            max_rotation_rate_rad_s: 2.5,
            max_speed_mps: 120.0,
            boost_multiplier: 2.5,
            boost_fuel_per_s: 1.0,
            boost_heat_per_s: 12.0,
            fuel_capacity: 100.0,
            heat_capacity: 100.0,
            heat_dissipation_per_s: 8.0,
            flight_assist_damping: 1.8,
            dampen_strength: 5.0,
            shield_capacity: 100.0,
            hull_capacity: 100.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct FlightInput {
    pub pitch: f32,
    pub yaw: f32,
    pub roll: f32,
    pub thrust_forward: f32,
    pub thrust_lateral: f32,
    pub thrust_vertical: f32,
    pub boost: bool,
    pub dampen: bool,
    pub flight_assist: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FlightState {
    pub position: UniversePosition,
    pub orientation: Quat,
    pub velocity: Vec3,
    pub angular_velocity: Vec3,
    pub fuel: f32,
    pub heat: f32,
    pub shield: f32,
    pub hull: f32,
    pub boost_active: bool,
    pub docked: bool,
    pub tick: u64,
}

impl FlightState {
    pub fn from_config(config: FlightConfig) -> Self {
        Self {
            position: UniversePosition::origin(),
            orientation: Quat::IDENTITY,
            velocity: Vec3::ZERO,
            angular_velocity: Vec3::ZERO,
            fuel: config.fuel_capacity,
            heat: 0.0,
            shield: config.shield_capacity,
            hull: config.hull_capacity,
            boost_active: false,
            docked: false,
            tick: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SpaceSnapshot {
    pub sector: SectorCoord,
    pub local_position: Vec3,
    pub orientation: Quat,
    pub velocity: Vec3,
    pub angular_velocity: Vec3,
    pub fuel: f32,
    pub heat: f32,
    pub shield: f32,
    pub hull: f32,
    pub boost_active: bool,
    pub docked: bool,
    pub tick: u64,
}

impl From<FlightState> for SpaceSnapshot {
    fn from(state: FlightState) -> Self {
        Self {
            sector: state.position.sector,
            local_position: state.position.local,
            orientation: state.orientation,
            velocity: state.velocity,
            angular_velocity: state.angular_velocity,
            fuel: state.fuel,
            heat: state.heat,
            shield: state.shield,
            hull: state.hull,
            boost_active: state.boost_active,
            docked: state.docked,
            tick: state.tick,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpaceSimulation {
    pub config: FlightConfig,
    pub input: FlightInput,
    pub state: FlightState,
}

impl Default for SpaceSimulation {
    fn default() -> Self {
        Self::new(FlightConfig::default())
    }
}

impl SpaceSimulation {
    pub fn new(config: FlightConfig) -> Self {
        Self {
            config,
            input: FlightInput::default(),
            state: FlightState::from_config(config),
        }
    }

    pub fn configure(&mut self, config: FlightConfig) {
        self.config = config;
        self.state.fuel = self.state.fuel.min(config.fuel_capacity).max(0.0);
        self.state.heat = self.state.heat.min(config.heat_capacity).max(0.0);
        self.state.shield = self.state.shield.min(config.shield_capacity).max(0.0);
        self.state.hull = self.state.hull.min(config.hull_capacity).max(0.0);
    }

    pub fn reset(&mut self) {
        self.input = FlightInput::default();
        self.state = FlightState::from_config(self.config);
    }

    pub fn set_input(&mut self, input: FlightInput) {
        self.input = input;
    }

    pub fn set_docked(&mut self, docked: bool) {
        self.state.docked = docked;
        if docked {
            self.state.velocity = Vec3::ZERO;
            self.state.angular_velocity = Vec3::ZERO;
            self.state.boost_active = false;
        }
    }

    pub fn set_transform(&mut self, position: Vec3, orientation: Quat) {
        self.state.position.local = position;
        self.state.position.translate(Vec3::ZERO);
        self.state.orientation = orientation.normalized();
    }

    pub fn set_status(&mut self, fuel: f32, heat: f32, shield: f32, hull: f32) {
        self.state.fuel = fuel.clamp(0.0, self.config.fuel_capacity.max(0.0));
        self.state.heat = heat.clamp(0.0, self.config.heat_capacity.max(0.0));
        self.state.shield = shield.clamp(0.0, self.config.shield_capacity.max(0.0));
        self.state.hull = hull.clamp(0.0, self.config.hull_capacity.max(0.0));
        self.state.boost_active = false;
    }

    pub fn consume_fuel(&mut self, amount: f32) -> bool {
        let amount = amount.max(0.0);
        if self.state.fuel + f32::EPSILON < amount {
            return false;
        }
        self.state.fuel -= amount;
        self.state.boost_active = false;
        true
    }

    pub fn stop_motion(&mut self) {
        self.state.velocity = Vec3::ZERO;
        self.state.angular_velocity = Vec3::ZERO;
        self.state.boost_active = false;
    }

    pub fn refuel_full(&mut self) {
        self.state.fuel = self.config.fuel_capacity;
    }

    pub fn apply_damage(&mut self, amount: f32) {
        if amount <= 0.0 || self.state.docked {
            return;
        }
        let mut remaining = amount;
        let absorbed = self.state.shield.min(remaining);
        self.state.shield -= absorbed;
        remaining -= absorbed;
        self.state.hull = (self.state.hull - remaining).max(0.0);
    }

    pub fn repair_full(&mut self) {
        self.state.shield = self.config.shield_capacity;
        self.state.hull = self.config.hull_capacity;
        self.state.heat = 0.0;
    }

    pub fn step(&mut self, dt: f32) {
        let dt = dt.clamp(0.0, 0.1);
        if dt <= 0.0 {
            return;
        }
        self.state.tick = self.state.tick.saturating_add(1);
        if self.state.docked || self.state.hull <= 0.0 {
            self.stop_motion();
            return;
        }

        let config = self.config;
        let input = self.input;
        let boost = input.boost && self.state.fuel > 0.0;
        self.state.boost_active = boost;

        let local_velocity = self.state.orientation.inverse_rotate_vector(self.state.velocity);
        let local_thrust = Vec3::new(
            input.thrust_lateral,
            input.thrust_vertical,
            -input.thrust_forward,
        )
        .clamp_length(1.0);
        let thrust_multiplier = if boost { config.boost_multiplier.max(1.0) } else { 1.0 };
        let acceleration = config.thrust_n.max(0.0) / config.mass_kg.max(1.0) * thrust_multiplier;
        let mut local_acceleration = local_thrust * acceleration;

        if input.flight_assist {
            local_acceleration -= local_velocity * config.flight_assist_damping.max(0.0);
        }
        if input.dampen {
            local_acceleration -= local_velocity * config.dampen_strength.max(0.0);
        }

        let world_acceleration = self.state.orientation.rotate_vector(local_acceleration);
        self.state.velocity += world_acceleration * dt;
        self.state.velocity = self.state.velocity.clamp_length(config.max_speed_mps.max(0.0));
        self.state.position.translate(self.state.velocity * dt);

        let desired_angular_velocity = Vec3::new(
            input.pitch,
            -input.yaw,
            input.roll,
        )
        .clamp_length(1.0)
            * config.max_rotation_rate_rad_s.max(0.0);
        let angular_delta = desired_angular_velocity - self.state.angular_velocity;
        let max_delta = config.rotation_accel_rad_s2.max(0.0) * dt;
        if angular_delta.length() > max_delta && max_delta > 0.0 {
            self.state.angular_velocity += angular_delta.normalized() * max_delta;
        } else if input.flight_assist || desired_angular_velocity.length_squared() > 0.0 {
            self.state.angular_velocity = desired_angular_velocity;
        }
        if input.flight_assist && desired_angular_velocity.length_squared() <= f32::EPSILON {
            self.state.angular_velocity = self.state.angular_velocity.lerp(Vec3::ZERO, (config.flight_assist_damping * dt).clamp(0.0, 1.0));
        }
        let rotation_delta = Quat::from_local_angular_velocity(self.state.angular_velocity, dt);
        self.state.orientation = (self.state.orientation * rotation_delta).normalized();

        if boost {
            self.state.fuel = (self.state.fuel - config.boost_fuel_per_s.max(0.0) * dt).max(0.0);
            self.state.heat += config.boost_heat_per_s.max(0.0) * dt;
        }
        self.state.heat = (self.state.heat - config.heat_dissipation_per_s.max(0.0) * dt).max(0.0);
        if self.state.heat > config.heat_capacity.max(0.0) {
            let overheat = self.state.heat - config.heat_capacity;
            self.state.hull = (self.state.hull - overheat * 0.02 * dt).max(0.0);
        }
    }

    pub fn snapshot(&self) -> SpaceSnapshot {
        self.state.into()
    }
}

/// Small adapter that lets a host feed real frame deltas into a fixed space
/// simulation without putting frame-rate dependence into flight code.
#[derive(Default)]
pub struct SpaceClock {
    fixed: FixedTimestep,
}

impl SpaceClock {
    pub fn reset(&mut self) {
        self.fixed = FixedTimestep::default();
    }

    pub fn advance(&mut self, real_dt: f32, simulation: &mut SpaceSimulation) -> u32 {
        self.fixed.advance(real_dt, |dt| simulation.step(dt))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forward_thrust_accelerates_and_preserves_velocity() {
        let mut simulation = SpaceSimulation::new(FlightConfig {
            flight_assist_damping: 0.0,
            max_speed_mps: 1_000.0,
            ..FlightConfig::default()
        });
        simulation.set_input(FlightInput {
            thrust_forward: 1.0,
            flight_assist: false,
            ..FlightInput::default()
        });
        simulation.step(1.0 / 60.0);
        assert!(simulation.state.velocity.z < 0.0);
        let speed_after_thrust = simulation.state.velocity.length();
        simulation.set_input(FlightInput::default());
        simulation.step(1.0 / 60.0);
        assert!((simulation.state.velocity.length() - speed_after_thrust).abs() < 1e-4);
    }

    #[test]
    fn flight_assist_damps_velocity() {
        let mut simulation = SpaceSimulation::new(FlightConfig {
            flight_assist_damping: 5.0,
            max_speed_mps: 1_000.0,
            ..FlightConfig::default()
        });
        simulation.state.velocity = Vec3::new(100.0, 0.0, 0.0);
        simulation.set_input(FlightInput {
            flight_assist: true,
            ..FlightInput::default()
        });
        simulation.step(1.0 / 60.0);
        assert!(simulation.state.velocity.x.abs() < 100.0);
    }

    #[test]
    fn sector_coordinates_remain_bounded() {
        let mut position = UniversePosition::origin();
        let moved = position.translate(Vec3::new(SECTOR_SIZE_M * 1.25, 0.0, -SECTOR_SIZE_M * 2.25));
        assert_eq!(moved.x, 1);
        assert_eq!(moved.z, -2);
        assert!(position.local.x.abs() <= SECTOR_SIZE_M * 0.5);
        assert!(position.local.z.abs() <= SECTOR_SIZE_M * 0.5);
        assert_eq!(position.sector.x, 1);
        assert_eq!(position.sector.z, -2);
    }

    #[test]
    fn fixed_clock_runs_multiple_ticks() {
        let mut clock = SpaceClock::default();
        let mut simulation = SpaceSimulation::default();
        let ticks = clock.advance(1.0 / 30.0, &mut simulation);
        assert_eq!(ticks, 2);
        assert_eq!(simulation.state.tick, 2);
    }

    #[test]
    fn status_and_motion_boundaries_are_authoritative() {
        let mut simulation = SpaceSimulation::default();
        simulation.state.velocity = Vec3::new(20.0, 0.0, 0.0);
        simulation.state.angular_velocity = Vec3::new(1.0, 0.0, 0.0);
        simulation.state.boost_active = true;
        simulation.set_status(-1.0, 200.0, 40.0, 75.0);
        assert_eq!(simulation.state.fuel, 0.0);
        assert_eq!(simulation.state.heat, simulation.config.heat_capacity);
        assert_eq!(simulation.state.shield, 40.0);
        assert_eq!(simulation.state.hull, 75.0);
        assert!(!simulation.state.boost_active);

        simulation.state.fuel = 5.0;
        assert!(!simulation.consume_fuel(6.0));
        assert_eq!(simulation.state.fuel, 5.0);
        assert!(simulation.consume_fuel(2.0));
        assert_eq!(simulation.state.fuel, 3.0);

        simulation.stop_motion();
        assert_eq!(simulation.state.velocity, Vec3::ZERO);
        assert_eq!(simulation.state.angular_velocity, Vec3::ZERO);
    }
}
