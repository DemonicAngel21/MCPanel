//! Lifecycle state machine. Process supervision lives in `server::process`.

pub mod state;

pub use state::LifecycleState;
