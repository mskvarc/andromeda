//! Shared timing and outcome accounting for domain operations.
//!
//! A domain operation is counted once, labelled by operation and outcome, and
//! timed into a per-operation histogram. Call sites start an [`OperationTimer`],
//! do their work, then finish it with the operation's [`Result`], so both the
//! success and error paths are recorded without hand-written timing at each site.

use crate::names::{LABEL_OPERATION, LABEL_OUTCOME};
use metrics::{counter, histogram};
use std::time::Instant;
use strum::IntoStaticStr;

/// Whether a recorded operation succeeded or failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub enum Outcome {
    /// The operation returned successfully.
    Ok,
    /// The operation returned an error.
    Error,
}

impl Outcome {
    /// The outcome a [`Result`] represents.
    #[must_use]
    pub const fn of<T, E>(result: &Result<T, E>) -> Outcome {
        match result {
            Ok(_) => Outcome::Ok,
            Err(_) => Outcome::Error,
        }
    }
}

/// A running measurement of one domain operation.
///
/// Records nothing until [`OperationTimer::finish`] (or
/// [`OperationTimer::finish_result`]) is called, so a timer dropped without
/// finishing on an early return records no misleading zero-duration sample.
#[derive(Debug)]
pub struct OperationTimer {
    /// When the measured work began.
    started: Instant,
    /// The counter metric name.
    total: &'static str,
    /// The duration histogram metric name.
    seconds: &'static str,
    /// The rendered operation label value.
    operation: &'static str,
}

impl OperationTimer {
    /// Start timing an operation recorded under `total` and `seconds`.
    #[must_use]
    pub fn start(total: &'static str, seconds: &'static str, operation: &'static str) -> OperationTimer {
        OperationTimer {
            started: Instant::now(),
            total,
            seconds,
            operation,
        }
    }

    /// Record the elapsed time and increment the counter under `outcome`.
    pub fn finish(self, outcome: Outcome) {
        histogram!(self.seconds, LABEL_OPERATION => self.operation).record(self.started.elapsed().as_secs_f64());
        counter!(self.total, LABEL_OPERATION => self.operation, LABEL_OUTCOME => <&'static str>::from(outcome)).increment(1);
    }

    /// Finish with the outcome a [`Result`] represents.
    pub fn finish_result<T, E>(self, result: &Result<T, E>) {
        self.finish(Outcome::of(result));
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::TelemetryError,
        names::{ENTITY_OPERATION_DURATION_SECONDS, ENTITY_OPERATIONS_TOTAL},
        operation::{OperationTimer, Outcome},
        recorder::TelemetryRecorder,
    };

    #[test]
    fn outcome_of_a_result_reflects_its_variant() {
        assert_eq!(Outcome::of::<(), ()>(&Ok(())), Outcome::Ok);
        assert_eq!(Outcome::of::<(), ()>(&Err(())), Outcome::Error);
    }

    #[test]
    fn finishing_records_the_counter_and_histogram_under_operation_and_outcome() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached()?;
        recorder.record_locally(|| {
            OperationTimer::start(ENTITY_OPERATIONS_TOTAL, ENTITY_OPERATION_DURATION_SECONDS, "create").finish(Outcome::Ok);
            let failed: Result<(), ()> = Err(());
            OperationTimer::start(ENTITY_OPERATIONS_TOTAL, ENTITY_OPERATION_DURATION_SECONDS, "delete").finish_result(&failed);
        });
        let rendered = recorder.render();
        assert!(
            rendered.contains(&format!("{ENTITY_OPERATIONS_TOTAL}{{operation=\"create\",outcome=\"ok\"}} 1")),
            "{rendered}"
        );
        assert!(
            rendered.contains(&format!("{ENTITY_OPERATIONS_TOTAL}{{operation=\"delete\",outcome=\"error\"}} 1")),
            "{rendered}"
        );
        assert!(rendered.contains(&format!("{ENTITY_OPERATION_DURATION_SECONDS}_bucket")), "{rendered}");
        Ok(())
    }
}
