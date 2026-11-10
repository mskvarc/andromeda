//! Framework-independent single-error problem details from clause 5.5.3.

use crate::{error_type::NgsiLdErrorType, problem_document_error::ProblemDocumentError, status_member::StatusMember};
use http::{StatusCode, Uri};
use iri_rs::{Iri, IriRefBuf, iri};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de, ser::SerializeStruct};
use thiserror::Error;

/// A typed NGSI-LD error classification with the status selected by its authoritative HTTP binding.
///
/// Ordinary categories derive their status from clause 6.3.2. The named distributed-operation classifications preserve the more specific statuses required by clause 6.3.17 without allowing callers to invent a category/status pair.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProblemClassification {
    /// An ordinary clause 5.5.2 category whose status comes from clause 6.3.2.
    Category(NgsiLdErrorType),
    /// A distributed operation detected a registration loop and returns 508.
    LoopDetected,
    /// A distributed operation timed out and returns 504.
    GatewayTimeout,
    /// A distributed operation did not find the requested resource and returns 404.
    DistributedResourceNotFound,
    /// Another distributed gateway failure occurred and returns 502.
    BadGateway,
    /// The one registered source holding the data cannot serve the requested
    /// representation and returns 501 (Table 6.4.3.2-3, clause 6.3.7).
    NotImplemented,
    /// A local operation cannot run now but the identical request can succeed
    /// shortly, and returns 503 with `Retry-After`: a saturated connection
    /// pool, an unreachable datastore, a transaction rolled back on every
    /// attempt, a Snapshot still preparing.
    ///
    /// Clause 6.3.2 admits the HTTP binding's own 503 beside Table 6.3.2-1, but
    /// Table 6.3.2-1 binds `InternalError` to 500, an unexpected failure a
    /// client does not retry. The document therefore carries a dedicated type
    /// under the ETSI errors namespace, as the clause 6.3.17 gateway failures
    /// do, so the type and the status agree that the request is retryable.
    ServiceUnavailable,
}

impl ProblemClassification {
    /// The clause 5.5.2 payload category represented by this classification.
    #[must_use]
    pub const fn error_type(self) -> NgsiLdErrorType {
        match self {
            ProblemClassification::Category(error_type) => error_type,
            ProblemClassification::LoopDetected
            | ProblemClassification::GatewayTimeout
            | ProblemClassification::BadGateway
            | ProblemClassification::ServiceUnavailable => NgsiLdErrorType::InternalError,
            ProblemClassification::DistributedResourceNotFound => NgsiLdErrorType::ResourceNotFound,
            ProblemClassification::NotImplemented => NgsiLdErrorType::OperationNotSupported,
        }
    }

    /// The HTTP status selected by the category or the more specific clause 6.3.17 binding.
    #[must_use]
    pub const fn status_code(self) -> StatusCode {
        match self {
            ProblemClassification::Category(error_type) => error_type.status_code(),
            ProblemClassification::LoopDetected => StatusCode::LOOP_DETECTED,
            ProblemClassification::GatewayTimeout => StatusCode::GATEWAY_TIMEOUT,
            ProblemClassification::DistributedResourceNotFound => StatusCode::NOT_FOUND,
            ProblemClassification::BadGateway => StatusCode::BAD_GATEWAY,
            ProblemClassification::NotImplemented => StatusCode::NOT_IMPLEMENTED,
            ProblemClassification::ServiceUnavailable => StatusCode::SERVICE_UNAVAILABLE,
        }
    }

    /// The problem `type` IRI this classification serialises with.
    ///
    /// Clause 6.3.17 names no IRI for the 508/504/502 distributed failures, and
    /// clause 5.5.2 none for a retryable 503, so the broker publishes dedicated
    /// IRIs under the ETSI errors namespace rather than collapsing all four
    /// onto `InternalError`, which Table 6.3.2-1 binds to 500 and which would
    /// leave the `type` disagreeing with the status. The remaining
    /// classifications carry their clause 5.5.2 category's canonical IRI.
    #[must_use]
    pub const fn type_iri(self) -> Iri<&'static str> {
        match self {
            ProblemClassification::LoopDetected => iri!("https://uri.etsi.org/ngsi-ld/errors/LoopDetected"),
            ProblemClassification::GatewayTimeout => iri!("https://uri.etsi.org/ngsi-ld/errors/GatewayTimeout"),
            ProblemClassification::BadGateway => iri!("https://uri.etsi.org/ngsi-ld/errors/BadGateway"),
            ProblemClassification::ServiceUnavailable => iri!("https://uri.etsi.org/ngsi-ld/errors/ServiceUnavailable"),
            ProblemClassification::Category(_) | ProblemClassification::DistributedResourceNotFound | ProblemClassification::NotImplemented => {
                self.error_type().type_iri()
            }
        }
    }

    /// The problem `title` this classification serialises with, paired with
    /// [`type_iri`](Self::type_iri).
    #[must_use]
    pub fn title(self) -> &'static str {
        match self {
            ProblemClassification::LoopDetected => "Loop detected",
            ProblemClassification::GatewayTimeout => "Gateway timeout",
            ProblemClassification::BadGateway => "Bad gateway",
            ProblemClassification::ServiceUnavailable => "Service unavailable",
            ProblemClassification::Category(_) | ProblemClassification::DistributedResourceNotFound | ProblemClassification::NotImplemented => {
                self.error_type().title()
            }
        }
    }

    /// The classification a dedicated `type` IRI names, if any.
    ///
    /// Recognises the four IRIs [`type_iri`](Self::type_iri) publishes beyond
    /// the clause 5.5.2 categories, the clause 6.3.17 gateway failures and the
    /// retryable 503, so a peer broker's document round-trips back into its
    /// classification.
    #[must_use]
    pub fn from_dedicated_type_iri(iri: &IriRefBuf) -> Option<ProblemClassification> {
        [
            ProblemClassification::LoopDetected,
            ProblemClassification::GatewayTimeout,
            ProblemClassification::BadGateway,
            ProblemClassification::ServiceUnavailable,
        ]
        .into_iter()
        .find(|classification| classification.type_iri().as_str() == iri.as_str())
    }
}

/// The single-error wire value required by ETSI GS CIM 009 v1.9.1 clause 5.5.3.
///
/// Its classification fixes `type`, `title`, and `status`; callers supply only client-safe detail and an optional request instance, and may leave `status` out of a problem that accompanies no error response.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProblemDetail {
    /// Typed source of the category and HTTP status.
    classification: ProblemClassification,
    /// Client-visible human-readable explanation.
    detail: String,
    /// Whether the document carries the `status` its classification binds.
    status_member: StatusMember,
    /// Optional request target supplied by the inbound HTTP boundary.
    ///
    /// Boxed because a `Uri` is 88 bytes against the 32 the rest of the detail
    /// occupies, and this type is the `Err` of nearly every handler's `Result`:
    /// carrying it inline would make every success path pay for the failure's
    /// width.
    instance: Option<Box<Uri>>,
}

impl ProblemDetail {
    /// Construct an ordinary clause 5.5.2 problem detail.
    pub fn new(error_type: NgsiLdErrorType, detail: impl Into<String>) -> ProblemDetail {
        ProblemDetail {
            classification: ProblemClassification::Category(error_type),
            detail: detail.into(),
            status_member: StatusMember::Stated,
            instance: None,
        }
    }

    /// Construct a problem detail from a binding-specific typed classification.
    pub fn from_classification(classification: ProblemClassification, detail: impl Into<String>) -> ProblemDetail {
        ProblemDetail {
            classification,
            detail: detail.into(),
            status_member: StatusMember::Stated,
            instance: None,
        }
    }

    /// The same problem without the `status` member, for a problem that
    /// accompanies a result which is not an error response.
    #[must_use]
    pub const fn without_status(mut self) -> ProblemDetail {
        self.status_member = StatusMember::Omitted;
        self
    }

    /// Attach the exact inbound request URI as the RFC 7807 `instance` member.
    #[must_use]
    pub fn with_instance(mut self, instance: Uri) -> ProblemDetail {
        self.instance = Some(Box::new(instance));
        self
    }

    /// The typed classification that fixes the payload category and response status.
    #[must_use]
    pub const fn classification(&self) -> ProblemClassification {
        self.classification
    }

    /// The clause 5.5.2 category carried by the payload.
    #[must_use]
    pub const fn error_type(&self) -> NgsiLdErrorType {
        self.classification.error_type()
    }

    /// The HTTP status fixed by the applicable binding.
    #[must_use]
    pub const fn status_code(&self) -> StatusCode {
        self.classification.status_code()
    }

    /// Whether the document carries the `status` member.
    #[must_use]
    pub const fn status_member(&self) -> StatusMember {
        self.status_member
    }

    /// The client-visible detail.
    #[must_use]
    pub fn detail(&self) -> &str {
        &self.detail
    }

    /// The request instance, when the HTTP boundary attached one.
    #[must_use]
    pub fn instance(&self) -> Option<&Uri> {
        self.instance.as_deref()
    }
}

impl Serialize for ProblemDetail {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let status_fields = match self.status_member {
            StatusMember::Stated => 1,
            StatusMember::Omitted => 0,
        };
        let instance_fields = match self.instance {
            Some(_) => 1,
            None => 0,
        };
        let classification = self.classification;
        let mut state = serializer.serialize_struct("ProblemDetail", 3 + status_fields + instance_fields)?;
        state.serialize_field("type", &classification.type_iri())?;
        state.serialize_field("title", classification.title())?;
        match self.status_member {
            StatusMember::Stated => state.serialize_field("status", &self.status_code().as_u16())?,
            StatusMember::Omitted => state.skip_field("status")?,
        }
        state.serialize_field("detail", &self.detail)?;
        if let Some(instance) = &self.instance {
            state.serialize_field("instance", instance.path_and_query().map_or(instance.path(), |value| value.as_str()))?;
        }
        state.end()
    }
}

/// The `(type, status)` pair a problem detail arrives with on the wire, and
/// the one rule that turns it back into a classification.
///
/// Shared by the single problem detail and the clause 5.2.17 batch error so
/// the two cannot disagree about which statuses name the clause 6.3.17
/// distributed classifications.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WireProblemClassification {
    /// The category the `type` IRI named.
    pub(crate) category: NgsiLdErrorType,
    /// The `status` member, when the wire value carried one.
    pub(crate) status: Option<u16>,
}

impl WireProblemClassification {
    /// The classification the pair names: a clause 6.3.17 status layered on
    /// its category, the category alone when the status agrees with clause
    /// 6.3.2 or is absent, and a rejection when it contradicts the category.
    pub(crate) const fn classification(self) -> Result<ProblemClassification, WireProblemClassificationError> {
        // A status that is the category's own clause 6.3.2 status names the
        // category, so a 404 ResourceNotFound reads back as the category it
        // is wire-identical to rather than as the distributed classification.
        match (self.category, self.status) {
            (category, None) => Ok(ProblemClassification::Category(category)),
            (category, Some(status)) if status == category.status_code().as_u16() => Ok(ProblemClassification::Category(category)),
            (NgsiLdErrorType::InternalError, Some(508)) => Ok(ProblemClassification::LoopDetected),
            (NgsiLdErrorType::InternalError, Some(504)) => Ok(ProblemClassification::GatewayTimeout),
            (NgsiLdErrorType::InternalError, Some(502)) => Ok(ProblemClassification::BadGateway),
            (NgsiLdErrorType::InternalError, Some(503)) => Ok(ProblemClassification::ServiceUnavailable),
            (category, Some(status)) => Err(WireProblemClassificationError::StatusDisagrees { category, status }),
        }
    }
}

/// A wire `(type, status)` pair that names no classification.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WireProblemClassificationError {
    /// The status is neither the category's clause 6.3.2 status nor a clause
    /// 6.3.17 status that category can carry.
    #[error("problem status {status} disagrees with its NGSI-LD type {category}")]
    StatusDisagrees {
        /// The category the type IRI named.
        category: NgsiLdErrorType,
        /// The status that contradicted it.
        status: u16,
    },
}

/// Deserializable representation used to validate the invariant-bearing public value.
#[derive(Deserialize)]
struct WireProblemDetail {
    /// Problem type IRI.
    #[serde(rename = "type")]
    type_iri: IriRefBuf,
    /// Informational title, derived again after classification.
    title: Option<String>,
    /// Binding-selected HTTP status.
    status: Option<u16>,
    /// Human-readable detail.
    detail: String,
    /// Optional request instance.
    instance: Option<IriRefBuf>,
}

impl<'de> Deserialize<'de> for ProblemDetail {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<ProblemDetail, D::Error> {
        let wire = WireProblemDetail::deserialize(deserializer)?;
        // A dedicated IRI is recognised before the category scan so a peer
        // broker's 508/504/502/503 document round-trips into its own
        // classification rather than the InternalError category.
        // `expected_title` is the title of the type IRI actually named on the
        // wire, not of the resolved classification: a peer may stamp such a
        // failure as `InternalError` plus its status, and its "Internal error"
        // title must still be accepted even though the pair resolves to a
        // distinct classification.
        let (classification, expected_title) = if let Some(dedicated) = ProblemClassification::from_dedicated_type_iri(&wire.type_iri) {
            if let Some(status) = wire.status
                && status != dedicated.status_code().as_u16()
            {
                return Err(de::Error::custom(ProblemDocumentError::DedicatedStatusMismatch));
            }
            (dedicated, dedicated.title())
        } else {
            let error_type = NgsiLdErrorType::from_type_iri(&wire.type_iri).ok_or_else(|| de::Error::custom(ProblemDocumentError::UnknownType))?;
            let classification = WireProblemClassification {
                category: error_type,
                status: wire.status,
            }
            .classification()
            .map_err(de::Error::custom)?;
            (classification, error_type.title())
        };
        if let Some(title) = wire.title
            && title != expected_title
        {
            return Err(de::Error::custom(ProblemDocumentError::TitleMismatch));
        }
        let instance = wire
            .instance
            .map(|instance| instance.as_str().parse::<Uri>().map(Box::new).map_err(de::Error::custom))
            .transpose()?;
        let status_member = match wire.status {
            Some(_) => StatusMember::Stated,
            None => StatusMember::Omitted,
        };
        Ok(ProblemDetail {
            classification,
            detail: wire.detail,
            status_member,
            instance,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error_type::NgsiLdErrorType,
        problem_detail::{ProblemClassification, ProblemDetail},
        status_member::StatusMember,
    };
    use http::{StatusCode, Uri};
    use serde_json::json;
    use strum::IntoEnumIterator;

    #[test]
    fn ordinary_categories_serialize_with_derived_type_title_and_status() -> Result<(), serde_json::Error> {
        for category in NgsiLdErrorType::iter() {
            let value = serde_json::to_value(ProblemDetail::new(category, "detail"))?;
            pretty_assertions::assert_eq!(value["type"], category.type_iri().as_str());
            pretty_assertions::assert_eq!(value["title"], category.title());
            pretty_assertions::assert_eq!(value["status"], category.status_code().as_u16());
            pretty_assertions::assert_eq!(value["detail"], "detail");
            assert!(value.get("instance").is_none());
        }
        Ok(())
    }

    #[test]
    fn request_instance_is_serialized_without_changing_category_fields() -> Result<(), serde_json::Error> {
        let problem = ProblemDetail::new(NgsiLdErrorType::ResourceNotFound, "Entity missing").with_instance(Uri::from_static("/ngsi-ld/v1/entities/x"));
        let value = serde_json::to_value(problem)?;
        pretty_assertions::assert_eq!(value["instance"], "/ngsi-ld/v1/entities/x");
        pretty_assertions::assert_eq!(value["status"], 404);
        pretty_assertions::assert_eq!(value["title"], "Resource not found");
        Ok(())
    }

    #[test]
    fn distributed_classifications_fix_their_payload_types_titles_and_statuses() -> Result<(), serde_json::Error> {
        for (classification, expected_type_iri, expected_title, expected_status) in [
            (
                ProblemClassification::LoopDetected,
                "https://uri.etsi.org/ngsi-ld/errors/LoopDetected",
                "Loop detected",
                StatusCode::LOOP_DETECTED,
            ),
            (
                ProblemClassification::GatewayTimeout,
                "https://uri.etsi.org/ngsi-ld/errors/GatewayTimeout",
                "Gateway timeout",
                StatusCode::GATEWAY_TIMEOUT,
            ),
            (
                ProblemClassification::DistributedResourceNotFound,
                "https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound",
                "Resource not found",
                StatusCode::NOT_FOUND,
            ),
            (
                ProblemClassification::BadGateway,
                "https://uri.etsi.org/ngsi-ld/errors/BadGateway",
                "Bad gateway",
                StatusCode::BAD_GATEWAY,
            ),
            (
                ProblemClassification::NotImplemented,
                "https://uri.etsi.org/ngsi-ld/errors/OperationNotSupported",
                "Operation not supported",
                StatusCode::NOT_IMPLEMENTED,
            ),
            (
                ProblemClassification::ServiceUnavailable,
                "https://uri.etsi.org/ngsi-ld/errors/ServiceUnavailable",
                "Service unavailable",
                StatusCode::SERVICE_UNAVAILABLE,
            ),
        ] {
            let problem = ProblemDetail::from_classification(classification, "distributed failure");
            let value = serde_json::to_value(&problem)?;
            pretty_assertions::assert_eq!(problem.status_code(), expected_status);
            pretty_assertions::assert_eq!(value["type"], expected_type_iri);
            pretty_assertions::assert_eq!(value["title"], expected_title);
            pretty_assertions::assert_eq!(value["status"], expected_status.as_u16());
        }
        Ok(())
    }

    #[test]
    fn dedicated_types_round_trip_through_their_own_iris() -> Result<(), serde_json::Error> {
        for classification in [
            ProblemClassification::LoopDetected,
            ProblemClassification::GatewayTimeout,
            ProblemClassification::BadGateway,
            ProblemClassification::ServiceUnavailable,
        ] {
            let rendered = serde_json::to_string(&ProblemDetail::from_classification(classification, "distributed failure"))?;
            let parsed = serde_json::from_str::<ProblemDetail>(&rendered)?;
            pretty_assertions::assert_eq!(parsed.classification(), classification);
        }
        Ok(())
    }

    #[test]
    fn an_internal_error_with_a_dedicated_status_is_read_inbound() -> Result<(), serde_json::Error> {
        // A peer may stamp these failures as InternalError plus the status
        // that names them; the broker reads that pairing as the classification.
        for (status, expected) in [
            (508, ProblemClassification::LoopDetected),
            (504, ProblemClassification::GatewayTimeout),
            (502, ProblemClassification::BadGateway),
            (503, ProblemClassification::ServiceUnavailable),
        ] {
            let value = json!({
                "type": "https://uri.etsi.org/ngsi-ld/errors/InternalError",
                "title": "Internal error",
                "status": status,
                "detail": "distributed failure"
            });
            let parsed = serde_json::from_value::<ProblemDetail>(value)?;
            pretty_assertions::assert_eq!(parsed.classification(), expected);
        }
        Ok(())
    }

    #[test]
    fn a_dedicated_type_with_a_disagreeing_status_is_rejected() {
        for type_iri in [
            "https://uri.etsi.org/ngsi-ld/errors/LoopDetected",
            "https://uri.etsi.org/ngsi-ld/errors/ServiceUnavailable",
        ] {
            let value = json!({ "type": type_iri, "status": 500, "detail": "x" });
            assert!(serde_json::from_value::<ProblemDetail>(value).is_err(), "{type_iri}");
        }
    }

    /// Table 6.3.2-1 binds `InternalError` to 500, so a document of any other
    /// status names another type.
    #[test]
    fn only_a_500_carries_the_internal_error_type() -> Result<(), serde_json::Error> {
        let classifications = NgsiLdErrorType::iter().map(ProblemClassification::Category).chain([
            ProblemClassification::LoopDetected,
            ProblemClassification::GatewayTimeout,
            ProblemClassification::DistributedResourceNotFound,
            ProblemClassification::BadGateway,
            ProblemClassification::NotImplemented,
            ProblemClassification::ServiceUnavailable,
        ]);
        for classification in classifications {
            let value = serde_json::to_value(ProblemDetail::from_classification(classification, "detail"))?;
            if value["type"] == NgsiLdErrorType::InternalError.type_iri().as_str() {
                pretty_assertions::assert_eq!(value["status"], 500, "{classification:?}");
            }
        }
        Ok(())
    }

    /// Clause 5.5.3 requires `type`, `title` and `detail` only, so a problem
    /// accompanying no error response leaves `status` out.
    #[test]
    fn a_problem_without_status_serializes_without_the_member() -> Result<(), serde_json::Error> {
        let problem = ProblemDetail::from_classification(ProblemClassification::BadGateway, "a source failed").without_status();
        pretty_assertions::assert_eq!(
            serde_json::to_value(&problem)?,
            json!({
                "type": "https://uri.etsi.org/ngsi-ld/errors/BadGateway",
                "title": "Bad gateway",
                "detail": "a source failed"
            })
        );
        pretty_assertions::assert_eq!(problem.status_member(), StatusMember::Omitted);
        pretty_assertions::assert_eq!(problem.status_code(), StatusCode::BAD_GATEWAY);
        Ok(())
    }

    #[test]
    fn whether_status_is_stated_round_trips() -> Result<(), serde_json::Error> {
        for problem in [
            ProblemDetail::from_classification(ProblemClassification::BadGateway, "a source failed"),
            ProblemDetail::from_classification(ProblemClassification::BadGateway, "a source failed").without_status(),
            ProblemDetail::new(NgsiLdErrorType::InternalError, "unexpected").without_status(),
        ] {
            let parsed = serde_json::from_str::<ProblemDetail>(&serde_json::to_string(&problem)?)?;
            pretty_assertions::assert_eq!(parsed, problem);
        }
        Ok(())
    }

    #[test]
    fn inconsistent_wire_status_is_rejected() {
        let value = json!({
            "type": "https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound",
            "title": "Resource not found",
            "status": 500,
            "detail": "missing"
        });
        assert!(serde_json::from_value::<ProblemDetail>(value).is_err());
    }
}
