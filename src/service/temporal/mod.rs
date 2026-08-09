//! Service layer for the NGSI-LD Temporal API.

use crate::{
    error::AndromedaError,
    temporal_render::{
        aggregation::TemporalAggregationRenderer,
        array_normalizer::TemporalArrayNormalizer,
        entity::TemporalEntityRenderer,
        values::TemporalValuesRenderer,
    },
};
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    committer::ContextCommitter,
    entity_mutation::{EntityMutation, EntityRecord},
    history_record::HistoryRecord,
};
use andromeda_entity::{port::reader::EntityReader, query::selector_compilation::compile_entity_selectors};
use andromeda_federation::{outcome::ForwardedTemporalEntities, temporal_result::merge_temporal_query_results};
use andromeda_http_response::content_range::build_content_range;
use andromeda_jsonld::{
    compaction::compact_attr_iri,
    context_spec::ContextSpec,
    engine::{options::CompactOptions, processor::JsonLdEngine, term_lookup::PreparedTermContext},
    term_expansion::ContextualExpander,
};
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    entity_type_name::EntityTypeName,
    instance_id::InstanceId,
    temporal_format::TemporalFormat,
    uri::Uri,
};
use andromeda_ngsi_ld_model::expanded::entity_member::extract_types;
use andromeda_ngsi_ld_query::{
    dataset::DatasetSelector,
    entity_selection::EntitySelection,
    error::QueryError,
    filter::{
        expansion::{apply_expand_values, apply_json_keys, expand_q_expr},
        path::expanded_attribute_name,
    },
    geo::{compile as geo_compile, query::GeoQuery},
    pagination::{CountPolicy, Pagination},
    projection::expansion::expand_attrs,
    temporal::{
        compiled::CompiledTemporalQuery,
        instance_selection::InstanceSelection,
        last_n::LastNOrigin,
        representation::Representation,
        window::{TemporalWindow, TimeInterval},
    },
};
use andromeda_ngsi_ld_representation::{normalization::normalize_entity_attributes, sys_attrs::SysAttrsInclusion};
use andromeda_ngsi_ld_request::query::{SharedQueryParams, temporal::TemporalQueryParameters};
use andromeda_ngsi_ld_vocabulary::expanded::classes as expanded_classes;
use andromeda_postgresql_temporal_store::{
    params::DeleteScope,
    recorder::TemporalRecorder,
    store::{TemporalAggregationRow, TemporalHistoryReader, TemporalMutator},
};
use chrono::{DateTime, Utc};
use iri_rs::IriBuf;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

/// Return short IRI token from fragment or last path segment.
fn iri_short_name(iri: &str) -> Option<String> {
    if let Some(pos) = iri.rfind('#') {
        return Some(iri[pos + 1..].to_string());
    }
    iri.rfind('/').map(|pos| iri[pos + 1..].to_string())
}

/// Return whether a context spec contains explicit (client-provided) context.
fn has_explicit_context(context: &ContextSpec) -> bool {
    !context.link_contexts().is_empty() || context.body_context().is_some()
}

/// Normalize compacted temporal payload when no explicit context was provided.
///
/// This keeps top-level keys and `type` values readable by collapsing expanded IRIs
/// to their fragment/last-path short names.
fn normalize_temporal_payload_without_context(value: &mut Value) {
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
    reader: Arc<dyn TemporalHistoryReader>,
    mutator: Arc<dyn TemporalMutator>,
    recorder: Arc<TemporalRecorder>,
    writes: Arc<dyn ContextCommitter>,
    entity_reader: Arc<dyn EntityReader>,
    engine: JsonLdEngine,
    default_last_n: u32,
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
    /// Pre-built Content-Range header value, present when system truncation occurred.
    pub content_range: Option<String>,
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
    /// Pre-built Content-Range header value when truncation occurred.
    pub content_range: Option<String>,
    /// Non-fatal warnings encountered during per-entity processing.
    pub warnings: Vec<TemporalQueryWarning>,
}

impl TemporalService {
    /// Create a new temporal service.
    ///
    /// The reader and mutator ports decouple the service from the concrete
    /// store implementation. The recorder and write coordinator handle the
    /// bulk-write path, while the entity reader provides base entity lookups.
    pub fn new(
        entity_reader: Arc<dyn EntityReader>,
        reader: Arc<dyn TemporalHistoryReader>,
        mutator: Arc<dyn TemporalMutator>,
        recorder: Arc<TemporalRecorder>,
        writes: Arc<dyn ContextCommitter>,
        engine: JsonLdEngine,
        default_last_n: u32,
    ) -> TemporalService {
        TemporalService {
            reader,
            mutator,
            recorder,
            writes,
            entity_reader,
            engine,
            default_last_n,
        }
    }

    /// Return a reference to the underlying JSON-LD engine.
    pub const fn engine(&self) -> &JsonLdEngine {
        &self.engine
    }
}

/// Temporal query compilation.
mod compilation;
/// Temporal entity retrieval and query execution.
mod read;
/// Temporal entity mutation execution.
mod write;
