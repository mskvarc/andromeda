//! Temporal history selection and observation reads.

use crate::{error::TemporalError, outcome::TemporalEntityIdOutcome};
use andromeda_ngsi_ld_lexicon::{aggr_method::AggrMethod, uri::Uri};
use andromeda_ngsi_ld_model::temporal::{TemporalAggregationRow, TemporalHistoryRow};
use andromeda_ngsi_ld_query::temporal::compiled::CompiledTemporalQuery;
use async_trait::async_trait;
use std::time::Duration;

/// Result of one entity history read.
#[derive(Debug)]
pub struct TemporalHistoryOutcome {
    /// Neutral history observations in stable temporal order.
    pub rows: Vec<TemporalHistoryRow>,
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
