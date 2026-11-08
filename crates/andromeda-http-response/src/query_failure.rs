//! Classifying an NGSI-LD query-language failure.

use crate::classified_report::ClassifiedReport;
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use andromeda_ngsi_ld_query::{error::QueryError, term_expander::TermExpansionFailure};

/// Classify a typed NGSI-LD query-language failure.
#[must_use]
pub fn classify_query_error(error: &QueryError) -> ClassifiedReport {
    match error {
        // Clauses 5.7.2.4, 5.7.3.4 and 5.7.4.4: a query, geoquery or context
        // source filter that is "not syntactically valid (as per the referred
        // clauses 4.9 and 4.10)" raises BadRequestData. A type selection, a
        // projection and a name outside their grammars are answered the same
        // way, as the query string and the request body that carry them are.
        // Clause 6.3.20 keeps InvalidRequest for a parameter the operation
        // does not define, which never reaches this classification.
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
        | QueryError::AttributeNameInvalid { .. }
        | QueryError::QInvalidNumber { .. }
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
        | QueryError::AttrsReservedMember { .. }
        | QueryError::ProjectionMemberPickedAndOmitted { .. } =>ClassifiedReport::public(NgsiLdErrorType::BadRequestData, error.to_string()),
        // Clause 5.5.6: a query "so complex that cannot be resolved" raises
        // TooComplexQuery, which Table 6.3.2-1 answers with 403.
        QueryError::ReferenceGeometryTooComplex { .. } | QueryError::QTooManyComparisons { .. } | QueryError::QNestedTooDeep { .. } => {
            ClassifiedReport::public(NgsiLdErrorType::TooComplexQuery, error.to_string())
        }
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
    use andromeda_ngsi_ld_query::{
        error::QueryError,
        filter::{complexity::FilterComplexityLimits, expression::Expr, language_targeting::LanguageTargeting, target_language::TargetLanguage},
        projection::grammar::ProjectionList,
        term_expander::TermExpansionFailure,
        type_selection::grammar::TypeExprRaw,
    };
    use std::num::NonZeroUsize;

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

    /// Clause 5.7.2.4 answers a query or geoquery outside the clauses 4.9 and
    /// 4.10 grammars with `BadRequestData`, wherever the request carries it, and
    /// a type selection, a projection or a name outside its grammar the same.
    #[test]
    fn a_query_geoquery_type_selection_projection_or_name_outside_its_grammar_is_bad_request_data() {
        let mut failures: Vec<QueryError> = [
            "",
            "speed==",
            "speed==\"open",
            "speed==@",
            "(speed>1",
            "speed>1)",
            "name==\"\\x\"",
            "speed-limit>1",
        ]
        .into_iter()
        .map(|q| q.parse::<Expr>().expect_err("outside the clause 4.9 grammar"))
        .collect();
        failures.extend(
            ["", "(Home", "Home)", "Home;Vehicle", "Home||Vehicle"]
                .into_iter()
                .map(|selection| selection.parse::<TypeExprRaw>().expect_err("outside the clause 4.17 grammar")),
        );
        failures.extend(
            ["a{b", "a,,b"]
                .into_iter()
                .map(|projection| projection.parse::<ProjectionList>().expect_err("outside the projection grammar")),
        );
        failures.push(QueryError::GeoIncomplete);
        for error in failures {
            pretty_assertions::assert_eq!(classify_query_error(&error).error_type(), NgsiLdErrorType::BadRequestData, "{error:?}");
        }
    }

    /// Clause 5.5.6: a filter making more comparisons or nesting deeper than
    /// the broker resolves is `TooComplexQuery`, which Table 6.3.2-1 answers
    /// with 403.
    #[test]
    fn a_filter_past_the_complexity_limits_is_too_complex_query() {
        let limits = FilterComplexityLimits::new(
            NonZeroUsize::new(2).expect("a positive comparison limit"),
            NonZeroUsize::new(1).expect("a positive nesting limit"),
        );
        let comparisons = limits.admit(&"a==1,2,3".parse::<Expr>().expect("a filter")).expect_err("too many comparisons");
        let nesting = limits.admit(&"a;(b|c)".parse::<Expr>().expect("a filter")).expect_err("nested too deep");
        let written = format!("{}a{}", "(".repeat(65), ")".repeat(65))
            .parse::<Expr>()
            .expect_err("parenthesised too deep");
        for error in [comparisons, nesting, written] {
            pretty_assertions::assert_eq!(classify_query_error(&error).error_type(), NgsiLdErrorType::TooComplexQuery, "{error:?}");
        }
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
