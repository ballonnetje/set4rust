//! Readers for The Settlers IV (History Edition) data files.
//!
//! Every format here is reconstructed from S4_Main.exe; each module documents
//! the original function(s) it mirrors so the claim can be re-checked.

pub mod cursor;
#[cfg_attr(windows, path = "desktop_windows.rs")]
pub mod desktop;
pub mod exe;
pub mod error;
pub mod reader;
pub mod gamedata;
pub mod gfx;
pub mod gui;
pub mod install;
pub mod logo;
pub mod map;
pub mod path;
pub mod snd;
pub mod text;

pub use error::{Error, Result};

pub use install::game_dir;
