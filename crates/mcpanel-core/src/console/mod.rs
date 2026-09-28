//! Console pipeline: dialect parsing, ring buffer, capture files, batched streaming.

pub mod dialect;
pub mod hub;

pub use dialect::{LogDialect, LogLevel};
pub use hub::{ConsoleBatch, ConsoleHub, ConsoleLine, ConsoleStream, ConsoleSubscription};
