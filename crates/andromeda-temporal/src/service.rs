//! Temporal lifecycle and query policy service.

use crate::{
    linked::candidates::TemporalLinkedCandidate,
    port::{history_reader::TemporalHistoryReader, mutation_barrier::TemporalMutationBarrier, mutator::TemporalMutator, service_ports::TemporalServicePorts},
    remote_linked::RemoteTemporalLinkedSource,
};
use andromeda_context_commit::committer::ContextCommitter;
use andromeda_entity::port::reader::EntityReader;
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_query::temporal::{last_n::LastNOrigin, window::TimeInterval};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::sync::Arc;

/// Return short IRI token from fragment or last path segment.
pub(crate) fn iri_short_name(iri: &str) -> Option<String> {
    if let Some(pos) = iri.rfind('#') {
        return Some(iri[pos + 1..].to_string());
    }
    iri.rfind('/').map(|pos| iri[pos + 1..].to_string())
}

/// Service layer for NGSI-LD Temporal Entity operations (read and write).
pub struct TemporalService {
    /// Temporal history reader selected by composition.
    pub(crate) reader: Arc<dyn TemporalHistoryReader>,
    /// Delivery barrier required before direct update and delete operations.
    pub(crate) mutation_barrier: Arc<dyn TemporalMutationBarrier>,
    /// Immediate temporal mutation capability selected by composition.
    pub(crate) mutator: Arc<dyn TemporalMutator>,
    /// Atomic semantic write boundary used by create and append operations.
    pub(crate) writes: Arc<dyn ContextCommitter>,
    /// Base-entity reads used for type metadata and residual completion.
    pub(crate) entity_reader: Arc<dyn EntityReader>,
    /// JSON-LD processor used by query compilation and representation compaction.
    pub(crate) engine: JsonLdEngine,
    /// Broker safety cap applied when the client omits `lastN`.
    pub(crate) default_last_n: u32,
    /// The maximum history instances one delete-by-query may remove before it is
    /// refused with `TooManyResults` (the `temporal_delete_by_query` extension).
    pub(crate) delete_by_query_max_matches: u64,
    /// Resolution of `objectType`-annotated Linked Entities whose history this
    /// broker does not hold (clause 4.5.23.1). Absent in a composition without
    /// federation, which resolves joins purely against its own history.
    pub(crate) remote_temporal_linked: Option<Arc<dyn RemoteTemporalLinkedSource>>,
}

/// Result of `create_or_update` operation.
#[derive(Debug)]
pub enum CreateOrUpdateResult {
    /// A new temporal entity was created, carrying the assigned entity ID.
    Created(Uri),
    /// Attributes were appended to an existing temporal entity.
    Updated,
}

/// Result of a temporal read operation.
pub struct TemporalReadResult {
    /// The compacted temporal entity JSON payload.
    pub value: Value,
    /// Range metadata present when system truncation occurred.
    pub range: Option<TemporalRange>,
    /// The Linked Entities the representation names, read off it before any
    /// format projection narrowed it (clause 4.5.23). A caller that joins
    /// hands these back as the traversal's first level; one that does not
    /// simply drops them.
    pub linked_candidates: Vec<TemporalLinkedCandidate>,
}

/// Neutral metadata needed by the HTTP binding to construct temporal `Content-Range`.
#[derive(Debug, Clone)]
pub struct TemporalRange {
    /// Compiled temporal interval used by the read.
    pub interval: TimeInterval,
    /// Observed minimum and maximum included in the representation.
    pub observed: Option<(DateTime<Utc>, DateTime<Utc>)>,
    /// Maximum partition count when the reader supplied it.
    pub partition_total: Option<i64>,
    /// Whether `lastN` came from the caller or the broker's safety cap.
    pub last_n_origin: LastNOrigin,
}

/// Warning collected when a per-entity temporal query step fails non-fatally.
#[derive(Debug)]
pub struct TemporalQueryWarning {
    /// The entity that triggered the warning.
    pub entity_id: Uri,
    /// Human-readable description of the problem.
    pub message: String,
}

/// Result of a temporal query operation.
pub struct TemporalQueryResult {
    /// Temporal entity representations matching the query.
    pub entities: Vec<Value>,
    /// Total entity count when the `count` option was requested.
    pub total_count: Option<i64>,
    /// Whether any entity's history was truncated by the system lastN cap.
    pub any_truncated: bool,
    /// Range metadata when at least one entity was system-truncated.
    pub range: Option<TemporalRange>,
    /// Non-fatal warnings encountered during per-entity processing.
    pub warnings: Vec<TemporalQueryWarning>,
    /// The Linked Entities the results name, in first-encounter order, read
    /// off each representation before any format projection narrowed it
    /// (clause 4.5.23).
    pub linked_candidates: Vec<TemporalLinkedCandidate>,
}

impl TemporalService {
    /// Create a new temporal service.
    ///
    /// The reader and mutator ports decouple policy from concrete storage. The write coordinator owns atomic append composition, while the entity reader provides base entity lookups.
    ///
    /// `remote_temporal_linked` is the clause 4.5.23.1 half of a join that this
    /// broker's own history cannot answer. `None` confines every join to local
    /// history.
    #[must_use]
    pub fn new(
        ports: TemporalServicePorts,
        engine: JsonLdEngine,
        default_last_n: u32,
        delete_by_query_max_matches: u64,
        remote_temporal_linked: Option<Arc<dyn RemoteTemporalLinkedSource>>,
    ) -> TemporalService {
        let TemporalServicePorts {
            entity_reader,
            reader,
            mutation_barrier,
            mutator,
            writes,
        } = ports;
        TemporalService {
            reader,
            mutation_barrier,
            mutator,
            writes,
            entity_reader,
            engine,
            default_last_n,
            delete_by_query_max_matches,
            remote_temporal_linked,
        }
    }

    /// Return a reference to the underlying JSON-LD engine.
    #[must_use]
    pub const fn engine(&self) -> &JsonLdEngine {
        &self.engine
    }
}
