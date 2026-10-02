//! Time abstraction. All persisted timestamps are UTC unix milliseconds.

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Timestamp(pub i64);

impl Timestamp {
    pub fn now() -> Self {
        let ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        Self(ms)
    }

    pub fn from_system_time(t: SystemTime) -> Option<Self> {
        t.duration_since(UNIX_EPOCH)
            .ok()
            .map(|d| Self(d.as_millis() as i64))
    }

    pub fn millis(self) -> i64 {
        self.0
    }

    pub fn parse_rfc3339(s: &str) -> Option<Self> {
        s.parse::<jiff::Timestamp>()
            .ok()
            .map(|t| Self(t.as_millisecond()))
    }
}

/// Injectable clock so time-dependent policies are testable.
pub trait Clock: Send + Sync {
    fn now(&self) -> Timestamp;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }
}
