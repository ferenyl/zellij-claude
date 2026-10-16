//! Pure core logic for the zellij-claude sessions plugin. No Zellij host calls,
//! so everything here builds and tests on the native target.

pub mod config;
pub mod hook_map;
pub mod state;
pub mod status_file;
