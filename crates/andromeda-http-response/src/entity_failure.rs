//! Exhaustive HTTP classification of failures owned by entity policy, shared
//! by every binding that answers one.

use crate::{
    classified_report::ClassifiedReport,
    internal_failure_summary::InternalFailureSummary,
    jsonld_failure::classify_jsonld_error,
    parameter_failure::classify_parameter_error,
    query_failure::classify_query_error,
};
use andromeda_entity::error::EntityError;
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;

/// Classify one entity-policy failure for both single and batch HTTP reporting.
#[must_use]
pub fn classify_entity_error(error: &EntityError) -> ClassifiedReport {
    match error {
        EntityError::Validation(reason) => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, reason.to_string()),
        EntityError::NotFound { .. } | EntityError::TypeNotFound { .. } | EntityError::AttributeNotFound { .. } => {
            ClassifiedReport::public(NgsiLdErrorType::ResourceNotFound, error.to_string())
        }
        EntityError::MemberNotFound(reason) => ClassifiedReport::public(NgsiLdErrorType::ResourceNotFound, reason.to_string()),
        EntityError::AlreadyExists { .. } => ClassifiedReport::public(NgsiLdErrorType::AlreadyExists, error.to_string()),
        // Clause 5.5.2 `Conflict`, not `InternalError`: nothing failed inside the
        // broker and nothing about the submitted content is wrong, so the client
        // is told the truth — the entity is contended and the identical request
        // may be sent again.
        EntityError::Contention { .. } => ClassifiedReport::public(NgsiLdErrorType::Conflict, error.to_string()),
        EntityError::OperationNotSupported(reason) => ClassifiedReport::public(NgsiLdErrorType::OperationNotSupported, reason.to_string()),
        // Clause 6.3.2 raises "Request Entity too large" (413) for an input too
        // large to process; the budget is counted in values and positions
        // rather than bytes, and the client's remedy is the same.
        EntityError::PayloadBudget(source) => ClassifiedReport::public(NgsiLdErrorType::RequestEntityTooLarge, source.to_string()),
        // A depth, not a size: clause 6.3.2 keeps 413 for too many bytes, and
        // clauses 5.7.1 and 5.7.2 answer a too deep query with `BadRequestData`.
        EntityError::AttributeNesting(source) => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, source.to_string()),
        EntityError::JsonLd(source) => classify_jsonld_error(source),
        EntityError::Query(source) => classify_query_error(source),
        EntityError::Request(source) => classify_parameter_error(source),
        EntityError::Representation(source) => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, source.to_string()),
        // Backpressure, not a fault: shed as 503 with `Retry-After`, not a 500.
        EntityError::StoreSaturated => ClassifiedReport::store_saturated(error.to_string()),
        // Nothing of the write persisted and it can be sent again unchanged:
        // a conflict with concurrent transactions is not a broker fault.
        EntityError::TransactionContended => ClassifiedReport::transaction_contended(error.to_string()),
        // The store could not be reached: nothing of the request is known to
        // have persisted, and it can be sent again once the store is back.
        EntityError::StoreUnavailable => ClassifiedReport::store_unavailable(error.to_string()),
        EntityError::Commit(_) => ClassifiedReport::internal(error, InternalFailureSummary::new("failed to commit the entity write")),
        EntityError::Persistence(_) => ClassifiedReport::internal(error, InternalFailureSummary::new("the entity store is unavailable")),
        EntityError::Internal(_) => ClassifiedReport::internal(error, InternalFailureSummary::new("internal entity error")),
    }
}

#[cfg(test)]
mod tests {
    use crate::{batch_projection::batch_entity_error, entity_failure::classify_entity_error};
    use andromeda_entity::{
        entity_invariant::EntityInvariant,
        entity_operation_unsupported::EntityOperationUnsupported,
        entity_validation_reason::EntityValidationReason,
        error::EntityError,
        residual_predicate::ResidualPredicate,
    };
    use andromeda_ngsi_ld_geometry::geometry_census::{GeometryOverLimit, GeometryPositionLimit};
    use andromeda_ngsi_ld_lexicon::{
        attribute_name::AttributeName,
        identifier::Identifier,
        ngsi_ld_urn::{AttributeListId, EntityTypeListId},
    };
    use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
    use andromeda_ngsi_ld_problem_details::{batch_operation::BatchErrorKind, error_type::NgsiLdErrorType, problem_detail::ProblemClassification};
    use andromeda_ngsi_ld_query::error::QueryError;
    use andromeda_ngsi_ld_representation::payload_budget::{GeometryOwner, PayloadBudgetExceeded};
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
    use axum::http::StatusCode;
    use std::{num::NonZeroUsize, time::Duration};

    fn entity_id() -> Identifier {
        "urn:ngsi-ld:Entity:1".parse::<Identifier>().expect("a valid identifier")
    }

    #[test]
    fn semantic_entity_variants_map_exhaustively_to_public_categories() {
        let identifier = entity_id();
        let attribute = "speed".parse::<AttributeName>().expect("a valid attribute name");
        for (error, expected) in [
            (
                EntityError::Validation(EntityValidationReason::ScopeNotReplaceable),
                NgsiLdErrorType::BadRequestData,
            ),
            (EntityError::not_found(&identifier), NgsiLdErrorType::ResourceNotFound),
            (EntityError::attribute_not_found(&attribute), NgsiLdErrorType::ResourceNotFound),
            (EntityError::already_exists(&identifier), NgsiLdErrorType::AlreadyExists),
            (
                EntityError::OperationNotSupported(EntityOperationUnsupported::Unnarrowable {
                    predicates: vec![ResidualPredicate::ValueFilter],
                }),
                NgsiLdErrorType::OperationNotSupported,
            ),
        ] {
            let report = classify_entity_error(&error);
            pretty_assertions::assert_eq!(report.error_type(), expected, "wrong category for {error:?}");
            pretty_assertions::assert_eq!(report.classification().status_code(), expected.status_code());
        }
    }

    #[test]
    fn exhausted_write_contention_is_reported_as_a_retryable_conflict() {
        let identifier = entity_id();
        let attempts = 12;
        let report = classify_entity_error(&EntityError::contention(&identifier, attempts));
        pretty_assertions::assert_eq!(report.error_type(), NgsiLdErrorType::Conflict);
        pretty_assertions::assert_eq!(report.classification().status_code(), NgsiLdErrorType::Conflict.status_code());
        assert!(
            report.detail().contains(identifier.as_str()),
            "the detail hides which entity is contended: {}",
            report.detail()
        );
        assert!(
            report.detail().contains(&attempts.to_string()),
            "the detail hides how hard the broker tried: {}",
            report.detail()
        );
    }

    #[test]
    fn internal_entity_variants_never_disclose_their_detail() {
        for (error, secret, expected_detail) in [
            (
                EntityError::Persistence(PersistenceFailure::StatementRejected),
                "rejected",
                "the entity store is unavailable",
            ),
            (
                EntityError::from(EntityInvariant::BatchPreparationFailed),
                "preparation executor",
                "internal entity error",
            ),
        ] {
            let report = classify_entity_error(&error);
            pretty_assertions::assert_eq!(report.error_type(), NgsiLdErrorType::InternalError);
            pretty_assertions::assert_eq!(report.detail(), expected_detail);
            assert!(!report.detail().contains(secret), "the raw cause leaked: {}", report.detail());
        }
    }

    /// A list identifier is minted by the broker, never sent by the client, so
    /// a mint that fails is a broken invariant answered as `InternalError`
    /// rather than a `BadRequestData` the client could not have caused.
    #[test]
    fn a_failed_list_identifier_mint_is_an_internal_error() {
        let type_list = "urn:ngsi-ld:Other:1".parse::<EntityTypeListId>().expect_err("another kind");
        let attribute_list = "urn:ngsi-ld:Other:1".parse::<AttributeListId>().expect_err("another kind");
        for error in [
            EntityError::from(EntityInvariant::MintedEntityTypeListId(type_list)),
            EntityError::from(EntityInvariant::MintedAttributeListId(attribute_list)),
        ] {
            let report = classify_entity_error(&error);
            pretty_assertions::assert_eq!(report.error_type(), NgsiLdErrorType::InternalError, "{error}");
            pretty_assertions::assert_eq!(report.detail(), "internal entity error");
        }
    }

    #[test]
    fn a_load_shed_answer_is_a_503_internal_error_with_retry_after() {
        // Clause 5.5.6: a failure the client did not cause -- a saturated pool,
        // a transaction every retry lost to a concurrent one, a store that
        // cannot be reached -- is an
        // InternalError, and clause 5.5.3 allows no `type` outside clause
        // 5.5.2. Only the binding status says the identical request can be
        // sent again shortly, and `Retry-After` how soon.
        for (source, retry_after) in [
            (EntityError::StoreSaturated, Duration::from_secs(1)),
            (EntityError::TransactionContended, Duration::from_secs(1)),
            (EntityError::StoreUnavailable, Duration::from_secs(5)),
        ] {
            let report = classify_entity_error(&source);
            let classification = report.classification();
            pretty_assertions::assert_eq!(classification, ProblemClassification::ServiceUnavailable, "{source}");
            pretty_assertions::assert_eq!(
                classification.type_iri().as_str(),
                NgsiLdErrorType::InternalError.type_iri().as_str(),
                "{source}"
            );
            pretty_assertions::assert_eq!(classification.title(), NgsiLdErrorType::InternalError.title(), "{source}");
            pretty_assertions::assert_eq!(classification.status_code(), StatusCode::SERVICE_UNAVAILABLE, "{source}");
            pretty_assertions::assert_eq!(report.retry_after(), Some(retry_after), "{source}");
        }
    }

    #[test]
    fn single_and_batch_paths_use_the_identical_classifier() {
        let identifier = entity_id();
        let source = EntityError::already_exists(&identifier);
        let single = classify_entity_error(&source);
        let batch = batch_entity_error(identifier.into(), &source, classify_entity_error);
        pretty_assertions::assert_eq!(
            batch.error.error_type,
            BatchErrorKind::Known(ProblemClassification::Category(single.error_type()))
        );
        pretty_assertions::assert_eq!(batch.error.detail, single.detail());
    }

    /// One million, the default of both entity budgets.
    fn one_million() -> NonZeroUsize {
        NonZeroUsize::new(1_000_000).expect("one million is not zero")
    }

    /// Clause 6.3.2: an entity over its value or geometry position budget is
    /// too large to process, the 413 "Request Entity too large", and the
    /// detail names the limit and, for a geometry, the attribute holding it.
    #[test]
    fn a_payload_budget_refusal_is_a_413_request_entity_too_large_naming_the_limit() {
        let location = "location".parse::<AttributeName>().expect("a valid attribute name");
        for (refusal, named) in [
            (
                PayloadBudgetExceeded::GeometryPositions {
                    owner: GeometryOwner::Attribute(location),
                    limit: GeometryPositionLimit::new(one_million()),
                },
                Some("'location'"),
            ),
            (
                PayloadBudgetExceeded::EntityJsonValues {
                    limit: JsonValueLimit::new(one_million()),
                },
                None,
            ),
        ] {
            let report = classify_entity_error(&EntityError::PayloadBudget(refusal));
            pretty_assertions::assert_eq!(report.error_type(), NgsiLdErrorType::RequestEntityTooLarge);
            pretty_assertions::assert_eq!(report.classification().status_code(), StatusCode::PAYLOAD_TOO_LARGE);
            pretty_assertions::assert_eq!(report.classification().title(), NgsiLdErrorType::RequestEntityTooLarge.title());
            assert!(report.detail().contains("1000000"), "the detail hides the limit: {}", report.detail());
            if let Some(attribute) = named {
                assert!(report.detail().contains(attribute), "the detail hides the attribute: {}", report.detail());
            }
        }
    }

    /// Clause 5.5.6: a query whose reference geometry holds more positions than
    /// the broker resolves is `TooComplexQuery`, which Table 6.3.2-1 answers
    /// with 403.
    #[test]
    fn a_reference_geometry_over_budget_is_a_403_too_complex_query() {
        let error = EntityError::Query(QueryError::ReferenceGeometryTooComplex {
            over: GeometryOverLimit::Positions {
                limit: GeometryPositionLimit::new(one_million()),
            },
        });

        let report = classify_entity_error(&error);

        pretty_assertions::assert_eq!(report.error_type(), NgsiLdErrorType::TooComplexQuery);
        pretty_assertions::assert_eq!(report.classification().status_code(), StatusCode::FORBIDDEN);
        assert!(report.detail().contains("1000000"), "{}", report.detail());
    }

    /// Clauses 5.7.1 and 5.7.2 answer a too deep query with `BadRequestData`,
    /// and clause 6.3.2 keeps 413 for too many bytes: an attribute nesting
    /// past the limit is a 400 `BadRequestData` naming the attribute and the
    /// limit.
    #[test]
    fn an_attribute_nesting_past_the_limit_is_a_400_bad_request_data() {
        use andromeda_ngsi_ld_representation::attribute_nesting::{AttributeNestingExceeded, AttributeNestingLimit};

        let exceeded = AttributeNestingExceeded {
            attribute: "https://example.org/reading".parse().expect("an attribute name"),
            limit: AttributeNestingLimit::new(NonZeroUsize::MIN.saturating_add(31)),
        };
        let report = classify_entity_error(&EntityError::AttributeNesting(exceeded));
        pretty_assertions::assert_eq!(report.error_type(), NgsiLdErrorType::BadRequestData);
        pretty_assertions::assert_eq!(report.classification().status_code(), StatusCode::BAD_REQUEST);
        assert!(
            report.detail().contains("'https://example.org/reading'") && report.detail().contains("32 levels"),
            "{}",
            report.detail()
        );
    }
}
