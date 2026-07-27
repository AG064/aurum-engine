//! Aurum VR module — stub.
//!
//! The VR module is in its earliest form. The shape will be:
//!
//! - `XrRig` component: head, two hands, playspace
//! - `Comfort` resource: vignette, snap turn, teleport options
//! - `Hand` / `Controller` components
//!
//! Today, this crate exists so the workspace builds and so the module
//! surface (`aurum_vr::XrRig`) is reserved.

#![allow(dead_code)]

use aurum_core::prelude::*;

/// Tag component marking an entity as an XR rig.
#[derive(Debug)]
pub struct XrRig;

/// Tag component marking an entity as a hand (left or right).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hand {
    Left,
    Right,
}
