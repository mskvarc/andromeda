//! Classifying a request-parameter failure.

use crate::{classified_report::ClassifiedReport, query_failure::classify_query_error};
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use andromeda_ngsi_ld_request::error::ParameterError;

/// Classify a parameter-model failure using its typed syntax-versus-value distinction.
#[must_use]
pub fn classify_parameter_error(error: &ParameterError) -> ClassifiedReport {
    match error {
        ParameterError::Query(source) => classify_query_error(source),
        // Clause 5.7.4.4 names the ineligible-aggregation failure an
        // InvalidRequest, beside the clause 6.3.20 parameter incompatibilities,
        // of which `type=*` contradicted by `local=false` is one.
        // The grouping-extension parameter incompatibilities are clause 6.3.20
        // InvalidRequest failures, beside the standard ones (study §4.9).
        ParameterError::OptionsInvalid(_)
        | ParameterError::FormatInvalid(_)
        | ParameterError::MergeFormatInvalid(_)
        | ParameterError::ResponseFormatHeaderInvalid(_)
        | ParameterError::AggregationIneligible { .. }
        | ParameterError::AggrGroupByWithoutGroupedValues
        | ParameterError::AggrGroupByInvalid(_)
        | ParameterError::AggrGroupByOnAggregatedAttribute(_)
        | ParameterError::LastNWithGrouping
        | ParameterError::CursorUnavailable
        | ParameterError::PaginationUndefined
        | ParameterError::WildcardTypeWithLocalFalse => ClassifiedReport::public(NgsiLdErrorType::InvalidRequest, error.to_string()),
        ParameterError::IdPatternEmpty
        | ParameterError::OrderGeometryCollection
        | ParameterError::DropKeepExclusive
        | ParameterError::DatasetIdWithDeleteAll
        | ParameterError::Projection(_)
        | ParameterError::LimitZeroWithoutCount
        | ParameterError::CursorWithOffset
        | ParameterError::CursorWithOrdering
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
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
    use andromeda_ngsi_ld_request::error::ParameterError;
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
}
