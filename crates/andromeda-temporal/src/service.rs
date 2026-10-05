//! Temporal lifecycle and query policy service.

use crate::{
    port::{history_reader::TemporalHistoryReader, mutation_barrier::TemporalMutationBarrier, mutator::TemporalMutator, service_ports::TemporalServicePorts},
    remote_linked::RemoteTemporalLinkedSource,
    service_limits::TemporalServiceLimits,
};
use andromeda_context_commit::committer::ContextCommitter;
use andromeda_entity::{
    change_notifier::ChangeNotifier,
    port::{reader::EntityReader, retry_delay::RetryDelay},
};
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_linked_retrieval::{admission::TemporalJoin, edge::LinkedEdge};
use andromeda_ngsi_ld_geometry::geometry_census::GeometryCensusLimits;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::temporal::{instance_page::PageDirection, window::TimeInterval};
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
    /// The wait between two attempts of a write a concurrent writer superseded.
    pub(crate) retry_delay: Arc<dyn RetryDelay>,
    /// JSON-LD processor used by query compilation and representation compaction.
    pub(crate) engine: JsonLdEngine,
    /// Raises the notifications a change a temporal write makes to current
    /// state owes, exactly as a Core write's (clause 5.8.6).
    pub(crate) notifier: Arc<ChangeNotifier>,
    /// The bounds applied to what a request may ask of the service.
    pub(crate) limits: TemporalServiceLimits,
    /// Resolution of `objectType`-annotated Linked Entities whose history this
    /// broker does not hold (clause 4.5.23.1). Absent in a composition without
    /// federation, which resolves joins purely against its own history.
    pub(crate) remote_temporal_linked: Option<Arc<dyn RemoteTemporalLinkedSource>>,
    /// Whether the temporal operations retrieve Linked Entities, which the
    /// `temporal_linked_retrieval` extension decides (clauses 5.7.3.4, 5.7.4.4).
    pub(crate) linked_retrieval: TemporalJoin,
}

/// Result of `create_or_update` operation.
#[derive(Debug)]
pub enum CreateOrUpdateResult {
    /// A new temporal entity was created, carrying the assigned entity ID.
    Created(Identifier),
    /// Attributes were appended to an existing temporal entity.
    Updated,
}

/// Result of a temporal read operation.
pub struct TemporalReadResult {
    /// The compacted temporal entity JSON payload.
    pub value: Value,
    /// Range metadata, present when the response is a partial page.
    pub range: Option<TemporalRange>,
    /// The edges to the Linked Entities the representation names, read off
    /// it before any format projection narrowed it (clause 4.5.23). A caller
    /// that joins hands these back as the roots' edges; one that does not
    /// simply drops them.
    pub linked_edges: Vec<LinkedEdge>,
}

/// Neutral metadata the HTTP binding builds a temporal `Content-Range` from
/// (clause 6.3.10).
#[derive(Debug, Clone)]
pub struct TemporalRange {
    /// Compiled temporal interval used by the read.
    pub interval: TimeInterval,
    /// The least and most recent instants the partial response carries.
    pub observed: Option<(DateTime<Utc>, DateTime<Utc>)>,
    /// The direction the remainder of the response is paged in.
    pub direction: PageDirection,
}

/// Warning collected when a per-entity temporal query step fails non-fatally.
#[derive(Debug)]
pub struct TemporalQueryWarning {
    /// The entity that triggered the warning.
    pub entity_id: Identifier,
    /// Human-readable description of the problem.
    pub message: String,
}

/// Result of a temporal query operation.
pub struct TemporalQueryResult {
    /// Temporal entity representations matching the query.
    pub entities: Vec<Value>,
    /// Total entity count when the `count` option was requested.
    pub total_count: Option<i64>,
    /// Whether the page cut the history of any entity short.
    pub any_truncated: bool,
    /// Range metadata, present when the response is a partial page.
    pub range: Option<TemporalRange>,
    /// Non-fatal warnings encountered during per-entity processing.
    pub warnings: Vec<TemporalQueryWarning>,
    /// The edges to the Linked Entities the results name, in result order,
    /// read off each representation before any format projection narrowed it
    /// (clause 4.5.23).
    pub linked_edges: Vec<LinkedEdge>,
}

impl TemporalService {
    /// Create a new temporal service.
    ///
    /// The reader and mutator ports decouple policy from concrete storage. The write coordinator owns atomic append composition, while the entity reader provides base entity lookups.
    ///
    /// `notifier` is the one the Core API notifies through, so a change to
    /// current state is notified the same way whichever API made it.
    ///
    /// `remote_temporal_linked` is the clause 4.5.23.1 half of a join that this
    /// broker's own history cannot answer. `None` confines every join to local
    /// history. `linked_retrieval` says whether the temporal operations join at
    /// all.
    #[must_use]
    pub fn new(
        ports: TemporalServicePorts,
        engine: JsonLdEngine,
        notifier: Arc<ChangeNotifier>,
        limits: TemporalServiceLimits,
        remote_temporal_linked: Option<Arc<dyn RemoteTemporalLinkedSource>>,
        linked_retrieval: TemporalJoin,
    ) -> TemporalService {
        let TemporalServicePorts {
            entity_reader,
            reader,
            mutation_barrier,
            mutator,
            writes,
            retry_delay,
        } = ports;
        TemporalService {
            reader,
            mutation_barrier,
            mutator,
            writes,
            entity_reader,
            retry_delay,
            engine,
            notifier,
            limits,
            remote_temporal_linked,
            linked_retrieval,
        }
    }

    /// Return a reference to the underlying JSON-LD engine.
    #[must_use]
    pub const fn engine(&self) -> &JsonLdEngine {
        &self.engine
    }

    /// Whether the temporal operations retrieve Linked Entities.
    #[must_use]
    pub const fn linked_retrieval(&self) -> TemporalJoin {
        self.linked_retrieval
    }

    /// The limits a temporal query's reference geometry stays within (clause
    /// 5.5.6).
    #[must_use]
    pub const fn reference_limits(&self) -> GeometryCensusLimits {
        self.limits.payload_budget.geometry_census_limits()
    }
}
