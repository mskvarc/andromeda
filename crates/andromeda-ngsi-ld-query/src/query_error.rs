//! The error of the NGSI-LD query languages as a whole.
//!
//! Each language refuses with its own error, and parsing returns it. A consumer
//! that reports a refusal of any language, or a step that can fail in more than
//! one (expansion fails in the language or in the `@context`), reports through
//! [`QueryError`], which holds each language error unchanged and adds the one
//! failure that belongs to no language: a term the `@context` does not expand.
//!
//! Rendering a compiled query as SQL can fail for reasons of its own (a bind value with no typed counterpart, a reference geometry that will not encode), and those belong to the backend that renders it, not here.
//!
//! Faults of a *request* rather than of a language (a `format` value naming no
//! rendering, `drop` and `keep` sent together) belong to the parameter model
//! that reads them, not here.
//!
//! Mapping onto the ETSI CIM 009 clause 5.5.2 error types lives with the
//! broker's transport-facing error type, not here.

use crate::{
    dataset_selector::DatasetSelectorError,
    filter::filter_error::FilterError,
    geo::geo_query_error::GeoQueryError,
    ordering::ordering_error::OrderingError,
    pattern::pattern_error::PatternError,
    projection::projection_error::ProjectionError,
    scope::scope_query_error::ScopeQueryError,
    temporal::{aggregation_period::AggregationPeriodError, time_interval_error::TimeIntervalError},
    term_expander::TermExpansionError,
    type_selection::type_selection_error::TypeSelectionError,
};
use thiserror::Error;

/// A refusal of one of the query languages, or a term the `@context` does not
/// expand.
#[derive(Debug, Error)]
pub enum QueryError {
    /// A `q` or `csf` filter, or a `ValuePath`, outside clause 4.9.
    #[error(transparent)]
    Filter(#[from] FilterError),
    /// An entity type selection outside clause 4.17.
    #[error(transparent)]
    TypeSelection(#[from] TypeSelectionError),
    /// A `scopeQ` outside clause 4.19.
    #[error(transparent)]
    ScopeQuery(#[from] ScopeQueryError),
    /// A `pick`, `omit` or `attrs` projection the broker refuses (clause 4.21).
    #[error(transparent)]
    Projection(#[from] ProjectionError),
    /// An ordering the broker refuses (clause 4.23).
    #[error(transparent)]
    Ordering(#[from] OrderingError),
    /// An `idPattern` regular expression the broker refuses.
    #[error(transparent)]
    Pattern(#[from] PatternError),
    /// A geo-query the broker refuses (clause 4.10).
    #[error(transparent)]
    GeoQuery(#[from] GeoQueryError),
    /// `timerel`, `timeAt` and `endTimeAt` that bound no interval (clause 4.11).
    #[error(transparent)]
    TimeInterval(#[from] TimeIntervalError),
    /// An `aggrPeriodDuration` that is no duration (clause 4.5.19).
    #[error(transparent)]
    AggregationPeriod(#[from] AggregationPeriodError),
    /// A `datasetId` that names no dataset (clause 4.5.5).
    #[error(transparent)]
    DatasetSelector(#[from] DatasetSelectorError),
    /// A compact term could not be expanded through the active `@context`.
    ///
    /// The expander is a [`TermExpander`](crate::term_expander::TermExpander)
    /// implementation chosen by the caller, so the cause is the expander's own
    /// classification of its failure rather than a type this module would have
    /// to name the JSON-LD layer to hold.
    #[error("could not expand a term: {0}")]
    TermExpansion(#[from] TermExpansionError),
}

#[cfg(test)]
mod tests {
    use crate::{
        filter::filter_error::FilterError,
        geo::geo_query_error::GeoQueryError,
        ordering::ordering_error::OrderingError,
        pattern::pattern_error::PatternError,
        projection::projection_error::ProjectionError,
        query_error::QueryError,
        scope::scope_query_error::ScopeQueryError,
        temporal::time_interval_error::TimeIntervalError,
        type_selection::type_selection_error::TypeSelectionError,
    };

    /// The text `error` displays alone, and the text it displays read through
    /// the aggregate.
    fn read_through_the_aggregate<E: Into<QueryError> + ToString>(error: E) -> (String, String) {
        let own = error.to_string();
        (own, error.into().to_string())
    }

    #[test]
    fn every_language_error_reads_through_the_aggregate_unchanged() {
        for ((own, aggregate), expected) in [
            (read_through_the_aggregate(FilterError::Empty), "q expression: empty input"),
            (read_through_the_aggregate(TypeSelectionError::Empty), "type expression: empty input"),
            (
                read_through_the_aggregate(ScopeQueryError::NestedGroup),
                "invalid scopeQ pattern: a parenthesised group may only hold scope patterns joined by ;",
            ),
            (
                read_through_the_aggregate(ProjectionError::EmptyTerm { position: 2 }),
                "projection: empty term at position 2",
            ),
            (
                read_through_the_aggregate(OrderingError::LanguageSuppression),
                "invalid orderBy member: the [*] language marker belongs to the q filter grammar, not to orderBy",
            ),
            (
                read_through_the_aggregate(PatternError::RepeatedQuantifier),
                "invalid POSIX regular expression: a repetition operator cannot follow another repetition operator",
            ),
            (
                read_through_the_aggregate(GeoQueryError::Incomplete),
                "geo: all of georel, geometry, coordinates must be provided together",
            ),
            (
                read_through_the_aggregate(TimeIntervalError::EndTimeAtRequired),
                "endTimeAt is required when timerel is 'between'",
            ),
        ] {
            pretty_assertions::assert_eq!(own, expected);
            pretty_assertions::assert_eq!(aggregate, expected);
        }
    }
}
