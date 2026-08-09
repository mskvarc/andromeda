//! The result of a batch operation, and the per-entity errors in it
//! (clauses 5.2.16 and 6.17).

use crate::error_type::NgsiLdErrorType;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use iri_rs::IriRefBuf;
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
    pub success: Vec<Uri>,
    /// One entry per entity the operation could not apply to.
    pub errors: Vec<BatchEntityError>,
}

/// One entity's failure within a batch operation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BatchEntityError {
    /// The entity the failure is about.
    #[serde(rename = "entityId")]
    pub entity_id: Uri,
    /// Registration responsible for a failed distributed operation.
    #[serde(rename = "registrationId", skip_serializing_if = "Option::is_none")]
    pub registration_id: Option<Uri>,
    /// What went wrong with it.
    pub error: BatchProblemDetail,
}

impl BatchEntityError {
    /// Report `error_type` against `entity_id`, with a human-readable detail.
    pub fn new(entity_id: Uri, error_type: NgsiLdErrorType, detail: &str) -> BatchEntityError {
        BatchEntityError {
            entity_id,
            registration_id: None,
            error: BatchProblemDetail {
                error_type: BatchErrorKind::Known(error_type),
                detail: detail.to_string(),
            },
        }
    }

    /// Attach the registration responsible for a failed distributed operation.
    pub fn with_registration_id(mut self, registration_id: Uri) -> BatchEntityError {
        self.registration_id = Some(registration_id);
        self
    }
}

/// The `type` of a batch problem detail.
///
/// A known NGSI-LD category carries its title and status; RFC 7807 clause 3
/// also permits any other type URI — which a peer broker (Scorpio, Orion-LD)
/// may report on a forwarded batch — so an unrecognised one is kept verbatim
/// rather than rejected.
#[derive(Debug, Clone, PartialEq)]
pub enum BatchErrorKind {
    /// One of the twelve ETSI error categories (clause 5.5.2).
    Known(NgsiLdErrorType),
    /// Any other RFC 7807 type URI.
    Other(IriRefBuf),
}

/// An RFC 7807 problem detail inside a batch error.
///
/// A known category derives its `title` and `status` from itself on the way
/// out, so a batch error we produce cannot report a status that disagrees with
/// its own type IRI; a foreign type is carried verbatim.
#[derive(Debug, Clone, PartialEq)]
pub struct BatchProblemDetail {
    /// The problem's type: a known NGSI-LD category, or any other RFC 7807 URI.
    pub error_type: BatchErrorKind,
    /// The human-readable explanation.
    pub detail: String,
}

impl Serialize for BatchProblemDetail {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match &self.error_type {
            BatchErrorKind::Known(category) => {
                let mut state = serializer.serialize_struct("BatchProblemDetail", 4)?;
                state.serialize_field("type", &category.type_iri())?;
                state.serialize_field("title", category.title())?;
                state.serialize_field("status", &category.status_code().as_u16())?;
                state.serialize_field("detail", &self.detail)?;
                state.end()
            }
            BatchErrorKind::Other(type_uri) => {
                // An unknown type has no category to derive a title or status
                // from; RFC 7807 makes both optional, so only type and detail
                // are written.
                let mut state = serializer.serialize_struct("BatchProblemDetail", 2)?;
                state.serialize_field("type", type_uri)?;
                state.serialize_field("detail", &self.detail)?;
                state.end()
            }
        }
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
        let mut type_uri: Option<IriRefBuf> = None;
        let mut detail: Option<String> = None;

        while let Some(key) = map.next_key::<&str>()? {
            match key {
                "type" => type_uri = Some(map.next_value()?),
                "detail" => detail = Some(map.next_value()?),
                // `title` and `status` are derived from a known category on the
                // way out; every other member is an RFC 7807 clause 3 extension,
                // which a consumer must ignore rather than reject — so an
                // extension a peer broker adds cannot poison the batch result.
                _other => {
                    let _ = map.next_value::<de::IgnoredAny>()?;
                }
            }
        }

        let type_uri = type_uri.ok_or_else(|| de::Error::missing_field("type"))?;
        let detail = detail.ok_or_else(|| de::Error::missing_field("detail"))?;
        // A recognised category keeps its typed convenience; any other URI is
        // accepted verbatim, since RFC 7807 clause 3 permits arbitrary type URIs.
        let error_type = match NgsiLdErrorType::from_type_iri(&type_uri) {
            Some(category) => BatchErrorKind::Known(category),
            None => BatchErrorKind::Other(type_uri),
        };

        Ok(BatchProblemDetail { error_type, detail })
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        batch_operation::{BatchEntityError, BatchErrorKind, BatchOperationResult, BatchProblemDetail},
        error_type::NgsiLdErrorType,
    };
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use iri_rs::IriRefBuf;
    use serde_json::json;
    use strum::IntoEnumIterator;

    fn uri(value: &str) -> Uri {
        value.parse().expect("valid test URI")
    }

    fn iri_ref(value: &str) -> IriRefBuf {
        IriRefBuf::new(value.to_string()).expect("valid test IRI reference")
    }

    #[test]
    fn a_batch_result_serialises_its_successes_and_its_failures() {
        let result = BatchOperationResult {
            success: vec![uri("urn:ngsi-ld:Entity:1")],
            errors: vec![BatchEntityError::new(
                uri("urn:ngsi-ld:Entity:2"),
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
    fn success_only_and_error_only_results_preserve_exact_identifiers() {
        let success_only = serde_json::to_value(BatchOperationResult {
            success: vec![uri("urn:ngsi-ld:Entity:1")],
            errors: vec![],
        })
        .expect("serialises");
        pretty_assertions::assert_eq!(success_only, json!({"success": ["urn:ngsi-ld:Entity:1"], "errors": []}));

        let error_only = serde_json::to_value(BatchOperationResult {
            success: vec![],
            errors: vec![BatchEntityError::new(
                uri("urn:ngsi-ld:Entity:2"),
                NgsiLdErrorType::AlreadyExists,
                "Entity already exists",
            )],
        })
        .expect("serialises");
        pretty_assertions::assert_eq!(error_only["success"], json!([]));
        pretty_assertions::assert_eq!(error_only["errors"][0]["entityId"], "urn:ngsi-ld:Entity:2");
        pretty_assertions::assert_eq!(error_only["errors"][0]["error"]["status"], 409);
        assert!(error_only["errors"][0].get("registrationId").is_none());
    }

    #[test]
    fn distributed_batch_error_uses_the_exact_registration_id_member() {
        let rendered = serde_json::to_value(
            BatchEntityError::new(uri("urn:ngsi-ld:Entity:2"), NgsiLdErrorType::InternalError, "forwarding failed")
                .with_registration_id(uri("urn:ngsi-ld:ContextSourceRegistration:7")),
        )
        .expect("serialises");
        pretty_assertions::assert_eq!(rendered["entityId"], "urn:ngsi-ld:Entity:2");
        pretty_assertions::assert_eq!(rendered["registrationId"], "urn:ngsi-ld:ContextSourceRegistration:7");
        assert!(rendered.get("registration_id").is_none());
    }

    #[test]
    fn a_problem_detail_derives_its_rfc_7807_members_from_its_category() {
        let rendered = serde_json::to_value(BatchProblemDetail {
            error_type: BatchErrorKind::Known(NgsiLdErrorType::BadRequestData),
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
                error_type: BatchErrorKind::Known(category),
                detail: "test".to_string(),
            };
            let rendered = serde_json::to_string(&original).expect("serialises");
            pretty_assertions::assert_eq!(serde_json::from_str::<BatchProblemDetail>(&rendered).expect("deserialises"), original);
        }
    }

    #[test]
    fn a_batch_result_survives_a_round_trip() {
        let original = BatchOperationResult {
            success: vec![uri("urn:ngsi-ld:Entity:A"), uri("https://example.com/entities/B")],
            errors: vec![BatchEntityError::new(
                uri("urn:ngsi-ld:Entity:C"),
                NgsiLdErrorType::InternalError,
                "Something went wrong",
            )],
        };
        let rendered = serde_json::to_string(&original).expect("serialises");
        pretty_assertions::assert_eq!(serde_json::from_str::<BatchOperationResult>(&rendered).expect("deserialises"), original);
    }

    #[test]
    fn a_problem_detail_naming_a_type_uri_this_build_does_not_know_is_kept_verbatim() {
        // RFC 7807 clause 3 permits arbitrary `type` URIs, so a foreign one from
        // a peer broker is accepted as `Other` rather than rejected.
        let rendered = json!({"type": "https://uri.etsi.org/ngsi-ld/errors/Invented", "detail": "x"}).to_string();
        let parsed = serde_json::from_str::<BatchProblemDetail>(&rendered).expect("a foreign type URI is accepted");
        pretty_assertions::assert_eq!(
            parsed.error_type,
            BatchErrorKind::Other(iri_ref("https://uri.etsi.org/ngsi-ld/errors/Invented"))
        );
    }

    #[test]
    fn an_extension_member_and_a_non_etsi_type_deserialise_rather_than_poisoning_the_batch() {
        // RFC 7807 clause 3: consumers ignore extension members and accept any
        // type URI, so a Scorpio/Orion-LD response cannot fail the whole batch.
        let rendered = json!({
            "type": "https://scorpio.example/errors/Custom",
            "title": "Something",
            "status": 418,
            "detail": "brewing",
            "instance": "urn:ngsi-ld:Entity:9",
            "vendorField": {"nested": true}
        })
        .to_string();
        let parsed = serde_json::from_str::<BatchProblemDetail>(&rendered).expect("a foreign problem detail deserialises");
        pretty_assertions::assert_eq!(parsed.error_type, BatchErrorKind::Other(iri_ref("https://scorpio.example/errors/Custom")));
        pretty_assertions::assert_eq!(parsed.detail, "brewing");
    }

    #[test]
    fn a_problem_detail_missing_a_required_member_is_rejected() {
        assert!(serde_json::from_str::<BatchProblemDetail>(&json!({"detail": "x"}).to_string()).is_err());
        assert!(serde_json::from_str::<BatchProblemDetail>(&json!({"type": "https://uri.etsi.org/ngsi-ld/errors/Conflict"}).to_string()).is_err());
    }
}
