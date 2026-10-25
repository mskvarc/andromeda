//! Which registration members a body may delete.
//!
//! Every member of a registration body is read as a [`Clearable`]: absent, the
//! NGSI-LD Null or a value. Clause 5.5.8 lets an update delete only optional
//! members, so `type`, `information`, `endpoint` and `mode` refuse the Null,
//! and clause 5.2.1 refuses the Null in every member of a creation body, which
//! has nothing to delete.

use crate::{csr_member::CsrMember, error::RegistrationError, registration_validation_reason::RegistrationValidationReason};
use andromeda_ngsi_ld_model::clearable::Clearable;

/// The value of a member that may not be deleted.
///
/// # Errors
///
/// Returns [`RegistrationValidationReason::MemberMustNotBeNull`] naming
/// `member` when the body wrote it as the NGSI-LD Null.
pub(crate) fn present<T>(written: Clearable<T>, member: CsrMember) -> Result<Option<T>, RegistrationError> {
    match written {
        Clearable::Absent => Ok(None),
        Clearable::Set(value) => Ok(Some(value)),
        Clearable::NgsiLdNull => Err(RegistrationError::Validation(RegistrationValidationReason::MemberMustNotBeNull { member })),
    }
}

/// The value of a member a creation body must carry (clause 5.2.9
/// cardinality 1).
///
/// # Errors
///
/// Returns [`RegistrationValidationReason::MemberRequired`] when the body
/// omits the member, and the refusal of [`present`] when it wrote the NGSI-LD
/// Null.
pub(crate) fn required<T>(written: Clearable<T>, member: CsrMember) -> Result<T, RegistrationError> {
    present(written, member)?.ok_or(RegistrationError::Validation(RegistrationValidationReason::MemberRequired { member }))
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        csr_member::CsrMember,
        error::RegistrationError,
        member_presence::{present, required},
        registration_validation_reason::RegistrationValidationReason,
    };
    use andromeda_ngsi_ld_model::clearable::Clearable;

    #[test]
    fn a_member_that_may_not_be_deleted_refuses_the_null_and_keeps_the_rest() {
        pretty_assertions::assert_eq!(present(Clearable::<u8>::Absent, CsrMember::Mode).expect("absent is allowed"), None);
        pretty_assertions::assert_eq!(present(Clearable::Set(3_u8), CsrMember::Mode).expect("a value is allowed"), Some(3));
        let refusal = present(Clearable::<u8>::NgsiLdNull, CsrMember::Mode).expect_err("the Null is refused");
        assert!(
            matches!(
                refusal,
                RegistrationError::Validation(RegistrationValidationReason::MemberMustNotBeNull { member: CsrMember::Mode })
            ),
            "{refusal:?}"
        );
        pretty_assertions::assert_eq!(refusal.to_string(), "invalid context-source registration: mode must not be the NGSI-LD Null");
    }

    #[test]
    fn a_required_member_must_be_carried_with_a_value() {
        pretty_assertions::assert_eq!(required(Clearable::Set(3_u8), CsrMember::Endpoint).expect("a value"), 3);
        let missing = required(Clearable::<u8>::Absent, CsrMember::Endpoint).expect_err("absent is refused");
        assert!(
            matches!(
                missing,
                RegistrationError::Validation(RegistrationValidationReason::MemberRequired { member: CsrMember::Endpoint })
            ),
            "{missing:?}"
        );
        assert!(required(Clearable::<u8>::NgsiLdNull, CsrMember::Endpoint).is_err());
    }
}
