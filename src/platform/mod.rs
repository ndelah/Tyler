//! Platform backends. Only macOS is implemented; other hosts build the core
//! library (for tests and `tyler-tiler check-config`) without a backend.

pub mod filter;

#[cfg(target_os = "macos")]
pub mod macos;
