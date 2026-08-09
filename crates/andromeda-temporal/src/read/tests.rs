//! Capability-refusal tests for temporal read policy.

use crate::{
    error::TemporalError,
    outcome::TemporalEntityIdOutcome,
    port::history_reader::{AggregationPushdown, HistoryVisibility, TemporalHistoryOutcome, TemporalHistoryReader, TemporalReaderCapabilities},
    read::ensure_aggregation_supported,
};
use andromeda_ngsi_ld_lexicon::{aggr_method::AggrMethod, uri::Uri};
use andromeda_ngsi_ld_model::temporal::TemporalAggregationRow;
use andromeda_ngsi_ld_query::temporal::compiled::CompiledTemporalQuery;
use async_trait::async_trait;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Reader that records any query invocation while refusing all aggregation.
struct RefusingReader {
    /// Number of backend query methods invoked.
    query_calls: AtomicUsize,
}

#[async_trait]
impl TemporalHistoryReader for RefusingReader {
    fn capabilities(&self) -> TemporalReaderCapabilities {
        TemporalReaderCapabilities {
            aggregation: AggregationPushdown::None,
            history_visibility: HistoryVisibility::Immediate,
        }
    }

    async fn query_entity_ids(&self, _compiled: &CompiledTemporalQuery) -> Result<TemporalEntityIdOutcome, TemporalError> {
        self.query_calls.fetch_add(1, Ordering::Relaxed);
        Err(TemporalError::internal("query must not execute"))
    }

    async fn query_entity_history(&self, _entity_id: &Uri, _compiled: &CompiledTemporalQuery) -> Result<TemporalHistoryOutcome, TemporalError> {
        self.query_calls.fetch_add(1, Ordering::Relaxed);
        Err(TemporalError::internal("query must not execute"))
    }

    async fn query_entity_aggregation(&self, _entity_id: &Uri, _compiled: &CompiledTemporalQuery) -> Result<Vec<TemporalAggregationRow>, TemporalError> {
        self.query_calls.fetch_add(1, Ordering::Relaxed);
        Err(TemporalError::internal("query must not execute"))
    }
}

#[test]
fn aggregation_refusal_precedes_every_reader_query() {
    let reader = RefusingReader {
        query_calls: AtomicUsize::new(0),
    };
    let error = ensure_aggregation_supported(&reader, &[AggrMethod::Avg]).expect_err("reader must refuse aggregation");
    assert!(matches!(error, TemporalError::OperationNotSupported { .. }));
    pretty_assertions::assert_eq!(reader.query_calls.load(Ordering::Relaxed), 0);
}
