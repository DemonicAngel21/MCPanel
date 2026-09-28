//! Server lifecycle state machine (process dimension only — see ADR-0004).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleState {
    /// Created but never started.
    Created,
    Starting,
    /// Process running and the server reported it is ready.
    Running,
    Stopping,
    Stopped,
    /// Exited unexpectedly.
    Crashed,
    /// Stopping as part of a restart; will start again automatically.
    Restarting,
    /// Failed to start or failed fatally with a known, non-restartable cause.
    Error,
    /// Process from a previous MCPanel session is still running; no console attached.
    Detached,
}

impl LifecycleState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Stopping => "stopping",
            Self::Stopped => "stopped",
            Self::Crashed => "crashed",
            Self::Restarting => "restarting",
            Self::Error => "error",
            Self::Detached => "detached",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "created" => Self::Created,
            "starting" => Self::Starting,
            "running" => Self::Running,
            "stopping" => Self::Stopping,
            "stopped" => Self::Stopped,
            "crashed" => Self::Crashed,
            "restarting" => Self::Restarting,
            "error" => Self::Error,
            "detached" => Self::Detached,
            _ => return None,
        })
    }

    /// A process exists (or is believed to exist) for this state.
    pub fn has_process(self) -> bool {
        matches!(
            self,
            Self::Starting | Self::Running | Self::Stopping | Self::Restarting | Self::Detached
        )
    }

    /// The server may be started from this state.
    pub fn can_start(self) -> bool {
        matches!(
            self,
            Self::Created | Self::Stopped | Self::Crashed | Self::Error
        )
    }

    /// Whether `self → next` is a legal transition.
    pub fn can_transition_to(self, next: Self) -> bool {
        use LifecycleState::*;
        match (self, next) {
            (a, b) if a == b => false,
            (Created | Stopped | Crashed | Error, Starting) => true,
            (Starting, Running | Stopping | Stopped | Crashed | Error | Restarting) => true,
            (Running, Stopping | Crashed | Restarting | Stopped | Error) => true,
            (Stopping, Stopped | Crashed | Error) => true,
            (Restarting, Starting | Stopped | Crashed | Error) => true,
            (Detached, Stopped | Stopping) => true,
            (Created | Stopped | Crashed | Error, Detached) => true,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::LifecycleState::*;

    #[test]
    fn legal_transitions() {
        assert!(Created.can_transition_to(Starting));
        assert!(Starting.can_transition_to(Running));
        assert!(Running.can_transition_to(Stopping));
        assert!(Stopping.can_transition_to(Stopped));
        assert!(Running.can_transition_to(Crashed));
        assert!(Detached.can_transition_to(Stopped));
    }

    #[test]
    fn illegal_transitions() {
        assert!(!Stopped.can_transition_to(Running));
        assert!(!Running.can_transition_to(Starting));
        assert!(!Created.can_transition_to(Stopping));
        assert!(!Running.can_transition_to(Running));
    }

    #[test]
    fn roundtrip_strings() {
        for s in [
            Created, Starting, Running, Stopping, Stopped, Crashed, Restarting, Error, Detached,
        ] {
            assert_eq!(super::LifecycleState::parse(s.as_str()), Some(s));
        }
    }
}
