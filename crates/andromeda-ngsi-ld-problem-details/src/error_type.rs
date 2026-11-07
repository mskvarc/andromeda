//! The NGSI-LD error categories of clause 5.5.2.

use http::StatusCode;
use iri_rs::{Iri, IriRefBuf, iri};
use strum::{Display, EnumIter, IntoEnumIterator, IntoStaticStr};

/// An NGSI-LD error category (ETSI GS CIM 009 v1.9.1, clause 5.5.2).
///
/// Each variant fixes three things the specification ties together: a canonical
/// `https://uri.etsi.org/ngsi-ld/errors/...` type IRI, an HTTP status code, and
/// a short title suitable for the `title` member of an
/// [RFC 7807](https://www.rfc-editor.org/rfc/rfc7807) problem detail. The
/// rendering is that title, so a category formats as the text a client sees.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, EnumIter, IntoStaticStr)]
pub enum NgsiLdErrorType {
    /// The request was syntactically invalid (400).
    #[strum(to_string = "Invalid request")]
    InvalidRequest,
    /// The request body contains invalid or incomplete data (400).
    #[strum(to_string = "Bad request data")]
    BadRequestData,
    /// The resource already exists (409).
    #[strum(to_string = "Already exists")]
    AlreadyExists,
    /// A state conflict prevented the operation (409).
    #[strum(to_string = "Conflict")]
    Conflict,
    /// The requested operation is not supported (422).
    #[strum(to_string = "Operation not supported")]
    OperationNotSupported,
    /// The target resource was not found (404).
    #[strum(to_string = "Resource not found")]
    ResourceNotFound,
    /// An unexpected internal error occurred (500).
    #[strum(to_string = "Internal error")]
    InternalError,
    /// The query is too complex to evaluate (403).
    #[strum(to_string = "Too complex query")]
    TooComplexQuery,
    /// The query would return too many results (403).
    #[strum(to_string = "Too many results")]
    TooManyResults,
    /// A required JSON-LD `@context` could not be retrieved (503).
    #[strum(to_string = "LD context not available")]
    LdContextNotAvailable,
    /// Multi-tenancy is not supported (501).
    #[strum(to_string = "No multi-tenant support")]
    NoMultiTenantSupport,
    /// The requested tenant does not exist (404).
    #[strum(to_string = "Nonexistent tenant")]
    NonexistentTenant,
    /// The request payload body is larger than the broker accepts (413).
    #[strum(to_string = "Request entity too large")]
    RequestEntityTooLarge,
    /// The request method is not one the resource serves (405).
    #[strum(to_string = "Method not allowed")]
    MethodNotAllowed,
    /// The client did not send the whole request within the broker's request
    /// deadline (408).
    #[strum(to_string = "Request timeout")]
    RequestTimeout,
}

impl NgsiLdErrorType {
    /// The canonical ETSI type IRI for this category.
    #[must_use]
    pub const fn type_iri(self) -> Iri<&'static str> {
        match self {
            NgsiLdErrorType::InvalidRequest => iri!("https://uri.etsi.org/ngsi-ld/errors/InvalidRequest"),
            NgsiLdErrorType::BadRequestData => iri!("https://uri.etsi.org/ngsi-ld/errors/BadRequestData"),
            NgsiLdErrorType::AlreadyExists => iri!("https://uri.etsi.org/ngsi-ld/errors/AlreadyExists"),
            NgsiLdErrorType::Conflict => iri!("https://uri.etsi.org/ngsi-ld/errors/Conflict"),
            NgsiLdErrorType::OperationNotSupported => iri!("https://uri.etsi.org/ngsi-ld/errors/OperationNotSupported"),
            NgsiLdErrorType::ResourceNotFound => iri!("https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound"),
            NgsiLdErrorType::InternalError => iri!("https://uri.etsi.org/ngsi-ld/errors/InternalError"),
            NgsiLdErrorType::TooComplexQuery => iri!("https://uri.etsi.org/ngsi-ld/errors/TooComplexQuery"),
            NgsiLdErrorType::TooManyResults => iri!("https://uri.etsi.org/ngsi-ld/errors/TooManyResults"),
            NgsiLdErrorType::LdContextNotAvailable => iri!("https://uri.etsi.org/ngsi-ld/errors/LdContextNotAvailable"),
            NgsiLdErrorType::NoMultiTenantSupport => iri!("https://uri.etsi.org/ngsi-ld/errors/NoMultiTenantSupport"),
            NgsiLdErrorType::NonexistentTenant => iri!("https://uri.etsi.org/ngsi-ld/errors/NonexistentTenant"),
            NgsiLdErrorType::RequestEntityTooLarge => iri!("https://uri.etsi.org/ngsi-ld/errors/RequestEntityTooLarge"),
            NgsiLdErrorType::MethodNotAllowed => iri!("https://uri.etsi.org/ngsi-ld/errors/MethodNotAllowed"),
            NgsiLdErrorType::RequestTimeout => iri!("https://uri.etsi.org/ngsi-ld/errors/RequestTimeout"),
        }
    }

    /// The short title of this category, as the `title` member of a problem
    /// detail carries it.
    #[must_use]
    pub fn title(self) -> &'static str {
        self.into()
    }

    /// The category a type IRI names, if it names one.
    ///
    /// Searches the variants rather than matching a second list of IRIs: a
    /// hand-written inverse can drift from [`type_iri`](Self::type_iri), and a
    /// fifteen-element scan costs nothing on a path that runs once per
    /// deserialised batch error.
    #[must_use]
    pub fn from_type_iri(iri: &IriRefBuf) -> Option<NgsiLdErrorType> {
        NgsiLdErrorType::iter().find(|category| category.type_iri().as_str() == iri.as_str())
    }

    /// The HTTP status code Table 6.3.2-1 assigns to this category.
    ///
    /// Clause 5.5.2 defines the error *types*; the HTTP binding's Table 6.3.2-1
    /// is what maps each type to a status code, so that table is the authority
    /// here — notably `LdContextNotAvailable`, which it assigns 504 rather than
    /// 503 because the broker is a gateway that failed to dereference an
    /// upstream `@context`, not a service that is itself unavailable.
    #[must_use]
    pub const fn status_code(self) -> StatusCode {
        match self {
            NgsiLdErrorType::InvalidRequest | NgsiLdErrorType::BadRequestData => StatusCode::BAD_REQUEST,
            NgsiLdErrorType::AlreadyExists | NgsiLdErrorType::Conflict => StatusCode::CONFLICT,
            NgsiLdErrorType::OperationNotSupported => StatusCode::UNPROCESSABLE_ENTITY,
            NgsiLdErrorType::ResourceNotFound | NgsiLdErrorType::NonexistentTenant => StatusCode::NOT_FOUND,
            NgsiLdErrorType::InternalError => StatusCode::INTERNAL_SERVER_ERROR,
            NgsiLdErrorType::TooComplexQuery | NgsiLdErrorType::TooManyResults => StatusCode::FORBIDDEN,
            NgsiLdErrorType::LdContextNotAvailable => StatusCode::GATEWAY_TIMEOUT,
            NgsiLdErrorType::NoMultiTenantSupport => StatusCode::NOT_IMPLEMENTED,
            NgsiLdErrorType::RequestEntityTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            NgsiLdErrorType::MethodNotAllowed => StatusCode::METHOD_NOT_ALLOWED,
            NgsiLdErrorType::RequestTimeout => StatusCode::REQUEST_TIMEOUT,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::error_type::NgsiLdErrorType;
    use http::StatusCode;
    use iri_rs::IriRefBuf;
    use strum::IntoEnumIterator;

    #[test]
    fn every_category_has_an_etsi_type_iri_that_resolves_back_to_it() {
        for category in NgsiLdErrorType::iter() {
            assert!(
                category.type_iri().as_str().starts_with("https://uri.etsi.org/ngsi-ld/errors/"),
                "{category} does not carry an ETSI type IRI"
            );
            let iri = IriRefBuf::new(category.type_iri().as_str().to_owned()).expect("IRI reference");
            pretty_assertions::assert_eq!(NgsiLdErrorType::from_type_iri(&iri), Some(category));
        }
    }

    #[test]
    fn every_category_has_a_non_empty_title_that_is_its_rendering() {
        for category in NgsiLdErrorType::iter() {
            assert_ne!(category.title(), "");
            pretty_assertions::assert_eq!(category.to_string(), category.title());
        }
    }

    #[test]
    fn a_uri_no_category_claims_resolves_to_nothing() {
        pretty_assertions::assert_eq!(
            NgsiLdErrorType::from_type_iri(&"https://uri.etsi.org/ngsi-ld/errors/Invented".parse().expect("IRI reference")),
            None
        );
        pretty_assertions::assert_eq!(NgsiLdErrorType::from_type_iri(&IriRefBuf::default()), None);
    }

    #[test]
    fn each_category_carries_its_exact_clause_5_5_2_metadata_and_binding_status() {
        for (category, suffix, title, status) in [
            (NgsiLdErrorType::AlreadyExists, "AlreadyExists", "Already exists", StatusCode::CONFLICT),
            (NgsiLdErrorType::BadRequestData, "BadRequestData", "Bad request data", StatusCode::BAD_REQUEST),
            (NgsiLdErrorType::Conflict, "Conflict", "Conflict", StatusCode::CONFLICT),
            (
                NgsiLdErrorType::InternalError,
                "InternalError",
                "Internal error",
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
            (NgsiLdErrorType::InvalidRequest, "InvalidRequest", "Invalid request", StatusCode::BAD_REQUEST),
            (
                NgsiLdErrorType::LdContextNotAvailable,
                "LdContextNotAvailable",
                "LD context not available",
                StatusCode::GATEWAY_TIMEOUT,
            ),
            (
                NgsiLdErrorType::NoMultiTenantSupport,
                "NoMultiTenantSupport",
                "No multi-tenant support",
                StatusCode::NOT_IMPLEMENTED,
            ),
            (
                NgsiLdErrorType::NonexistentTenant,
                "NonexistentTenant",
                "Nonexistent tenant",
                StatusCode::NOT_FOUND,
            ),
            (
                NgsiLdErrorType::OperationNotSupported,
                "OperationNotSupported",
                "Operation not supported",
                StatusCode::UNPROCESSABLE_ENTITY,
            ),
            (
                NgsiLdErrorType::ResourceNotFound,
                "ResourceNotFound",
                "Resource not found",
                StatusCode::NOT_FOUND,
            ),
            (NgsiLdErrorType::TooComplexQuery, "TooComplexQuery", "Too complex query", StatusCode::FORBIDDEN),
            (NgsiLdErrorType::TooManyResults, "TooManyResults", "Too many results", StatusCode::FORBIDDEN),
            (
                NgsiLdErrorType::RequestEntityTooLarge,
                "RequestEntityTooLarge",
                "Request entity too large",
                StatusCode::PAYLOAD_TOO_LARGE,
            ),
            (
                NgsiLdErrorType::MethodNotAllowed,
                "MethodNotAllowed",
                "Method not allowed",
                StatusCode::METHOD_NOT_ALLOWED,
            ),
            (
                NgsiLdErrorType::RequestTimeout,
                "RequestTimeout",
                "Request timeout",
                StatusCode::REQUEST_TIMEOUT,
            ),
        ] {
            pretty_assertions::assert_eq!(category.type_iri().as_str(), format!("https://uri.etsi.org/ngsi-ld/errors/{suffix}"));
            pretty_assertions::assert_eq!(category.title(), title);
            pretty_assertions::assert_eq!(category.status_code(), status);
        }
    }
}
