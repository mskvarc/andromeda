//! Exhaustive HTTP classification of failures owned by entity policy.

use andromeda_entity::error::EntityError;
use andromeda_http_extraction::classification::{classify_jsonld_error, classify_parameter_error, classify_query_error};
use andromeda_http_response::classified_report::ClassifiedReport;
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;

/// Classify one entity-policy failure for both single and batch HTTP reporting.
pub fn classify_entity_error(error: &EntityError) -> ClassifiedReport {
    match error {
        EntityError::Validation { detail } => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, detail),
        EntityError::NotFound { identifier } => ClassifiedReport::public(NgsiLdErrorType::ResourceNotFound, format!("Entity {identifier} not found")),
        EntityError::TypeNotFound { name } => ClassifiedReport::public(NgsiLdErrorType::ResourceNotFound, format!("Entity type {name} not found")),
        EntityError::AttributeNotFound { name } => ClassifiedReport::public(NgsiLdErrorType::ResourceNotFound, format!("Attribute {name} not found")),
        EntityError::MemberNotFound { detail } => ClassifiedReport::public(NgsiLdErrorType::ResourceNotFound, detail),
        EntityError::AlreadyExists { identifier } => ClassifiedReport::public(NgsiLdErrorType::AlreadyExists, format!("Entity {identifier} already exists")),
        // Clause 5.5.2 `Conflict`, not `InternalError`: nothing failed inside the
        // broker and nothing about the submitted content is wrong, so the client
        // is told the truth — the entity is contended and the identical request
        // may be sent again.
        EntityError::Contention { .. } => ClassifiedReport::public(NgsiLdErrorType::Conflict, error.to_string()),
        EntityError::OperationNotSupported { detail } => ClassifiedReport::public(NgsiLdErrorType::OperationNotSupported, detail),
        EntityError::JsonLd(source) => classify_jsonld_error(source),
        EntityError::Query(source) => classify_query_error(source),
        EntityError::Request(source) => classify_parameter_error(source),
        EntityError::Representation(source) => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, source.to_string()),
        EntityError::Identifier(source) => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, source.to_string()),
        EntityError::EntityTypeListIdentifier(source) => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, source.to_string()),
        EntityError::AttributeListIdentifier(source) => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, source.to_string()),
        EntityError::Commit(_) | EntityError::Persistence { .. } | EntityError::Internal { .. } => ClassifiedReport::internal(error),
    }
}

#[cfg(test)]
mod tests {
    use crate::entity_error::classify_entity_error;
    use andromeda_entity::error::EntityError;
    use andromeda_http_response::batch_projection::batch_entity_error;
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
    use andromeda_ngsi_ld_problem_details::{batch_operation::BatchErrorKind, error_type::NgsiLdErrorType, problem_detail::ProblemClassification};

    fn entity_id() -> Result<Uri, EntityError> {
        "urn:ngsi-ld:Entity:1"
            .parse::<Uri>()
            .map_err(|error| EntityError::validation(error.to_string()))
    }

    #[test]
    fn semantic_entity_variants_map_exhaustively_to_public_categories() -> Result<(), EntityError> {
        let identifier = entity_id()?;
        let attribute = "speed".parse::<AttributeName>().map_err(|error| EntityError::validation(error.to_string()))?;
        for (error, expected) in [
            (EntityError::validation("missing id"), NgsiLdErrorType::BadRequestData),
            (EntityError::not_found(&identifier), NgsiLdErrorType::ResourceNotFound),
            (EntityError::attribute_not_found(&attribute), NgsiLdErrorType::ResourceNotFound),
            (EntityError::already_exists(&identifier), NgsiLdErrorType::AlreadyExists),
            (EntityError::operation_not_supported("unsupported"), NgsiLdErrorType::OperationNotSupported),
        ] {
            let report = classify_entity_error(&error);
            pretty_assertions::assert_eq!(report.error_type(), expected, "wrong category for {error:?}");
            pretty_assertions::assert_eq!(report.classification().status_code(), expected.status_code());
        }
        Ok(())
    }

    #[test]
    fn exhausted_write_contention_is_reported_as_a_retryable_conflict() -> Result<(), EntityError> {
        let identifier = entity_id()?;
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
        Ok(())
    }

    #[test]
    fn internal_entity_variants_never_disclose_their_detail() {
        for error in [EntityError::persistence("db-7.internal"), EntityError::internal("secret invariant")] {
            let report = classify_entity_error(&error);
            pretty_assertions::assert_eq!(report.error_type(), NgsiLdErrorType::InternalError);
            pretty_assertions::assert_eq!(report.detail(), "Internal error");
        }
    }

    #[test]
    fn single_and_batch_paths_use_the_identical_classifier() -> Result<(), EntityError> {
        let identifier = entity_id()?;
        let source = EntityError::already_exists(&identifier);
        let single = classify_entity_error(&source);
        let batch = batch_entity_error(identifier, &source, classify_entity_error);
        pretty_assertions::assert_eq!(
            batch.error.error_type,
            BatchErrorKind::Known(ProblemClassification::Category(single.error_type()))
        );
        pretty_assertions::assert_eq!(batch.error.detail, single.detail());
        Ok(())
    }
}
