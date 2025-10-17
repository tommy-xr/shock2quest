// VR Teleport Movement System
//
// This module implements a standard VR teleport system that eliminates motion sickness
// by allowing players to point to a location and instantly teleport there, rather than
// using smooth locomotion.

pub mod teleport_system;

pub use teleport_system::{TeleportSystem, TeleportConfig, TeleportButton};