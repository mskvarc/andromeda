//! Framework-independent single-error problem details from clause 5.5.3.

use crate::error_type::NgsiLdErrorType;
use http::{StatusCode, Uri};
use iri_rs::IriRefBuf;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de, ser::SerializeStruct};

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
}

impl ProblemClassification {
    /// The clause 5.5.2 payload category represented by this classification.
    pub const fn error_type(self) -> NgsiLdErrorType {
        match self {
            ProblemClassification::Category(error_type) => error_type,
            ProblemClassification::LoopDetected | ProblemClassification::GatewayTimeout | ProblemClassification::BadGateway => NgsiLdErrorType::InternalError,
            ProblemClassification::DistributedResourceNotFound => NgsiLdErrorType::ResourceNotFound,
        }
    }

    /// The HTTP status selected by the category or the more specific clause 6.3.17 binding.
    pub const fn status_code(self) -> StatusCode {
        match self {
            ProblemClassification::Category(error_type) => error_type.status_code(),
            ProblemClassification::LoopDetected => StatusCode::LOOP_DETECTED,
            ProblemClassification::GatewayTimeout => StatusCode::GATEWAY_TIMEOUT,
            ProblemClassification::DistributedResourceNotFound => StatusCode::NOT_FOUND,
            ProblemClassification::BadGateway => StatusCode::BAD_GATEWAY,
        }
    }
}

/// The single-error wire value required by ETSI GS CIM 009 v1.9.1 clause 5.5.3.
///
/// Its classification fixes `type`, `title`, and `status`; callers supply only client-safe detail and an optional request instance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProblemDetail {
    /// Typed source of the category and HTTP status.
    classification: ProblemClassification,
    /// Client-visible human-readable explanation.
    detail: String,
    /// Optional request target supplied by the inbound HTTP boundary.
    instance: Option<Uri>,
}

impl ProblemDetail {
    /// Construct an ordinary clause 5.5.2 problem detail.
    pub fn new(error_type: NgsiLdErrorType, detail: impl Into<String>) -> ProblemDetail {
        ProblemDetail {
            classification: ProblemClassification::Category(error_type),
            detail: detail.into(),
            instance: None,
        }
    }

    /// Construct a problem detail from a binding-specific typed classification.
    pub fn from_classification(classification: ProblemClassification, detail: impl Into<String>) -> ProblemDetail {
        ProblemDetail {
            classification,
            detail: detail.into(),
            instance: None,
        }
    }

    /// Attach the exact inbound request URI as the RFC 7807 `instance` member.
    pub fn with_instance(mut self, instance: Uri) -> ProblemDetail {
        self.instance = Some(instance);
        self
    }

    /// The typed classification that fixes the payload category and response status.
    pub const fn classification(&self) -> ProblemClassification {
        self.classification
    }

    /// The clause 5.5.2 category carried by the payload.
    pub const fn error_type(&self) -> NgsiLdErrorType {
        self.classification.error_type()
    }

    /// The HTTP status fixed by the applicable binding.
    pub const fn status_code(&self) -> StatusCode {
        self.classification.status_code()
    }

    /// The client-visible detail.
    pub fn detail(&self) -> &str {
        &self.detail
    }

    /// The request instance, when the HTTP boundary attached one.
    pub const fn instance(&self) -> Option<&Uri> {
        self.instance.as_ref()
    }
}

impl Serialize for ProblemDetail {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let field_count = match self.instance {
            Some(_) => 5,
            None => 4,
        };
        let error_type = self.error_type();
        let mut state = serializer.serialize_struct("ProblemDetail", field_count)?;
        state.serialize_field("type", &error_type.type_iri())?;
        state.serialize_field("title", error_type.title())?;
        state.serialize_field("status", &self.status_code().as_u16())?;
        state.serialize_field("detail", &self.detail)?;
        if let Some(instance) = &self.instance {
            state.serialize_field("instance", instance.path_and_query().map_or(instance.path(), |value| value.as_str()))?;
        }
        state.end()
    }
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
        let error_type = NgsiLdErrorType::from_type_iri(&wire.type_iri).ok_or_else(|| de::Error::custom("unknown NGSI-LD problem type"))?;
        let classification = match (error_type, wire.status) {
            (NgsiLdErrorType::InternalError, Some(508)) => ProblemClassification::LoopDetected,
            (NgsiLdErrorType::InternalError, Some(504)) => ProblemClassification::GatewayTimeout,
            (NgsiLdErrorType::InternalError, Some(502)) => ProblemClassification::BadGateway,
            (NgsiLdErrorType::ResourceNotFound, Some(404)) => ProblemClassification::DistributedResourceNotFound,
            (category, Some(status)) if status == category.status_code().as_u16() => ProblemClassification::Category(category),
            (category, None) => ProblemClassification::Category(category),
            (_category, Some(_status)) => return Err(de::Error::custom("problem status disagrees with its NGSI-LD type")),
        };
        if let Some(title) = wire.title
            && title != error_type.title()
        {
            return Err(de::Error::custom("problem title disagrees with its NGSI-LD type"));
        }
        let instance = wire
            .instance
            .map(|instance| instance.as_str().parse::<Uri>().map_err(de::Error::custom))
            .transpose()?;
        Ok(ProblemDetail {
            classification,
            detail: wire.detail,
            instance,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error_type::NgsiLdErrorType,
        problem_detail::{ProblemClassification, ProblemDetail},
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
    fn distributed_classifications_fix_their_payload_categories_and_statuses() -> Result<(), serde_json::Error> {
        for (classification, expected_type, expected_status) in [
            (ProblemClassification::LoopDetected, NgsiLdErrorType::InternalError, StatusCode::LOOP_DETECTED),
            (
                ProblemClassification::GatewayTimeout,
                NgsiLdErrorType::InternalError,
                StatusCode::GATEWAY_TIMEOUT,
            ),
            (
                ProblemClassification::DistributedResourceNotFound,
                NgsiLdErrorType::ResourceNotFound,
                StatusCode::NOT_FOUND,
            ),
            (ProblemClassification::BadGateway, NgsiLdErrorType::InternalError, StatusCode::BAD_GATEWAY),
        ] {
            let problem = ProblemDetail::from_classification(classification, "distributed failure");
            let value = serde_json::to_value(&problem)?;
            pretty_assertions::assert_eq!(problem.status_code(), expected_status);
            pretty_assertions::assert_eq!(value["type"], expected_type.type_iri().as_str());
            pretty_assertions::assert_eq!(value["status"], expected_status.as_u16());
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
