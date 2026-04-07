//! Batch operation error types for NGSI-LD batch endpoints (clause 6.17).
//!
//! These types represent per-entity errors in batch operations and the
//! aggregate result. They serialise directly into the JSON response body
//! expected by the NGSI-LD specification.

use crate::{error::core::AndromedaError, http::error::problem::NgsiLdErrorType, service::entity::batch::BatchResult};
use serde::{Deserialize, Serialize};

/// Result of a batch operation (clause 6.17).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BatchOperationResult {
    pub success: Vec<String>,
    pub errors: Vec<BatchEntityError>,
}

/// An error for a single entity within a batch operation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BatchEntityError {
    #[serde(rename = "entityId")]
    pub entity_id: String,
    pub error: BatchProblemDetail,
}

/// RFC 7807 Problem Detail within a batch error.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BatchProblemDetail {
    #[serde(rename = "type")]
    pub type_: String,
    pub title: String,
    pub status: u16,
    pub detail: String,
}

impl From<BatchResult> for BatchOperationResult {
    fn from(result: BatchResult) -> BatchOperationResult {
        let errors = result.errors.iter().map(|(entity_id, err)| to_batch_error(entity_id, err)).collect();
        BatchOperationResult {
            success: result.success,
            errors,
        }
    }
}

/// Converts a domain error into a [`BatchEntityError`] for the given entity.
///
/// Maps the [`AndromedaErrorKind`] to its corresponding [`NgsiLdErrorType`] to
/// populate the RFC 7807 fields (type URI, title, status code).
pub fn to_batch_error(entity_id: &str, err: &AndromedaError) -> BatchEntityError {
    let error_type: NgsiLdErrorType = err.kind().into();

    BatchEntityError {
        entity_id: entity_id.to_string(),
        error: BatchProblemDetail {
            type_: error_type.type_uri().to_string(),
            title: error_type.title().to_string(),
            status: error_type.status_code().as_u16(),
            detail: err.message().to_string(),
        },
    }
}

/// Converts a [`NgsiLdErrorType`] directly into a [`BatchEntityError`].
///
/// Used for parse-level errors where no [`AndromedaError`] exists.
pub fn make_batch_parse_error(entity_id: &str, error_type: NgsiLdErrorType, detail: &str) -> BatchEntityError {
    BatchEntityError {
        entity_id: entity_id.to_string(),
        error: BatchProblemDetail {
            type_: error_type.type_uri().to_string(),
            title: error_type.title().to_string(),
            status: error_type.status_code().as_u16(),
            detail: detail.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_operation_result_serializes_correctly() {
        let result = BatchOperationResult {
            success: vec!["urn:ngsi-ld:Entity:1".to_string()],
            errors: vec![BatchEntityError {
                entity_id: "urn:ngsi-ld:Entity:2".to_string(),
                error: BatchProblemDetail {
                    type_: "https://uri.etsi.org/ngsi-ld/errors/AlreadyExists".to_string(),
                    title: "Already exists".to_string(),
                    status: 409,
                    detail: "Entity already exists".to_string(),
                },
            }],
        };

        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["success"], serde_json::json!(["urn:ngsi-ld:Entity:1"]));
        assert!(json["errors"].is_array());
        assert_eq!(json["errors"][0]["entityId"], "urn:ngsi-ld:Entity:2");
    }

    #[test]
    fn batch_entity_error_renames_entity_id() {
        let error = BatchEntityError {
            entity_id: "urn:ngsi-ld:Entity:42".to_string(),
            error: BatchProblemDetail {
                type_: "https://uri.etsi.org/ngsi-ld/errors/NotFound".to_string(),
                title: "Not found".to_string(),
                status: 404,
                detail: "Not found".to_string(),
            },
        };

        let json = serde_json::to_value(&error).unwrap();
        assert_eq!(json["entityId"], "urn:ngsi-ld:Entity:42");
        assert!(json.get("entity_id").is_none());
    }

    #[test]
    fn batch_problem_detail_renames_type() {
        let detail = BatchProblemDetail {
            type_: "https://uri.etsi.org/ngsi-ld/errors/BadRequestData".to_string(),
            title: "Bad request".to_string(),
            status: 400,
            detail: "Invalid payload".to_string(),
        };

        let json = serde_json::to_value(&detail).unwrap();
        assert_eq!(json["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData");
        assert!(json.get("type_").is_none());
    }

    #[test]
    fn empty_batch_result_serializes() {
        let result = BatchOperationResult {
            success: vec![],
            errors: vec![],
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["success"], serde_json::json!([]));
        assert_eq!(json["errors"], serde_json::json!([]));
    }

    #[test]
    fn batch_operation_result_round_trips() {
        let original = BatchOperationResult {
            success: vec!["urn:ngsi-ld:Entity:A".to_string(), "urn:ngsi-ld:Entity:B".to_string()],
            errors: vec![BatchEntityError {
                entity_id: "urn:ngsi-ld:Entity:C".to_string(),
                error: BatchProblemDetail {
                    type_: "https://uri.etsi.org/ngsi-ld/errors/InternalError".to_string(),
                    title: "Internal error".to_string(),
                    status: 500,
                    detail: "Something went wrong".to_string(),
                },
            }],
        };

        let json_str = serde_json::to_string(&original).unwrap();
        let deserialized: BatchOperationResult = serde_json::from_str(&json_str).unwrap();
        assert_eq!(deserialized, original);
    }

    #[test]
    fn to_batch_error_already_exists() {
        let err = AndromedaError::already_exists("urn:ngsi-ld:Entity:1");
        let result = to_batch_error("urn:ngsi-ld:Entity:1", &err);

        assert_eq!(result.error.status, 409);
        assert_eq!(result.error.type_, "https://uri.etsi.org/ngsi-ld/errors/AlreadyExists");
    }

    #[test]
    fn to_batch_error_not_found() {
        let err = AndromedaError::not_found("urn:ngsi-ld:Entity:2");
        let result = to_batch_error("urn:ngsi-ld:Entity:2", &err);

        assert_eq!(result.error.status, 404);
        assert_eq!(result.error.type_, "https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound");
    }

    #[test]
    fn to_batch_error_bad_request_data() {
        let err = AndromedaError::bad_request_data("invalid payload");
        let result = to_batch_error("urn:ngsi-ld:Entity:3", &err);

        assert_eq!(result.error.status, 400);
        assert_eq!(result.error.type_, "https://uri.etsi.org/ngsi-ld/errors/BadRequestData");
    }

    #[test]
    fn to_batch_error_invalid_request() {
        let err = AndromedaError::invalid_request("invalid request");
        let result = to_batch_error("urn:ngsi-ld:Entity:7", &err);

        assert_eq!(result.error.status, 400);
        assert_eq!(result.error.type_, "https://uri.etsi.org/ngsi-ld/errors/InvalidRequest");
    }

    #[test]
    fn to_batch_error_internal() {
        let err = AndromedaError::internal("something broke");
        let result = to_batch_error("urn:ngsi-ld:Entity:4", &err);

        assert_eq!(result.error.status, 500);
        assert_eq!(result.error.type_, "https://uri.etsi.org/ngsi-ld/errors/InternalError");
    }

    #[test]
    fn to_batch_error_ld_context_not_available() {
        let err = AndromedaError::ld_context_not_available("context fetch failed");
        let result = to_batch_error("urn:ngsi-ld:Entity:6", &err);

        assert_eq!(result.error.status, 503);
        assert_eq!(result.error.type_, "https://uri.etsi.org/ngsi-ld/errors/LdContextNotAvailable");
    }

    #[test]
    fn to_batch_error_preserves_entity_id() {
        let err = AndromedaError::not_found("urn:ngsi-ld:Entity:99");
        let result = to_batch_error("urn:ngsi-ld:Entity:99", &err);

        assert_eq!(result.entity_id, "urn:ngsi-ld:Entity:99");
    }

    #[test]
    fn to_batch_error_preserves_detail_message() {
        let err = AndromedaError::bad_request_data("missing required attribute");
        let result = to_batch_error("urn:ngsi-ld:Entity:5", &err);

        assert_eq!(result.error.detail, "missing required attribute");
    }

    #[test]
    fn make_batch_parse_error_populates_fields() {
        let err = make_batch_parse_error("urn:ngsi-ld:Entity:1", NgsiLdErrorType::BadRequestData, "missing @context");
        assert_eq!(err.entity_id, "urn:ngsi-ld:Entity:1");
        assert_eq!(err.error.type_, NgsiLdErrorType::BAD_REQUEST_DATA_URI);
        assert_eq!(err.error.title, "Bad request data");
        assert_eq!(err.error.status, 400);
        assert_eq!(err.error.detail, "missing @context");
    }
}
