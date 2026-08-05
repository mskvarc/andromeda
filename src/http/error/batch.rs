//! Batch operation error types for NGSI-LD batch endpoints (clause 6.17).
//!
//! These types represent per-entity errors in batch operations and the
//! aggregate result. They serialise directly into the JSON response body
//! expected by the NGSI-LD specification.

use crate::{core::error::AndromedaError, http::error::problem::NgsiLdErrorType, service::entity::batch::BatchResult};
use serde::{
    Deserialize,
    Deserializer,
    Serialize,
    Serializer,
    de::{self, MapAccess, Visitor},
    ser::SerializeStruct,
};
use std::fmt;

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
///
/// Stores the strongly-typed [`NgsiLdErrorType`] and a detail message.
/// The `type` URI, `title`, and `status` fields are derived from the error
/// type during serialisation to avoid redundant stringly-typed storage.
#[derive(Debug, Clone, PartialEq)]
pub struct BatchProblemDetail {
    pub error_type: NgsiLdErrorType,
    pub detail: String,
}

impl Serialize for BatchProblemDetail {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("BatchProblemDetail", 4)?;
        state.serialize_field("type", self.error_type.type_uri())?;
        state.serialize_field("title", Into::<&'static str>::into(self.error_type))?;
        state.serialize_field("status", &self.error_type.status_code().as_u16())?;
        state.serialize_field("detail", &self.detail)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for BatchProblemDetail {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<BatchProblemDetail, D::Error> {
        deserializer.deserialize_struct("BatchProblemDetail", &["type", "title", "status", "detail"], BatchProblemDetailVisitor)
    }
}

struct BatchProblemDetailVisitor;

impl<'de> Visitor<'de> for BatchProblemDetailVisitor {
    type Value = BatchProblemDetail;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a BatchProblemDetail object with type, title, status, and detail fields")
    }

    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<BatchProblemDetail, M::Error> {
        let mut type_uri: Option<String> = None;
        let mut detail: Option<String> = None;

        while let Some(key) = map.next_key::<&str>()? {
            match key {
                "type" => type_uri = Some(map.next_value()?),
                "detail" => detail = Some(map.next_value()?),
                // title and status are derived from error_type; skip them
                "title" | "status" => {
                    let _ = map.next_value::<de::IgnoredAny>()?;
                }
                other => {
                    return Err(de::Error::unknown_field(other, &["type", "title", "status", "detail"]));
                }
            }
        }

        let type_uri = type_uri.ok_or_else(|| de::Error::missing_field("type"))?;
        let detail = detail.ok_or_else(|| de::Error::missing_field("detail"))?;
        let error_type = NgsiLdErrorType::from_type_uri(&type_uri).ok_or_else(|| de::Error::custom(format!("unknown NGSI-LD error type URI: {type_uri}")))?;

        Ok(BatchProblemDetail { error_type, detail })
    }
}

impl BatchEntityError {
    /// Creates a [`BatchEntityError`] from a domain error, mapping the
    /// [`ErrorKind`](crate::core::error::ErrorKind) to its corresponding [`NgsiLdErrorType`].
    ///
    /// Internal errors are sanitised: the original detail is replaced with
    /// `"Internal error"` and the full error is logged at `error` level.
    pub fn from_domain_error(entity_id: &str, err: &AndromedaError) -> BatchEntityError {
        let error_type: NgsiLdErrorType = err.kind().into();

        let detail = if err.is_internal() {
            tracing::error!(
                error = %err,
                kind = ?err.kind(),
                entity_id = %entity_id,
                "Internal batch error (sanitized before sending to client)"
            );
            "Internal error".to_string()
        } else {
            err.to_string()
        };

        BatchEntityError {
            entity_id: entity_id.to_string(),
            error: BatchProblemDetail { error_type, detail },
        }
    }

    /// Creates a [`BatchEntityError`] from a parse-level error where no
    /// [`AndromedaError`] exists.
    pub fn from_parse_error(entity_id: &str, error_type: NgsiLdErrorType, detail: &str) -> BatchEntityError {
        BatchEntityError {
            entity_id: entity_id.to_string(),
            error: BatchProblemDetail {
                error_type,
                detail: detail.to_string(),
            },
        }
    }
}

impl From<BatchResult> for BatchOperationResult {
    fn from(result: BatchResult) -> BatchOperationResult {
        let errors = result
            .errors
            .iter()
            .map(|(entity_id, err)| BatchEntityError::from_domain_error(entity_id, err))
            .collect();
        BatchOperationResult {
            success: result.success,
            errors,
        }
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
                    error_type: NgsiLdErrorType::AlreadyExists,
                    detail: "Entity already exists".to_string(),
                },
            }],
        };

        let json = serde_json::to_value(&result).unwrap();
        pretty_assertions::assert_eq!(json["success"], serde_json::json!(["urn:ngsi-ld:Entity:1"]));
        assert!(json["errors"].is_array());
        pretty_assertions::assert_eq!(json["errors"][0]["entityId"], "urn:ngsi-ld:Entity:2");
    }

    #[test]
    fn batch_entity_error_renames_entity_id() {
        let error = BatchEntityError {
            entity_id: "urn:ngsi-ld:Entity:42".to_string(),
            error: BatchProblemDetail {
                error_type: NgsiLdErrorType::ResourceNotFound,
                detail: "Not found".to_string(),
            },
        };

        let json = serde_json::to_value(&error).unwrap();
        pretty_assertions::assert_eq!(json["entityId"], "urn:ngsi-ld:Entity:42");
        assert!(json.get("entity_id").is_none());
    }

    #[test]
    fn batch_problem_detail_serializes_rfc7807_fields() {
        let detail = BatchProblemDetail {
            error_type: NgsiLdErrorType::BadRequestData,
            detail: "Invalid payload".to_string(),
        };

        let json = serde_json::to_value(&detail).unwrap();
        pretty_assertions::assert_eq!(json["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData");
        pretty_assertions::assert_eq!(json["title"], "Bad request data");
        pretty_assertions::assert_eq!(json["status"], 400);
        pretty_assertions::assert_eq!(json["detail"], "Invalid payload");
        assert!(json.get("error_type").is_none());
    }

    #[test]
    fn empty_batch_result_serializes() {
        let result = BatchOperationResult {
            success: vec![],
            errors: vec![],
        };
        let json = serde_json::to_value(&result).unwrap();
        pretty_assertions::assert_eq!(json["success"], serde_json::json!([]));
        pretty_assertions::assert_eq!(json["errors"], serde_json::json!([]));
    }

    #[test]
    fn batch_operation_result_round_trips() {
        let original = BatchOperationResult {
            success: vec!["urn:ngsi-ld:Entity:A".to_string(), "urn:ngsi-ld:Entity:B".to_string()],
            errors: vec![BatchEntityError {
                entity_id: "urn:ngsi-ld:Entity:C".to_string(),
                error: BatchProblemDetail {
                    error_type: NgsiLdErrorType::InternalError,
                    detail: "Something went wrong".to_string(),
                },
            }],
        };

        let json_str = serde_json::to_string(&original).unwrap();
        let deserialized: BatchOperationResult = serde_json::from_str(&json_str).unwrap();
        pretty_assertions::assert_eq!(deserialized, original);
    }

    #[test]
    fn from_domain_error_already_exists() {
        let err = AndromedaError::entity_already_exists("urn:ngsi-ld:Entity:1");
        let result = BatchEntityError::from_domain_error("urn:ngsi-ld:Entity:1", &err);

        pretty_assertions::assert_eq!(result.error.error_type, NgsiLdErrorType::AlreadyExists);
        let json = serde_json::to_value(&result.error).unwrap();
        pretty_assertions::assert_eq!(json["status"], 409);
        pretty_assertions::assert_eq!(json["type"], "https://uri.etsi.org/ngsi-ld/errors/AlreadyExists");
    }

    #[test]
    fn from_domain_error_not_found() {
        let err = AndromedaError::entity_not_found("urn:ngsi-ld:Entity:2");
        let result = BatchEntityError::from_domain_error("urn:ngsi-ld:Entity:2", &err);

        pretty_assertions::assert_eq!(result.error.error_type, NgsiLdErrorType::ResourceNotFound);
        let json = serde_json::to_value(&result.error).unwrap();
        pretty_assertions::assert_eq!(json["status"], 404);
        pretty_assertions::assert_eq!(json["type"], "https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound");
    }

    #[test]
    fn from_domain_error_entity_validation() {
        let err = AndromedaError::entity_validation("invalid payload");
        let result = BatchEntityError::from_domain_error("urn:ngsi-ld:Entity:3", &err);

        pretty_assertions::assert_eq!(result.error.error_type, NgsiLdErrorType::BadRequestData);
        let json = serde_json::to_value(&result.error).unwrap();
        pretty_assertions::assert_eq!(json["status"], 400);
        pretty_assertions::assert_eq!(json["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData");
    }

    #[test]
    fn from_domain_error_malformed_request() {
        let err = AndromedaError::malformed_request("invalid request");
        let result = BatchEntityError::from_domain_error("urn:ngsi-ld:Entity:7", &err);

        pretty_assertions::assert_eq!(result.error.error_type, NgsiLdErrorType::InvalidRequest);
        let json = serde_json::to_value(&result.error).unwrap();
        pretty_assertions::assert_eq!(json["status"], 400);
        pretty_assertions::assert_eq!(json["type"], "https://uri.etsi.org/ngsi-ld/errors/InvalidRequest");
    }

    #[test]
    fn from_domain_error_sanitizes_internal_errors() {
        let err = AndromedaError::internal("something broke");
        let result = BatchEntityError::from_domain_error("urn:ngsi-ld:Entity:4", &err);

        pretty_assertions::assert_eq!(result.error.error_type, NgsiLdErrorType::InternalError);
        pretty_assertions::assert_eq!(result.error.detail, "Internal error");
        let json = serde_json::to_value(&result.error).unwrap();
        pretty_assertions::assert_eq!(json["status"], 500);
        pretty_assertions::assert_eq!(json["type"], "https://uri.etsi.org/ngsi-ld/errors/InternalError");
    }

    #[test]
    fn from_domain_error_context_not_available() {
        let err = AndromedaError::context_not_available("context fetch failed");
        let result = BatchEntityError::from_domain_error("urn:ngsi-ld:Entity:6", &err);

        pretty_assertions::assert_eq!(result.error.error_type, NgsiLdErrorType::LdContextNotAvailable);
        let json = serde_json::to_value(&result.error).unwrap();
        pretty_assertions::assert_eq!(json["status"], 503);
        pretty_assertions::assert_eq!(json["type"], "https://uri.etsi.org/ngsi-ld/errors/LdContextNotAvailable");
    }

    #[test]
    fn from_domain_error_preserves_entity_id() {
        let err = AndromedaError::entity_not_found("urn:ngsi-ld:Entity:99");
        let result = BatchEntityError::from_domain_error("urn:ngsi-ld:Entity:99", &err);

        pretty_assertions::assert_eq!(result.entity_id, "urn:ngsi-ld:Entity:99");
    }

    #[test]
    fn from_domain_error_preserves_detail_message() {
        let err = AndromedaError::entity_validation("missing required attribute");
        let result = BatchEntityError::from_domain_error("urn:ngsi-ld:Entity:5", &err);

        pretty_assertions::assert_eq!(result.error.detail, "missing required attribute");
    }

    #[test]
    fn from_parse_error_populates_fields() {
        let err = BatchEntityError::from_parse_error("urn:ngsi-ld:Entity:1", NgsiLdErrorType::BadRequestData, "missing @context");
        pretty_assertions::assert_eq!(err.entity_id, "urn:ngsi-ld:Entity:1");
        pretty_assertions::assert_eq!(err.error.error_type, NgsiLdErrorType::BadRequestData);
        pretty_assertions::assert_eq!(err.error.detail, "missing @context");

        let json = serde_json::to_value(&err.error).unwrap();
        pretty_assertions::assert_eq!(json["type"], NgsiLdErrorType::BAD_REQUEST_DATA_URI);
        pretty_assertions::assert_eq!(json["title"], "Bad request data");
        pretty_assertions::assert_eq!(json["status"], 400);
        pretty_assertions::assert_eq!(json["detail"], "missing @context");
    }

    #[test]
    fn from_type_uri_round_trips_all_variants() {
        let variants = [
            NgsiLdErrorType::InvalidRequest,
            NgsiLdErrorType::BadRequestData,
            NgsiLdErrorType::AlreadyExists,
            NgsiLdErrorType::Conflict,
            NgsiLdErrorType::OperationNotSupported,
            NgsiLdErrorType::ResourceNotFound,
            NgsiLdErrorType::InternalError,
            NgsiLdErrorType::TooComplexQuery,
            NgsiLdErrorType::TooManyResults,
            NgsiLdErrorType::LdContextNotAvailable,
            NgsiLdErrorType::NoMultiTenantSupport,
            NgsiLdErrorType::NonexistentTenant,
        ];
        for v in variants {
            let detail = BatchProblemDetail {
                error_type: v,
                detail: "test".to_string(),
            };
            let json_str = serde_json::to_string(&detail).unwrap();
            let deserialized: BatchProblemDetail = serde_json::from_str(&json_str).unwrap();
            pretty_assertions::assert_eq!(deserialized.error_type, v);
            pretty_assertions::assert_eq!(deserialized.detail, "test");
        }
    }
}
