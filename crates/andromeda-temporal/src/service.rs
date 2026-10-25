//! Temporal lifecycle and query policy service.

use crate::{
    port::{history_reader::TemporalHistoryReader, mutation_barrier::TemporalMutationBarrier, mutator::TemporalMutator, service_ports::TemporalServicePorts},
    query_warning_reason::TemporalQueryWarningReason,
    remote_linked::RemoteTemporalLinkedSource,
    service_limits::TemporalServiceLimits,
};
use andromeda_context_commit::committer::ContextCommitter;
use andromeda_entity::{change_notifier::ChangeNotifier, port::reader::EntityReader};
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_linked_retrieval::admission::TemporalJoin;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::temporal::temporal_history::TemporalHistory;
use andromeda_ngsi_ld_query::{
    filter::language_targeting::LanguageTargeting,
    geo::admission::GeoQueryAdmission,
    page_reach::PageReach,
    projection::compiled::Projection,
    temporal::{continuation::TemporalContinuation, instance_page::PageDirection, window::TimeInterval},
};
use andromeda_retry::retry_delay::RetryDelay;
use chrono::{DateTime, Utc};
use std::sync::Arc;
use thiserror::Error;

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
    /// The history of the entity, before it is compacted under the request
    /// context.
    pub history: TemporalHistory,
    /// Range metadata, present when the response is a partial page.
    pub range: Option<TemporalRange>,
    /// The request's clause 4.21 projection, expanded, which narrows the
    /// members the response writes of the entity.
    pub projection: Projection,
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
    /// The request the client continues the response with, serving no
    /// instance of this page again; `None` when nothing of the request
    /// remains or the rest of a backward `lastN` cannot be stated.
    pub next: Option<TemporalContinuation>,
}

/// Warning collected when a per-entity temporal query step fails non-fatally.
#[derive(Debug, Error)]
#[error("{entity_id}: {reason}")]
pub struct TemporalQueryWarning {
    /// The entity that triggered the warning.
    pub entity_id: Identifier,
    /// What failed for the entity.
    #[source]
    pub reason: TemporalQueryWarningReason,
}

/// Result of a temporal query operation.
pub struct TemporalQueryResult {
    /// The histories of the entities matching the query, before they are
    /// compacted under the request context.
    pub entities: Vec<TemporalHistory>,
    /// Total entity count when the `count` option was requested.
    pub total_count: Option<i64>,
    /// Whether the matching entities continue past the page in the direction
    /// it was read (clause 5.5.9.1).
    pub reach: PageReach,
    /// Whether the page cut the history of any entity short.
    pub any_truncated: bool,
    /// Range metadata, present when the response is a partial page.
    pub range: Option<TemporalRange>,
    /// Non-fatal warnings encountered during per-entity processing.
    pub warnings: Vec<TemporalQueryWarning>,
    /// The request's clause 4.21 projection, expanded, which narrows the
    /// members the response writes of every entity.
    pub projection: Projection,
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

    /// What a temporal query's geo-query is admitted under: the limits its
    /// reference geometry stays within (clause 5.5.6) and whether a `near`
    /// distance band is served.
    #[must_use]
    pub const fn geo_admission(&self) -> GeoQueryAdmission {
        GeoQueryAdmission::new(self.limits.payload_budget.geometry_census_limits(), self.limits.near_band)
    }

    /// What a temporal `q`'s target languages may be and how they select
    /// languageMap keys, before a request's negotiation with the sources it
    /// reaches.
    #[must_use]
    pub const fn language_targeting(&self) -> LanguageTargeting {
        self.limits.language_targeting
    }
}
