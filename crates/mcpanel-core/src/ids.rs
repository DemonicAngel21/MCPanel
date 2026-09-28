//! Strongly typed identifiers (UUIDv7: time-ordered, globally unique).

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use uuid::Uuid;

macro_rules! id_type {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub Uuid);

        impl $name {
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }

        impl FromStr for $name {
            type Err = crate::error::CoreError;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(s).map(Self).map_err(|_| {
                    crate::error::CoreError::invalid(concat!("Invalid ", stringify!($name)))
                })
            }
        }
    };
}

id_type!(
    /// Identifies a Minecraft server managed by MCPanel.
    ServerId
);
id_type!(
    /// Identifies a registered Java runtime.
    JavaRuntimeId
);
id_type!(
    /// Identifies a long-running job.
    JobId
);
id_type!(
    /// Identifies an audit record.
    AuditId
);
id_type!(
    /// Identifies a domain event.
    EventId
);
id_type!(
    /// Identifies a backup archive.
    BackupId
);
