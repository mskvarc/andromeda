//! The concrete reasons the selected history capability cannot answer a
//! temporal request safely.
//!
//! [`TemporalError::OperationNotSupported`](crate::error::TemporalError::OperationNotSupported)
//! carries one of these rather than an ad-hoc string, so every unsupported-
//! capability failure names exactly what the history backend could not do.

use andromeda_ngsi_ld_lexicon::aggr_method::AggrMethod;
use thiserror::Error;

/// Why a temporal operation is not supported by the selected history capability.
#[derive(Debug, Error)]
pub enum TemporalOperationUnsupported {
    /// The history reader cannot evaluate one or more requested aggregation
    /// methods.
    #[error("temporal reader refused the aggregation methods {}", render_methods(.methods))]
    ReaderRefusedAggregation {
        /// The aggregation methods the reader declined.
        methods: Vec<AggrMethod>,
    },
    /// The recording sink offers no aggregation capability at all.
    #[error("recording sink has no aggregation")]
    RecordingSinkNoAggregation,
}

/// Render an aggregation-method list as its comma-separated spec tokens.
fn render_methods(methods: &[AggrMethod]) -> String {
    methods.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")
}
