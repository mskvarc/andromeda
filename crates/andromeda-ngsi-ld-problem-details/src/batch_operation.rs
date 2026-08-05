//! The result of a batch operation, and the per-entity errors in it
//! (clauses 5.2.16 and 6.17).

use crate::error_type::NgsiLdErrorType;
use serde::{
    Deserialize,
    Deserializer,
    Serialize,
    Serializer,
    de::{self, MapAccess, Visitor},
    ser::SerializeStruct,
};
use std::fmt;

/// The outcome of a batch operation (clause 6.17).
///
/// A batch answers per entity, so a partial failure is the normal case rather
/// than an exception: the identifiers that succeeded and the ones that did not
/// are reported side by side.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BatchOperationResult {
    /// Identifiers of the entities the operation applied to.
    pub success: Vec<String>,
    /// One entry per entity the operation could not apply to.
    pub errors: Vec<BatchEntityError>,
}

/// One entity's failure within a batch operation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BatchEntityError {
    /// The entity the failure is about.
    #[serde(rename = "entityId")]
    pub entity_id: String,
    /// What went wrong with it.
    pub error: BatchProblemDetail,
}

impl BatchEntityError {
    /// Report `error_type` against `entity_id`, with a human-readable detail.
    pub fn new(entity_id: &str, error_type: NgsiLdErrorType, detail: &str) -> BatchEntityError {
        BatchEntityError {
            entity_id: entity_id.to_string(),
            error: BatchProblemDetail {
                error_type,
                detail: detail.to_string(),
            },
        }
    }
}

/// An RFC 7807 problem detail inside a batch error.
///
/// Stores the category and the detail message only. The `type`, `title` and
/// `status` members are derived from the category when it is serialised, so a
/// batch error cannot report a status that disagrees with its own type IRI.
#[derive(Debug, Clone, PartialEq)]
pub struct BatchProblemDetail {
    /// The clause 5.5.2 category this failure falls into.
    pub error_type: NgsiLdErrorType,
    /// The human-readable explanation.
    pub detail: String,
}

impl Serialize for BatchProblemDetail {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("BatchProblemDetail", 4)?;
        state.serialize_field("type", self.error_type.type_uri())?;
        state.serialize_field("title", self.error_type.title())?;
        state.serialize_field("status", &self.error_type.status_code().as_u16())?;
        state.serialize_field("detail", &self.detail)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for BatchProblemDetail {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<BatchProblemDetail, D::Error> {
        deserializer.deserialize_struct("BatchProblemDetail", FIELDS, BatchProblemDetailVisitor)
    }
}

/// The members a batch problem detail is written with.
const FIELDS: &[&str] = &["type", "title", "status", "detail"];

/// Reads a problem detail back into its category and message.
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
                // Derived from the category on the way out, so ignored on the
                // way in rather than trusted.
                "title" | "status" => {
                    let _ = map.next_value::<de::IgnoredAny>()?;
                }
                other => return Err(de::Error::unknown_field(other, FIELDS)),
            }
        }

        let type_uri = type_uri.ok_or_else(|| de::Error::missing_field("type"))?;
        let detail = detail.ok_or_else(|| de::Error::missing_field("detail"))?;
        let error_type = NgsiLdErrorType::from_type_uri(&type_uri).ok_or_else(|| de::Error::custom(format!("unknown NGSI-LD error type URI: {type_uri}")))?;

        Ok(BatchProblemDetail { error_type, detail })
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        batch_operation::{BatchEntityError, BatchOperationResult, BatchProblemDetail},
        error_type::NgsiLdErrorType,
    };
    use serde_json::json;
    use strum::IntoEnumIterator;

    #[test]
    fn a_batch_result_serialises_its_successes_and_its_failures() {
        let result = BatchOperationResult {
            success: vec!["urn:ngsi-ld:Entity:1".to_string()],
            errors: vec![BatchEntityError::new(
                "urn:ngsi-ld:Entity:2",
                NgsiLdErrorType::AlreadyExists,
                "Entity already exists",
            )],
        };

        let rendered = serde_json::to_value(&result).expect("serialises");
        pretty_assertions::assert_eq!(rendered["success"], json!(["urn:ngsi-ld:Entity:1"]));
        pretty_assertions::assert_eq!(rendered["errors"][0]["entityId"], "urn:ngsi-ld:Entity:2");
        assert!(rendered["errors"][0].get("entity_id").is_none());
    }

    #[test]
    fn an_empty_batch_result_still_carries_both_members() {
        let rendered = serde_json::to_value(BatchOperationResult {
            success: vec![],
            errors: vec![],
        })
        .expect("serialises");
        pretty_assertions::assert_eq!(rendered["success"], json!([]));
        pretty_assertions::assert_eq!(rendered["errors"], json!([]));
    }

    #[test]
    fn a_problem_detail_derives_its_rfc_7807_members_from_its_category() {
        let rendered = serde_json::to_value(BatchProblemDetail {
            error_type: NgsiLdErrorType::BadRequestData,
            detail: "Invalid payload".to_string(),
        })
        .expect("serialises");

        pretty_assertions::assert_eq!(rendered["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData");
        pretty_assertions::assert_eq!(rendered["title"], "Bad request data");
        pretty_assertions::assert_eq!(rendered["status"], 400);
        pretty_assertions::assert_eq!(rendered["detail"], "Invalid payload");
        assert!(rendered.get("error_type").is_none());
    }

    #[test]
    fn every_category_survives_a_problem_detail_round_trip() {
        for category in NgsiLdErrorType::iter() {
            let original = BatchProblemDetail {
                error_type: category,
                detail: "test".to_string(),
            };
            let rendered = serde_json::to_string(&original).expect("serialises");
            pretty_assertions::assert_eq!(serde_json::from_str::<BatchProblemDetail>(&rendered).expect("deserialises"), original);
        }
    }

    #[test]
    fn a_batch_result_survives_a_round_trip() {
        let original = BatchOperationResult {
            success: vec!["urn:ngsi-ld:Entity:A".to_string(), "urn:ngsi-ld:Entity:B".to_string()],
            errors: vec![BatchEntityError::new(
                "urn:ngsi-ld:Entity:C",
                NgsiLdErrorType::InternalError,
                "Something went wrong",
            )],
        };
        let rendered = serde_json::to_string(&original).expect("serialises");
        pretty_assertions::assert_eq!(serde_json::from_str::<BatchOperationResult>(&rendered).expect("deserialises"), original);
    }

    #[test]
    fn a_problem_detail_naming_a_category_this_build_does_not_know_is_rejected() {
        let rendered = json!({"type": "https://uri.etsi.org/ngsi-ld/errors/Invented", "detail": "x"}).to_string();
        assert!(serde_json::from_str::<BatchProblemDetail>(&rendered).is_err());
    }

    #[test]
    fn a_problem_detail_missing_a_required_member_is_rejected() {
        assert!(serde_json::from_str::<BatchProblemDetail>(&json!({"detail": "x"}).to_string()).is_err());
        assert!(serde_json::from_str::<BatchProblemDetail>(&json!({"type": "https://uri.etsi.org/ngsi-ld/errors/Conflict"}).to_string()).is_err());
    }
}
