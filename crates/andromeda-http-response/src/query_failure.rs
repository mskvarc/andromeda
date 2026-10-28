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
        // A member that is neither a name nor a tag is well-formed `q` text
        // naming no target, as is a full tag or a range the broker's
        // `q_target_languages` level does not admit.
        | QueryError::TrailingMemberInvalid { .. }
        | QueryError::LanguageTagNotAdmitted(_)
        | QueryError::LanguageRangeNotAdmitted(_)
        | QueryError::PatternRegex(_)
        | QueryError::PatternRepeatedQuantifier
        | QueryError::PatternNonPosixConstruct { .. }
        | QueryError::PatternTranslation(_)
        | QueryError::Unholdable(_)
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
        | QueryError::ParamAggrPeriodDurationInvalid(_)
        | QueryError::ParamDatasetIdInvalid { .. }
        | QueryError::ParamEndTimeAtRequired
        | QueryError::ParamEndTimeAtWithoutBetween
        | QueryError::ParamEndTimeAtNotAfterTimeAt
        | QueryError::ProjectionKeywordWithLinkedTerm { .. }
        | QueryError::ProjectionMemberPickedAndOmitted { .. } => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, error.to_string()),
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
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::query_failure::classify_query_error;
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
    use andromeda_ngsi_ld_query::{
        error::QueryError,
        filter::{expression::Expr, language_targeting::LanguageTargeting, target_language::TargetLanguage},
        term_expander::TermExpansionFailure,
    };

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

    /// A bracket member neither a name nor an RFC 5646 tag, and a full tag or
    /// a range the level does not admit, are request data naming no target.
    #[test]
    fn a_bracket_naming_no_target_language_is_bad_request_data() {
        let neither = TargetLanguage::parse("en_GB-x").expect_err("neither a tag nor a range");
        let tagged: Expr = "name[en-GB]==\"x\"".parse().expect("a filter");
        let tag = LanguageTargeting::Specification.admit(&tagged).expect_err("a full tag needs the tags level");
        let ranged: Expr = "name[de-*-DE]==\"x\"".parse().expect("a filter");
        let range = LanguageTargeting::LanguageTags.admit(&ranged).expect_err("a range needs the ranges level");
        for error in [neither, tag, range] {
            pretty_assertions::assert_eq!(classify_query_error(&error).error_type(), NgsiLdErrorType::BadRequestData, "{error}");
        }
    }
}
