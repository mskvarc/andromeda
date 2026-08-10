//! Canonical NGSI-LD error-type IRIs used as expected protocol data.

use iri_rs::{Iri, iri};

/// `AlreadyExists` error type.
pub const ALREADY_EXISTS: &Iri<&'static str> = &iri!("https://uri.etsi.org/ngsi-ld/errors/AlreadyExists");
/// `BadRequestData` error type.
pub const BAD_REQUEST_DATA: &Iri<&'static str> = &iri!("https://uri.etsi.org/ngsi-ld/errors/BadRequestData");
/// `InvalidRequest` error type.
pub const INVALID_REQUEST: &Iri<&'static str> = &iri!("https://uri.etsi.org/ngsi-ld/errors/InvalidRequest");
/// `ResourceNotFound` error type.
pub const RESOURCE_NOT_FOUND: &Iri<&'static str> = &iri!("https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound");
/// `LdContextNotAvailable` error type.
pub const LD_CONTEXT_NOT_AVAILABLE: &Iri<&'static str> = &iri!("https://uri.etsi.org/ngsi-ld/errors/LdContextNotAvailable");
/// `InternalError` error type.
pub const INTERNAL_ERROR: &Iri<&'static str> = &iri!("https://uri.etsi.org/ngsi-ld/errors/InternalError");
/// `OperationNotSupported` error type.
pub const OPERATION_NOT_SUPPORTED: &Iri<&'static str> = &iri!("https://uri.etsi.org/ngsi-ld/errors/OperationNotSupported");
/// `TooComplexQuery` error type.
pub const TOO_COMPLEX_QUERY: &Iri<&'static str> = &iri!("https://uri.etsi.org/ngsi-ld/errors/TooComplexQuery");
/// `TooManyResults` error type.
pub const TOO_MANY_RESULTS: &Iri<&'static str> = &iri!("https://uri.etsi.org/ngsi-ld/errors/TooManyResults");
/// `NonexistentTenant` error type.
pub const NONEXISTENT_TENANT: &Iri<&'static str> = &iri!("https://uri.etsi.org/ngsi-ld/errors/NonexistentTenant");
/// `NoMultiTenantSupport` error type.
pub const NO_MULTI_TENANT_SUPPORT: &Iri<&'static str> = &iri!("https://uri.etsi.org/ngsi-ld/errors/NoMultiTenantSupport");
/// `Conflict` error type.
pub const CONFLICT: &Iri<&'static str> = &iri!("https://uri.etsi.org/ngsi-ld/errors/Conflict");
