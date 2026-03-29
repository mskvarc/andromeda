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
}
