//! Accounting for the committed entity changes live notification evaluation
//! never saw.

use crate::names::NOTIFICATION_LIVE_CHANGES_LOST_TOTAL;
use metrics::counter;

/// Count `changes` committed entity changes that left the live change buffer
/// before live notification evaluation read them, so the notifications they
/// owed are not sent.
pub fn record_lost_live_changes(changes: u64) {
    counter!(NOTIFICATION_LIVE_CHANGES_LOST_TOTAL).increment(changes);
}

#[cfg(test)]
mod tests {
    use crate::{error::TelemetryError, live_change_loss::record_lost_live_changes, names::NOTIFICATION_LIVE_CHANGES_LOST_TOTAL, recorder::TelemetryRecorder};

    #[test]
    fn lost_changes_accumulate_on_one_counter() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached()?;
        recorder.record_locally(|| {
            record_lost_live_changes(3);
            record_lost_live_changes(4);
        });
        let rendered = recorder.render();
        assert!(rendered.contains(&format!("{NOTIFICATION_LIVE_CHANGES_LOST_TOTAL} 7")), "{rendered}");
        Ok(())
    }
}
