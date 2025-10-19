// VR Teleport Movement System
//
// This module implements a standard VR teleport system that eliminates motion sickness
// by allowing players to point to a location and instantly teleport there, rather than
// using smooth locomotion.

pub mod teleport_system;
pub mod trajectory;

pub use teleport_system::{TeleportButton, TeleportConfig, TeleportHandState, TeleportSystem};
pub use trajectory::ArcTrajectory;
