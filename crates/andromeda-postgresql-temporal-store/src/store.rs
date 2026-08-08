//! Backend-neutral temporal store types and trait declarations.
//!
//! These types decouple the temporal service from PostgreSQL: a row returned
//! by a store implementation carries already-decoded, strongly-typed values
//! instead of a raw `sqlx::postgres::PgRow`. [`TemporalHistoryReader`] and
//! [`TemporalMutator`] follow the consumer-declares-the-port pattern -- this
//! service module declares the contract it needs, and the repository module
//! provides the PostgreSQL-backed implementation.

use crate::{entry::TeaEntry, error::TemporalStoreError, params::DeleteScope};
use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
use andromeda_ngsi_ld_query::temporal::compiled::CompiledTemporalQuery;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use iri_rs::IriBuf;
use serde_json::Value;
use urn_rs::Urn;

/// A single decoded value from the store's typed history columns.
///
/// The store implementation reads exactly the typed column matching the
/// row's `ValueType` and decodes it into this enum; the service layer then
/// builds an expanded NGSI-LD attribute instance without importing any
/// database decoding machinery.
#[derive(Debug, Clone)]
pub enum TemporalValue {
    /// A numeric attribute value.
    Number(f64),
    /// A string attribute value.
    String(String),
    /// A boolean attribute value.
    Boolean(bool),
    /// A GeoJSON geometry, decoded from the store's canonical geometry encoding.
    Geometry(Value),
    /// A JSON object attribute value.
    Object(Value),
    /// A datetime attribute value.
    DateTime(DateTime<Utc>),
    /// A URI attribute value (used by `Relationship` and `VocabProperty`).
    Uri(String),
    /// A deletion marker row (NGSI-LD `deletedAt` tombstone).
    Tombstone,
}

/// A single decoded row from an attribute history query.
///
/// Carries everything the temporal renderers need to build an expanded
/// NGSI-LD attribute instance, replacing direct `sqlx::postgres::PgRow`
/// column access.
#[derive(Debug, Clone)]
pub struct TemporalHistoryRow {
    /// Expanded IRI of the attribute this observation belongs to.
    pub attribute_name: IriBuf,
    /// NGSI-LD attribute meta-type (`Property`, `Relationship`, etc.).
    pub attribute_type: AttributeType,
    /// Multi-attribute discriminator; `None` for the default instance
    /// (NGSI-LD 5.2.5).
    pub dataset_id: Option<IriBuf>,
    /// The decoded observation value, or `Tombstone` for a deletion marker.
    pub value: TemporalValue,
    /// Application-level observation timestamp, per NGSI-LD `observedAt`.
    pub observed_at: Option<DateTime<Utc>>,
    /// System-recorded observation time (the TimescaleDB partition key).
    pub time: DateTime<Utc>,
    /// Unique identifier for this attribute instance.
    pub instance_id: String,
    /// Additional metadata (sub-attributes, `unitCode`, etc.) attached to the instance.
    pub metadata: Option<Value>,
    /// Total row count for the attribute's partition, when a window function computed it.
    pub partition_total: Option<i64>,
}

/// A typed min/max aggregation result.
///
/// The concrete variant is chosen by the attribute's `ValueType`: string
/// attributes get lexicographic min/max, datetime attributes get temporal
/// min/max, and numeric-compatible attributes (`Number`, `Boolean`, `Object`)
/// get numeric min/max. `Uri` and `Geometry` attributes have no min/max.
#[derive(Debug, Clone)]
pub enum AggregationMinMax {
    /// Numeric min/max, for `Number`, `Boolean`, and `Object` value types.
    Number(f64),
    /// Lexicographic min/max, for the `String` value type.
    String(String),
    /// Temporal min/max, for the `DateTime` value type.
    DateTime(DateTime<Utc>),
}

/// A single decoded row from a temporal aggregation query.
///
/// One row represents one aggregation bucket for one attribute (and dataset,
/// when the attribute is multi-instance).
#[derive(Debug, Clone)]
pub struct TemporalAggregationRow {
    /// Expanded IRI of the aggregated attribute.
    pub attribute_name: String,
    /// NGSI-LD attribute meta-type as its wire string (e.g. `"Property"`).
    pub attribute_type: String,
    /// Multi-attribute discriminator; `None` for the default instance.
    pub dataset_id: Option<String>,
    /// Start of the aggregation bucket, when period-based bucketing is active.
    pub bucket_start: Option<DateTime<Utc>>,
    /// End of the aggregation bucket, when period-based bucketing is active.
    pub bucket_end: Option<DateTime<Utc>>,
    /// Arithmetic mean of numeric values in the bucket.
    pub avg: Option<f64>,
    /// Minimum value in the bucket, typed per the attribute's value type.
    pub min: Option<AggregationMinMax>,
    /// Maximum value in the bucket, typed per the attribute's value type.
    pub max: Option<AggregationMinMax>,
    /// Sum of numeric values in the bucket.
    pub sum: Option<f64>,
    /// Sum of squares of numeric values in the bucket.
    pub sumsq: Option<f64>,
    /// Standard deviation of numeric values in the bucket.
    pub stddev: Option<f64>,
    /// Total number of observations in the bucket.
    pub total_count: Option<i64>,
    /// Number of distinct values in the bucket.
    pub distinct_count: Option<i64>,
}

/// Result of the stage-1 entity ID resolution query.
///
/// Stage 1 resolves which entities match the query's filters before stage 2
/// fetches each matching entity's attribute history.
#[derive(Debug)]
pub struct EntityIdOutcome {
    /// Entity identifiers matching the query, in result order.
    pub entity_ids: Vec<String>,
    /// Total number of matching entities, when the `count` option was requested.
    pub total_count: Option<i64>,
}

/// Result of the stage-2 attribute history query for a single entity.
#[derive(Debug)]
pub struct TemporalHistoryOutcome {
    /// Decoded history rows for the entity, in the store's natural order.
    pub rows: Vec<TemporalHistoryRow>,
}

/// Whether the store can answer aggregation queries from pre-materialised
/// views (continuous aggregates, AggregatingMergeTree) or must compute
/// them on the fly from raw history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AggregationPushdown {
    /// Pre-materialised views are available for eligible bucket sizes and methods.
    Materialised,
    /// Aggregation is computed from raw history on every query.
    OnTheFly,
    /// The backend does not support aggregation at all.
    None,
}

/// Whether history writes are immediately visible to subsequent reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryVisibility {
    /// Writes are visible on the next read.
    Immediate,
    /// Writes become visible after a bounded lag.
    Eventual {
        /// Upper bound on the lag between a write and its visibility to reads.
        max_lag: std::time::Duration,
    },
}

/// Capability descriptor for the temporal store.
///
/// Tells the policy layer what the underlying storage can and cannot do,
/// so query compilation and response building can adapt without probing
/// per request.
#[derive(Debug, Clone)]
pub struct TemporalStorageProfile {
    /// Whether aggregation queries can be pushed down to pre-materialised views.
    pub aggregation_pushdown: AggregationPushdown,
    /// Whether history writes are immediately visible to reads.
    pub history_visibility: HistoryVisibility,
}

/// Read-side port for the temporal history store.
///
/// Declared by the service module and implemented by the repository module
/// (consumer-declares-the-port pattern), so the temporal service depends on
/// this trait rather than directly on PostgreSQL or `sqlx`.
#[async_trait]
pub trait TemporalHistoryReader: Send + Sync {
    /// The temporal store's capability descriptor.
    fn profile(&self) -> TemporalStorageProfile;

    /// Resolve the entity IDs matching a compiled temporal query (stage 1).
    ///
    /// The implementation handles backend-specific concerns such as the
    /// attribute-name dictionary resolution and the `attribute_current`
    /// projection barrier wait.
    async fn query_entity_ids(&self, compiled: &CompiledTemporalQuery) -> Result<EntityIdOutcome, TemporalStoreError>;

    /// Fetch the attribute history rows for a single entity (stage 2).
    async fn query_entity_history(&self, entity_id: &str, compiled: &CompiledTemporalQuery) -> Result<TemporalHistoryOutcome, TemporalStoreError>;

    /// Fetch aggregated attribute values for a single entity.
    async fn query_entity_aggregation(&self, entity_id: &str, compiled: &CompiledTemporalQuery) -> Result<Vec<TemporalAggregationRow>, TemporalStoreError>;
}

/// Write-side port for temporal mutation preconditions and instance edits.
///
/// Declared by the service module and implemented by the repository module.
/// Covers the reads and writes the temporal service performs while
/// validating and applying create/update/delete operations (clauses
/// 5.6.13--5.6.15), distinct from the bulk-write path owned by
/// `TemporalRecorder`.
#[async_trait]
pub trait TemporalMutator: Send + Sync {
    /// Return whether any temporal entity attribute exists for the entity.
    async fn entity_exists(&self, entity_id: &str) -> Result<bool, TemporalStoreError>;

    /// Return the TEA catalogue entries for a specific entity/attribute pair.
    async fn attribute_entries(&self, entity_id: &str, attribute_name: &str) -> Result<Vec<TeaEntry>, TemporalStoreError>;

    /// Return the expanded attribute names known for an entity.
    async fn attribute_names_for_entity(&self, entity_id: &str) -> Result<Vec<String>, TemporalStoreError>;

    /// Delete all temporal data for an entity. Returns the number of rows deleted.
    async fn delete_all_for_entity(&self, entity_id: &str) -> Result<u64, TemporalStoreError>;

    /// Delete a specific attribute, optionally scoped to a dataset ID. Returns the number of rows deleted.
    async fn delete_attribute(&self, entity_id: &str, attribute_name: &str, dataset_id: Option<&Urn>, scope: DeleteScope) -> Result<u64, TemporalStoreError>;

    /// Return whether a specific attribute instance exists.
    async fn get_instance(&self, instance_id: &str, entity_id: &str, attribute_name: &str) -> Result<bool, TemporalStoreError>;

    /// Delete a specific attribute instance by its instance ID.
    async fn delete_instance(&self, instance_id: &str, entity_id: &str, attribute_name: &str) -> Result<(), TemporalStoreError>;
}
