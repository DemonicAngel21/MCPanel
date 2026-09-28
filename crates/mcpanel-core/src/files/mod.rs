//! Server file management with a strict safety model (see `safepath`).

pub mod archive;
pub mod fsx;
pub mod safepath;
pub mod sensitivity;
pub mod service;
pub mod text;

pub use fsx::path_starts_with_ci;
pub use safepath::{SafePath, SafeRoot};
pub use sensitivity::Sensitivity;
