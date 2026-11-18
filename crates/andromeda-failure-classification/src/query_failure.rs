//! Classifying an NGSI-LD query-language failure.

use crate::classified_report::ClassifiedReport;
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use andromeda_ngsi_ld_query::{
    dataset_selector::DatasetSelectorError,
    filter::filter_error::FilterError,
    geo::geo_query_error::GeoQueryError,
    ordering::ordering_error::OrderingError,
    pattern::pattern_error::PatternError,
    projection::projection_error::ProjectionError,
    query_error::QueryError,
    scope::scope_query_error::ScopeQueryError,
    temporal::{aggregation_period::AggregationPeriodError, time_interval_error::TimeIntervalError},
    term_expander::TermExpansionError,
    type_selection::type_selection_error::TypeSelectionError,
};

/// Classify a typed NGSI-LD query-language failure.
#[must_use]
pub fn classify_query_error(error: &QueryError) -> ClassifiedReport {
    let error_type = match error {
        QueryError::Filter(filter) => filter_error_type(filter),
        QueryError::TypeSelection(selection) => type_selection_error_type(selection),
        QueryError::ScopeQuery(scope) => scope_query_error_type(scope),
        QueryError::Projection(projection) => projection_error_type(projection),
        QueryError::Ordering(ordering) => ordering_error_type(ordering),
        QueryError::Pattern(pattern) => pattern_error_type(pattern),
        QueryError::GeoQuery(geo) => geo_query_error_type(geo),
        QueryError::TimeInterval(interval) => time_interval_error_type(*interval),
        QueryError::AggregationPeriod(period) => aggregation_period_error_type(period),
        QueryError::DatasetSelector(selector) => dataset_selector_error_type(selector),
        QueryError::TermExpansion(expansion) => term_expansion_error_type(*expansion),
    };
    ClassifiedReport::public(error_type, error.to_string())
}

/// Clauses 5.7.2.4, 5.7.3.4 and 5.7.4.4: a query, geoquery or context source
/// filter that is "not syntactically valid (as per the referred clauses 4.9
/// and 4.10)" raises `BadRequestData`. Clause 5.5.6: a filter "so complex that
/// cannot be resolved" raises `TooComplexQuery`, which Table 6.3.2-1 answers
/// with 403.
const fn filter_error_type(error: &FilterError) -> NgsiLdErrorType {
    match error {
        FilterError::TooManyComparisons { .. } | FilterError::NestedTooDeep { .. } => NgsiLdErrorType::TooComplexQuery,
        // A member that is neither a name nor a tag is well-formed `q` text
        // naming no target, as is a full tag or a range the broker's
        // `q_target_languages` level does not admit.
        FilterError::Empty
        | FilterError::UnterminatedString
        | FilterError::InvalidStringEscape { .. }
        | FilterError::InvalidUnicodeEscape { .. }
        | FilterError::UnescapedControlCharacter { .. }
        | FilterError::UnexpectedToken { .. }
        | FilterError::Expected { .. }
        | FilterError::AmbiguousTypeGuard
        | FilterError::UnexpectedCharacter { .. }
        | FilterError::InvalidNumber(_)
        | FilterError::PathTooDeep { .. }
        | FilterError::InvalidTrailingMember { .. }
        | FilterError::LanguageTagNotAdmitted(_)
        | FilterError::LanguageRangeNotAdmitted(_)
        | FilterError::BareWord
        | FilterError::InvalidDateTime(_)
        | FilterError::InvalidDate(_)
        | FilterError::InvalidTime(_)
        | FilterError::InvalidAttributeName(_)
        | FilterError::UnholdableCharacter(_)
        | FilterError::Pattern(_) => NgsiLdErrorType::BadRequestData,
    }
}

/// A type selection outside the clause 4.17 grammar is answered as a filter
/// outside clause 4.9 is.
const fn type_selection_error_type(error: &TypeSelectionError) -> NgsiLdErrorType {
    match error {
        TypeSelectionError::Empty
        | TypeSelectionError::UnexpectedEnd
        | TypeSelectionError::UnbalancedParenthesis { .. }
        | TypeSelectionError::UnexpectedCharacter { .. }
        | TypeSelectionError::InvalidName { .. } => NgsiLdErrorType::BadRequestData,
    }
}

/// A `scopeQ` outside the clause 4.19 grammar is answered as a filter outside
/// clause 4.9 is.
const fn scope_query_error_type(error: &ScopeQueryError) -> NgsiLdErrorType {
    match error {
        ScopeQueryError::EmptyPattern
        | ScopeQueryError::LevelTooLong { .. }
        | ScopeQueryError::InvalidLevel
        | ScopeQueryError::MultiLevelNotLast
        | ScopeQueryError::UngroupedConjunction
        | ScopeQueryError::NestedGroup
        | ScopeQueryError::UnclosedGroup
        | ScopeQueryError::MissingPattern
        | ScopeQueryError::TrailingInput => NgsiLdErrorType::BadRequestData,
    }
}

/// A projection outside the clause 4.21 grammar is answered as a filter
/// outside clause 4.9 is, as the query string and the request body that carry
/// it are. Clause 6.3.20 keeps `InvalidRequest` for a parameter the operation
/// does not define, which never reaches this classification.
const fn projection_error_type(error: &ProjectionError) -> NgsiLdErrorType {
    match error {
        ProjectionError::UnbalancedBrace { .. }
        | ProjectionError::EmptyTerm { .. }
        | ProjectionError::InvalidAttributeName(_)
        | ProjectionError::KeywordWithLinkedTerm { .. }
        | ProjectionError::MemberPickedAndOmitted { .. }
        | ProjectionError::ReservedAttrsMember { .. } => NgsiLdErrorType::BadRequestData,
    }
}

/// Clause 5.7.4.4 answers a temporal ordering the broker refuses with
/// `BadRequestData`. Clause 5.5.6: a reference geometry holding more positions
/// than the broker resolves is `TooComplexQuery`.
const fn ordering_error_type(error: &OrderingError) -> NgsiLdErrorType {
    match error {
        OrderingError::ReferenceBudget(_) => NgsiLdErrorType::TooComplexQuery,
        OrderingError::Path(_)
        | OrderingError::InvalidDirection(_)
        | OrderingError::LanguageSuppression
        | OrderingError::LanguageRange
        | OrderingError::DistanceWithoutReference
        | OrderingError::TemporalMember(_)
        | OrderingError::TemporalDirection(_)
        | OrderingError::Collation(_)
        | OrderingError::InvalidReferenceGeometry(_)
        | OrderingError::InvalidCoordinates(_)
        | OrderingError::GeometryWithoutCoordinates => NgsiLdErrorType::BadRequestData,
    }
}

/// A regular expression outside IEEE 1003.2 or the dialect the broker admits is
/// request data the operation does not accept.
const fn pattern_error_type(error: &PatternError) -> NgsiLdErrorType {
    match error {
        PatternError::Regex(_) | PatternError::RepeatedQuantifier | PatternError::NonPosixConstruct { .. } | PatternError::Translation(_) => {
            NgsiLdErrorType::BadRequestData
        }
    }
}

/// Clause 5.7.3.4: a geoquery "not syntactically valid" raises
/// `BadRequestData`. Clause 5.5.6: a reference geometry holding more positions
/// than the broker resolves is `TooComplexQuery`.
const fn geo_query_error_type(error: &GeoQueryError) -> NgsiLdErrorType {
    match error {
        GeoQueryError::ReferenceBudget(_) => NgsiLdErrorType::TooComplexQuery,
        GeoQueryError::MissingMember(_)
        | GeoQueryError::Incomplete
        | GeoQueryError::InvalidGeorel(_)
        | GeoQueryError::InvalidGeometryType(_)
        | GeoQueryError::InvalidCoordinates(_)
        | GeoQueryError::InvalidReferenceGeometry(_)
        | GeoQueryError::NearWithoutDistance
        | GeoQueryError::NearDistanceBand
        | GeoQueryError::InvalidGeoproperty(_) => NgsiLdErrorType::BadRequestData,
    }
}

/// `timerel`, `timeAt` and `endTimeAt` that bound no interval (clause 4.11) are
/// request data the operation does not accept.
const fn time_interval_error_type(error: TimeIntervalError) -> NgsiLdErrorType {
    match error {
        TimeIntervalError::UnpairedTimerel
        | TimeIntervalError::EndTimeAtRequired
        | TimeIntervalError::EndTimeAtWithoutBetween
        | TimeIntervalError::EndTimeAtNotAfterTimeAt => NgsiLdErrorType::BadRequestData,
    }
}

/// An `aggrPeriodDuration` that is no duration (clause 4.5.19) is request data
/// the operation does not accept.
const fn aggregation_period_error_type(error: &AggregationPeriodError) -> NgsiLdErrorType {
    match error {
        AggregationPeriodError::InvalidDuration(_) => NgsiLdErrorType::BadRequestData,
    }
}

/// A `datasetId` that names no dataset (clause 4.5.5) is request data the
/// operation does not accept.
const fn dataset_selector_error_type(error: &DatasetSelectorError) -> NgsiLdErrorType {
    match error {
        DatasetSelectorError::InvalidIdentifier(_) => NgsiLdErrorType::BadRequestData,
    }
}

/// Table 5.5.2-1: the terms and the `@context` both come with the request, so a
/// term naming no IRI under it (a keyword alias included) is input that "does
/// not meet the requirements of the operation", as the same term is on a write.
/// Only a `@context` that cannot be retrieved is `LdContextNotAvailable`.
const fn term_expansion_error_type(error: TermExpansionError) -> NgsiLdErrorType {
    match error {
        TermExpansionError::ContextUnavailable => NgsiLdErrorType::LdContextNotAvailable,
        TermExpansionError::NotExpanded | TermExpansionError::Keyword | TermExpansionError::NotAnIri | TermExpansionError::ContextUnprocessable => {
            NgsiLdErrorType::BadRequestData
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::query_failure::classify_query_error;
    use andromeda_ngsi_ld_geometry::geometry_census::{GeometryOverLimit, GeometryPositionLimit};
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
    use andromeda_ngsi_ld_query::{
        dataset_selector::DatasetSelector,
        filter::{
            Filter,
            complexity::FilterComplexityLimits,
            filter_error::FilterError,
            language_targeting::LanguageTargeting,
            target_language::TargetLanguage,
        },
        geo::{geo_query_error::GeoQueryError, reference_budget::ReferenceBudgetError},
        ordering::{ordering_error::OrderingError, written_ordering_term::WrittenOrderingTerm},
        pattern::pattern_error::PatternError,
        projection::{projection_error::ProjectionError, written_projection_list::WrittenProjectionList},
        query_error::QueryError,
        scope::scope_query_error::ScopeQueryError,
        temporal::{aggregation_period::AggregationPeriod, time_interval_error::TimeIntervalError},
        term_expander::TermExpansionError,
        type_selection::{type_selection_error::TypeSelectionError, written_type_selection::WrittenTypeSelection},
    };
    use std::num::NonZeroUsize;

    fn over_budget() -> ReferenceBudgetError {
        ReferenceBudgetError {
            over: GeometryOverLimit::Positions {
                limit: GeometryPositionLimit::new(NonZeroUsize::new(4).expect("a positive position limit")),
            },
        }
    }

    #[test]
    fn every_other_expansion_failure_is_bad_request_data() {
        for failure in [
            TermExpansionError::NotExpanded,
            TermExpansionError::Keyword,
            TermExpansionError::NotAnIri,
            TermExpansionError::ContextUnprocessable,
        ] {
            let error = QueryError::TermExpansion(failure);
            pretty_assertions::assert_eq!(classify_query_error(&error).error_type(), NgsiLdErrorType::BadRequestData, "{failure:?}");
        }
    }

    #[test]
    fn an_unavailable_context_is_ld_context_not_available() {
        let error = QueryError::TermExpansion(TermExpansionError::ContextUnavailable);
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
        .map(|q| QueryError::from(q.parse::<Filter>().expect_err("outside the clause 4.9 grammar")))
        .collect();
        failures.extend(
            ["", "(Home", "Home)", "Home;Vehicle", "Home||Vehicle"]
                .into_iter()
                .map(|selection| QueryError::from(selection.parse::<WrittenTypeSelection>().expect_err("outside the clause 4.17 grammar"))),
        );
        failures.extend(
            ["a{b", "a,,b"]
                .into_iter()
                .map(|projection| QueryError::from(projection.parse::<WrittenProjectionList>().expect_err("outside the projection grammar"))),
        );
        failures.push(QueryError::from(GeoQueryError::Incomplete));
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
        let comparisons = limits
            .admit(&"a==1,2,3".parse::<Filter>().expect("a filter"))
            .expect_err("too many comparisons");
        let nesting = limits.admit(&"a;(b|c)".parse::<Filter>().expect("a filter")).expect_err("nested too deep");
        let written = format!("{}a{}", "(".repeat(65), ")".repeat(65))
            .parse::<Filter>()
            .expect_err("parenthesised too deep");
        for error in [comparisons, nesting, written] {
            let error = QueryError::from(error);
            pretty_assertions::assert_eq!(classify_query_error(&error).error_type(), NgsiLdErrorType::TooComplexQuery, "{error:?}");
        }
    }

    /// A bracket member neither a name nor an RFC 5646 tag, and a full tag or
    /// a range the level does not admit, are request data naming no target.
    #[test]
    fn a_bracket_naming_no_target_language_is_bad_request_data() {
        let neither = TargetLanguage::parse("en_GB-x").expect_err("neither a tag nor a range");
        let tagged: Filter = "name[en-GB]==\"x\"".parse().expect("a filter");
        let tag = LanguageTargeting::Specification.admit(&tagged).expect_err("a full tag needs the tags level");
        let ranged: Filter = "name[de-*-DE]==\"x\"".parse().expect("a filter");
        let range = LanguageTargeting::LanguageTags.admit(&ranged).expect_err("a range needs the ranges level");
        for error in [neither, tag, range] {
            let error = QueryError::from(error);
            pretty_assertions::assert_eq!(classify_query_error(&error).error_type(), NgsiLdErrorType::BadRequestData, "{error}");
        }
    }

    #[test]
    fn a_filter_refusal_is_bad_request_data() {
        for error in [
            FilterError::Empty,
            FilterError::BareWord,
            FilterError::Pattern(PatternError::RepeatedQuantifier),
        ] {
            let error = QueryError::Filter(error);
            pretty_assertions::assert_eq!(classify_query_error(&error).error_type(), NgsiLdErrorType::BadRequestData, "{error:?}");
        }
    }

    /// Clause 5.5.6: a reference geometry holding more positions than the
    /// broker resolves is `TooComplexQuery`, whether a geoquery or an ordering
    /// carries it.
    #[test]
    fn a_reference_geometry_past_its_budget_is_too_complex_query_wherever_it_stands() {
        for error in [
            QueryError::GeoQuery(GeoQueryError::ReferenceBudget(over_budget())),
            QueryError::Ordering(OrderingError::ReferenceBudget(over_budget())),
        ] {
            pretty_assertions::assert_eq!(classify_query_error(&error).error_type(), NgsiLdErrorType::TooComplexQuery, "{error:?}");
        }
    }

    #[test]
    fn an_ordering_refusal_is_bad_request_data() {
        let direction = "name;sideways".parse::<WrittenOrderingTerm>().expect_err("a direction outside asc and desc");
        for error in [direction, OrderingError::DistanceWithoutReference, OrderingError::Path(FilterError::Empty)] {
            let error = QueryError::Ordering(error);
            pretty_assertions::assert_eq!(classify_query_error(&error).error_type(), NgsiLdErrorType::BadRequestData, "{error:?}");
        }
    }

    #[test]
    fn every_other_language_refusal_is_bad_request_data() {
        let period = AggregationPeriod::parse("one hour").expect_err("no ISO 8601 duration");
        let dataset = DatasetSelector::parse("not a uri").expect_err("no URI");
        for error in [
            QueryError::TypeSelection(TypeSelectionError::Empty),
            QueryError::ScopeQuery(ScopeQueryError::NestedGroup),
            QueryError::Projection(ProjectionError::EmptyTerm { position: 0 }),
            QueryError::Pattern(PatternError::RepeatedQuantifier),
            QueryError::GeoQuery(GeoQueryError::Incomplete),
            QueryError::TimeInterval(TimeIntervalError::EndTimeAtRequired),
            QueryError::AggregationPeriod(period),
            QueryError::DatasetSelector(dataset),
        ] {
            pretty_assertions::assert_eq!(classify_query_error(&error).error_type(), NgsiLdErrorType::BadRequestData, "{error:?}");
        }
    }
}
