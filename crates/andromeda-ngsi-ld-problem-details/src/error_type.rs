//! The NGSI-LD error categories of clause 5.5.2.

use http::StatusCode;
use iri_rs::{Iri, iri};
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
}

impl NgsiLdErrorType {
    /// The canonical ETSI type IRI for this category.
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
        }
    }

    /// The canonical ETSI type IRI as a static string slice.
    pub fn type_uri(self) -> &'static str {
        self.type_iri().into_inner()
    }

    /// The short title of this category, as the `title` member of a problem
    /// detail carries it.
    pub fn title(self) -> &'static str {
        self.into()
    }

    /// The category a type IRI names, if it names one.
    ///
    /// Searches the variants rather than matching a second list of IRIs: a
    /// hand-written inverse can drift from [`type_iri`](Self::type_iri), and a
    /// twelve-element scan costs nothing on a path that runs once per
    /// deserialised batch error.
    pub fn from_type_uri(uri: &str) -> Option<NgsiLdErrorType> {
        NgsiLdErrorType::iter().find(|category| category.type_uri() == uri)
    }

    /// The HTTP status code clause 5.5.2 assigns to this category.
    pub const fn status_code(self) -> StatusCode {
        match self {
            NgsiLdErrorType::InvalidRequest | NgsiLdErrorType::BadRequestData => StatusCode::BAD_REQUEST,
            NgsiLdErrorType::AlreadyExists | NgsiLdErrorType::Conflict => StatusCode::CONFLICT,
            NgsiLdErrorType::OperationNotSupported => StatusCode::UNPROCESSABLE_ENTITY,
            NgsiLdErrorType::ResourceNotFound | NgsiLdErrorType::NonexistentTenant => StatusCode::NOT_FOUND,
            NgsiLdErrorType::InternalError => StatusCode::INTERNAL_SERVER_ERROR,
            NgsiLdErrorType::TooComplexQuery | NgsiLdErrorType::TooManyResults => StatusCode::FORBIDDEN,
            NgsiLdErrorType::LdContextNotAvailable => StatusCode::SERVICE_UNAVAILABLE,
            NgsiLdErrorType::NoMultiTenantSupport => StatusCode::NOT_IMPLEMENTED,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::error_type::NgsiLdErrorType;
    use http::StatusCode;
    use strum::IntoEnumIterator;

    #[test]
    fn every_category_has_an_etsi_type_iri_that_resolves_back_to_it() {
        for category in NgsiLdErrorType::iter() {
            assert!(
                category.type_uri().starts_with("https://uri.etsi.org/ngsi-ld/errors/"),
                "{category} does not carry an ETSI type IRI"
            );
            pretty_assertions::assert_eq!(NgsiLdErrorType::from_type_uri(category.type_uri()), Some(category));
        }
    }

    #[test]
    fn every_category_has_a_non_empty_title_that_is_its_rendering() {
        for category in NgsiLdErrorType::iter() {
            assert!(!category.title().is_empty());
            pretty_assertions::assert_eq!(category.to_string(), category.title());
        }
    }

    #[test]
    fn a_uri_no_category_claims_resolves_to_nothing() {
        pretty_assertions::assert_eq!(NgsiLdErrorType::from_type_uri("https://uri.etsi.org/ngsi-ld/errors/Invented"), None);
        pretty_assertions::assert_eq!(NgsiLdErrorType::from_type_uri(""), None);
    }

    #[test]
    fn each_category_carries_the_status_clause_5_5_2_assigns_it() {
        pretty_assertions::assert_eq!(NgsiLdErrorType::InvalidRequest.status_code(), StatusCode::BAD_REQUEST);
        pretty_assertions::assert_eq!(NgsiLdErrorType::BadRequestData.status_code(), StatusCode::BAD_REQUEST);
        pretty_assertions::assert_eq!(NgsiLdErrorType::AlreadyExists.status_code(), StatusCode::CONFLICT);
        pretty_assertions::assert_eq!(NgsiLdErrorType::Conflict.status_code(), StatusCode::CONFLICT);
        pretty_assertions::assert_eq!(NgsiLdErrorType::OperationNotSupported.status_code(), StatusCode::UNPROCESSABLE_ENTITY);
        pretty_assertions::assert_eq!(NgsiLdErrorType::ResourceNotFound.status_code(), StatusCode::NOT_FOUND);
        pretty_assertions::assert_eq!(NgsiLdErrorType::NonexistentTenant.status_code(), StatusCode::NOT_FOUND);
        pretty_assertions::assert_eq!(NgsiLdErrorType::InternalError.status_code(), StatusCode::INTERNAL_SERVER_ERROR);
        pretty_assertions::assert_eq!(NgsiLdErrorType::TooComplexQuery.status_code(), StatusCode::FORBIDDEN);
        pretty_assertions::assert_eq!(NgsiLdErrorType::TooManyResults.status_code(), StatusCode::FORBIDDEN);
        pretty_assertions::assert_eq!(NgsiLdErrorType::LdContextNotAvailable.status_code(), StatusCode::SERVICE_UNAVAILABLE);
        pretty_assertions::assert_eq!(NgsiLdErrorType::NoMultiTenantSupport.status_code(), StatusCode::NOT_IMPLEMENTED);
    }
}
