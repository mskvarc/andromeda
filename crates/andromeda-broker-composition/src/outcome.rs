//! Owner-produced terminal process outcomes.

use std::process::ExitCode;

/// Process outcome returned after its owning boundary has reported any failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessExitOutcome {
    /// The requested operation completed successfully.
    Success,
    /// Configuration loading or rendering failed and was reported by configuration.
    ConfigurationFailure,
    /// Broker startup failed and was reported by composition.
    StartupFailure,
    /// Broker HTTP execution or orderly shutdown failed and was reported by composition.
    RuntimeFailure,
    /// The readiness probe found the broker not ready and reported why.
    NotReady,
}

impl From<ProcessExitOutcome> for ExitCode {
    fn from(outcome: ProcessExitOutcome) -> ExitCode {
        match outcome {
            ProcessExitOutcome::Success => ExitCode::SUCCESS,
            ProcessExitOutcome::ConfigurationFailure
            | ProcessExitOutcome::StartupFailure
            | ProcessExitOutcome::RuntimeFailure
            | ProcessExitOutcome::NotReady => ExitCode::FAILURE,
        }
    }
}
