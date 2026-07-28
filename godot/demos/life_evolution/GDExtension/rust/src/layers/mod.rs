//! Higher-level simulation layers.
//!
//! These modules implement cellular and organism-level simulations.
//! They are built on top of the atomic/molecular layer.

pub mod cellular;
pub mod organism;

pub use cellular::*;
pub use organism::*;
