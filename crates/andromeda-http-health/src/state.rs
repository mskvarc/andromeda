//! Focused health-route state.

use andromeda_health::aggregate::Health;

/// State containing only the health aggregation capability.
#[derive(Clone)]
pub struct HealthRouteState {
    /// Registered health indicators.
    health: Health,
}

impl HealthRouteState {
    /// Construct focused health-route state.
    #[must_use]
    pub const fn new(health: Health) -> HealthRouteState {
        HealthRouteState { health }
    }

    /// Borrow the health aggregation capability.
    #[must_use]
    pub const fn health(&self) -> &Health {
        &self.health
    }
}
