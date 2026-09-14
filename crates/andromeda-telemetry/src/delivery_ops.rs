//! Outcome accounting for notification deliveries.

use crate::{
    names::{LABEL_OUTCOME, NOTIFICATION_DELIVERIES_TOTAL, NOTIFICATION_DELIVERY_SECONDS},
    operation::Outcome,
};
use metrics::{counter, histogram};
use std::time::Instant;

/// A running measurement of one notification delivery.
///
/// Records nothing until finished, so a timer dropped on an early return leaves
/// no misleading sample.
#[derive(Debug)]
pub struct DeliveryTimer {
    /// When the delivery attempt began.
    started: Instant,
}

impl DeliveryTimer {
    /// Start timing a delivery attempt.
    #[must_use]
    pub fn start() -> DeliveryTimer {
        DeliveryTimer { started: Instant::now() }
    }

    /// Record the elapsed time and count the delivery under `outcome`.
    pub fn finish(self, outcome: Outcome) {
        histogram!(NOTIFICATION_DELIVERY_SECONDS).record(self.started.elapsed().as_secs_f64());
        counter!(NOTIFICATION_DELIVERIES_TOTAL, LABEL_OUTCOME => <&'static str>::from(outcome)).increment(1);
    }

    /// Finish with the outcome a [`Result`] represents.
    pub fn finish_result<T, E>(self, result: &Result<T, E>) {
        self.finish(Outcome::of(result));
    }
}

#[cfg(test)]
mod tests {
    use crate::{delivery_ops::DeliveryTimer, error::TelemetryError, names::NOTIFICATION_DELIVERIES_TOTAL, operation::Outcome, recorder::TelemetryRecorder};

    #[test]
    fn a_delivery_is_counted_under_its_outcome() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached()?;
        recorder.record_locally(|| {
            DeliveryTimer::start().finish(Outcome::Ok);
            DeliveryTimer::start().finish(Outcome::Error);
        });
        let rendered = recorder.render();
        assert!(rendered.contains(&format!("{NOTIFICATION_DELIVERIES_TOTAL}{{outcome=\"ok\"}} 1")), "{rendered}");
        assert!(
            rendered.contains(&format!("{NOTIFICATION_DELIVERIES_TOTAL}{{outcome=\"error\"}} 1")),
            "{rendered}"
        );
        Ok(())
    }
}
