//! tyler-tiler: automatic tiling for macOS with Omarchy-inspired layouts
//! and shortcuts that preserve standard Command-key actions.
//!
//! Everything except [`platform`] is platform-independent and unit tested on
//! any host. The macOS backend (Accessibility API + CGEventTap) lives behind
//! `cfg(target_os = "macos")`.

pub mod action;
pub mod config;
pub mod geometry;
pub mod keys;
pub mod layout;
pub mod motion;
pub mod navigator;
pub mod platform;
pub mod spaces;
pub mod wm;

/// Opaque window identifier. On macOS this is the `CGWindowID`.
pub type WindowId = u64;
