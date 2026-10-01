//! Exhaustive classification shared by HTTP extractors.

use crate::{api_root::ApiRootError, request_links::ContextLinkError};
use andromeda_http_response::{classified_report::ClassifiedReport, internal_failure_summary::InternalFailureSummary};
use andromeda_jsonld::error::JsonLdError;
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use andromeda_ngsi_ld_query::error::QueryError;
use andromeda_ngsi_ld_request::error::ParameterError;
use andromeda_tenancy::error::TenancyError;

/// Classify tenant resolution independently of any policy route binding.
#[must_use]
pub fn classify_tenancy_error(error: &TenancyError) -> ClassifiedReport {
    match error {
        TenancyError::MultiTenancyDisabled => ClassifiedReport::public(NgsiLdErrorType::NoMultiTenantSupport, error.to_string()),
        TenancyError::UnknownTenant { .. } => ClassifiedReport::public(NgsiLdErrorType::NonexistentTenant, error.to_string()),
        // Backpressure, not a fault: a saturated backend pool during cold
        // resolution is shed as a 503 with Retry-After, the same as the
        // request-path stores, rather than reported as an internal 500 the
        // client cannot act on.
        TenancyError::Saturated { .. } => ClassifiedReport::store_saturated(error.to_string()),
        TenancyError::RegistryUnavailable { .. } | TenancyError::ProvisioningFailed { .. } | TenancyError::RuntimeUnavailable { .. } => {
            ClassifiedReport::internal(error, InternalFailureSummary::new("failed to resolve the tenant"))
        }
    }
}

/// Classify a request authority no `apiRoot` can be built from at the shared
/// extraction boundary.
#[must_use]
pub fn classify_api_root_error(error: &ApiRootError) -> ClassifiedReport {
    match error {
        ApiRootError::InvalidOrigin(_) => ClassifiedReport::public(NgsiLdErrorType::InvalidRequest, error.to_string()),
    }
}

/// Classify a request's `Link` headers that name no usable context.
///
/// Clause 5.5.4 validates the context of every request, and clause 6.3.5 reads
/// it from one `Link` header and refuses one beside an `application/ld+json`
/// body: a malformed link, a second context link, a relative target that does
/// not resolve against the request URI and a context link beside an
/// `application/ld+json` body are each a `BadRequestData`. A request authority
/// that forms no origin is the same `InvalidRequest` it draws wherever the
/// broker reads it ([`classify_api_root_error`]).
#[must_use]
pub fn classify_context_link_error(error: &ContextLinkError) -> ClassifiedReport {
    match error {
        ContextLinkError::InvalidOrigin(source) => classify_api_root_error(source),
        ContextLinkError::NotVisibleAscii(_)
        | ContextLinkError::Malformed(_)
        | ContextLinkError::SeveralContexts { .. }
        | ContextLinkError::NoRequestAuthority
        | ContextLinkError::RequestUriNotIri(_)
        | ContextLinkError::Unresolvable(_)
        | ContextLinkError::ResolvedNotAbsolute(_)
        | ContextLinkError::CombinedWithLdJson => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, error.to_string()),
    }
}

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
        | QueryError::ParamIdInvalid { .. }
        | QueryError::ParamIdListEmpty
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

/// Classify JSON-LD context availability separately from malformed input.
#[must_use]
pub fn classify_jsonld_error(error: &JsonLdError) -> ClassifiedReport {
    match error {
        JsonLdError::ContextFetchDisabled { .. } | JsonLdError::ContextLoad { .. } | JsonLdError::ContextHttp { .. } | JsonLdError::ContextTooLarge { .. } => {
            ClassifiedReport::public(NgsiLdErrorType::LdContextNotAvailable, error.to_string())
        }
        JsonLdError::Json(source) if source.is_syntax() || source.is_eof() => ClassifiedReport::public(NgsiLdErrorType::InvalidRequest, error.to_string()),
        // Clause 5.5.7: parts of a user @context that contain a JSON-LD Scoped
        // Context "should result in an error of type BadRequestData".
        JsonLdError::ProtectedContextPreload { .. }
        | JsonLdError::ContextInvalid { .. }
        | JsonLdError::PreloadedContextUnreadable { .. }
        | JsonLdError::PreloadedContextInvalid { .. }
        | JsonLdError::PreloadedContextUrl(_)
        | JsonLdError::EmbeddedContextMissing { .. }
        | JsonLdError::EmbeddedContextInvalid { .. }
        | JsonLdError::InlineContextInvalid(_)
        | JsonLdError::ScopedUserContext { .. }
        | JsonLdError::ClientBuild(_)
        | JsonLdError::Expansion(_)
        | JsonLdError::Compaction(_)
        | JsonLdError::TypeNotExpanded
        | JsonLdError::TermNotExpanded
        | JsonLdError::ExpandedTermNotIri(_)
        | JsonLdError::ContextUrlNotIri(_)
        | JsonLdError::GeneratedVocabularyIri(_)
        | JsonLdError::JsonLdContextText(_)
        | JsonLdError::JsonLdContextShape
        | JsonLdError::DocumentShape
        | JsonLdError::ContextGraph(_)
        | JsonLdError::Json(_) => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use crate::classification::classify_jsonld_error;
    use andromeda_jsonld::{context_graph::ContextGraphError, error::JsonLdError};
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
    use andromeda_tenancy::runtime_build_failure::RuntimeBuildFailure;
    use iri_rs::IriBuf;

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

    /// Clause 5.5.6: a `@context` whose references cycle, run too deep or
    /// overflow the processor is invalid, a `BadRequestData`.
    #[test]
    fn an_invalid_context_reference_graph_is_bad_request_data() {
        let url = IriBuf::new("https://example.org/cycle.jsonld".to_owned()).expect("a valid IRI");
        for refusal in [
            ContextGraphError::Cycle(url.clone()),
            ContextGraphError::TooDeep(url),
            ContextGraphError::ProcessorOverflow,
        ] {
            let error = JsonLdError::ContextGraph(refusal);
            pretty_assertions::assert_eq!(classify_jsonld_error(&error).error_type(), NgsiLdErrorType::BadRequestData, "{error}");
        }
    }

    #[test]
    fn a_saturated_tenant_resolution_is_a_503_with_retry_after() {
        use crate::classification::classify_tenancy_error;
        use andromeda_ngsi_ld_lexicon::tenant::TenantName;
        use andromeda_ngsi_ld_problem_details::problem_detail::ProblemClassification;
        use andromeda_tenancy::{error::TenancyError, tenant_ref::TenantRef};
        use std::time::Duration;

        let tenant = TenantRef::Named(TenantName::try_from("acme".to_owned()).expect("valid tenant name"));
        let saturated = classify_tenancy_error(&TenancyError::Saturated { tenant: tenant.clone() });
        pretty_assertions::assert_eq!(saturated.classification(), ProblemClassification::ServiceUnavailable);
        pretty_assertions::assert_eq!(saturated.retry_after(), Some(Duration::from_secs(1)));

        // A genuine construction fault stays an internal 500, not backpressure.
        let fault = classify_tenancy_error(&TenancyError::RuntimeUnavailable {
            tenant,
            cause: RuntimeBuildFailure::Invariant,
        });
        pretty_assertions::assert_eq!(fault.error_type(), NgsiLdErrorType::InternalError);
    }
}
