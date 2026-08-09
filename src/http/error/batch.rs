//! Reporting a broker failure inside a clause 6.17 batch result.
//!
//! The wire shapes live in `andromeda-ngsi-ld-problem-details`; what is here is
//! the same decision [`crate::http::error::category`] makes for a
//! single response, applied per entity.

use crate::{error::AndromedaError, http::error::category};
use andromeda_entity::batch::BatchResult;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_problem_details::batch_operation::{BatchEntityError, BatchOperationResult};

/// Report a broker failure against one entity of a batch.
///
/// An internal failure is logged in full and reported as the bare phrase
/// "Internal error", for the same reason a single-response failure is: the
/// message may name a table, a query or a host.
pub fn entity_error(entity_id: Uri, error: &AndromedaError) -> BatchEntityError {
    let error_type = category::of(error.kind());

    if error.is_internal() {
        tracing::error!(
            error = %error,
            kind = ?error.kind(),
            entity_id = %entity_id,
            "Internal batch error (sanitized before sending to client)"
        );
        return BatchEntityError::new(entity_id, error_type, "Internal error");
    }

    BatchEntityError::new(entity_id, error_type, &error.to_string())
}

/// Turn the outcome of a batch into the clause 6.17 response body.
pub fn operation_result(result: BatchResult) -> BatchOperationResult {
    let errors = result
        .errors
        .into_iter()
        .map(|(entity_id, error)| {
            let error = AndromedaError::from(error);
            entity_error(entity_id, &error)
        })
        .collect();
    BatchOperationResult {
        success: result.success,
        errors,
    }
}

#[cfg(test)]
mod tests {
    use crate::{error::AndromedaError, http::error::batch::entity_error};
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use andromeda_ngsi_ld_problem_details::{batch_operation::BatchErrorKind, error_type::NgsiLdErrorType};

    #[test]
    fn each_failure_is_reported_under_the_category_its_kind_maps_to() {
        for (error, expected) in [
            (AndromedaError::entity_already_exists("urn:ngsi-ld:Entity:1"), NgsiLdErrorType::AlreadyExists),
            (AndromedaError::entity_not_found("urn:ngsi-ld:Entity:2"), NgsiLdErrorType::ResourceNotFound),
            (AndromedaError::entity_validation("invalid payload"), NgsiLdErrorType::BadRequestData),
            (AndromedaError::malformed_request("invalid request"), NgsiLdErrorType::InvalidRequest),
            (
                AndromedaError::context_not_available("context fetch failed"),
                NgsiLdErrorType::LdContextNotAvailable,
            ),
        ] {
            let reported = entity_error("urn:ngsi-ld:Entity:X".parse::<Uri>().expect("valid entity URI"), &error);
            pretty_assertions::assert_eq!(reported.error.error_type, BatchErrorKind::Known(expected));
            pretty_assertions::assert_eq!(reported.entity_id.as_str(), "urn:ngsi-ld:Entity:X");
        }
    }

    #[test]
    fn a_failure_that_is_not_internal_keeps_its_message() {
        let reported = entity_error(
            "urn:ngsi-ld:Entity:5".parse::<Uri>().expect("valid entity URI"),
            &AndromedaError::entity_validation("missing required attribute"),
        );
        pretty_assertions::assert_eq!(reported.error.detail, "missing required attribute");
    }

    #[test]
    fn an_internal_failure_never_reaches_the_client_in_detail() {
        let reported = entity_error(
            "urn:ngsi-ld:Entity:4".parse::<Uri>().expect("valid entity URI"),
            &AndromedaError::internal("something broke"),
        );
        pretty_assertions::assert_eq!(reported.error.error_type, BatchErrorKind::Known(NgsiLdErrorType::InternalError));
        pretty_assertions::assert_eq!(reported.error.detail, "Internal error");
    }
}
