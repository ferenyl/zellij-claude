//! Pure core logic for the zellij-claude sessions plugin. No Zellij host calls,
//! so everything here builds and tests on the native target.

pub mod bar;
pub mod color;
pub mod config;
pub mod hook_map;
pub mod icons;
pub mod keys;
pub mod list;
pub mod state;
pub mod status_file;
pub mod truncate;
