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

        // The one category this function does not decide alone. Whether a
        // JSON-LD failure is a context the broker could not obtain or a
        // document it could not reshape is the processor's own distinction,
        // and asking it is what keeps the two readings from drifting.
        ErrorKind::JsonLd(error) => {
            if error.is_context_not_available() {
                NgsiLdErrorType::LdContextNotAvailable
            } else {
                NgsiLdErrorType::BadRequestData
            }
        }
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
        | ErrorKind::Rendering(_)
        | ErrorKind::Internal(_) => NgsiLdErrorType::InternalError,
    }
}

#[cfg(test)]
mod tests {
    use crate::{core::error::ErrorKind, http::error::category};
    use andromeda_jsonld::error::JsonLdError;
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
    use andromeda_ngsi_ld_render::error::{RenderedShape, RepresentationError};

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
            (
                ErrorKind::Rendering(RepresentationError::serialization(
                    RenderedShape::Entity,
                    serde_json::from_str::<u8>("not a number").expect_err("the input is not a u8"),
                )),
                NgsiLdErrorType::InternalError,
            ),
        ] {
            pretty_assertions::assert_eq!(category::of(&kind), expected, "wrong category for {kind:?}");
        }
    }

    #[test]
    fn a_jsonld_failure_is_categorised_by_the_processor_rather_than_by_its_message() {
        let unavailable = ErrorKind::JsonLd(JsonLdError::ContextFetchDisabled {
            url: "https://example.com/context.jsonld".to_string(),
        });
        pretty_assertions::assert_eq!(category::of(&unavailable), NgsiLdErrorType::LdContextNotAvailable);

        let malformed = ErrorKind::JsonLd(JsonLdError::invalid_document("not an object"));
        pretty_assertions::assert_eq!(category::of(&malformed), NgsiLdErrorType::BadRequestData);
    }
}
