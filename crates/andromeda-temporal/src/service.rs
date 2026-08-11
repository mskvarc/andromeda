//! Temporal lifecycle and query policy service.

use crate::{
    linked::candidates::TemporalLinkedCandidate,
    port::{history_reader::TemporalHistoryReader, mutation_barrier::TemporalMutationBarrier, mutator::TemporalMutator},
    remote_linked::RemoteTemporalLinkedSource,
};
use andromeda_context_commit::committer::ContextCommitter;
use andromeda_entity::port::reader::EntityReader;
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_query::temporal::{last_n::LastNOrigin, window::TimeInterval};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::{collections::HashSet, sync::Arc};

/// Return short IRI token from fragment or last path segment.
pub(crate) fn iri_short_name(iri: &str) -> Option<String> {
    if let Some(pos) = iri.rfind('#') {
        return Some(iri[pos + 1..].to_string());
    }
    iri.rfind('/').map(|pos| iri[pos + 1..].to_string())
}

/// Return whether a context spec contains explicit (client-provided) context.
pub(crate) fn has_explicit_context(context: &ContextSpec) -> bool {
    !context.link_contexts().is_empty() || context.body_context().is_some()
}

/// Normalize compacted temporal payload when no explicit context was provided.
///
/// This keeps top-level keys and `type` values readable by collapsing expanded IRIs
/// to their fragment/last-path short names.
pub(crate) fn normalize_temporal_payload_without_context(value: &mut Value) {
    let reserved: HashSet<&str> = ["id", "type", "@context", "createdAt", "modifiedAt", "deletedAt"].into_iter().collect();

    let Some(obj) = value.as_object_mut() else {
        return;
    };

    if let Some(type_value) = obj.get_mut("type") {
        match type_value {
            Value::String(type_name) => {
                if type_name.starts_with("http")
                    && let Some(short) = iri_short_name(type_name)
                {
                    *type_name = short;
                }
            }
            Value::Array(types) => {
                for t in types {
                    if let Some(type_name) = t.as_str()
                        && type_name.starts_with("http")
                        && let Some(short) = iri_short_name(type_name)
                    {
                        *t = Value::String(short);
                    }
                }
            }
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::Object(_) => {}
        }
    }

    let keys_to_rename: Vec<(String, String)> = obj
        .keys()
        .filter(|k| !reserved.contains(k.as_str()))
        .filter_map(|key| {
            if !key.starts_with("http") && !key.starts_with("urn:") {
                return None;
            }
            iri_short_name(key).map(|short| (key.clone(), short)) // clone: keys are borrowed from map iterator
        })
        .filter(|(old_key, new_key)| old_key != new_key && !obj.contains_key(new_key))
        .collect();

    for (old_key, new_key) in keys_to_rename {
        if let Some(value) = obj.remove(&old_key) {
            obj.insert(new_key, value);
        }
    }
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
    /// Resolution of `objectType`-annotated Linked Entities whose history this
    /// broker does not hold (clause 4.5.23.1). Absent in a composition without
    /// federation, which resolves joins purely against its own history.
    pub(crate) remote_temporal_linked: Option<Arc<dyn RemoteTemporalLinkedSource>>,
}

/// Result of create_or_update operation.
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
    pub fn new(
        entity_reader: Arc<dyn EntityReader>,
        reader: Arc<dyn TemporalHistoryReader>,
        mutation_barrier: Arc<dyn TemporalMutationBarrier>,
        mutator: Arc<dyn TemporalMutator>,
        writes: Arc<dyn ContextCommitter>,
        engine: JsonLdEngine,
        default_last_n: u32,
        remote_temporal_linked: Option<Arc<dyn RemoteTemporalLinkedSource>>,
    ) -> TemporalService {
        TemporalService {
            reader,
            mutation_barrier,
            mutator,
            writes,
            entity_reader,
            engine,
            default_last_n,
            remote_temporal_linked,
        }
    }

    /// Return a reference to the underlying JSON-LD engine.
    pub const fn engine(&self) -> &JsonLdEngine {
        &self.engine
    }
}
