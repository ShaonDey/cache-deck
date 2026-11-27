//! Code that only exists on one operating system. Everything shared lives in the other modules.

#[cfg(target_os = "macos")]
pub mod macos;
