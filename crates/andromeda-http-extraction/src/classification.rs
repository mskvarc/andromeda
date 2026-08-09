//! Exhaustive classification shared by HTTP extractors.

use andromeda_http_response::classified_report::ClassifiedReport;
use andromeda_jsonld::error::JsonLdError;
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use andromeda_ngsi_ld_query::error::QueryError;
use andromeda_ngsi_ld_request::error::ParameterError;

/// Classify a parameter-model failure using its typed syntax-versus-value distinction.
pub fn classify_parameter_error(error: &ParameterError) -> ClassifiedReport {
    match error {
        ParameterError::Query(source) => classify_query_error(source),
        ParameterError::OptionsInvalid(_) | ParameterError::FormatInvalid(_) => ClassifiedReport::public(NgsiLdErrorType::InvalidRequest, error.to_string()),
        ParameterError::IdPatternEmpty
        | ParameterError::OrderByDirectionInvalid(_)
        | ParameterError::OrderGeometryInvalid(_)
        | ParameterError::OrderGeometryCollection
        | ParameterError::OrderFromInvalid { .. }
        | ParameterError::DropKeepExclusive
        | ParameterError::AttrsReservedMember(_)
        | ParameterError::TimerelInvalid(_)
        | ParameterError::TimePropertyInvalid(_)
        | ParameterError::TimerelTimeatRequired
        | ParameterError::AggrMethodInvalid(_)
        | ParameterError::SelectorRequired(_)
        | ParameterError::AggrMethodsRequired
        | ParameterError::RequestTypeInvalid(_)
        | ParameterError::EntityQueryTemporalMembers
        | ParameterError::TemporalQueryMissing
        | ParameterError::TemporalAggregationConflict => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, error.to_string()),
    }
}

/// Classify a typed NGSI-LD query-language failure.
pub fn classify_query_error(error: &QueryError) -> ClassifiedReport {
    match error {
        QueryError::QEmpty
        | QueryError::QUnterminatedString
        | QueryError::QUnexpectedToken { .. }
        | QueryError::QExpected { .. }
        | QueryError::QUnexpectedChar { .. }
        | QueryError::QAttributePathTooDeep { .. }
        | QueryError::TypeExprEmpty
        | QueryError::TypeExprUnexpectedEnd
        | QueryError::TypeExprUnbalanced(_)
        | QueryError::TypeExprUnexpected { .. }
        | QueryError::TypeExprEmptyName(_)
        | QueryError::TypeExprInvalidName { .. }
        | QueryError::GeoIncomplete => ClassifiedReport::public(NgsiLdErrorType::InvalidRequest, error.to_string()),
        QueryError::QInvalidNumber(_)
        | QueryError::QInvalidDateTime { .. }
        | QueryError::QInvalidDate { .. }
        | QueryError::QInvalidTime { .. }
        | QueryError::PatternRegex(_)
        | QueryError::PatternRepeatedQuantifier
        | QueryError::GeoRel(_)
        | QueryError::GeoCoordinatesInvalid(_)
        | QueryError::ReferenceGeometry(_)
        | QueryError::NearWithoutDistance
        | QueryError::DistanceOrderWithoutReference
        | QueryError::ScopePatternInvalid(_)
        | QueryError::IcuCollation(_)
        | QueryError::ParamIdInvalid(_)
        | QueryError::ParamIdListEmpty
        | QueryError::ParamTimerelTimeatPair
        | QueryError::ParamAggrPeriodDurationInvalid(_)
        | QueryError::ParamDatasetIdInvalid(_)
        | QueryError::ParamEndTimeAtRequired => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, error.to_string()),
        QueryError::TermExpansion { .. } | QueryError::AttributeNameInvalid { .. } | QueryError::TermNotAnIri { .. } => ClassifiedReport::internal(error),
    }
}

/// Classify JSON-LD context availability separately from malformed input.
pub fn classify_jsonld_error(error: &JsonLdError) -> ClassifiedReport {
    match error {
        JsonLdError::ContextFetchDisabled { .. } | JsonLdError::ContextLoad { .. } | JsonLdError::ContextHttp { .. } | JsonLdError::ContextTooLarge { .. } => {
            ClassifiedReport::public(NgsiLdErrorType::LdContextNotAvailable, error.to_string())
        }
        JsonLdError::ProtectedContextPreload { .. }
        | JsonLdError::ContextInvalid { .. }
        | JsonLdError::EmbeddedContextInvalid(_)
        | JsonLdError::InlineContextInvalid(_)
        | JsonLdError::ClientBuild(_)
        | JsonLdError::Expansion(_)
        | JsonLdError::Compaction(_)
        | JsonLdError::InvalidIri(_)
        | JsonLdError::InvalidDocument(_)
        | JsonLdError::Json(_) => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, error.to_string()),
    }
}
