//! The wake that asks for a background spatial catch-up pass.

/// Asks the background catch-up of one tenant's entity spatial index for a
/// pass.
///
/// A latency hint only. Asks made before the pass runs are served by that one
/// pass, and a lost ask costs the time until the catch-up's next fallback
/// pass, never a missed entity, because until the index catches up the store
/// answers every geo query it would narrow.
pub trait SpatialCatchUpSignal: Send + Sync + 'static {
    /// Ask for a pass soon.
    fn wake(&self);
}
