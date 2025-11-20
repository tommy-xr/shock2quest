// VR Teleport Movement System
//
// This module implements a standard VR teleport system that eliminates motion sickness
// by allowing players to point to a location and instantly teleport there, rather than
// using smooth locomotion.

pub mod arc_renderer;
pub mod teleport_system;
pub mod teleport_ui;
pub mod trajectory;

pub use arc_renderer::{ArcRenderConfig, ArcRenderer};
pub use teleport_system::{TeleportButton, TeleportConfig, TeleportHandState, TeleportSystem};
pub use teleport_ui::{TeleportUI, TeleportVisualStyle};
pub use trajectory::ArcTrajectory;
