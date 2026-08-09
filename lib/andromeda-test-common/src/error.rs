//! NGSI-LD error-type URIs used in Problem-Details assertions.
//!
//! Spec refs: NGSI-LD 6.3.17 (Problem-Details envelope) and 4.14.2 / 5.5.2
//! (error type registry).

use iri_rs::{Iri, iri};

/// `AlreadyExists` (409) — attempted to create a resource with an id that
/// already exists.
pub const ALREADY_EXISTS: &Iri<&'static str> = &iri!("https://uri.etsi.org/ngsi-ld/errors/AlreadyExists");

/// `BadRequestData` (400) — malformed request body or parameter.
pub const BAD_REQUEST_DATA: &Iri<&'static str> = &iri!("https://uri.etsi.org/ngsi-ld/errors/BadRequestData");

/// `InvalidRequest` (400) — request was syntactically valid but semantically
/// invalid (e.g. unsupported feature toggles).
pub const INVALID_REQUEST: &Iri<&'static str> = &iri!("https://uri.etsi.org/ngsi-ld/errors/InvalidRequest");

/// `ResourceNotFound` (404).
pub const RESOURCE_NOT_FOUND: &Iri<&'static str> = &iri!("https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound");

/// `LdContextNotAvailable` (504) — a referenced `@context` could not be fetched.
pub const LD_CONTEXT_NOT_AVAILABLE: &Iri<&'static str> = &iri!("https://uri.etsi.org/ngsi-ld/errors/LdContextNotAvailable");

/// `InternalError` (500).
pub const INTERNAL_ERROR: &Iri<&'static str> = &iri!("https://uri.etsi.org/ngsi-ld/errors/InternalError");

/// `OperationNotSupported` (422) — method or option combination not
/// implemented by the broker.
pub const OPERATION_NOT_SUPPORTED: &Iri<&'static str> = &iri!("https://uri.etsi.org/ngsi-ld/errors/OperationNotSupported");

/// `TooComplexQuery` (403) — query expression exceeds the broker's complexity
/// ceiling.
pub const TOO_COMPLEX_QUERY: &Iri<&'static str> = &iri!("https://uri.etsi.org/ngsi-ld/errors/TooComplexQuery");

/// `TooManyResults` (403) — query would return more entities than the
/// configured ceiling.
pub const TOO_MANY_RESULTS: &Iri<&'static str> = &iri!("https://uri.etsi.org/ngsi-ld/errors/TooManyResults");

/// `NonexistentTenant` (404) — named tenant does not exist.
pub const NONEXISTENT_TENANT: &Iri<&'static str> = &iri!("https://uri.etsi.org/ngsi-ld/errors/NonexistentTenant");

/// `NoMultiTenantSupport` (501) — broker build or configuration disables
/// multi-tenant support.
pub const NO_MULTI_TENANT_SUPPORT: &Iri<&'static str> = &iri!("https://uri.etsi.org/ngsi-ld/errors/NoMultiTenantSupport");

/// `Conflict` (409) — optimistic concurrency or spec-defined conflict.
pub const CONFLICT: &Iri<&'static str> = &iri!("https://uri.etsi.org/ngsi-ld/errors/Conflict");
