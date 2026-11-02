//! Temporal history selection and observation reads.

use crate::{
    append_intent::TemporalObservationIdentity,
    error::TemporalError,
    merged_parts::LocalPart,
    outcome::TemporalEntityIdOutcome,
    port::{
        in_effect_reads::InEffectReads,
        read_scope::{ScopeReach, TemporalReadScope},
    },
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::{
    aggr_method::AggrMethod,
    attribute_name::AttributeName,
    attribute_type::AttributeType,
    entity_type_name::EntityTypeName,
    identifier::Identifier,
};
use andromeda_ngsi_ld_model::temporal::{TemporalAggregationRow, TemporalHistoryRow};
use andromeda_ngsi_ld_query::{count_policy::CountPolicy, page_window::PageWindow, split_filter::LeafTruths, temporal::compiled::CompiledTemporalQuery};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

/// What a history read answered for one entity.
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

/// What an aggregation read answered for one entity.
#[derive(Debug)]
pub struct TemporalAggregationOutcome {
    /// The periods read, each stream's in the order the representation lists
    /// them.
    pub periods: Vec<TemporalAggregationRow>,
    /// The instant the periods were laid from (clause 4.5.19); `None` when the
    /// whole range is one period or no period was read.
    pub origin: Option<DateTime<Utc>>,
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

/// The Entities whose instances an aggregation eligibility read looks among.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShapeHolders<'a> {
    /// The one Entity a retrieval names (clause 5.7.3).
    Entity(&'a Identifier),
    /// Every Entity the query selects, before it is paginated (clause
    /// 5.7.4.4, set S8).
    Selected,
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

    /// The scope a read of `target` that begins now runs in, reaching the
    /// history `reach` names: every later call of the read takes it, so the
    /// reader inspects the branch once per read rather than once per
    /// statement.
    #[must_use = "temporal read scopes must be awaited"]
    async fn read_scope<'a>(&self, target: &'a ContextTarget, reach: ScopeReach<'_>) -> Result<TemporalReadScope<'a>, TemporalError>;

    /// Return whether the logical history view contains an entity.
    #[must_use = "temporal entity existence reads must be awaited"]
    async fn entity_exists(&self, scope: &TemporalReadScope<'_>, entity_id: &Identifier) -> Result<bool, TemporalError>;

    /// Read the entity types each named Temporal Evolution records.
    ///
    /// The temporal catalogue is the authority for them, not current state:
    /// clause 5.6.6 deletes the entity and leaves its Temporal Evolution
    /// standing, while clause 4.5.1 keeps `type` mandatory in every
    /// representation — so an answer taken from `entities` would drop the type
    /// of exactly the entities whose history outlived them. An entity the view
    /// does not contain is absent from the result.
    #[must_use = "temporal entity type reads must be awaited"]
    async fn entity_types(&self, scope: &TemporalReadScope<'_>, entity_ids: &[Identifier]) -> Result<HashMap<Identifier, Vec<EntityTypeName>>, TemporalError>;

    /// Resolve the temporal entities `window` selects, counting every match
    /// when `count` asks for it (clause 6.3.13).
    ///
    /// The page is returned in the order it is presented: ascending by
    /// identifier for a cursor window, in the query's order for an offset one.
    #[must_use = "temporal entity selection must be awaited"]
    async fn query_entity_ids(
        &self,
        scope: &TemporalReadScope<'_>,
        compiled: &CompiledTemporalQuery,
        window: &PageWindow,
        count: CountPolicy,
    ) -> Result<TemporalEntityIdOutcome, TemporalError>;

    /// The truth of every leaf of the query's value, geo and scope filters
    /// over the part of each of `entity_ids` this view holds, in the order
    /// [`SplitFilter::leaves`](andromeda_ngsi_ld_query::split_filter::SplitFilter::leaves)
    /// yields the leaves.
    ///
    /// Clause 5.7.4.4 applies those filters to an entity held in several parts
    /// once the parts are aggregated, and every leaf is existential over the
    /// instances it reads, so the part held here is judged leaf by leaf over
    /// the instances inside the window. The clause 4.11 instance in effect
    /// when the window opens is read apart, by [`Self::in_effect_instances`],
    /// because it is the latest across every part, and a Query Term crossing a
    /// Relationship hop is judged again in memory over the targets of every
    /// part ([`Self::window_relationship_instances`]). An entity this view
    /// holds no Temporal Evolution of is absent from the result.
    #[must_use = "filter leaf reads must be awaited"]
    async fn filter_leaf_truths(
        &self,
        scope: &TemporalReadScope<'_>,
        compiled: &CompiledTemporalQuery,
        entity_ids: &[Identifier],
    ) -> Result<HashMap<Identifier, LeafTruths>, TemporalError>;

    /// The instances in effect when the query's window opens, in the part of
    /// each of `entity_ids` this view holds: the latest instance of every
    /// stream of the attributes `reads` names placed before the start, a
    /// deletion included, and the Scope event in force at the start when
    /// `reads` covers it, as history rows.
    ///
    /// Clause 4.11 counts the instance in effect when the window opens among
    /// the instances valid within it, and under clause 5.7.4.4 that is the
    /// latest across every part of a split entity, which a deletion in one
    /// part can end, so each part answers its own and the parts are merged
    /// before the filters judge the survivor. A window with no instance in
    /// effect before it reads nothing, and an entity this view holds nothing
    /// in effect of is absent from the result.
    #[must_use = "in-effect instance reads must be awaited"]
    async fn in_effect_instances(
        &self,
        scope: &TemporalReadScope<'_>,
        compiled: &CompiledTemporalQuery,
        entity_ids: &[Identifier],
        reads: InEffectReads<'_>,
    ) -> Result<HashMap<Identifier, Vec<TemporalHistoryRow>>, TemporalError>;

    /// The instances inside the window of the Relationships the first hop of
    /// every Query Term of the query crosses, in the part of each of
    /// `entity_ids` this view holds, as history rows.
    ///
    /// Clause 4.9 follows a hop through every object the Relationship names,
    /// and under clause 5.7.4.4 the entity it names may be held in any part,
    /// so a split query judges a hop in memory over the targets of every part,
    /// this view's among them. An entity holding none is absent.
    #[must_use = "window Relationship reads must be awaited"]
    async fn window_relationship_instances(
        &self,
        scope: &TemporalReadScope<'_>,
        compiled: &CompiledTemporalQuery,
        entity_ids: &[Identifier],
    ) -> Result<HashMap<Identifier, Vec<TemporalHistoryRow>>, TemporalError>;

    /// Read neutral temporal observations for each of `entity_ids`, in one
    /// read whatever their number.
    ///
    /// An entity the compiled query selects nothing of is absent from the
    /// result. One whose stored history does not read back maps to that
    /// failure, so a caller can drop it alone; a failure of the read itself
    /// fails the call.
    #[must_use = "temporal history reads must be awaited"]
    async fn query_entities_history(
        &self,
        scope: &TemporalReadScope<'_>,
        entity_ids: &[Identifier],
        compiled: &CompiledTemporalQuery,
    ) -> Result<HashMap<Identifier, Result<TemporalHistoryOutcome, TemporalError>>, TemporalError>;

    /// Read neutral aggregation buckets for each of `entity_ids`, with the
    /// origin each entity's were laid from, in one read whatever their
    /// number.
    ///
    /// An entity the compiled query aggregates nothing of is absent from the
    /// result. One whose aggregates do not read back, or whose groups span
    /// inputs a requested method cannot, maps to that failure; a failure of
    /// the read itself fails the call.
    #[must_use = "temporal aggregation reads must be awaited"]
    async fn query_entities_aggregation(
        &self,
        scope: &TemporalReadScope<'_>,
        entity_ids: &[Identifier],
        compiled: &CompiledTemporalQuery,
    ) -> Result<HashMap<Identifier, Result<TemporalAggregationOutcome, TemporalError>>, TemporalError>;

    /// Read the aggregation buckets of one entity over its instances held in
    /// this view together with `answered`, the instances Context Sources
    /// answered for the rest of it.
    ///
    /// Clause 5.7.4.4 renders an entity held in several parts with the
    /// "aggregated values of Attribute instances" of the aggregated entity,
    /// which is one aggregation over the union of the parts, never a union of
    /// per-part aggregates. An instance a local one already carries the
    /// `instanceId` of is the same instance and counts once (clause 4.5.7).
    /// `local` says whether the instances held in this view are a part of the
    /// merged entity at all; when they are not, or when this view holds none of
    /// the entity, `entity_types` names it.
    #[must_use = "temporal aggregation reads must be awaited"]
    async fn query_merged_entity_aggregation(
        &self,
        scope: &TemporalReadScope<'_>,
        entity_id: &Identifier,
        local: LocalPart,
        entity_types: &[EntityTypeName],
        compiled: &CompiledTemporalQuery,
        answered: &[TemporalHistoryRow],
    ) -> Result<Vec<TemporalAggregationRow>, TemporalError>;

    /// The family-and-value-shape combinations each named attribute has ever
    /// been recorded with across the whole history view, keyed by expanded
    /// attribute name; an attribute never recorded is absent from the result.
    ///
    /// Clause 5.7.4.4 rejects an aggregated representation over an attribute
    /// ineligible for a requested method before any history is read, and the
    /// eligibility of an attribute is a property of both the family it was
    /// written as and the values it carries. Every shape the request could
    /// aggregate is among these, so when all of them admit the requested
    /// methods the request is eligible without a further read.
    #[must_use = "recorded attribute shape reads must be awaited"]
    async fn recorded_attribute_shapes(
        &self,
        scope: &TemporalReadScope<'_>,
        attributes: &[AttributeName],
    ) -> Result<HashMap<AttributeName, HashSet<RecordedAttributeShape>>, TemporalError>;

    /// The combinations among `shapes` that an instance `compiled` would
    /// aggregate carries, keyed like `shapes`: an instance of an Entity
    /// `holders` names, inside the temporal window on its axis and of a
    /// dataset `datasetId` names; an attribute none of whose combinations is
    /// carried is absent from the result.
    ///
    /// Clause 5.7.4.4 judges the eligibility of the requested Attributes of
    /// the selected Entities over those instances, so a shape the attribute
    /// holds only for another Entity, or only outside the window, refuses
    /// nothing.
    #[must_use = "held attribute shape reads must be awaited"]
    async fn held_attribute_shapes(
        &self,
        scope: &TemporalReadScope<'_>,
        holders: ShapeHolders<'_>,
        compiled: &CompiledTemporalQuery,
        shapes: &HashMap<AttributeName, HashSet<RecordedAttributeShape>>,
    ) -> Result<HashMap<AttributeName, HashSet<RecordedAttributeShape>>, TemporalError>;
}
