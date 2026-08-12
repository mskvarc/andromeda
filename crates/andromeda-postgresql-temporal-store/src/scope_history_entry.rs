//! One scope observation offered to the scope-history bulk insert.

use crate::{scope_history_value::ScopeHistoryValue, semantic_time_axes::SemanticTimeAxes};
use andromeda_ngsi_ld_lexicon::{instance_identity::InstanceIdentity, uri::Uri};
use andromeda_temporal::append_intent::TemporalObservationIdentity;
use chrono::{DateTime, Utc};

/// One scope-history observation offered for a bulk insert.
///
/// Scope history hangs off the temporal entity catalogue rather than off
/// current state, because clause 5.6.6 leaves temporal evolution standing when
/// an entity is deleted. The narrow catalogue key the row is stored under is
/// resolved from `entity_id` by the insert itself, so callers that have only
/// the entity's identifier do not have to read the catalogue first.
#[derive(Debug)]
pub struct ScopeHistoryEntry {
    /// The instant the broker durably accepted the write this row belongs to.
    pub recorded_at: DateTime<Utc>,
    /// The entity whose scope this observation records.
    pub entity_id: Uri,
    /// The atomic write that produced this row, and this row's place in it.
    pub identity: TemporalObservationIdentity,
    /// What the row records.
    pub value: ScopeHistoryValue,
    /// The NGSI-LD time axes a temporal query may select on this row.
    pub axes: SemanticTimeAxes,
    /// The identifier this scope instance is published to clients under.
    pub instance: InstanceIdentity,
}
