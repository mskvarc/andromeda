//! Failure accounting for background tasks.

use crate::names::{LABEL_CLASS, LABEL_CRITICALITY, TASK_FAILURES_TOTAL};
use metrics::counter;

/// Count one background task failure of `class` at `criticality`.
///
/// The two labels are rendered by the caller from its own task-failure and
/// criticality types, so this crate stays a leaf that names no task-runtime type.
pub fn record_task_failure(class: &'static str, criticality: &'static str) {
    counter!(TASK_FAILURES_TOTAL, LABEL_CLASS => class, LABEL_CRITICALITY => criticality).increment(1);
}

#[cfg(test)]
mod tests {
    use crate::{error::TelemetryError, names::TASK_FAILURES_TOTAL, recorder::TelemetryRecorder, task_failure::record_task_failure};

    #[test]
    fn a_failure_is_counted_under_its_class_and_criticality() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached()?;
        recorder.record_locally(|| record_task_failure("transient", "observed"));
        let rendered = recorder.render();
        assert!(
            rendered.contains(&format!("{TASK_FAILURES_TOTAL}{{class=\"transient\",criticality=\"observed\"}} 1")),
            "{rendered}"
        );
        Ok(())
    }
}
