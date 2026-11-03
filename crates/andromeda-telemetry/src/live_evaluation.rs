//! Accounting for committed changes live evaluation did not finish.

use crate::names::LIVE_CHANGES_ABANDONED_TOTAL;
use metrics::counter;

/// Count `changes` committed entity changes a stopping tenant abandoned
/// without evaluating them.
pub fn record_abandoned_changes(changes: usize) {
    counter!(LIVE_CHANGES_ABANDONED_TOTAL).increment(u64::try_from(changes).unwrap_or(u64::MAX));
}

#[cfg(test)]
mod tests {
    use crate::{error::TelemetryError, live_evaluation::record_abandoned_changes, names::LIVE_CHANGES_ABANDONED_TOTAL, recorder::TelemetryRecorder};

    #[test]
    fn abandoned_changes_are_counted_by_how_many_were_dropped() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached()?;
        recorder.record_locally(|| {
            record_abandoned_changes(3);
            record_abandoned_changes(4);
        });
        assert!(
            recorder.render().contains(&format!("{LIVE_CHANGES_ABANDONED_TOTAL} 7")),
            "{}",
            recorder.render()
        );
        Ok(())
    }
}
