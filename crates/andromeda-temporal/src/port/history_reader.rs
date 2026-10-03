//! Temporal history selection and observation reads.

use crate::{append_intent::TemporalObservationIdentity, error::TemporalError, outcome::TemporalEntityIdOutcome};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::{
    aggr_method::AggrMethod,
    attribute_name::AttributeName,
    attribute_type::AttributeType,
    entity_type_name::EntityTypeName,
    identifier::Identifier,
};
use andromeda_ngsi_ld_model::temporal::{TemporalAggregationRow, TemporalHistoryRow};
use andromeda_ngsi_ld_query::{count_policy::CountPolicy, page_window::PageWindow, temporal::compiled::CompiledTemporalQuery};
use async_trait::async_trait;
use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

/// Result of one entity history read.
#[derive(Debug)]
pub struct TemporalHistoryOutcome {
    /// Neutral history observations in stable temporal order with optional append provenance.
    pub observations: Vec<TemporalHistoryObservation>,
}

impl TemporalHistoryOutcome {
    /// Consume the outcome and discard adapter provenance after overlay deduplication.
    #[must_use]
    pub fn into_rows(self) -> Vec<TemporalHistoryRow> {
        self.observations.into_iter().map(|observation| observation.row).collect()
    }
}

/// One stored observation and the idempotent append identity that produced it.
#[derive(Debug, Clone, PartialEq)]
pub struct TemporalHistoryObservation {
    /// Backend-neutral temporal row.
    pub row: TemporalHistoryRow,
    /// Intent identity when the sink retained append provenance.
    pub identity: Option<TemporalObservationIdentity>,
}

/// The shape of a value an attribute has been recorded with, as far as the
/// aggregation eligibility of Tables 4.5.19.1-1, 4.5.19.1-2 and 4.5.19.1-3 is
/// concerned.
///
/// Backend-neutral: a store maps whatever it keeps in its value columns onto
/// these. One member per column of those tables and no fewer — a store that
/// folds two columns of a table into one storage representation still has to
/// tell them apart here, because the tables give them different aggregation
/// methods. A JSON array is not a JSON object (Table 4.5.19.1-1 defines
/// `avg`/`min`/`max`/`sum` over array sizes and nothing at all over objects),
/// and a `Time` is neither a `Date` nor a `DateTime` (Table 4.5.19.1-2 defines
/// `avg` over `Time` alone).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecordedValueKind {
    /// A JSON number.
    Number,
    /// A JSON string that is none of the clause 4.6.3 temporal forms.
    String,
    /// A JSON boolean.
    Boolean,
    /// A `GeoJSON` geometry.
    Geometry,
    /// A JSON object.
    Object,
    /// A JSON array, whose aggregates are taken over its size.
    Array,
    /// A clause 4.6.3 `DateTime`: a point in time.
    DateTime,
    /// A clause 4.6.3 `Date`: a calendar day carrying no time of day.
    Date,
    /// A clause 4.6.3 `Time`: a time of day carrying no calendar day.
    Time,
    /// A URI, which is how a Relationship object is held.
    Uri,
}

/// One way an attribute has been recorded: the family the instance was written
/// as, and the shape its value took.
///
/// Aggregation eligibility needs both, because the three tables of clause
/// 4.5.19.1 are not indexed the same way. Tables 4.5.19.1-1 and 4.5.19.1-2 give
/// one column per value datatype and govern the Property families; Table
/// 4.5.19.1-3 has one column and no datatype at all, and governs Relationships
/// — and, through clause 4.5.22.1's "specialized type of Relationship",
/// `ListRelationship` with them. A shape read without its family answers out of
/// the wrong table: a `ListRelationship` holds its `objectList` as an array,
/// whose column in Table 4.5.19.1-1 defines `avg`, `min`, `max` and `sum` over
/// array sizes, while Table 4.5.19.1-3 marks every one of them N/A.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RecordedAttributeShape {
    /// The attribute family the instance was written as (clause 5.2.4).
    pub family: AttributeType,
    /// The shape the instance's value was recorded in.
    pub value_kind: RecordedValueKind,
}

/// Whether aggregation is materialized, computed from raw history, or unavailable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AggregationPushdown {
    /// Pre-materialized aggregates are available for eligible requests.
    Materialized,
    /// Aggregation is computed from raw history for each request.
    OnTheFly,
    /// Aggregation is not supported by this reader.
    None,
}

/// Whether committed history is immediately visible to subsequent reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryVisibility {
    /// Writes are visible on the next read.
    Immediate,
    /// Writes become visible only after the bounded lag.
    Eventual {
        /// Maximum expected visibility lag.
        maximum_lag: Duration,
    },
}

/// Capabilities of one selected temporal history reader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TemporalReaderCapabilities {
    /// Aggregation execution capability.
    pub aggregation: AggregationPushdown,
    /// Committed-history visibility capability.
    pub history_visibility: HistoryVisibility,
}

/// Reads compiled temporal history without exposing adapter rows or query languages.
#[async_trait]
pub trait TemporalHistoryReader: Send + Sync {
    /// Report this reader's narrow temporal capabilities.
    fn capabilities(&self) -> TemporalReaderCapabilities;

    /// Return requested aggregation methods this reader refuses.
    fn refused_aggregation_methods(&self, requested: &[AggrMethod]) -> Vec<AggrMethod> {
        match self.capabilities().aggregation {
            AggregationPushdown::Materialized | AggregationPushdown::OnTheFly => Vec::new(),
            AggregationPushdown::None => requested.to_vec(),
        }
    }

    /// Return whether the logical history view contains an entity.
    #[must_use = "temporal entity existence reads must be awaited"]
    async fn entity_exists(&self, target: &ContextTarget, entity_id: &Identifier) -> Result<bool, TemporalError>;

    /// Read the entity types each named Temporal Evolution records.
    ///
    /// The temporal catalogue is the authority for them, not current state:
    /// clause 5.6.6 deletes the entity and leaves its Temporal Evolution
    /// standing, while clause 4.5.1 keeps `type` mandatory in every
    /// representation — so an answer taken from `entities` would drop the type
    /// of exactly the entities whose history outlived them. An entity the view
    /// does not contain is absent from the result.
    #[must_use = "temporal entity type reads must be awaited"]
    async fn entity_types(&self, target: &ContextTarget, entity_ids: &[Identifier]) -> Result<HashMap<Identifier, Vec<EntityTypeName>>, TemporalError>;

    /// Resolve the temporal entity candidates `window` selects, counting every
    /// match when `count` asks for it (clause 6.3.13), and report every
    /// unapplied predicate and window operation.
    ///
    /// An applied window is returned in the order it is presented: ascending by
    /// identifier for a cursor window, in the query's order for an offset one.
    /// A reader that defers the window returns every candidate in that order.
    #[must_use = "temporal entity selection must be awaited"]
    async fn query_entity_ids(
        &self,
        target: &ContextTarget,
        compiled: &CompiledTemporalQuery,
        window: &PageWindow,
        count: CountPolicy,
    ) -> Result<TemporalEntityIdOutcome, TemporalError>;

    /// Read neutral temporal observations for one entity.
    #[must_use = "temporal history reads must be awaited"]
    async fn query_entity_history(
        &self,
        target: &ContextTarget,
        entity_id: &Identifier,
        compiled: &CompiledTemporalQuery,
    ) -> Result<TemporalHistoryOutcome, TemporalError>;

    /// Read neutral aggregation buckets for one entity.
    #[must_use = "temporal aggregation reads must be awaited"]
    async fn query_entity_aggregation(
        &self,
        target: &ContextTarget,
        entity_id: &Identifier,
        compiled: &CompiledTemporalQuery,
    ) -> Result<Vec<TemporalAggregationRow>, TemporalError>;

    /// The family-and-value-shape combinations each named attribute has ever
    /// been recorded with across the whole history view, keyed by expanded
    /// attribute name; an attribute never recorded is absent from the result.
    ///
    /// Clause 5.7.4.4 rejects an aggregated representation over an attribute
    /// ineligible for a requested method before any history is read, and the
    /// eligibility of an attribute is a property of both the family it was
    /// written as and the values it carries.
    #[must_use = "recorded attribute shape reads must be awaited"]
    async fn recorded_attribute_shapes(
        &self,
        target: &ContextTarget,
        attributes: &[AttributeName],
    ) -> Result<HashMap<AttributeName, HashSet<RecordedAttributeShape>>, TemporalError>;
}
