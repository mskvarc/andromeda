//! What one matching pass over the candidate index produces.

use crate::compiled::CompiledSubscription;
use andromeda_telemetry::subscription::CandidateFunnel;
use std::sync::Arc;

/// The subscriptions one change event matched, with the stage counts of the
/// candidate funnel that produced them.
///
/// The funnel is returned rather than recorded here so a caller can log it
/// against the event and record it once, whichever code path it took.
#[derive(Debug, Default)]
pub struct MatchOutcome {
    /// Subscriptions accepted by the authoritative matcher.
    pub matched: Vec<Arc<CompiledSubscription>>,
    /// Stage counts of the pass.
    pub funnel: CandidateFunnel,
}
