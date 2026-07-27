//! Time, fixed timestep, and time scale.
//!
//! Game time is split into two notions:
//!
//! - **Real time** (wall clock) — drives rendering and input.
//! - **Game time** (scaled real time) — drives gameplay: physics, AI,
//!   animations. The GDScript side exposes a slider that maps to
//!   `TimeScale` (1.0 = normal, 2.0 = fast, 0.5 = slow, 0.0 = paused).
//!
//! - **Fixed timestep** — for systems that need stable physics (e.g. at 60 Hz
//!   regardless of frame rate). Use `FixedTimestep::advance(real_dt)` to ask
//!   for zero or more `1.0/60` ticks; each call gets the same dt.

/// Time scale, applied to game time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimeScale(pub f32);

impl Default for TimeScale {
    fn default() -> Self {
        Self(1.0)
    }
}

impl TimeScale {
    pub fn new(scale: f32) -> Self {
        Self(scale.max(0.0).min(100.0))
    }
    pub fn get(self) -> f32 {
        self.0
    }
    pub fn set(&mut self, scale: f32) {
        self.0 = scale.max(0.0).min(100.0);
    }
    pub fn paused(self) -> bool {
        self.0 == 0.0
    }
}

/// Tracks fixed-timestep accumulation. Default step is 1/60.
pub struct FixedTimestep {
    step: f32,
    accumulator: f32,
    max_steps_per_frame: u32,
}

impl Default for FixedTimestep {
    fn default() -> Self {
        Self {
            step: 1.0 / 60.0,
            accumulator: 0.0,
            max_steps_per_frame: 5,
        }
    }
}

impl FixedTimestep {
    pub fn new(steps_per_second: f32) -> Self {
        Self {
            step: 1.0 / steps_per_second.max(1.0),
            accumulator: 0.0,
            max_steps_per_frame: 5,
        }
    }

    /// Step the accumulator. Calls `on_step` for each fixed tick that fits.
    /// Returns the number of ticks that ran (0 to `max_steps_per_frame`).
    pub fn advance<F: FnMut(f32)>(&mut self, real_dt: f32, mut on_step: F) -> u32 {
        self.accumulator += real_dt;
        let mut ticks = 0u32;
        while self.accumulator >= self.step && ticks < self.max_steps_per_frame {
            on_step(self.step);
            self.accumulator -= self.step;
            ticks += 1;
        }
        // Drop excess if the frame was so slow we'd never catch up. Better
        // to slow down than to spiral.
        if self.accumulator > self.step {
            self.accumulator = 0.0;
        }
        ticks
    }

    pub fn step(&self) -> f32 {
        self.step
    }
    pub fn set_step(&mut self, hz: f32) {
        self.step = 1.0 / hz.max(1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_step_runs_correctly() {
        let mut ft = FixedTimestep::new(60.0);
        let mut ticks = 0;
        // 80ms at 60Hz → ~5 ticks, comfortably under the per-frame cap
        let _ = ft.advance(0.08, |_dt| ticks += 1);
        assert_eq!(ticks, 4);
    }

    #[test]
    fn cap_prevents_spiral() {
        let mut ft = FixedTimestep::new(60.0);
        let mut ticks = 0;
        // 1 second in one frame → capped at 5
        let _ = ft.advance(1.0, |_dt| ticks += 1);
        assert_eq!(ticks, 5);
    }

    #[test]
    fn time_scale_clamps() {
        let mut t = TimeScale::new(2.0);
        t.set(999.0);
        assert_eq!(t.get(), 100.0);
        t.set(-1.0);
        assert_eq!(t.get(), 0.0);
    }
}
