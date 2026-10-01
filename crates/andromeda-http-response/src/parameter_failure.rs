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
        | ParameterError::WildcardTypeWithLocalFalse => ClassifiedReport::public(NgsiLdErrorType::InvalidRequest, error.to_string()),
        ParameterError::IdPatternEmpty
        | ParameterError::OrderGeometryCollection
        | ParameterError::DropKeepExclusive
        | ParameterError::DatasetIdWithDeleteAll
        | ParameterError::Projection(_)
        | ParameterError::LimitZeroWithoutCount
        | ParameterError::CursorWithOffset
        | ParameterError::CursorInvalid(_)
        | ParameterError::ProjectionLinkedTermsWithoutJoin
        | ParameterError::ProjectionDepthExceedsJoinLevel { .. }
        | ParameterError::FilterTraversalWithoutJoin
        | ParameterError::FilterDepthExceedsJoinLevel { .. }
        | ParameterError::PurgeNamesLinkedEntities(_)
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
