//! Exhaustive classification shared by HTTP extractors.

use crate::origin::RequestOriginError;
use andromeda_http_response::classified_report::ClassifiedReport;
use andromeda_jsonld::error::JsonLdError;
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use andromeda_ngsi_ld_query::error::QueryError;
use andromeda_ngsi_ld_request::error::ParameterError;
use andromeda_tenancy::error::TenancyError;

/// Classify tenant resolution independently of any policy route binding.
pub fn classify_tenancy_error(error: &TenancyError) -> ClassifiedReport {
    match error {
        TenancyError::MultiTenancyDisabled => ClassifiedReport::public(NgsiLdErrorType::NoMultiTenantSupport, error.to_string()),
        TenancyError::UnknownTenant { name } => ClassifiedReport::public(NgsiLdErrorType::NonexistentTenant, format!("Tenant {name} does not exist")),
        TenancyError::RegistryUnavailable { .. } | TenancyError::ProvisioningFailed { .. } | TenancyError::RuntimeUnavailable { .. } => {
            ClassifiedReport::internal(error)
        }
    }
}

/// Classify malformed request-origin headers at the shared extraction boundary.
pub fn classify_request_origin_error(error: &RequestOriginError) -> ClassifiedReport {
    match error {
        RequestOriginError::InvalidText { .. } | RequestOriginError::InvalidOrigin { .. } => {
            ClassifiedReport::public(NgsiLdErrorType::InvalidRequest, error.to_string())
        }
    }
}

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
        | ParameterError::PickOmitExclusive
        | ParameterError::PickOmitWithAttrs
        | ParameterError::LimitZeroWithoutCount
        | ParameterError::ProjectionLinkedTermsWithoutJoin
        | ParameterError::ProjectionDepthExceedsJoinLevel { .. }
        | ParameterError::FilterTraversalWithoutJoin
        | ParameterError::FilterDepthExceedsJoinLevel { .. }
        | ParameterError::PurgeNamesLinkedEntities(_)
        | ParameterError::InlineJoinWithoutInstances(_)
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
        | ParameterError::WildcardTypeWithLocalFalse
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
        | QueryError::GeoIncomplete
        | QueryError::ProjectionUnbalancedBrace { .. }
        | QueryError::ProjectionEmptyTerm { .. } => ClassifiedReport::public(NgsiLdErrorType::InvalidRequest, error.to_string()),
        QueryError::QInvalidNumber { .. }
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
        | QueryError::ParamEndTimeAtRequired
        | QueryError::ProjectionKeywordWithLinkedTerm { .. } => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, error.to_string()),
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
        | JsonLdError::InvalidDocument(_) => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, error.to_string()),
        JsonLdError::Json(source) if source.is_syntax() || source.is_eof() => ClassifiedReport::public(NgsiLdErrorType::InvalidRequest, error.to_string()),
        JsonLdError::Json(_) => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::classify_jsonld_error;
    use andromeda_jsonld::error::JsonLdError;
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;

    #[test]
    fn malformed_json_is_invalid_request() {
        let error = JsonLdError::Json(serde_json::from_str::<serde_json::Value>("{").expect_err("malformed JSON"));
        pretty_assertions::assert_eq!(classify_jsonld_error(&error).error_type(), NgsiLdErrorType::InvalidRequest);
    }

    #[test]
    fn valid_json_with_a_deserialization_error_is_bad_request_data() {
        let error = JsonLdError::Json(serde_json::from_value::<u64>(serde_json::json!("not a number")).expect_err("wrong JSON type"));
        pretty_assertions::assert_eq!(classify_jsonld_error(&error).error_type(), NgsiLdErrorType::BadRequestData);
    }
}
