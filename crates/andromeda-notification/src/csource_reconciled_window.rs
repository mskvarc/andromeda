//! The registration changes a reconciliation has already reported.
//!
//! A reconciliation reads every registration as it stands, so a change
//! committed while it ran is reported by it, and the change's own event
//! reaches the worker afterwards. Evaluating that event would report the
//! change a second time (clause 5.3.3 owes one `updated` per update).

use andromeda_ngsi_ld_model::notification::CsrChange;
use chrono::{DateTime, Utc};

/// The modification time up to which a reconciliation read the registrations,
/// until a change beyond it shows the worker has moved past the window.
///
/// Registration modification times are stamped by the writing broker, so the
/// window is closed by the first change past it rather than kept for good: a
/// broker whose clock runs behind would otherwise have its changes skipped
/// indefinitely.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReconciledWindow {
    /// The latest modification time the reconciliation read, `None` when no
    /// window is open.
    through: Option<DateTime<Utc>>,
}

impl ReconciledWindow {
    /// A window over what a reconciliation read up to `through`.
    #[must_use]
    pub const fn through(through: Option<DateTime<Utc>>) -> ReconciledWindow {
        ReconciledWindow { through }
    }

    /// Whether `change` was already reported by the reconciliation.
    ///
    /// A creation or update written at or before the window was read by it. A
    /// deletion writes nothing and is always evaluated, which reports nothing
    /// for a registration the reconciliation already withdrew. A change past
    /// the window closes it.
    pub fn covers(&mut self, change: &CsrChange) -> bool {
        match (self.through, change.written_at()) {
            (Some(through), Some(written)) if written <= through => true,
            (Some(_), Some(_)) => {
                self.through = None;
                false
            }
            (Some(_) | None, None) | (None, Some(_)) => false,
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests;
