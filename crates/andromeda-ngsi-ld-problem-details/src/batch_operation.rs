//! The result of a batch operation, and the per-entity errors in it
//! (clause 5.2.16).

use crate::{
    error_type::NgsiLdErrorType,
    problem_detail::{ProblemClassification, WireProblemClassification},
    problem_document_error::ProblemDocumentError,
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
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

/// The outcome of a batch operation (clause 5.2.16).
///
/// A batch answers per entity, so a partial failure is the normal case rather
/// than an exception: the identifiers that succeeded and the ones that did not
/// are reported side by side.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BatchOperationResult {
    /// Identifiers of the entities the operation applied to.
    pub success: Vec<Identifier>,
    /// One entry per entity the operation could not apply to.
    pub errors: Vec<BatchEntityError>,
}

/// One entity's failure within a batch operation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BatchEntityError {
    /// The entity the failure is about.
    #[serde(rename = "entityId")]
    pub entity_id: Identifier,
    /// Registration responsible for a failed distributed operation.
    #[serde(rename = "registrationId", skip_serializing_if = "Option::is_none")]
    pub registration_id: Option<Identifier>,
    /// What went wrong with it.
    pub error: BatchProblemDetail,
}

impl BatchEntityError {
    /// Report `error_type` against `entity_id`, with a human-readable detail.
    #[must_use]
    pub fn new(entity_id: Identifier, error_type: NgsiLdErrorType, detail: &str) -> BatchEntityError {
        BatchEntityError::from_classification(entity_id, ProblemClassification::Category(error_type), detail)
    }

    /// Report a binding-specific classification against `entity_id`: the
    /// clause 6.3.17 statuses a forwarded part of a distributed operation can
    /// fail with (502, 504, 508) beside the ordinary clause 5.5.2 categories.
    #[must_use]
    pub fn from_classification(entity_id: Identifier, classification: ProblemClassification, detail: &str) -> BatchEntityError {
        BatchEntityError {
            entity_id,
            registration_id: None,
            error: BatchProblemDetail {
                error_type: BatchErrorKind::Known(classification),
                detail: detail.to_string(),
            },
        }
    }

    /// Attach the registration responsible for a failed distributed operation.
    #[must_use]
    pub fn with_registration_id(mut self, registration_id: Identifier) -> BatchEntityError {
        self.registration_id = Some(registration_id);
        self
    }
}

/// The `type` of a batch problem detail.
///
/// A known NGSI-LD classification carries its title and status; RFC 7807
/// clause 3 also permits any other type URI — which a peer broker (Scorpio,
/// Orion-LD) may report on a forwarded batch — so an unrecognised one is kept
/// verbatim rather than rejected.
#[derive(Debug, Clone, PartialEq)]
pub enum BatchErrorKind {
    /// One of the twelve ETSI error categories (clause 5.5.2), or one of the
    /// clause 6.3.17 distributed-operation statuses layered on a category.
    Known(ProblemClassification),
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
            BatchErrorKind::Known(classification) => {
                let mut state = serializer.serialize_struct("BatchProblemDetail", 4)?;
                state.serialize_field("type", &classification.type_iri())?;
                state.serialize_field("title", classification.title())?;
                state.serialize_field("status", &classification.status_code().as_u16())?;
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

/// The detail a problem detail without a `detail` member is read with: its
/// `title` when it has one, else nothing. RFC 7807 clause 3.1 makes both
/// members optional, and a peer broker that omits `detail` must still have its
/// error merged into the batch rather than failing the whole response.
fn detail_or_title(detail: Option<String>, title: Option<String>) -> String {
    detail.or(title).unwrap_or_default()
}

/// Reads a problem detail back into its category and message.
struct BatchProblemDetailVisitor;

impl<'de> Visitor<'de> for BatchProblemDetailVisitor {
    type Value = BatchProblemDetail;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a BatchProblemDetail object with type, title, status, and detail fields")
    }

    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<BatchProblemDetail, M::Error> {
        let mut type_uri: Option<IriRefBuf> = None;
        let mut title: Option<String> = None;
        let mut status: Option<u16> = None;
        let mut detail: Option<String> = None;

        // Keys are read owned rather than borrowed so the detail deserialises
        // from an in-memory `Value` (a forwarded problem detail) as well as
        // from a byte buffer.
        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "type" => type_uri = Some(map.next_value()?),
                "title" => title = Some(map.next_value()?),
                "status" => status = Some(map.next_value()?),
                "detail" => detail = Some(map.next_value()?),
                // Every other member is an RFC 7807 clause 3 extension, which a
                // consumer must ignore rather than reject — so an extension a
                // peer broker adds cannot poison the batch result.
                _other => {
                    let _ = map.next_value::<de::IgnoredAny>()?;
                }
            }
        }

        let type_uri = type_uri.ok_or_else(|| de::Error::missing_field("type"))?;
        let detail = detail_or_title(detail, title);
        // A recognised category keeps its typed convenience, with the clause
        // 6.3.17 statuses read back as the distributed classifications they
        // name; any other URI is accepted verbatim, since RFC 7807 clause 3
        // permits arbitrary type URIs.
        let error_type = if let Some(distributed) = ProblemClassification::from_distributed_type_iri(&type_uri) {
            if let Some(status) = status
                && status != distributed.status_code().as_u16()
            {
                return Err(de::Error::custom(ProblemDocumentError::DistributedStatusMismatch));
            }
            BatchErrorKind::Known(distributed)
        } else {
            match NgsiLdErrorType::from_type_iri(&type_uri) {
                Some(category) => BatchErrorKind::Known(WireProblemClassification { category, status }.classification().map_err(de::Error::custom)?),
                None => BatchErrorKind::Other(type_uri),
            }
        };

        Ok(BatchProblemDetail { error_type, detail })
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        batch_operation::{BatchEntityError, BatchErrorKind, BatchOperationResult, BatchProblemDetail},
        error_type::NgsiLdErrorType,
        problem_detail::ProblemClassification,
    };
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use iri_rs::IriRefBuf;
    use serde_json::json;
    use strum::IntoEnumIterator;

    fn uri(value: &str) -> Identifier {
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
            error_type: BatchErrorKind::Known(ProblemClassification::Category(NgsiLdErrorType::BadRequestData)),
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
                error_type: BatchErrorKind::Known(ProblemClassification::Category(category)),
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
    fn a_problem_detail_missing_its_type_is_rejected() {
        assert!(serde_json::from_str::<BatchProblemDetail>(&json!({"detail": "x"}).to_string()).is_err());
    }

    #[test]
    fn a_problem_detail_without_detail_reads_its_title_and_without_both_reads_nothing() {
        // RFC 7807 clause 3.1 makes `detail` optional; a peer that reports only
        // a title must still merge into the batch.
        let titled =
            serde_json::from_str::<BatchProblemDetail>(&json!({"type": "https://uri.etsi.org/ngsi-ld/errors/Conflict", "title": "exists"}).to_string())
                .expect("a title-only problem detail deserialises");
        pretty_assertions::assert_eq!(titled.detail, "exists");
        let bare = serde_json::from_str::<BatchProblemDetail>(&json!({"type": "https://uri.etsi.org/ngsi-ld/errors/Conflict"}).to_string())
            .expect("a type-only problem detail deserialises");
        pretty_assertions::assert_eq!(bare.detail, "");
        pretty_assertions::assert_eq!(
            bare.error_type,
            BatchErrorKind::Known(ProblemClassification::Category(NgsiLdErrorType::Conflict))
        );
    }

    #[test]
    fn the_clause_6_3_17_statuses_serialise_and_read_back_as_their_distributed_classification() {
        // A distributed 404 is wire-identical to the ResourceNotFound category
        // and reads back as it; the gateway statuses have no category of their
        // own and read back as the distributed classification.
        for (classification, status, expected) in [
            (ProblemClassification::LoopDetected, 508, ProblemClassification::LoopDetected),
            (ProblemClassification::GatewayTimeout, 504, ProblemClassification::GatewayTimeout),
            (ProblemClassification::BadGateway, 502, ProblemClassification::BadGateway),
            (
                ProblemClassification::DistributedResourceNotFound,
                404,
                ProblemClassification::Category(NgsiLdErrorType::ResourceNotFound),
            ),
        ] {
            let rendered = serde_json::to_value(BatchEntityError::from_classification(
                uri("urn:ngsi-ld:Entity:1"),
                classification,
                "forwarding failed",
            ))
            .expect("serialises");
            pretty_assertions::assert_eq!(rendered["error"]["status"], status, "{classification:?}");
            pretty_assertions::assert_eq!(rendered["error"]["type"], classification.type_iri().as_str());
            let parsed = serde_json::from_value::<BatchEntityError>(rendered).expect("deserialises");
            pretty_assertions::assert_eq!(parsed.error.error_type, BatchErrorKind::Known(expected), "{classification:?}");
        }
    }

    #[test]
    fn a_known_type_with_a_disagreeing_status_is_rejected() {
        let rendered = json!({"type": "https://uri.etsi.org/ngsi-ld/errors/Conflict", "status": 500, "detail": "x"}).to_string();
        assert!(serde_json::from_str::<BatchProblemDetail>(&rendered).is_err());
    }
}
