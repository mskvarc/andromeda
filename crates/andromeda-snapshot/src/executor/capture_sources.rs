//! Where a snapshot capture reads the context it freezes.

use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::query::EntityQueryService;
use andromeda_federation::FederationService;
use andromeda_temporal::service::TemporalService;
use std::sync::Arc;

/// The live branch a snapshot capture reads and the query operations it reads
/// through.
///
/// The four belong together because every captured page is produced by all of
/// them at once: clause 5.16.1.4 freezes the same result a client query would
/// have received, so the local entity and temporal queries and the registered
/// context sources merged into them are all evaluated against one live branch.
pub struct SnapshotCaptureSources {
    /// Live branch every capture query is evaluated against.
    pub live_target: ContextTarget,
    /// Entity query the current-state capture pages through.
    pub entity_query: Arc<EntityQueryService>,
    /// Temporal query the history capture pages through.
    pub temporal: Arc<TemporalService>,
    /// Forwarding the registered context sources are captured through.
    pub forwarding: Arc<FederationService>,
}
