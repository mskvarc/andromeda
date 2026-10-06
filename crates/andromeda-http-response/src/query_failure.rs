//! Classifying an NGSI-LD query-language failure.

use crate::{classified_report::ClassifiedReport, internal_failure_summary::InternalFailureSummary};
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use andromeda_ngsi_ld_query::error::QueryError;

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
        | QueryError::TypeExprEmptyName(_)
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
        | QueryError::PatternRegex(_)
        | QueryError::PatternRepeatedQuantifier
        | QueryError::PatternNonPosixConstruct { .. }
        | QueryError::PatternTranslation(_)
        | QueryError::GeoRel(_)
        | QueryError::GeoCoordinatesInvalid(_)
        | QueryError::ReferenceGeometry(_)
        | QueryError::NearWithoutDistance
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
        // The expander's own failure is the one arm that can still be a broker
        // fault: it carries whatever the JSON-LD processor raised, which
        // includes conditions no request text can provoke.
        QueryError::TermExpansion(_) => ClassifiedReport::internal(error, InternalFailureSummary::new("failed to expand a query term against the context")),
    }
}
