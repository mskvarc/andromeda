//! Which clause 5.5.2 category a broker failure is reported as.
//!
//! The mapping lives on this side of the boundary rather than beside
//! [`NgsiLdErrorType`] because it is not part of the specification: the
//! categories are fixed by ETSI GS CIM 009, but which of Andromeda's internal
//! failures falls into which of them is a decision this broker makes.

use crate::core::error::ErrorKind;
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;

/// The category a failure of this kind is reported to a client as.
pub const fn of(kind: &ErrorKind) -> NgsiLdErrorType {
    match kind {
        ErrorKind::EntityNotFound(_)
        | ErrorKind::SubscriptionNotFound(_)
        | ErrorKind::CsrNotFound(_)
        | ErrorKind::SnapshotNotFound(_)
        | ErrorKind::ContextNotFound(_)
        | ErrorKind::EntityMapNotFound(_)
        | ErrorKind::TemporalNotFound(_) => NgsiLdErrorType::ResourceNotFound,

        ErrorKind::EntityAlreadyExists(_)
        | ErrorKind::SubscriptionAlreadyExists(_)
        | ErrorKind::CsrAlreadyExists(_)
        | ErrorKind::SnapshotAlreadyExists(_)
        | ErrorKind::DatabaseUniqueViolation(_) => NgsiLdErrorType::AlreadyExists,

        ErrorKind::Conflict(_) => NgsiLdErrorType::Conflict,
        ErrorKind::OperationNotSupported(_) => NgsiLdErrorType::OperationNotSupported,
        ErrorKind::ContextNotAvailable(_) => NgsiLdErrorType::LdContextNotAvailable,
        ErrorKind::NonexistentTenant(_) => NgsiLdErrorType::NonexistentTenant,
        ErrorKind::MultiTenancyNotSupported => NgsiLdErrorType::NoMultiTenantSupport,

        ErrorKind::JsonLdProcessing(_)
        | ErrorKind::SubscriptionValidation(_)
        | ErrorKind::EntityValidation(_)
        | ErrorKind::TemporalValidation(_)
        | ErrorKind::CsrValidation(_)
        | ErrorKind::SnapshotValidation(_)
        | ErrorKind::EntityMapValidation(_)
        | ErrorKind::QueryValidation(_)
        | ErrorKind::JsonParse(_)
        | ErrorKind::InvalidUrn(_)
        | ErrorKind::InvalidSnapshotId(_)
        | ErrorKind::InvalidSnapshotNotificationId(_)
        | ErrorKind::InvalidEntityMapId(_)
        | ErrorKind::InvalidEntityTypeListId(_)
        | ErrorKind::InvalidAttributeListId(_)
        | ErrorKind::DatabaseRegexError(_) => NgsiLdErrorType::BadRequestData,

        ErrorKind::MalformedRequest(_) => NgsiLdErrorType::InvalidRequest,

        ErrorKind::Database(_)
        | ErrorKind::Migration(_)
        | ErrorKind::Config(_)
        | ErrorKind::TracingInit(_)
        | ErrorKind::Io(_)
        | ErrorKind::ExternalRequest(_)
        | ErrorKind::Internal(_) => NgsiLdErrorType::InternalError,
    }
}

#[cfg(test)]
mod tests {
    use crate::{core::error::ErrorKind, http::error::category};
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;

    #[test]
    fn each_family_of_failures_reports_the_category_clause_5_5_2_gives_it() {
        for (kind, expected) in [
            (ErrorKind::MalformedRequest("x".into()), NgsiLdErrorType::InvalidRequest),
            (ErrorKind::EntityValidation("x".into()), NgsiLdErrorType::BadRequestData),
            (ErrorKind::EntityNotFound("x".into()), NgsiLdErrorType::ResourceNotFound),
            (ErrorKind::EntityAlreadyExists("x".into()), NgsiLdErrorType::AlreadyExists),
            (ErrorKind::Conflict("x".into()), NgsiLdErrorType::Conflict),
            (ErrorKind::OperationNotSupported("x".into()), NgsiLdErrorType::OperationNotSupported),
            (ErrorKind::ContextNotAvailable("x".into()), NgsiLdErrorType::LdContextNotAvailable),
            (ErrorKind::MultiTenancyNotSupported, NgsiLdErrorType::NoMultiTenantSupport),
            (ErrorKind::NonexistentTenant("x".into()), NgsiLdErrorType::NonexistentTenant),
            (ErrorKind::Internal("x".into()), NgsiLdErrorType::InternalError),
        ] {
            pretty_assertions::assert_eq!(category::of(&kind), expected, "wrong category for {kind:?}");
        }
    }
}
