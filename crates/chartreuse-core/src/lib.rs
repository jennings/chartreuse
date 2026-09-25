//! Shared types and pure logic used by every Chartreuse crate.
//!
//! This crate has no GUI or platform dependencies, so every other crate (including
//! `xtask`) can depend on it.

pub mod capture;
pub mod color;
pub mod display;
pub mod error;
pub mod geometry;
pub mod hotkey;
pub mod image;
pub mod permission;
pub mod window;

pub use error::{Error, Result};
