//! Temporal history selection and observation reads.

use crate::{append_intent::TemporalObservationIdentity, error::TemporalError, outcome::TemporalEntityIdOutcome};
use andromeda_ngsi_ld_lexicon::{aggr_method::AggrMethod, entity_type_name::EntityTypeName, uri::Uri};
use andromeda_ngsi_ld_model::temporal::{TemporalAggregationRow, TemporalHistoryRow};
use andromeda_ngsi_ld_query::temporal::compiled::CompiledTemporalQuery;
use async_trait::async_trait;
use std::{collections::HashMap, time::Duration};

/// Result of one entity history read.
#[derive(Debug)]
pub struct TemporalHistoryOutcome {
    /// Neutral history observations in stable temporal order with optional append provenance.
    pub observations: Vec<TemporalHistoryObservation>,
}

impl TemporalHistoryOutcome {
    /// Consume the outcome and discard adapter provenance after overlay deduplication.
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
    async fn entity_exists(&self, entity_id: &Uri) -> Result<bool, TemporalError>;

    /// Read the entity types each named Temporal Evolution records.
    ///
    /// The temporal catalogue is the authority for them, not current state:
    /// clause 5.6.6 deletes the entity and leaves its Temporal Evolution
    /// standing, while clause 4.5.1 keeps `type` mandatory in every
    /// representation — so an answer taken from `entities` would drop the type
    /// of exactly the entities whose history outlived them. An entity the view
    /// does not contain is absent from the result.
    #[must_use = "temporal entity type reads must be awaited"]
    async fn entity_types(&self, entity_ids: &[Uri]) -> Result<HashMap<Uri, Vec<EntityTypeName>>, TemporalError>;

    /// Resolve temporal entity candidates and report every unapplied predicate and window operation.
    #[must_use = "temporal entity selection must be awaited"]
    async fn query_entity_ids(&self, compiled: &CompiledTemporalQuery) -> Result<TemporalEntityIdOutcome, TemporalError>;

    /// Read neutral temporal observations for one entity.
    #[must_use = "temporal history reads must be awaited"]
    async fn query_entity_history(&self, entity_id: &Uri, compiled: &CompiledTemporalQuery) -> Result<TemporalHistoryOutcome, TemporalError>;

    /// Read neutral aggregation buckets for one entity.
    #[must_use = "temporal aggregation reads must be awaited"]
    async fn query_entity_aggregation(&self, entity_id: &Uri, compiled: &CompiledTemporalQuery) -> Result<Vec<TemporalAggregationRow>, TemporalError>;
}
