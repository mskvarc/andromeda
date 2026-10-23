//! Classifying a request-parameter failure.

use crate::{classified_report::ClassifiedReport, query_failure::classify_query_error};
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use andromeda_ngsi_ld_request::error::ParameterError;

/// Classify a parameter-model failure using its typed syntax-versus-value distinction.
#[must_use]
pub fn classify_parameter_error(error: &ParameterError) -> ClassifiedReport {
    match error {
        ParameterError::Query(source) => classify_query_error(source),
        // Clause 5.5.6 answers a request the broker will not serve whole with
        // `TooManyResults`, whose threshold it leaves to the implementation.
        ParameterError::PageSizeExceeded(_) => ClassifiedReport::public(NgsiLdErrorType::TooManyResults, error.to_string()),
        // Clause 5.7.4.4 names the ineligible-aggregation failure an
        // InvalidRequest, beside the clause 6.3.20 parameter incompatibilities,
        // of which `type=*` contradicted by `local=false` is one.
        // The grouping-extension parameter incompatibilities are clause 6.3.20
        // InvalidRequest failures, beside the standard ones.
        ParameterError::OptionsInvalid(_)
        | ParameterError::FormatInvalid(_)
        | ParameterError::MergeFormatInvalid(_)
        | ParameterError::AggregationIneligible { .. }
        | ParameterError::AggrGroupByWithoutGroupedValues
        | ParameterError::AggrGroupByInvalid(_)
        | ParameterError::AggrGroupByOnAggregatedAttribute(_)
        | ParameterError::LastNWithGrouping
        | ParameterError::AggregationWithoutAggregatedValues
        | ParameterError::JoinLevelWithoutJoin
        | ParameterError::CursorUnavailable
        | ParameterError::PaginationUndefined
        | ParameterError::WildcardTypeWithLocalFalse => ClassifiedReport::public(NgsiLdErrorType::InvalidRequest, error.to_string()),
        ParameterError::IdPatternEmpty
        | ParameterError::OrderGeometryCollection
        | ParameterError::DropKeepExclusive
        | ParameterError::DatasetIdWithDeleteAll
        | ParameterError::GeometryPropertyWithoutGeoJson(_)
        | ParameterError::Projection(_)
        | ParameterError::LimitZeroWithoutCount
        | ParameterError::CursorWithOffset
        | ParameterError::CursorWithOrdering
        | ParameterError::PageContextChanged
        | ParameterError::ProjectionLinkedTermsWithoutJoin
        | ParameterError::ProjectionDepthExceedsJoinLevel { .. }
        | ParameterError::FilterTraversalWithoutJoin
        | ParameterError::FilterDepthExceedsJoinLevel { .. }
        | ParameterError::LinkedRetrievalUnavailable { .. }
        | ParameterError::InlineJoinWithoutInstances(_)
        | ParameterError::AttrsReservedMember(_)
        | ParameterError::TimerelTimeatRequired
        | ParameterError::SelectorRequired(_)
        | ParameterError::AggrMethodsRequired
        | ParameterError::EntityQueryTemporalMembers
        | ParameterError::TemporalQueryMissing
        | ParameterError::TemporalAggregationConflict
        | ParameterError::OrderingWithParticipatingSource => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use crate::parameter_failure::classify_parameter_error;
    use andromeda_http_headers::media_type::ResponseMediaType;
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
    use andromeda_ngsi_ld_request::{error::ParameterError, pagination::page_size::PageSizeExceeded};
    use pretty_assertions::assert_eq;

    /// Clause 6.3.20: a cursor where the collection offers no cursor walk is a
    /// parameter the operation does not offer.
    #[test]
    fn cursor_unavailable_is_invalid_request() {
        assert_eq!(
            classify_parameter_error(&ParameterError::CursorUnavailable).error_type(),
            NgsiLdErrorType::InvalidRequest
        );
    }

    /// Clause 6.3.20: Purge Entities defines no pagination parameter.
    #[test]
    fn pagination_undefined_is_invalid_request() {
        assert_eq!(
            classify_parameter_error(&ParameterError::PaginationUndefined).error_type(),
            NgsiLdErrorType::InvalidRequest
        );
    }

    /// Clause 6.3.20: a `joinLevel` with no join to bound, and an aggregation
    /// parameter with no aggregated representation, are parameters
    /// incompatible with the request.
    #[test]
    fn a_dependent_parameter_without_its_dependency_is_invalid_request() {
        for error in [ParameterError::JoinLevelWithoutJoin, ParameterError::AggregationWithoutAggregatedValues] {
            assert_eq!(classify_parameter_error(&error).error_type(), NgsiLdErrorType::InvalidRequest, "{error:?}");
        }
    }

    /// Clause 5.5.6: a `limit` above the cap is `TooManyResults`, and the
    /// detail names the cap.
    #[test]
    fn a_limit_above_the_cap_is_too_many_results_naming_the_cap() {
        let report = classify_parameter_error(&ParameterError::PageSizeExceeded(PageSizeExceeded { requested: 1001, cap: 1000 }));
        assert_eq!(report.error_type(), NgsiLdErrorType::TooManyResults);
        assert!(report.detail().contains("at most 1000"), "{}", report.detail());
    }

    /// A cursor over a requested ordering is input the operation cannot meet.
    #[test]
    fn cursor_with_ordering_is_bad_request_data() {
        assert_eq!(
            classify_parameter_error(&ParameterError::CursorWithOrdering).error_type(),
            NgsiLdErrorType::BadRequestData
        );
    }

    /// Clause 5.5.9: `cursor` beside `offset` is input the operation cannot meet.
    #[test]
    fn cursor_with_offset_is_bad_request_data() {
        assert_eq!(
            classify_parameter_error(&ParameterError::CursorWithOffset).error_type(),
            NgsiLdErrorType::BadRequestData
        );
    }

    /// Clauses 5.7.1.4 and 5.7.2.4: `geometryProperty` without a `GeoJSON`
    /// `Accept` is `BadRequestData`.
    #[test]
    fn geometry_property_without_geojson_is_bad_request_data() {
        assert_eq!(
            classify_parameter_error(&ParameterError::GeometryPropertyWithoutGeoJson(ResponseMediaType::Json)).error_type(),
            NgsiLdErrorType::BadRequestData
        );
    }
}
