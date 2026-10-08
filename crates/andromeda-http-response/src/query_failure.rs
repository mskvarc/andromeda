//! Classifying an NGSI-LD query-language failure.

use crate::classified_report::ClassifiedReport;
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use andromeda_ngsi_ld_query::{error::QueryError, term_expander::TermExpansionFailure};

/// Classify a typed NGSI-LD query-language failure.
#[must_use]
pub fn classify_query_error(error: &QueryError) -> ClassifiedReport {
    match error {
        QueryError::QEmpty
        | QueryError::QUnterminatedString
        | QueryError::QInvalidStringEscape { .. }
        | QueryError::QInvalidUnicodeEscape { .. }
        | QueryError::QUnescapedControlCharacter { .. }
        | QueryError::QUnexpectedToken { .. }
        | QueryError::QExpected { .. }
        | QueryError::QAmbiguousTypeGuard
        | QueryError::QUnexpectedChar { .. }
        | QueryError::QAttributePathTooDeep { .. }
        | QueryError::TypeExprEmpty
        | QueryError::TypeExprUnexpectedEnd
        | QueryError::TypeExprUnbalanced(_)
        | QueryError::TypeExprUnexpected { .. }
        | QueryError::TypeExprInvalidName { .. }
        | QueryError::GeoIncomplete
        | QueryError::ProjectionUnbalancedBrace { .. }
        | QueryError::ProjectionEmptyTerm { .. }
        // A name outside the clause 4.6.2 grammar is a syntactically invalid
        // request parameter, not a broker fault: every one of these reaches the
        // parser from `q`, `attrs`, `orderBy`, `pick`/`omit` or `geoproperty`,
        // all of them client text.
        | QueryError::AttributeNameInvalid { .. } => ClassifiedReport::public(NgsiLdErrorType::InvalidRequest, error.to_string()),
        QueryError::QInvalidNumber { .. }
        | QueryError::QInvalidDateTime { .. }
        | QueryError::QInvalidDate { .. }
        | QueryError::QInvalidTime { .. }
        | QueryError::QBareWord
        | QueryError::PatternRegex(_)
        | QueryError::PatternRepeatedQuantifier
        | QueryError::PatternNonPosixConstruct { .. }
        | QueryError::PatternTranslation(_)
        | QueryError::GeoRel(_)
        | QueryError::GeoCoordinatesInvalid(_)
        | QueryError::ReferenceGeometry(_)
        | QueryError::NearWithoutDistance
        | QueryError::NearDistanceBand
        | QueryError::DistanceOrderWithoutReference
        | QueryError::ScopePatternInvalid(_)
        | QueryError::IcuCollation(_)
        | QueryError::TemporalOrderingMember(_)
        | QueryError::TemporalOrderingDirection(_)
        | QueryError::ParamTimerelTimeatPair
        | QueryError::ParamAggrPeriodDurationInvalid { .. }
        | QueryError::ParamDatasetIdInvalid { .. }
        | QueryError::ParamEndTimeAtRequired
        | QueryError::ProjectionKeywordWithLinkedTerm { .. } => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, error.to_string()),
        // Clause 5.5.6: a query "so complex that cannot be resolved" raises
        // TooComplexQuery, which Table 6.3.2-1 answers with 403.
        QueryError::ReferenceGeometryTooComplex { .. } => ClassifiedReport::public(NgsiLdErrorType::TooComplexQuery, error.to_string()),
        // Table 5.5.2-1: the terms and the @context both come with the request,
        // so a term naming no IRI under it (a keyword alias included) is input
        // that "does not meet the requirements of the operation", as the same
        // term is on a write.
        QueryError::TermExpansion(TermExpansionFailure::NotExpanded | TermExpansionFailure::NotAnIri | TermExpansionFailure::ContextUnprocessable) => {
            ClassifiedReport::public(NgsiLdErrorType::BadRequestData, error.to_string())
        }
        QueryError::TermExpansion(TermExpansionFailure::ContextUnavailable) => ClassifiedReport::public(NgsiLdErrorType::LdContextNotAvailable, error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use crate::query_failure::classify_query_error;
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
    use andromeda_ngsi_ld_query::{error::QueryError, term_expander::TermExpansionFailure};

    #[test]
    fn a_term_naming_no_iri_under_the_request_context_is_bad_request_data() {
        for failure in [
            TermExpansionFailure::NotExpanded,
            TermExpansionFailure::NotAnIri,
            TermExpansionFailure::ContextUnprocessable,
        ] {
            let error = QueryError::TermExpansion(failure);
            pretty_assertions::assert_eq!(classify_query_error(&error).error_type(), NgsiLdErrorType::BadRequestData, "{failure:?}");
        }
    }

    #[test]
    fn a_context_that_cannot_be_retrieved_is_ld_context_not_available() {
        let error = QueryError::TermExpansion(TermExpansionFailure::ContextUnavailable);
        pretty_assertions::assert_eq!(classify_query_error(&error).error_type(), NgsiLdErrorType::LdContextNotAvailable);
    }
}
