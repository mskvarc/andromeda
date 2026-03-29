//! NGSI-LD error type URIs as defined in ETSI GS CIM 009 v1.9.1, clause 5.5.2.

pub const INVALID_REQUEST: &str = "https://uri.etsi.org/ngsi-ld/errors/InvalidRequest";
pub const BAD_REQUEST_DATA: &str = "https://uri.etsi.org/ngsi-ld/errors/BadRequestData";
pub const ALREADY_EXISTS: &str = "https://uri.etsi.org/ngsi-ld/errors/AlreadyExists";
pub const CONFLICT: &str = "https://uri.etsi.org/ngsi-ld/errors/Conflict";
pub const OPERATION_NOT_SUPPORTED: &str = "https://uri.etsi.org/ngsi-ld/errors/OperationNotSupported";
pub const RESOURCE_NOT_FOUND: &str = "https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound";
pub const INTERNAL_ERROR: &str = "https://uri.etsi.org/ngsi-ld/errors/InternalError";
pub const TOO_COMPLEX_QUERY: &str = "https://uri.etsi.org/ngsi-ld/errors/TooComplexQuery";
pub const TOO_MANY_RESULTS: &str = "https://uri.etsi.org/ngsi-ld/errors/TooManyResults";
pub const LD_CONTEXT_NOT_AVAILABLE: &str = "https://uri.etsi.org/ngsi-ld/errors/LdContextNotAvailable";
pub const NO_MULTI_TENANT_SUPPORT: &str = "https://uri.etsi.org/ngsi-ld/errors/NoMultiTenantSupport";
pub const NON_EXISTENT_TENANT: &str = "https://uri.etsi.org/ngsi-ld/errors/NonexistentTenant";
