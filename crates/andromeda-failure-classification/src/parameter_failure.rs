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
        | ParameterError::ContainedByWithoutJoinLevel
        | ParameterError::EntityMapLifetimeWithoutEntityMap
        | ParameterError::CursorUnavailable
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
        | ParameterError::CursorQueryChanged
        | ParameterError::ProjectionLinkedTermsWithoutJoin
        | ParameterError::ProjectionDepthExceedsJoinLevel { .. }
        | ParameterError::FilterTraversalWithoutJoin
        | ParameterError::FilterDepthExceedsJoinLevel { .. }
        | ParameterError::LinkedRetrievalUnavailable { .. }
        | ParameterError::InlineJoinWithoutInstances(_)
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
    use crate::{parameter_failure::classify_parameter_error, query_failure::classify_query_error};
    use andromeda_http_headers::media_type::ResponseMediaType;
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
    use andromeda_ngsi_ld_query::filter::{complexity::FilterComplexityLimits, expression::Expr, language_targeting::LanguageTargeting};
    use andromeda_ngsi_ld_request::{error::ParameterError, pagination::page_size::PageSizeExceeded};
    use pretty_assertions::assert_eq;
    use std::num::NonZeroUsize;

    /// A `q` refused once, at request validation, is the parameter error
    /// wrapping the language's own refusal, and answers exactly as that
    /// refusal does: `TooComplexQuery` naming the count for a filter past the
    /// limits (clause 5.5.6), `BadRequestData` for a target language the level
    /// does not admit.
    #[test]
    fn a_q_refused_at_validation_is_answered_as_the_language_refuses_it() {
        let limits = FilterComplexityLimits::new(
            NonZeroUsize::new(2).expect("a positive comparison limit"),
            NonZeroUsize::new(1).expect("a positive nesting limit"),
        );
        let too_many = limits.admit(&"a==1,2,3".parse::<Expr>().expect("a filter")).expect_err("too many comparisons");
        let too_deep = limits.admit(&"a;(b|c)".parse::<Expr>().expect("a filter")).expect_err("nested too deep");
        let tagged: Expr = "name[en-GB]==\"x\"".parse().expect("a filter");
        let tag = LanguageTargeting::Specification.admit(&tagged).expect_err("a full tag needs the tags level");
        let expected = [
            NgsiLdErrorType::TooComplexQuery,
            NgsiLdErrorType::TooComplexQuery,
            NgsiLdErrorType::BadRequestData,
        ];
        for (refusal, error_type) in [too_many, too_deep, tag].into_iter().zip(expected) {
            let direct = classify_query_error(&refusal);
            let wrapped = classify_parameter_error(&ParameterError::Query(refusal));
            assert_eq!(wrapped, direct);
            assert_eq!(wrapped.error_type(), error_type);
        }
        let comparisons = classify_parameter_error(&ParameterError::Query(
            limits.admit(&"a==1,2,3".parse::<Expr>().expect("a filter")).expect_err("too many comparisons"),
        ));
        assert!(comparisons.detail().contains("3 comparisons"), "{}", comparisons.detail());
    }

    /// Clauses 5.7.2.4 and 5.14.4.4: a hop refused at validation answers
    /// `BadRequestData`, whichever variant of the Linked Entity admission
    /// refused it.
    #[test]
    fn a_hop_refused_at_validation_is_bad_request_data() {
        for error in [
            ParameterError::FilterTraversalWithoutJoin,
            ParameterError::FilterDepthExceedsJoinLevel { depth: 2, join_level: 1 },
        ] {
            assert_eq!(classify_parameter_error(&error).error_type(), NgsiLdErrorType::BadRequestData, "{error:?}");
        }
    }

    /// Clause 6.3.20: a cursor where the collection offers no cursor walk is a
    /// parameter the operation does not offer.
    #[test]
    fn cursor_unavailable_is_invalid_request() {
        assert_eq!(
            classify_parameter_error(&ParameterError::CursorUnavailable).error_type(),
            NgsiLdErrorType::InvalidRequest
        );
    }

    /// Clause 6.3.20: a `joinLevel` with no join to bound, and an aggregation
    /// parameter with no aggregated representation, are parameters
    /// incompatible with the request.
    #[test]
    fn a_dependent_parameter_without_its_dependency_is_invalid_request() {
        for error in [
            ParameterError::JoinLevelWithoutJoin,
            ParameterError::AggregationWithoutAggregatedValues,
            ParameterError::ContainedByWithoutJoinLevel,
            ParameterError::EntityMapLifetimeWithoutEntityMap,
        ] {
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
