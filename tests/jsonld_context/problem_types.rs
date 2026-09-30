//! The clause 5.5.2 error types the suite expects, as the IRIs a problem
//! detail carries in its `type` member.

use andromeda_test_fixtures::problem_detail as error;
use iri_rs::Iri;

/// Clause 5.5.2 `InvalidRequest`.
pub const INVALID_REQUEST_TYPE: &Iri<&str> = error::INVALID_REQUEST;

/// Clause 5.5.2 `BadRequestData`.
pub const BAD_REQUEST_DATA_TYPE: &Iri<&str> = error::BAD_REQUEST_DATA;

/// Clause 5.5.2 `ResourceNotFound`.
pub const RESOURCE_NOT_FOUND_TYPE: &Iri<&str> = error::RESOURCE_NOT_FOUND;

/// Clause 5.5.2 `LdContextNotAvailable`.
pub const LD_CONTEXT_NOT_AVAILABLE_TYPE: &Iri<&str> = error::LD_CONTEXT_NOT_AVAILABLE;

/// Clause 5.5.2 `OperationNotSupported`.
pub const OPERATION_NOT_SUPPORTED_TYPE: &Iri<&str> = error::OPERATION_NOT_SUPPORTED;

/// Clause 5.5.2 `NonexistentTenant`.
pub const NONEXISTENT_TENANT_TYPE: &Iri<&str> = error::NONEXISTENT_TENANT;

/// Clause 5.5.2 `InternalError`.
pub const INTERNAL_ERROR_TYPE: &Iri<&str> = error::INTERNAL_ERROR;
